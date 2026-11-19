//! Retrying HTTP execution shared by the token endpoint and the report endpoints.

use std::time::Duration;

use reqwest::header::{CONTENT_TYPE, RETRY_AFTER};
use serde::Deserialize;

use crate::config::{Config, RetryPolicy};
use crate::error::{Error, Result};

/// Redirect hops followed per request; the CDN bot check bounces through a few.
const MAX_REDIRECTS: usize = 10;

/// Bytes of an unexpected body kept for error messages.
const SNIPPET_LEN: usize = 160;

/// A fully-read successful HTTP response.
#[derive(Debug)]
pub(crate) struct RawResponse {
    pub body: Vec<u8>,
    pub file_name: Option<String>,
}

/// Shared `reqwest` client (cookie jar on) plus the retry policy.
#[derive(Debug)]
pub(crate) struct Http {
    client: reqwest::Client,
    retry: RetryPolicy,
}

impl Http {
    pub(crate) fn new(config: &Config) -> Result<Self> {
        let client = reqwest::Client::builder()
            .user_agent(&config.user_agent)
            .timeout(config.timeout)
            .cookie_store(true)
            .redirect(reqwest::redirect::Policy::limited(MAX_REDIRECTS))
            .build()
            .map_err(|e| Error::Config(format!("cannot build HTTP client: {e}")))?;
        Ok(Self {
            client,
            retry: config.retry,
        })
    }

    /// Sends the request built by `build`, retrying retryable failures with jittered backoff.
    ///
    /// `build` runs once per attempt because a `RequestBuilder` is consumed on send.
    pub(crate) async fn execute<F>(&self, build: F) -> Result<RawResponse>
    where
        F: Fn(&reqwest::Client) -> reqwest::RequestBuilder,
    {
        let mut attempt = 1u32;
        loop {
            match self.send_once(build(&self.client)).await {
                Ok(raw) => return Ok(raw),
                Err(e) if e.is_retryable() && attempt < self.retry.max_attempts => {
                    let wait = self.retry.delay(attempt, e.retry_after(), fastrand::f64());
                    tokio::time::sleep(wait).await;
                    attempt += 1;
                }
                Err(e) => return Err(e),
            }
        }
    }

    async fn send_once(&self, request: reqwest::RequestBuilder) -> Result<RawResponse> {
        let response = request.send().await?;
        let status = response.status();
        let retry_after = response
            .headers()
            .get(RETRY_AFTER)
            .and_then(|v| v.to_str().ok())
            .and_then(parse_retry_after);
        let content_type = response
            .headers()
            .get(CONTENT_TYPE)
            .and_then(|v| v.to_str().ok())
            .unwrap_or_default()
            .to_ascii_lowercase();
        let file_name = response
            .headers()
            .get(reqwest::header::CONTENT_DISPOSITION)
            .and_then(|v| v.to_str().ok())
            .and_then(parse_content_disposition_name);
        let body = response.bytes().await?.to_vec();

        if status.is_success() {
            return if looks_like_html(&content_type, &body) {
                Err(Error::BotCheck {
                    snippet: snippet(&body),
                })
            } else {
                Ok(RawResponse { body, file_name })
            };
        }
        if status.is_redirection() && looks_like_html(&content_type, &body) {
            return Err(Error::BotCheck {
                snippet: snippet(&body),
            });
        }
        let (message, unknown_params) = parse_error_body(&body);
        Err(Error::Api {
            status: status.as_u16(),
            message,
            unknown_params,
            retry_after,
        })
    }
}

/// Parses a `Retry-After` header given as whole seconds.
fn parse_retry_after(value: &str) -> Option<Duration> {
    value.trim().parse::<u64>().ok().map(Duration::from_secs)
}

/// Extracts `filename=...` from a `Content-Disposition` header value.
fn parse_content_disposition_name(value: &str) -> Option<String> {
    value
        .split(';')
        .map(str::trim)
        .find_map(|part| part.strip_prefix("filename="))
        .map(|name| name.trim_matches('"').to_owned())
        .filter(|name| !name.is_empty())
}

/// True for a challenge page: an HTML content type, or a body that opens with `<`.
///
/// Every endpoint this crate calls answers JSON or a zip, so a leading `<` is never data.
fn looks_like_html(content_type: &str, body: &[u8]) -> bool {
    content_type.contains("text/html")
        || body.iter().find(|b| !b.is_ascii_whitespace()) == Some(&b'<')
}

fn snippet(body: &[u8]) -> String {
    String::from_utf8_lossy(&body[..body.len().min(SNIPPET_LEN)]).into_owned()
}

#[derive(Debug, Deserialize)]
struct ErrorBody {
    message: Option<String>,
    error_description: Option<String>,
    error: Option<String>,
    #[serde(default)]
    data: Vec<String>,
}

/// Reads ERCOT (`message`, `data`) and B2C (`error_description`) error bodies.
fn parse_error_body(body: &[u8]) -> (String, Vec<String>) {
    match serde_json::from_slice::<ErrorBody>(body) {
        Ok(parsed) => (
            parsed
                .message
                .or(parsed.error_description)
                .or(parsed.error)
                .unwrap_or_else(|| snippet(body)),
            parsed.data,
        ),
        Err(_) => (snippet(body), Vec::new()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn html_is_detected_by_header_or_by_leading_angle_bracket() {
        assert!(looks_like_html("text/html; charset=utf-8", b"{}"));
        assert!(looks_like_html("text/plain", b"  \n<HTML>Loading</HTML>"));
        assert!(!looks_like_html("application/json", br#"{"a":1}"#));
        assert!(!looks_like_html("application/zip", b"PK\x03\x04"));
        assert!(!looks_like_html("application/json", b""));
    }

    #[test]
    fn retry_after_seconds_only() {
        assert_eq!(parse_retry_after(" 7 "), Some(Duration::from_secs(7)));
        assert_eq!(parse_retry_after("Wed, 21 Oct 2026 07:28:00 GMT"), None);
    }

    #[test]
    fn content_disposition_filename() {
        assert_eq!(
            parse_content_disposition_name("attachment; filename=1300006200.cdr.zip").as_deref(),
            Some("1300006200.cdr.zip")
        );
        assert_eq!(
            parse_content_disposition_name("attachment; filename=\"a b.zip\"").as_deref(),
            Some("a b.zip")
        );
        assert_eq!(parse_content_disposition_name("inline"), None);
    }

    #[test]
    fn ercot_error_body_exposes_rejected_params() {
        let body = br#"{"timestamp":"t","code":400,"status":"BAD_REQUEST","message":"One or more of the query parameters specified are not available for this resource.","data":["hourEndingFrom","bogus"]}"#;
        let (msg, params) = parse_error_body(body);
        assert!(msg.starts_with("One or more"), "{msg}");
        assert_eq!(params, ["hourEndingFrom", "bogus"]);
    }

    #[test]
    fn b2c_error_body_uses_description() {
        let (msg, params) = parse_error_body(
            br#"{"error":"invalid_grant","error_description":"AADB2C90225: bad"}"#,
        );
        assert_eq!(msg, "AADB2C90225: bad");
        assert!(params.is_empty());
    }

    #[test]
    fn non_json_error_body_falls_back_to_snippet() {
        let (msg, _) = parse_error_body(b"<html>nope</html>");
        assert_eq!(msg, "<html>nope</html>");
    }
}

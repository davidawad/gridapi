//! The one place that sends HTTP requests: retries, status mapping, body limits.

use std::time::Duration;

use reqwest::header::{HeaderMap, RETRY_AFTER};
use reqwest::StatusCode;

use crate::error::{Error, Result};
use crate::retry::RetryPolicy;

/// How much of an error body is kept for diagnostics.
const BODY_SNIPPET_BYTES: usize = 300;

/// A cheap-to-clone HTTP sender shared by every endpoint group.
#[derive(Debug, Clone)]
pub(crate) struct Transport {
    http: reqwest::Client,
    retry: RetryPolicy,
}

/// What to do after one attempt.
enum Outcome {
    Done(Vec<u8>),
    Retry(Error, Option<Duration>),
    Fail(Error),
}

impl Transport {
    pub(crate) fn new(http: reqwest::Client, retry: RetryPolicy) -> Self {
        Self { http, retry }
    }

    /// GET `url` with `query` and `headers`, retrying 429/5xx/connection errors with jittered backoff.
    pub(crate) async fn get(
        &self,
        url: &str,
        query: &[(String, String)],
        headers: &[(&'static str, String)],
    ) -> Result<Vec<u8>> {
        let attempts = self.retry.max_attempts.max(1);
        let mut attempt = 0;
        loop {
            match self.attempt(url, query, headers).await {
                Outcome::Done(body) => return Ok(body),
                Outcome::Fail(err) => return Err(err),
                Outcome::Retry(err, retry_after) => {
                    attempt += 1;
                    if attempt >= attempts {
                        return Err(err);
                    }
                    let delay = self.retry.delay(attempt - 1, retry_after);
                    tracing::debug!(url, attempt, ?delay, error = %err, "retrying request");
                    tokio::time::sleep(delay).await;
                }
            }
        }
    }

    async fn attempt(
        &self,
        url: &str,
        query: &[(String, String)],
        headers: &[(&'static str, String)],
    ) -> Outcome {
        let request = headers
            .iter()
            .fold(self.http.get(url).query(query), |req, (name, value)| {
                req.header(*name, value)
            });
        let response = match request.send().await {
            Ok(response) => response,
            Err(source) => return classify_transport(url, source),
        };
        let status = response.status();
        let retry_after = parse_retry_after(response.headers());
        let body = match response.bytes().await {
            Ok(bytes) => bytes.to_vec(),
            Err(source) => return classify_transport(url, source),
        };
        if status.is_success() {
            tracing::debug!(
                url,
                status = status.as_u16(),
                bytes = body.len(),
                "response"
            );
            return Outcome::Done(body);
        }
        let err = Error::Status {
            url: url.to_owned(),
            status: status.as_u16(),
            body: snippet(&body),
            retry_after,
        };
        if is_retryable_status(status) {
            Outcome::Retry(err, retry_after)
        } else {
            Outcome::Fail(err)
        }
    }
}

fn classify_transport(url: &str, source: reqwest::Error) -> Outcome {
    let err = Error::Transport {
        url: url.to_owned(),
        source,
    };
    if err.is_retryable() {
        Outcome::Retry(err, None)
    } else {
        Outcome::Fail(err)
    }
}

fn is_retryable_status(status: StatusCode) -> bool {
    status == StatusCode::TOO_MANY_REQUESTS || status.is_server_error()
}

fn parse_retry_after(headers: &HeaderMap) -> Option<Duration> {
    headers
        .get(RETRY_AFTER)?
        .to_str()
        .ok()?
        .trim()
        .parse::<u64>()
        .ok()
        .map(Duration::from_secs)
}

fn snippet(body: &[u8]) -> String {
    let cut = &body[..body.len().min(BODY_SNIPPET_BYTES)];
    String::from_utf8_lossy(cut).trim().to_owned()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn retryable_statuses() {
        assert!(is_retryable_status(StatusCode::TOO_MANY_REQUESTS));
        assert!(is_retryable_status(StatusCode::BAD_GATEWAY));
        assert!(!is_retryable_status(StatusCode::NOT_FOUND));
        assert!(!is_retryable_status(StatusCode::UNAUTHORIZED));
    }

    #[test]
    fn snippet_is_bounded_and_lossy() {
        let long = vec![b'x'; 5000];
        assert_eq!(snippet(&long).len(), BODY_SNIPPET_BYTES);
        assert_eq!(snippet(b"  hi \n"), "hi");
    }

    #[test]
    fn retry_after_seconds_only() {
        let mut h = HeaderMap::new();
        h.insert(RETRY_AFTER, "7".parse().expect("valid header"));
        assert_eq!(parse_retry_after(&h), Some(Duration::from_secs(7)));
        h.insert(
            RETRY_AFTER,
            "Wed, 21 Oct 2026 07:28:00 GMT"
                .parse()
                .expect("valid header"),
        );
        assert_eq!(parse_retry_after(&h), None);
    }
}

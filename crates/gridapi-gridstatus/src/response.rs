//! Response-header metadata, including quota headers when the server sends them.

use std::time::Duration;

use reqwest::header::HeaderMap;
use reqwest::StatusCode;

/// Rate-limit headers, parsed from the common `X-RateLimit-*` / `RateLimit-*`
/// spellings.
///
/// Observed 2026-11-18 on the free tier: the API sends none of these, so all
/// fields are `None` there. Use [`Client::usage`](crate::Client::usage) for
/// the authoritative request and row quota.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct RateLimitHeaders {
    /// Requests allowed in the window.
    pub limit: Option<u64>,
    /// Requests left in the window.
    pub remaining: Option<u64>,
    /// Seconds (or epoch, as the server defines it) until the window resets.
    pub reset: Option<u64>,
    /// `Retry-After` as a duration (seconds form only).
    pub retry_after: Option<Duration>,
}

impl RateLimitHeaders {
    /// True when the server sent no rate-limit information at all.
    pub fn is_empty(&self) -> bool {
        *self == Self::default()
    }
}

/// Metadata taken from the HTTP response, kept apart from the JSON body.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ResponseMeta {
    /// HTTP status.
    pub status: StatusCode,
    /// `x-request-id`, useful when reporting an issue.
    pub request_id: Option<String>,
    /// `x-rows-in-response`; omitted by the server on streamed JSON responses.
    pub rows_in_response: Option<u64>,
    /// `x-has-next-page`.
    pub has_next_page: Option<bool>,
    /// `x-page`.
    pub page: Option<u64>,
    /// `x-page-size`.
    pub page_size: Option<u64>,
    /// Quota headers, when present.
    pub rate_limit: RateLimitHeaders,
}

impl ResponseMeta {
    pub(crate) fn from_parts(status: StatusCode, headers: &HeaderMap) -> Self {
        let text = |name: &str| headers.get(name).and_then(|v| v.to_str().ok());
        let num = |name: &str| text(name).and_then(|v| v.trim().parse::<u64>().ok());
        let first_num = |names: &[&str]| names.iter().find_map(|n| num(n));
        Self {
            status,
            request_id: text("x-request-id").map(str::to_owned),
            rows_in_response: num("x-rows-in-response"),
            has_next_page: text("x-has-next-page").and_then(|v| match v.trim() {
                "true" | "True" => Some(true),
                "false" | "False" => Some(false),
                _ => None,
            }),
            page: num("x-page"),
            page_size: num("x-page-size"),
            rate_limit: RateLimitHeaders {
                limit: first_num(&["x-ratelimit-limit", "ratelimit-limit"]),
                remaining: first_num(&["x-ratelimit-remaining", "ratelimit-remaining"]),
                reset: first_num(&["x-ratelimit-reset", "ratelimit-reset"]),
                retry_after: retry_after(headers),
            },
        }
    }
}

/// Parse a `Retry-After` header in its delta-seconds form.
pub(crate) fn retry_after(headers: &HeaderMap) -> Option<Duration> {
    headers
        .get(reqwest::header::RETRY_AFTER)
        .and_then(|v| v.to_str().ok())
        .and_then(|v| v.trim().parse::<f64>().ok())
        .filter(|s| s.is_finite() && *s >= 0.0)
        .map(Duration::from_secs_f64)
}

#[cfg(test)]
mod tests {
    use reqwest::header::{HeaderName, HeaderValue};

    use super::*;

    fn headers(pairs: &[(&str, &str)]) -> HeaderMap {
        pairs
            .iter()
            .map(|(k, v)| {
                (
                    HeaderName::from_bytes(k.as_bytes()).expect("name"),
                    HeaderValue::from_str(v).expect("value"),
                )
            })
            .collect()
    }

    #[test]
    fn parses_real_observed_query_headers() {
        // Headers seen on a real query response, 2026-11-18.
        let h = headers(&[
            ("x-limit", "3"),
            ("x-page", "1"),
            ("x-page-size", "50000"),
            ("x-request-id", "7575a8d5-324c-4e4c"),
            ("x-rows-in-response", "549"),
            ("x-has-next-page", "false"),
        ]);
        let m = ResponseMeta::from_parts(StatusCode::OK, &h);
        assert_eq!(m.request_id.as_deref(), Some("7575a8d5-324c-4e4c"));
        assert_eq!(m.rows_in_response, Some(549));
        assert_eq!(m.has_next_page, Some(false));
        assert_eq!(m.page, Some(1));
        assert_eq!(m.page_size, Some(50_000));
        assert!(m.rate_limit.is_empty());
    }

    #[test]
    fn x_page_size_none_is_none() {
        let m = ResponseMeta::from_parts(StatusCode::OK, &headers(&[("x-page-size", "None")]));
        assert_eq!(m.page_size, None);
    }

    #[test]
    fn exposes_quota_headers_when_present() {
        let m = ResponseMeta::from_parts(
            StatusCode::OK,
            &headers(&[
                ("x-ratelimit-limit", "250"),
                ("x-ratelimit-remaining", "244"),
                ("x-ratelimit-reset", "3600"),
            ]),
        );
        assert_eq!(m.rate_limit.limit, Some(250));
        assert_eq!(m.rate_limit.remaining, Some(244));
        assert_eq!(m.rate_limit.reset, Some(3600));
    }

    #[test]
    fn retry_after_seconds() {
        let h = headers(&[("retry-after", "7")]);
        assert_eq!(retry_after(&h), Some(Duration::from_secs(7)));
        assert_eq!(retry_after(&headers(&[("retry-after", "soon")])), None);
    }
}

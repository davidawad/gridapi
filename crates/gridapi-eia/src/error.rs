//! Error type for the whole crate.

use std::fmt;

/// Result alias used throughout the crate.
pub type Result<T, E = Error> = std::result::Result<T, E>;

/// Everything that can go wrong talking to EIA or reading its bulk files.
///
/// No variant ever contains the API key: transport errors have their URL
/// stripped (EIA authenticates with a query parameter, so the URL is secret).
#[derive(Debug, thiserror::Error)]
#[non_exhaustive]
pub enum Error {
    /// No API key was configured and `EIA_API_KEY` is unset or empty.
    #[error("missing EIA API key: pass one to ClientBuilder::api_key or set EIA_API_KEY")]
    MissingApiKey,

    /// The request was rejected locally before any network call.
    #[error("invalid request: {0}")]
    InvalidRequest(String),

    /// Transport failure (DNS, TLS, connect, timeout, body read). URL stripped.
    #[error("transport error: {0}")]
    Http(String),

    /// EIA answered with a non-success status or an error envelope.
    #[error("EIA API error (HTTP {status}{}): {message}", code_suffix(code.as_deref()))]
    Api {
        /// HTTP status code.
        status: u16,
        /// EIA's machine code, for example `API_KEY_INVALID`, when present.
        code: Option<String>,
        /// Human-readable message from EIA.
        message: String,
    },

    /// The response was not the JSON shape this crate expects.
    #[error("could not decode EIA response: {0}")]
    Decode(String),

    /// Local filesystem failure while saving or reading a bulk file.
    #[error("io error: {0}")]
    Io(#[from] std::io::Error),

    /// A bulk file is not (yet) published at the URL tried. EIA answers
    /// unpublished months with a redirect or 404.
    #[error("bulk file not published: {0}")]
    NotPublished(String),

    /// A bulk file was fetched but could not be parsed.
    #[error("bulk file parse error: {0}")]
    Bulk(String),
}

fn code_suffix(code: Option<&str>) -> String {
    code.map(|c| format!(", {c}")).unwrap_or_default()
}

impl Error {
    /// Wrap a reqwest error, dropping its URL so the api key cannot leak.
    pub(crate) fn from_reqwest(e: reqwest::Error) -> Self {
        Self::Http(e.without_url().to_string())
    }

    pub(crate) fn decode(e: impl fmt::Display) -> Self {
        Self::Decode(e.to_string())
    }

    /// True for failures worth retrying: transport errors, 429 and 5xx.
    #[must_use]
    pub fn is_retryable(&self) -> bool {
        match self {
            Self::Http(_) => true,
            Self::Api { status, .. } => crate::retry::is_retryable_status(*status),
            _ => false,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn api_error_display_includes_code() {
        let e = Error::Api {
            status: 403,
            code: Some("API_KEY_INVALID".into()),
            message: "bad key".into(),
        };
        assert_eq!(
            e.to_string(),
            "EIA API error (HTTP 403, API_KEY_INVALID): bad key"
        );
    }

    #[test]
    fn retryable_classification() {
        let api = |status| Error::Api {
            status,
            code: None,
            message: String::new(),
        };
        assert!(api(429).is_retryable());
        assert!(api(503).is_retryable());
        assert!(!api(400).is_retryable());
        assert!(!api(403).is_retryable());
        assert!(Error::Http("timeout".into()).is_retryable());
        assert!(!Error::MissingApiKey.is_retryable());
    }
}

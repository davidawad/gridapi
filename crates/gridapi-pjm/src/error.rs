//! Error types for the PJM client.

use std::time::Duration;

use serde::Deserialize;

/// Boxed transport-level cause, so no third-party HTTP type leaks into the API.
pub type BoxError = Box<dyn std::error::Error + Send + Sync + 'static>;

/// One field-level validation message from a `400` response.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
pub struct FieldError {
    /// Query parameter the message is about (`StartRow`, `Filters`, ...).
    #[serde(default)]
    pub field: String,
    /// Human-readable message from PJM.
    #[serde(default)]
    pub message: String,
    /// Extra detail tokens (for example `datetime_filter`).
    #[serde(default)]
    pub detail: Vec<String>,
}

/// Everything that can go wrong talking to the PJM Data Miner 2 API.
#[derive(Debug, thiserror::Error)]
#[non_exhaustive]
pub enum Error {
    /// The request was rejected locally before any HTTP call was made.
    #[error("invalid query: {0}")]
    InvalidQuery(String),

    /// No subscription key was supplied: `KeyMode::Auto` found no
    /// `PJM_API_KEY`. Register your own key at <https://apiportal.pjm.com/>
    /// (non-members confirm internal business use with PJM when they register).
    #[error(
        "no PJM subscription key: set PJM_API_KEY or pass ClientBuilder::api_key; \
         register for a key at https://apiportal.pjm.com/"
    )]
    MissingKey,

    /// The subscription key is syntactically unusable (empty, non-ASCII, ...).
    #[error("invalid subscription key: {0}")]
    InvalidKey(&'static str),

    /// Connection, TLS, timeout or body-read failure, after retries.
    #[error("transport error: {0}")]
    Transport(#[source] BoxError),

    /// HTTP 401. For a user key it is returned immediately (with the
    /// `pjm-public-web-key` feature, a public web-app key is refetched once
    /// first).
    #[error("unauthorized (HTTP 401): the subscription key was rejected")]
    Unauthorized,

    /// HTTP 429 persisted through every retry.
    #[error("rate limited (HTTP 429), server retry-after: {retry_after:?}")]
    RateLimited {
        /// The `Retry-After` the server sent on the last attempt, if any.
        retry_after: Option<Duration>,
    },

    /// HTTP 5xx persisted through every retry.
    #[error("PJM server error (HTTP {status})")]
    Server {
        /// The status of the last attempt.
        status: u16,
    },

    /// HTTP 4xx other than 401/429. PJM sends its validation messages here.
    #[error("PJM rejected the request (HTTP {status}): {}", summarize(.errors, .code.as_deref(), .message.as_deref()))]
    Rejected {
        /// The HTTP status (normally 400).
        status: u16,
        /// Field-level messages (`{"errors": [...]}` bodies).
        errors: Vec<FieldError>,
        /// Error code (`API_1005`, ...) for the other 400 body shape.
        code: Option<String>,
        /// Free-form message for the other 400 body shape.
        message: Option<String>,
    },

    /// A non-success status this client has no better description for.
    #[error("unexpected HTTP status {status}: {snippet}")]
    Status {
        /// The HTTP status.
        status: u16,
        /// Start of the response body.
        snippet: String,
    },

    /// The body was not the JSON shape the typed structs expect.
    #[error("could not decode response: {source} (body starts: {snippet})")]
    Decode {
        /// The serde failure.
        #[source]
        source: serde_json::Error,
        /// Start of the response body.
        snippet: String,
    },

    /// The public web-app subscription key could not be obtained. Only with
    /// the `pjm-public-web-key` feature.
    #[cfg(feature = "pjm-public-web-key")]
    #[error("could not obtain the public subscription key: {0}")]
    PublicKey(String),

    /// `KeyMode::PublicWebApp` was selected without
    /// `ClientBuilder::i_accept_pjm_public_key_risk()`. Only with the
    /// `pjm-public-web-key` feature.
    #[cfg(feature = "pjm-public-web-key")]
    #[error(
        "the PJM public web-app key is not issued to you: call \
         ClientBuilder::i_accept_pjm_public_key_risk() to use it anyway, or register your own key at \
         https://apiportal.pjm.com/"
    )]
    PublicKeyNotAcknowledged,
}

impl Error {
    /// True for HTTP 401 (key rejected).
    #[must_use]
    pub fn is_unauthorized(&self) -> bool {
        matches!(self, Self::Unauthorized)
    }

    /// True when the request itself was refused by PJM's validation (4xx body).
    #[must_use]
    pub fn is_rejected(&self) -> bool {
        matches!(self, Self::Rejected { .. })
    }
}

/// One-line summary of a PJM rejection body, for `Display`.
fn summarize(errors: &[FieldError], code: Option<&str>, message: Option<&str>) -> String {
    let fields = errors
        .iter()
        .map(|e| format!("{}: {}", e.field, e.message))
        .collect::<Vec<_>>()
        .join("; ");
    [
        (!fields.is_empty()).then_some(fields),
        code.map(str::to_owned),
        message.map(str::to_owned),
    ]
    .into_iter()
    .flatten()
    .collect::<Vec<_>>()
    .join(" | ")
}

/// Best-effort copy of the start of a body for error messages.
pub(crate) fn snippet(body: &[u8]) -> String {
    const MAX: usize = 200;
    let text = String::from_utf8_lossy(body);
    text.chars().take(MAX).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rejected_display_lists_field_messages() {
        let err = Error::Rejected {
            status: 400,
            errors: vec![FieldError {
                field: "StartRow".into(),
                message: "StartRow is missing.".into(),
                detail: vec![],
            }],
            code: None,
            message: None,
        };
        assert_eq!(
            err.to_string(),
            "PJM rejected the request (HTTP 400): StartRow: StartRow is missing."
        );
        assert!(err.is_rejected());
    }

    #[test]
    fn rejected_display_uses_code_and_message_shape() {
        let err = Error::Rejected {
            status: 400,
            errors: vec![],
            code: Some("API_1005".into()),
            message: Some("Search specification validation failed.".into()),
        };
        assert!(err.to_string().contains("API_1005 | Search specification"));
    }

    #[test]
    fn snippet_truncates() {
        assert_eq!(snippet(&[b'a'; 500]).len(), 200);
    }
}

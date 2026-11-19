//! The error type returned by every fallible operation in this crate.

use std::time::Duration;

/// OASIS error code meaning "no data returned for the specified selection".
pub const OASIS_NO_DATA: u32 = 1000;

/// OASIS error code meaning the requested period exceeds the 31-day maximum.
pub const OASIS_RANGE_TOO_LONG: u32 = 1004;

/// Everything that can go wrong while talking to CAISO or decoding its answers.
#[derive(Debug, thiserror::Error)]
#[non_exhaustive]
pub enum Error {
    /// The request never produced an HTTP response (DNS, connect, TLS, timeout).
    #[error("transport error: {message}")]
    Transport {
        /// Human-readable description of the failure.
        message: String,
        /// True when the failure was a connect or request timeout.
        timeout: bool,
    },

    /// The server answered with a non-success HTTP status.
    #[error("unexpected http status {status}: {body}")]
    Status {
        /// The HTTP status code.
        status: u16,
        /// A short, lossy prefix of the response body.
        body: String,
        /// The server's `Retry-After` hint, when it sent one.
        retry_after: Option<Duration>,
    },

    /// OASIS answered `200` but the zip held an error document instead of data.
    #[error("oasis error {code}: {description}")]
    Oasis {
        /// The numeric `ERR_CODE` (1000 = no data, 1004 = range over 31 days).
        code: u32,
        /// The `ERR_DESC` text.
        description: String,
    },

    /// The response body exceeded the configured size cap.
    #[error("response exceeds the {limit} byte limit")]
    ResponseTooLarge {
        /// The configured cap in bytes.
        limit: u64,
    },

    /// The body was neither a zip archive nor an OASIS XML document.
    #[error("unexpected response body: {0}")]
    UnexpectedBody(String),

    /// The zip archive was malformed, empty, or larger than the safety cap.
    #[error("invalid zip archive: {0}")]
    Zip(String),

    /// A CSV document could not be tokenised.
    #[error("invalid csv: {0}")]
    Csv(String),

    /// An XML document could not be parsed.
    #[error("invalid xml: {0}")]
    Xml(String),

    /// A typed row needed a column the response did not contain.
    #[error("record is missing field {field}")]
    MissingField {
        /// The column name(s) that were looked up, `|`-separated.
        field: String,
    },

    /// A column held a value that does not parse as the expected type.
    #[error("invalid value {value:?} for field {field}")]
    InvalidField {
        /// The column name(s) that were looked up.
        field: String,
        /// The offending raw text.
        value: String,
    },

    /// The query failed local validation, so no request was sent.
    #[error("invalid query: {0}")]
    InvalidQuery(String),

    /// The client could not be built from the supplied configuration.
    #[error("invalid client configuration: {0}")]
    Config(String),
}

impl Error {
    /// True when OASIS reported "no data returned for the specified selection".
    #[must_use]
    pub fn is_no_data(&self) -> bool {
        matches!(self, Self::Oasis { code, .. } if *code == OASIS_NO_DATA)
    }

    /// True when retrying the identical request may succeed.
    ///
    /// Covers transport failures, HTTP 429 and 5xx, and non-zip bodies such as a
    /// proxy error page. OASIS error documents are never retried.
    #[must_use]
    pub fn is_retryable(&self) -> bool {
        match self {
            Self::Transport { .. } | Self::UnexpectedBody(_) => true,
            Self::Status { status, .. } => *status == 429 || *status >= 500,
            _ => false,
        }
    }

    /// The server-suggested wait before the next attempt, if any.
    #[must_use]
    pub fn retry_after(&self) -> Option<Duration> {
        match self {
            Self::Status { retry_after, .. } => *retry_after,
            _ => None,
        }
    }
}

impl From<reqwest::Error> for Error {
    fn from(err: reqwest::Error) -> Self {
        Self::Transport {
            timeout: err.is_timeout(),
            message: err.without_url().to_string(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn status(code: u16) -> Error {
        Error::Status {
            status: code,
            body: String::new(),
            retry_after: None,
        }
    }

    #[test]
    fn retryable_classification() {
        assert!(status(429).is_retryable());
        assert!(status(503).is_retryable());
        assert!(!status(404).is_retryable());
        assert!(Error::UnexpectedBody("<html>".into()).is_retryable());
        let oasis = Error::Oasis {
            code: OASIS_NO_DATA,
            description: "none".into(),
        };
        assert!(!oasis.is_retryable());
        assert!(oasis.is_no_data());
    }
}

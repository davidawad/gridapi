//! Error type for every fallible operation in the crate.

use std::time::Duration;

/// Convenience alias used across the crate.
pub type Result<T, E = Error> = std::result::Result<T, E>;

/// Opaque transport-layer failure (DNS, TLS, connect, timeout, redirect loop).
///
/// Wraps the underlying HTTP stack error without exposing its type.
#[derive(Debug)]
pub struct TransportError {
    source: reqwest::Error,
}

impl TransportError {
    pub(crate) fn new(source: reqwest::Error) -> Self {
        Self { source }
    }

    /// True when the request timed out.
    #[must_use]
    pub fn is_timeout(&self) -> bool {
        self.source.is_timeout()
    }

    /// True when the connection could not be established.
    #[must_use]
    pub fn is_connect(&self) -> bool {
        self.source.is_connect()
    }

    /// True when the redirect chain looped or exceeded the limit.
    #[must_use]
    pub fn is_redirect(&self) -> bool {
        self.source.is_redirect()
    }
}

impl std::fmt::Display for TransportError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        // Walk the chain: reqwest's own Display often hides the root cause.
        write!(f, "{}", self.source)?;
        let mut cause = std::error::Error::source(&self.source);
        while let Some(c) = cause {
            write!(f, ": {c}")?;
            cause = c.source();
        }
        Ok(())
    }
}

impl std::error::Error for TransportError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        Some(&self.source)
    }
}

/// Everything that can go wrong talking to the ERCOT Public API.
#[derive(Debug, thiserror::Error)]
#[non_exhaustive]
pub enum Error {
    /// The client configuration or a credential was invalid or missing.
    #[error("invalid configuration: {0}")]
    Config(String),

    /// The HTTP request never produced a response.
    #[error("transport error: {0}")]
    Transport(#[source] TransportError),

    /// The Azure B2C token endpoint refused the credentials or the refresh token.
    #[error("authentication failed (HTTP {status}): {message}")]
    Auth {
        /// HTTP status of the token endpoint response.
        status: u16,
        /// `error_description` (or body excerpt) from the token endpoint.
        message: String,
    },

    /// The API answered with a non-success status.
    #[error("ERCOT API returned HTTP {status}: {message}")]
    Api {
        /// HTTP status code.
        status: u16,
        /// `message` from the JSON error body, or a body excerpt.
        message: String,
        /// Query parameters the API rejected as unknown for the report (HTTP 400).
        unknown_params: Vec<String>,
        /// Server-requested delay from a `Retry-After` header, when present.
        retry_after: Option<Duration>,
    },

    /// An HTML page (the CDN bot check) was returned where JSON or a file was expected.
    #[error("bot-check page returned instead of data: {snippet}")]
    BotCheck {
        /// First bytes of the HTML body.
        snippet: String,
    },

    /// A response body could not be decoded into the expected shape.
    #[error("could not decode {what}: {detail}")]
    Decode {
        /// What was being decoded.
        what: &'static str,
        /// Parser message.
        detail: String,
    },

    /// A report row did not contain the expected column or value.
    #[error("report row field `{field}`: {detail}")]
    Row {
        /// Column name from the report's `fields` metadata.
        field: String,
        /// What was wrong with it.
        detail: String,
    },

    /// A multi-page fetch would exceed the caller's page budget.
    #[error("query spans {total_pages} pages, over the limit of {limit}")]
    TooManyPages {
        /// The `max_pages` the caller allowed.
        limit: u32,
        /// Pages the query actually spans.
        total_pages: u32,
    },

    /// An archive (zip) could not be read.
    #[error("archive error: {0}")]
    Archive(String),
}

impl Error {
    /// True when retrying the same request later may succeed.
    #[must_use]
    pub fn is_retryable(&self) -> bool {
        match self {
            Error::Transport(t) => t.is_timeout() || t.is_connect() || t.is_redirect(),
            Error::Api { status, .. } => *status == 429 || (500..=599).contains(status),
            Error::BotCheck { .. } => true,
            _ => false,
        }
    }

    /// True when the API throttled the request (HTTP 429).
    #[must_use]
    pub fn is_rate_limited(&self) -> bool {
        matches!(self, Error::Api { status: 429, .. })
    }

    /// Server-requested delay before the next attempt, if any.
    #[must_use]
    pub fn retry_after(&self) -> Option<Duration> {
        match self {
            Error::Api { retry_after, .. } => *retry_after,
            _ => None,
        }
    }

    pub(crate) fn row(field: &str, detail: impl Into<String>) -> Self {
        Error::Row {
            field: field.to_owned(),
            detail: detail.into(),
        }
    }

    pub(crate) fn decode(what: &'static str, detail: impl ToString) -> Self {
        Error::Decode {
            what,
            detail: detail.to_string(),
        }
    }
}

impl From<reqwest::Error> for Error {
    fn from(source: reqwest::Error) -> Self {
        Error::Transport(TransportError::new(source))
    }
}

const _: () = {
    const fn assert_send_sync<T: Send + Sync + 'static>() {}
    assert_send_sync::<Error>();
};

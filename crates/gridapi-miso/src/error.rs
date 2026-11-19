//! The crate error type.

use std::time::Duration;

/// Everything that can go wrong talking to MISO.
#[derive(Debug, thiserror::Error)]
#[non_exhaustive]
pub enum Error {
    /// The request could not be sent or the connection failed.
    #[error("request to {url} failed: {source}")]
    Transport {
        /// The URL that was requested.
        url: String,
        /// The underlying transport error.
        #[source]
        source: reqwest::Error,
    },
    /// The server answered with a non-success status.
    #[error("{url} answered HTTP {status}: {body}")]
    Status {
        /// The URL that was requested.
        url: String,
        /// The HTTP status code.
        status: u16,
        /// The first part of the response body, for diagnosis.
        body: String,
        /// The `Retry-After` the server asked for, when it sent one.
        retry_after: Option<Duration>,
    },
    /// The body arrived but did not have the expected shape.
    #[error("could not decode {what}: {message}")]
    Decode {
        /// What was being decoded (an endpoint or file name).
        what: String,
        /// Why decoding failed.
        message: String,
    },
    /// The service answered `{"error": "no data"}`; MISO retired that feed.
    #[error("{url} returned no data (feed retired or not yet published)")]
    NoData {
        /// The URL that was requested.
        url: String,
    },
    /// A keyed (Data Exchange) call was made without an API key.
    #[cfg(feature = "miso-dataexchange")]
    #[error(
        "no MISO Data Exchange API key configured (set MISO_API_KEY or use ClientBuilder::api_key)"
    )]
    MissingApiKey,
    /// A builder was given an unusable value.
    #[error("invalid configuration: {0}")]
    Config(String),
}

impl Error {
    #[expect(
        clippy::needless_pass_by_value,
        reason = "callers pass temporaries such as format!() results"
    )]
    pub(crate) fn decode(what: impl Into<String>, message: impl ToString) -> Self {
        Self::Decode {
            what: what.into(),
            message: message.to_string(),
        }
    }

    /// True when retrying the same request later could succeed.
    pub fn is_retryable(&self) -> bool {
        match self {
            Self::Transport { source, .. } => {
                source.is_timeout() || source.is_connect() || source.is_request()
            }
            Self::Status { status, .. } => *status == 429 || (500..600).contains(status),
            _ => false,
        }
    }
}

/// Crate result alias.
pub type Result<T, E = Error> = std::result::Result<T, E>;

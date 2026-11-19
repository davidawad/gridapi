//! The crate error type.

use std::time::Duration;

/// Everything that can go wrong talking to the SPP portal.
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
    ///
    /// For file downloads a 404 means the file does not exist (not published yet, or aged out).
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
        /// What was being decoded (a file or endpoint name).
        what: String,
        /// Why decoding failed.
        message: String,
    },
    /// A folder that should hold files was empty.
    #[error("nothing found under {fs_name}:{path}")]
    Empty {
        /// The file source that was searched.
        fs_name: String,
        /// The folder that held nothing usable.
        path: String,
    },
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

    /// True when the server said the requested file or folder does not exist.
    pub fn is_not_found(&self) -> bool {
        matches!(self, Self::Status { status: 404, .. })
    }
}

/// Crate result alias.
pub type Result<T, E = Error> = std::result::Result<T, E>;

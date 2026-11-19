//! Error type for the client.

use std::time::Duration;

use reqwest::StatusCode;

/// Everything that can go wrong talking to the Grid Status API.
#[derive(Debug, thiserror::Error)]
#[non_exhaustive]
pub enum Error {
    /// No API key was configured and `GRIDSTATUS_API_KEY` is unset or empty.
    #[error("no api key: set GRIDSTATUS_API_KEY or call ClientBuilder::api_key")]
    MissingApiKey,

    /// The request was rejected locally before any HTTP call was made.
    #[error("invalid request: {0}")]
    InvalidRequest(String),

    /// The client's configured request budget is spent; nothing was sent.
    #[error("request budget of {budget} exhausted")]
    BudgetExhausted {
        /// The budget configured with `ClientBuilder::request_budget`.
        budget: u64,
    },

    /// Transport-level failure (DNS, TLS, connect, timeout, body read).
    #[error("http transport error: {0}")]
    Transport(#[from] reqwest::Error),

    /// The server answered with a non-success status after retries.
    #[error("api error {status}: {message}")]
    Api {
        /// HTTP status of the final attempt.
        status: StatusCode,
        /// The `detail` field of the error body when present, else the raw body (truncated).
        message: String,
        /// `Retry-After` from the final response when the server sent one.
        retry_after: Option<Duration>,
    },

    /// The body was not the JSON shape this client expects.
    #[error("could not decode response: {source}; body starts: {snippet}")]
    Decode {
        /// The serde error.
        #[source]
        source: serde_json::Error,
        /// First bytes of the body, for diagnosis. Never contains the API key.
        snippet: String,
    },
}

impl Error {
    /// True when retrying later could succeed (rate limit, 5xx, transport).
    pub fn is_retryable(&self) -> bool {
        match self {
            Self::Transport(e) => e.is_timeout() || e.is_connect() || e.is_request() || e.is_body(),
            Self::Api { status, .. } => is_retryable_status(*status),
            _ => false,
        }
    }

    /// True for HTTP 429.
    pub fn is_rate_limited(&self) -> bool {
        matches!(self, Self::Api { status, .. } if *status == StatusCode::TOO_MANY_REQUESTS)
    }

    /// True for HTTP 401 or 403 (bad or missing key, or dataset not on the plan).
    pub fn is_auth(&self) -> bool {
        matches!(
            self,
            Self::Api { status, .. }
                if *status == StatusCode::UNAUTHORIZED || *status == StatusCode::FORBIDDEN
        )
    }
}

/// Statuses worth retrying with backoff: 429 and the transient 5xx.
pub(crate) fn is_retryable_status(status: StatusCode) -> bool {
    status == StatusCode::TOO_MANY_REQUESTS || matches!(status.as_u16(), 500 | 502 | 503 | 504)
}

/// Result alias used across the crate.
pub type Result<T, E = Error> = std::result::Result<T, E>;

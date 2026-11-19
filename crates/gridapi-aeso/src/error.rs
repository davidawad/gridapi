//! Error type shared by the whole crate.

/// Everything that can go wrong talking to, or decoding, the AESO API.
#[derive(Debug, thiserror::Error)]
#[non_exhaustive]
pub enum Error {
    /// No API key was supplied. Register at <https://developer-apim.aeso.ca/>,
    /// subscribe to the "AESO Public API" product, and pass the key to
    /// [`crate::ClientBuilder::api_key`] (or set `AESO_API_KEY` for
    /// [`crate::Client::from_env`]).
    #[error("no AESO API key supplied (set it with ClientBuilder::api_key or AESO_API_KEY)")]
    MissingApiKey,

    /// The key contains characters that cannot go in an HTTP header.
    #[error("the AESO API key is not a valid HTTP header value")]
    InvalidApiKey,

    /// A query was rejected before any request was made.
    #[error("invalid query: {0}")]
    InvalidQuery(String),

    /// The request never produced a response (DNS, TLS, connect, timeout, body read).
    #[error("transport error for {url}: {source}")]
    Transport {
        /// The URL that was requested (never contains the key).
        url: String,
        /// The underlying reqwest error.
        #[source]
        source: reqwest::Error,
    },

    /// The API rejected the request with a JSON error body
    /// (`{"message": "...", "details": "uri=..."}`), usually HTTP 400.
    #[error("AESO API error (HTTP {status}): {message}")]
    Api {
        /// The HTTP status code.
        status: u16,
        /// The `message` field, for example `No Data available for this day.`.
        message: String,
        /// The `details` field, usually `uri=/ets-api/...`.
        details: Option<String>,
    },

    /// HTTP 404. The gateway answers 404 both for an unknown route and for a
    /// missing or invalid `API-KEY`, so check the key first.
    #[error(
        "not found: {url} (the AESO gateway also answers 404 for an invalid or missing API-KEY)"
    )]
    NotFound {
        /// The URL that was requested.
        url: String,
    },

    /// Any other non-success status.
    #[error("{url} returned HTTP {status}")]
    Status {
        /// The URL that was requested.
        url: String,
        /// The HTTP status code.
        status: u16,
        /// The first part of the response body, for diagnostics.
        body: String,
    },

    /// The response body was not the JSON shape we expected.
    #[error("could not decode the response from {endpoint}: {source}")]
    Decode {
        /// The endpoint path that was called.
        endpoint: String,
        /// The serde error.
        #[source]
        source: serde_json::Error,
    },

    /// The HTTP client could not be built.
    #[error("could not build HTTP client: {0}")]
    ClientBuild(#[source] reqwest::Error),
}

impl Error {
    /// True when the API said the requested day has no data (for example the
    /// merit order inside its 60-day lag, or a day before the data begins).
    pub fn is_no_data(&self) -> bool {
        matches!(self, Self::Api { message, .. } if message.to_ascii_lowercase().contains("no data"))
    }
}

/// Crate-wide result alias.
pub type Result<T, E = Error> = std::result::Result<T, E>;

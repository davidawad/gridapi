//! Error type shared by the whole crate.

/// Everything that can go wrong talking to, or decoding, the IESO reports site.
#[derive(Debug, thiserror::Error)]
#[non_exhaustive]
pub enum Error {
    /// The request never produced a response (DNS, TLS, connect, timeout, body read).
    #[error("transport error for {url}: {source}")]
    Transport {
        /// The URL that was requested.
        url: String,
        /// The underlying reqwest error.
        #[source]
        source: reqwest::Error,
    },

    /// The server answered with a non-success status that is not `404`.
    #[error("{url} returned HTTP {status}")]
    Status {
        /// The URL that was requested.
        url: String,
        /// The HTTP status code.
        status: u16,
        /// The first part of the response body, for diagnostics.
        body: String,
    },

    /// The file does not exist (yet). IESO publishes files on a schedule, so
    /// a `404` for a future or very recent period is normal.
    #[error("report file not found: {url}")]
    NotFound {
        /// The URL that was requested.
        url: String,
    },

    /// The directory listing held no file matching the requested report/stamp.
    #[error("no published version of {report} for stamp {stamp}")]
    NoVersions {
        /// The report name.
        report: String,
        /// The stamp that was asked for (`latest` when none).
        stamp: String,
    },

    /// The XML document could not be decoded into the expected shape.
    #[error("XML decode error: {0}")]
    Xml(#[from] quick_xml::DeError),

    /// The CSV document could not be decoded into the expected shape.
    #[error("CSV decode error: {0}")]
    Csv(#[from] csv::Error),

    /// The document decoded but its content breaks an invariant (bad header
    /// line, unparsable timestamp, missing required section).
    #[error("malformed report: {0}")]
    Malformed(String),

    /// The HTTP client could not be built.
    #[error("could not build HTTP client: {0}")]
    ClientBuild(#[source] reqwest::Error),
}

/// Crate-wide result alias.
pub type Result<T, E = Error> = std::result::Result<T, E>;

//! The crate's error type.

use std::time::Duration;

use chrono::{NaiveDate, NaiveDateTime};

/// Boxed foreign error, so no third-party error type appears in the public API.
pub type Source = Box<dyn std::error::Error + Send + Sync + 'static>;

/// Everything that can go wrong while fetching or decoding NYISO data.
#[derive(Debug, thiserror::Error)]
#[non_exhaustive]
pub enum Error {
    /// The request never produced an HTTP response (DNS, connect, TLS, timeout, body read).
    #[error("request to {url} failed: {source}")]
    Transport {
        /// The requested URL.
        url: String,
        /// The underlying cause.
        #[source]
        source: Source,
    },

    /// The server answered with a non-success status other than 404.
    #[error("{url} returned HTTP {status}")]
    Status {
        /// The requested URL.
        url: String,
        /// The HTTP status code.
        status: u16,
        /// The server's `Retry-After` hint, when it sent one in seconds.
        retry_after: Option<Duration>,
    },

    /// The file does not exist on the server (HTTP 404).
    #[error("{url} not found")]
    NotFound {
        /// The requested URL.
        url: String,
    },

    /// The HTTP client could not be built (bad TLS setup, bad user agent).
    #[error("could not build the http client: {0}")]
    ClientBuild(#[source] Source),

    /// The configured base URL is not an absolute http(s) URL.
    #[error("invalid base url {0:?}")]
    InvalidBaseUrl(String),

    /// A CSV record could not be decoded into the expected shape.
    #[error("{dataset}: csv row {row}: {source}")]
    Csv {
        /// Which dataset was being decoded.
        dataset: String,
        /// One-based row number in the file, counting the header as row 1.
        row: usize,
        /// The underlying cause.
        #[source]
        source: Source,
    },

    /// A required column is absent from the file's header.
    #[error("{dataset}: missing column {column:?} (header: {header})")]
    MissingColumn {
        /// Which dataset was being decoded.
        dataset: String,
        /// The column that was expected.
        column: String,
        /// The header row as published.
        header: String,
    },

    /// A cell could not be read as the type its column requires.
    #[error("{dataset}: row {row}, column {column:?}: cannot read {value:?}")]
    BadCell {
        /// Which dataset was being decoded.
        dataset: String,
        /// One-based row number in the file, counting the header as row 1.
        row: usize,
        /// The column the cell belongs to.
        column: String,
        /// The offending cell.
        value: String,
    },

    /// The file uses a column layout this crate does not decode (NYISO changed
    /// the ancillary-service layout twice before 2017).
    #[error("{dataset}: unsupported legacy layout (header: {header})")]
    UnsupportedLayout {
        /// Which dataset was being decoded.
        dataset: String,
        /// The header row as published.
        header: String,
    },

    /// A monthly archive could not be opened or read.
    #[error("archive {archive}: {source}")]
    Archive {
        /// The archive URL.
        archive: String,
        /// The underlying cause.
        #[source]
        source: Source,
    },

    /// A date/time cell did not match any published format.
    #[error("{dataset}: cannot parse timestamp {value:?}")]
    Timestamp {
        /// Which dataset was being decoded.
        dataset: String,
        /// The offending cell.
        value: String,
    },

    /// The local wall-clock time falls in the spring-forward gap and never happened.
    #[error("local time {0} does not exist in America/New_York")]
    NonexistentLocalTime(NaiveDateTime),

    /// A `Time Zone` cell held something other than `EST` or `EDT`.
    #[error("unknown time zone abbreviation {0:?} (expected EST or EDT)")]
    UnknownZone(String),

    /// The requested range ends before it starts.
    #[error("empty date range {start}..={end}")]
    EmptyRange {
        /// First requested day.
        start: NaiveDate,
        /// Last requested day.
        end: NaiveDate,
    },

    /// A strict fetch found days with no published file.
    #[error("no data published for {} day(s), first missing {first}", .count)]
    MissingDays {
        /// How many days had no file.
        count: usize,
        /// The earliest of them.
        first: NaiveDate,
    },
}

impl Error {
    /// Whether retrying the same request later may succeed: transport failures,
    /// HTTP 429 and HTTP 5xx.
    #[must_use]
    pub fn is_retryable(&self) -> bool {
        match self {
            Self::Transport { .. } => true,
            Self::Status { status, .. } => *status == 429 || (500..600).contains(status),
            _ => false,
        }
    }

    /// The server's `Retry-After` hint, if the error carried one.
    #[must_use]
    pub fn retry_after(&self) -> Option<Duration> {
        match self {
            Self::Status { retry_after, .. } => *retry_after,
            _ => None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn status(code: u16) -> Error {
        Error::Status {
            url: "u".into(),
            status: code,
            retry_after: None,
        }
    }

    #[test]
    fn retryable_statuses() {
        assert!(status(429).is_retryable(), "429 is a rate limit");
        assert!(status(500).is_retryable(), "500 is transient");
        assert!(status(503).is_retryable(), "503 is transient");
        assert!(!status(400).is_retryable(), "400 is the caller's fault");
        assert!(!status(403).is_retryable(), "403 will not change");
        assert!(
            !Error::NotFound { url: "u".into() }.is_retryable(),
            "404 means the file is not there"
        );
    }

    #[test]
    fn retry_after_is_exposed() {
        let e = Error::Status {
            url: "u".into(),
            status: 429,
            retry_after: Some(Duration::from_secs(7)),
        };
        assert_eq!(e.retry_after(), Some(Duration::from_secs(7)));
        assert_eq!(status(500).retry_after(), None);
    }
}

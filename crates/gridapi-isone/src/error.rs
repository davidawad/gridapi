//! The crate's error type.

use std::time::Duration;

use chrono::NaiveDateTime;

/// Boxed foreign error, so no third-party error type appears in the public API.
pub type Source = Box<dyn std::error::Error + Send + Sync + 'static>;

/// Everything that can go wrong while fetching or decoding ISO New England data.
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

    /// The server answered with a non-success status other than 401 and 404.
    #[error("{url} returned HTTP {status}")]
    Status {
        /// The requested URL.
        url: String,
        /// The HTTP status code.
        status: u16,
        /// The server's `Retry-After` hint, when it sent one in seconds.
        retry_after: Option<Duration>,
    },

    /// There is nothing at that address: HTTP 404, or ISO Express's
    /// `No data exists for this period.` body.
    #[error("{url} not found")]
    NotFound {
        /// The requested URL.
        url: String,
    },

    /// The Web Services API rejected the credentials (HTTP 401).
    #[error("{url} rejected the credentials (HTTP 401); check ISONE_USERNAME and ISONE_PASSWORD")]
    Unauthorized {
        /// The requested URL.
        url: String,
    },

    /// ISO Express refused the session even after a fresh token was fetched.
    #[error("{url} refused the ISO Express session token (HTTP 403)")]
    SessionRejected {
        /// The requested URL.
        url: String,
    },

    /// A required environment variable is unset or empty.
    #[error("environment variable {variable} is not set")]
    MissingCredentials {
        /// The variable's name.
        variable: String,
    },

    /// The HTTP client could not be built (bad TLS setup, bad user agent).
    #[error("could not build the http client: {0}")]
    ClientBuild(#[source] Source),

    /// A configured base URL is not an absolute http(s) URL.
    #[error("invalid base url {0:?}")]
    InvalidBaseUrl(String),

    /// A CSV record could not be decoded into the expected shape.
    #[error("{report}: csv row {row}: {source}")]
    Csv {
        /// Which report was being decoded.
        report: String,
        /// One-based line number in the file.
        row: usize,
        /// The underlying cause.
        #[source]
        source: Source,
    },

    /// A required column is absent from the report's header.
    #[error("{report}: missing column {column:?} (header: {header})")]
    MissingColumn {
        /// Which report was being decoded.
        report: String,
        /// The column that was expected.
        column: String,
        /// The header row as published.
        header: String,
    },

    /// A cell could not be read as the type its column requires.
    #[error("{report}: line {row}, column {column:?}: cannot read {value:?}")]
    BadCell {
        /// Which report was being decoded.
        report: String,
        /// One-based line number in the file.
        row: usize,
        /// The column the cell belongs to.
        column: String,
        /// The offending cell.
        value: String,
    },

    /// The report's trailer line disagrees with the rows received, so the
    /// download was cut short or corrupted.
    #[error("{report}: trailer promises {expected} rows but {found} arrived")]
    Truncated {
        /// Which report was being decoded.
        report: String,
        /// Row count from the trailer line.
        expected: usize,
        /// Data rows actually present.
        found: usize,
    },

    /// A date/time cell did not match any published format.
    #[error("{report}: cannot parse timestamp {value:?}")]
    Timestamp {
        /// Which report was being decoded.
        report: String,
        /// The offending cell.
        value: String,
    },

    /// An `Hour Ending` cell is not `01`..`24` with an optional `X` suffix, or
    /// the `X` does not match a repeated hour.
    #[error("invalid hour ending {value:?} on {day}")]
    HourEnding {
        /// The offending cell.
        value: String,
        /// The report day.
        day: chrono::NaiveDate,
    },

    /// The local wall-clock time falls in the spring-forward gap and never happened.
    #[error("local time {0} does not exist in America/New_York")]
    NonexistentLocalTime(NaiveDateTime),

    /// The Web Services response was not the JSON shape the API documents.
    #[error("{url}: unexpected response: {detail}")]
    UnexpectedShape {
        /// The requested URL.
        url: String,
        /// What was wrong.
        detail: String,
    },

    /// The Web Services response body was not valid JSON for the expected type.
    #[error("{url}: invalid json: {source}")]
    Json {
        /// The requested URL.
        url: String,
        /// The underlying cause.
        #[source]
        source: Source,
    },

    /// The requested range ends before it starts.
    #[error("empty date range {start}..={end}")]
    EmptyRange {
        /// First requested day.
        start: chrono::NaiveDate,
        /// Last requested day.
        end: chrono::NaiveDate,
    },

    /// A strict fetch found days with no published report.
    #[error("{report}: no data published for {count} day(s), first missing {first}")]
    MissingDays {
        /// Which report was requested.
        report: String,
        /// How many days had no report.
        count: usize,
        /// The earliest of them.
        first: chrono::NaiveDate,
    },

    /// ISO-NE has no report or endpoint for the requested combination.
    #[error("unsupported query: {0}")]
    UnsupportedQuery(String),
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
        assert!(status(502).is_retryable(), "502 is transient");
        assert!(
            !status(403).is_retryable(),
            "403 will not change by waiting"
        );
        assert!(
            !Error::Unauthorized { url: "u".into() }.is_retryable(),
            "bad credentials stay bad"
        );
        assert!(!Error::NotFound { url: "u".into() }.is_retryable());
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

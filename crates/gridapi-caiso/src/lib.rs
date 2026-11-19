//! Async Rust client for CAISO's public data: the OASIS `SingleZip` API and the
//! "Today's Outlook" CSV feeds. No API key is needed.
//!
//! OASIS quirks this crate handles for you:
//!
//! - Responses are zip files holding CSV (`resultformat=6`) or XML.
//! - Failures arrive as **HTTP 200** with an XML error document inside the zip
//!   (no data, range over 31 days, ...); they surface as [`Error::Oasis`].
//! - One request per five seconds is allowed; each [`Client`] paces itself
//!   (no global lock) and retries 429/5xx and timeouts with jittered backoff.
//! - A window over 31 days is rejected, so [`Client::fetch_all`] and every typed
//!   helper split long ranges into contiguous requests.
//! - Asking for every ancillary service/region redirects to a whole-day group
//!   file, which [`Client::ancillary_prices`] iterates day by day.
//!
//! # Examples
//!
//! ```no_run
//! use chrono::{TimeZone, Utc};
//! use gridapi_caiso::{pivot_lmp, Client, TradingHub};
//!
//! # async fn run() -> Result<(), gridapi_caiso::Error> {
//! let client = Client::builder()
//!     .user_agent("my-app/1.0 (me@example.com)")
//!     .build()?;
//! let start = Utc.with_ymd_and_hms(2026, 11, 10, 8, 0, 0).single().unwrap_or_default();
//! let end = Utc.with_ymd_and_hms(2026, 11, 11, 8, 0, 0).single().unwrap_or_default();
//! let rows = client
//!     .day_ahead_lmp(&TradingHub::all_node_ids(), start, end)
//!     .await?;
//! for point in pivot_lmp(rows) {
//!     println!("{} {} {:?}", point.interval_start, point.node, point.lmp);
//! }
//! # Ok(())
//! # }
//! ```

// Unit tests return `Result` so `?` can propagate, and assert inside them.
#![cfg_attr(test, allow(clippy::panic_in_result_fn, clippy::assert_is_empty))]

mod ancillary;
mod client;
mod error;
mod hub;
mod lmp;
mod load;
mod node;
mod outlook;
mod pacer;
mod payload;
mod query;
mod record;
mod retry;
mod time;

pub use ancillary::{AncillaryPriceRow, AncillaryType};
pub use client::{
    Client, ClientBuilder, DEFAULT_CHUNK_DAYS, DEFAULT_CONNECT_TIMEOUT, DEFAULT_MAX_RESPONSE_BYTES,
    DEFAULT_MIN_INTERVAL, DEFAULT_OASIS_BASE_URL, DEFAULT_OUTLOOK_BASE_URL,
    DEFAULT_REQUEST_TIMEOUT,
};
pub use error::{Error, OASIS_NO_DATA, OASIS_RANGE_TOO_LONG};
pub use hub::TradingHub;
pub use lmp::{pivot_lmp, LmpComponent, LmpPoint, LmpRow};
pub use load::{LoadForecastRow, SystemScheduleRow};
pub use node::NodeRow;
pub use outlook::{parse_outlook_csv, OutlookDataset, OutlookPoint, OutlookSeries};
pub use pacer::Pacer;
pub use payload::{decode_response, oasis_error, unzip, ArchiveEntry, MAX_UNZIPPED_BYTES};
pub use query::{MarketRun, Query, QueryBuilder, QueryName, ResultFormat};
pub use record::{parse_csv, parse_csv_with_header, parse_xml, FromRecord, Record};
pub use retry::RetryPolicy;
pub use time::{chunk_range, format_oasis_time, OASIS_MAX_SPAN_DAYS, OASIS_TIME_FORMAT};

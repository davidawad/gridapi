//! Async client for the [PJM Data Miner 2 API](https://api.pjm.com/api/v1).
//!
//! ```no_run
//! use chrono::NaiveDate;
//! use gridapi_pjm::{Client, LmpRequest};
//!
//! # async fn run() -> Result<(), gridapi_pjm::Error> {
//! // PJM_API_KEY if set, otherwise the public key the Data Miner 2 web app ships.
//! let client = Client::new()?;
//! let day = NaiveDate::from_ymd_opt(2026, 11, 17).expect("valid date");
//! let rows = client.da_hrl_lmps(LmpRequest::new(day).pnode_id(51288)).await?;
//! println!("{} {}: ${:.2}/MWh", rows[0].pnode_name, rows[0].datetime_beginning_ept, rows[0].total_lmp_da);
//! # Ok(()) }
//! ```
//!
//! # What is here
//!
//! * [`Client::feeds`] and [`Client::feed_metadata`]: the feed catalog and
//!   field definitions (filterable, sortable, types).
//! * [`Client::query`] / [`Client::query_all`] with a [`Query`] builder: any
//!   feed, with filters, paging, `fields` and `sort`. Rows decode into your
//!   own `Deserialize` type or the untyped [`Row`].
//! * Typed helpers for day-ahead, real-time and five-minute LMPs, metered
//!   load, seven-day and five-minute load forecasts, generation by fuel and
//!   pnode lookup (see [`feeds`]).
//!
//! # Keys
//!
//! [`KeyMode`] selects the subscription key: your own (`PJM_API_KEY`, from
//! <https://apiportal.pjm.com/>) or the public key the web app ships, fetched
//! from `settings.json` and refetched once on a 401 because it rotates.
//!
//! # Rate limits
//!
//! PJM caps non-members at 6 data connections per minute and members at 600.
//! Each [`Client`] throttles itself to [`RateLimit::NON_MEMBER`] by default
//! (a strict sliding window per client; there is no global state). Retries
//! on 429, 5xx and transient transport errors use full-jitter exponential
//! back-off and honour `Retry-After`.

mod auth;
mod catalog;
mod client;
mod error;
pub mod feeds;
mod page;
mod query;
mod throttle;

pub use auth::{fetch_public_key, ApiKey, KeyMode, API_KEY_ENV, DEFAULT_SETTINGS_URL};
pub use catalog::{Column, FeedMetadata, FeedSummary};
pub use client::{Client, ClientBuilder, DEFAULT_BASE_URL};
pub use error::{BoxError, Error, FieldError};
pub use feeds::{
    DaHourlyLmp, FiveMinuteLoadForecast, ForecastRequest, GenByFuel, HourlyLoadMetered, LmpRequest,
    LoadForecast7Day, LoadRequest, Pnode, PnodeRequest, RtLmp,
};
pub use page::{Link, Page, Row};
pub use query::{
    DateFilter, DateRange, Order, Query, RelativeDate, VersionFilter, MAX_RANGE_DAYS, MAX_ROW_COUNT,
};
pub use throttle::{RateLimit, RetryPolicy};

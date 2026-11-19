//! Async client for the [Grid Status](https://www.gridstatus.io) hosted API
//! (`https://api.gridstatus.io/v1`).
//!
//! A pure optional adapter: bring your own Grid Status account, API key and plan. The key is
//! yours alone and must not be shared. Use is subject to the Grid Status Terms of Use
//! (<https://gridstatus.io/legal/terms_of_use.pdf>). Data provided by Grid Status
//! (<https://www.gridstatus.io/>).
//!
//! - Dataset catalog ([`Client::list_datasets`]) and metadata ([`Client::dataset`]).
//! - Generic dataset query with every documented parameter ([`Query`], [`Client::query`],
//!   [`Client::query_records`], [`Client::query_all`]) returning dynamic [`Record`]s
//!   or any `Deserialize` row type.
//! - Typed helpers for the common price, fuel-mix and load datasets ([`typed`]).
//! - Quota: [`Client::usage`] plus any rate-limit headers the server sends
//!   ([`ResponseMeta::rate_limit`]), and a hard per-client request budget.
//! - Per-instance lock-free throttle (default one request per 1.5 s; the free tier allows 1 per second),
//!   retry with jittered exponential backoff on 429/5xx, no global state.
//!
//! ```no_run
//! use gridapi_gridstatus::{Client, Query};
//!
//! # async fn demo() -> Result<(), gridapi_gridstatus::Error> {
//! let client = Client::from_env()?; // reads GRIDSTATUS_API_KEY
//! let page = client
//!     .ercot_spp_day_ahead_hourly(
//!         &Query::new()
//!             .start_time("2026-11-17T00:00:00Z")
//!             .end_time("2026-11-17T03:00:00Z")
//!             .location("HB_HOUSTON"),
//!     )
//!     .await?;
//! for row in &page.rows {
//!     println!("{} {:?}", row.interval_start_utc, row.spp);
//! }
//! # Ok(()) }
//! ```

mod client;
mod error;
mod models;
mod query;
mod response;
mod retry;
mod throttle;
pub mod typed;

pub use client::{
    AllRows, Client, ClientBuilder, API_KEY_ENV, DEFAULT_BASE_URL, DEFAULT_MIN_INTERVAL,
};
pub use error::{Error, Result};
pub use models::{
    Column, Dataset, DatasetList, DatasetUpdate, PageMeta, PeriodUsage, QueryPage, Record, Usage,
    UsageLimits,
};
pub use query::{
    ApiTime, FilterOperator, FilterValue, Order, Query, Resample, ResampleFrequency,
    ResampleFunction,
};
pub use response::{RateLimitHeaders, ResponseMeta};
pub use retry::RetryPolicy;
pub use typed::{FuelMixRow, Iso, LmpRow, LoadRow, SppRow};

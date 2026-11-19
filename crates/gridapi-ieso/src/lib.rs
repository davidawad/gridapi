//! Async client for the [IESO](https://www.ieso.ca) (Ontario) public reports site.
//!
//! IESO publishes market data as files under
//! `https://reports-public.ieso.ca/public/<Report>/PUB_<Report>[_stamp][_vN].<ext>`.
//! No API key is needed. This crate names those files
//! ([`FileSpec`], [`Report`], [`Stamp`], [`Version`]), fetches them with retry and
//! jittered backoff ([`Client`]), and decodes the ones people ask for into typed
//! values ([`data`]).
//!
//! ```no_run
//! # async fn demo() -> gridapi_ieso::Result<()> {
//! use chrono::NaiveDate;
//! use gridapi_ieso::Client;
//!
//! let client = Client::new()?;
//! let date = NaiveDate::from_ymd_opt(2026, 11, 17).expect("valid date");
//! let da = client.da_ontario_zonal_prices(date).await?;
//! let hour1 = da.body.hour(1).and_then(|h| h.zonal_price);
//! println!("day-ahead Ontario zonal price, hour ending 1: {hour1:?} CAD/MWh");
//! # Ok(()) }
//! ```
//!
//! Times in IESO files are Eastern *Standard* Time all year (UTC-5) and hours are
//! hour-ending; see [`time`].

pub mod client;
pub mod data;
pub mod error;
pub mod index;
pub mod report;
pub mod retry;
mod serde_util;
pub mod time;

pub use client::{Client, ClientBuilder, Fetched, DEFAULT_BASE_URL, DEFAULT_USER_AGENT};
pub use error::{Error, Result};
pub use index::{ReportDirectory, ReportFile};
pub use report::{Cadence, FileSpec, Format, Report, Stamp, Version};
pub use retry::RetryPolicy;

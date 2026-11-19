//! Async client for NYISO's public market data.
//!
//! NYISO publishes its market data as CSV files on `mis.nyiso.com`, with no
//! account or API key. This crate hides the file naming, the switch from daily
//! files to monthly zip archives for older days, and the Eastern-time quirks
//! (including the repeated hour on the fall-back day), and returns typed rows.
//!
//! ```no_run
//! use gridapi_nyiso::{chrono::NaiveDate, Client, Level};
//!
//! # async fn demo() -> Result<(), gridapi_nyiso::Error> {
//! let client = Client::new()?;
//! let day = NaiveDate::from_ymd_opt(2026, 11, 17).expect("valid date");
//!
//! // Day-ahead hourly LBMP by zone.
//! let prices = client.day_ahead_lbmp(Level::Zone, day).fetch_strict().await?;
//! let capitl = prices.iter().find(|p| p.name == "CAPITL");
//! println!("{capitl:?}");
//!
//! // A month of five-minute fuel mix; old days come from the monthly archive.
//! let end = NaiveDate::from_ymd_opt(2026, 11, 3).expect("valid date");
//! let first = NaiveDate::from_ymd_opt(2026, 11, 1).expect("valid date");
//! let mix = client.fuel_mix(first).through(end).fetch().await?;
//! println!("{} rows, {} days missing", mix.rows.len(), mix.missing.len());
//! # Ok(())
//! # }
//! ```
//!
//! # Datasets
//!
//! | Method | NYISO dataset | Interval |
//! |---|---|---|
//! | [`Client::day_ahead_lbmp`] | `damlbmp` (zone, generator) | hourly |
//! | [`Client::real_time_lbmp`] | `realtime` (zone, generator) | 5 minutes |
//! | [`Client::integrated_real_time_lbmp`] | `rtlbmp` (zone, generator) | hourly |
//! | [`Client::actual_load`] | `pal` | 5 minutes |
//! | [`Client::integrated_load`] | `palIntegrated` | hourly |
//! | [`Client::load_forecast`] | `isolf` | hourly |
//! | [`Client::fuel_mix`] | `rtfuelmix` | 5 minutes |
//! | [`Client::day_ahead_ancillary`] | `damasp` | hourly |
//! | [`Client::real_time_ancillary`] | `rtasp` | 5 minutes |
//! | [`Client::interface_flows`] | `ExternalLimitsFlows` | 5 minutes |
//!
//! # Time
//!
//! Timestamps are [`DateTime<FixedOffset>`](chrono::DateTime) in Eastern time:
//! offset `-05:00` in winter, `-04:00` in summer. Files with a `Time Zone` column
//! use it; for the rest, the repeated hour on the fall-back day is told apart by
//! row order (first pass EDT, second pass EST).
//!
//! # Errors
//!
//! Every fallible function returns [`Error`]. Transient failures (transport
//! errors, HTTP 429 and 5xx) are retried with jittered exponential backoff, see
//! [`RetryPolicy`]. A day NYISO never published is not an error for
//! [`Request::fetch`]; it is listed in [`Series::missing`].

mod client;
mod dataset;
mod error;
mod http;
mod records;
mod table;
mod time;

pub use chrono;
pub use client::{
    Client, ClientBuilder, Request, Series, Source, DEFAULT_BASE_URL, DEFAULT_DAILY_RETENTION_DAYS,
};
pub use dataset::{Dataset, Level};
pub use error::Error;
pub use http::RetryPolicy;
pub use records::{AncillaryPrice, FuelMix, InterfaceFlow, Lbmp, Load, LoadForecast, Record};

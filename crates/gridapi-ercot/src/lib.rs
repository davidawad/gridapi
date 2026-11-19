//! Async Rust client for the ERCOT Public API (`api.ercot.com`).
//!
//! - **Auth handled for you.** Logs in through ERCOT's Azure B2C flow, caches the
//!   ~1 hour `id_token`, refreshes it ahead of expiry and re-logs in after a 401.
//! - **Report catalog.** [`Client::products`] / [`Client::product`] list EMIL products
//!   and their data endpoints.
//! - **Generic queries.** [`ReportRequest`] with any filter, paging and sorting;
//!   [`Client::query`] returns one page, [`Client::query_all`] follows every page.
//! - **Typed helpers.** Real-time and day-ahead prices, LMPs, actual and forecast load,
//!   wind and solar actuals, system generation: see [`reports`].
//! - **Fuel mix.** [`Client::fuel_mix`] reads ERCOT's dashboard feed.
//! - **Archive.** List and download posted documents, with zip extraction.
//! - **Resilient.** Jittered exponential backoff on 429 / 5xx / transport errors,
//!   `Retry-After` honoured, cookie jar and redirect following for the CDN bot check.
//!
//! # Example
//!
//! ```no_run
//! use chrono::NaiveDate;
//! use gridapi_ercot::{Client, Credentials, DayAheadSpp, Query};
//!
//! # async fn run() -> Result<(), gridapi_ercot::Error> {
//! // Reads ERCOT_USERNAME, ERCOT_PASSWORD, ERCOT_SUBSCRIPTION_KEY.
//! let client = Client::new(Credentials::from_env()?)?;
//! let day = NaiveDate::from_ymd_opt(2026, 11, 17).expect("valid date");
//! let prices = client
//!     .fetch_all(
//!         &Query::<DayAheadSpp>::new().delivery_date(day, day).settlement_point("HB_NORTH"),
//!         10,
//!     )
//!     .await?;
//! for p in prices {
//!     println!("HE{} {:.2} $/MWh", p.hour_ending.get(), p.price);
//! }
//! # Ok(()) }
//! ```
//!
//! All timestamps ERCOT publishes are Central Prevailing Time without an offset; the
//! typed records use [`chrono::NaiveDateTime`] and `dst_flag` marks the repeated hour
//! at the autumn clock change.

pub use chrono;

mod archive;
mod auth;
mod catalog;
mod client;
mod config;
mod error;
mod fuel_mix;
mod http;
mod page;
pub mod reports;
mod request;

pub use archive::{ArchiveEntry, ArchiveFile, ArchivePage, ArchiveQuery, ExtractedFile};
pub use catalog::{parse_catalog, parse_product, Artifact, Product};
pub use client::{Client, ClientBuilder, Page};
pub use config::{
    Credentials, RetryPolicy, DEFAULT_BASE_URL, DEFAULT_CLIENT_ID, DEFAULT_FUEL_MIX_URL,
    DEFAULT_TOKEN_URL, DEFAULT_USER_AGENT, ENV_PASSWORD, ENV_SUBSCRIPTION_KEY, ENV_USERNAME,
};
pub use error::{Error, Result, TransportError};
pub use fuel_mix::{FuelMix, FuelMixSnapshot};
pub use page::{
    DataType, Field, FromRow, HourEnding, PageMeta, ReportInfo, ReportPage, Row, Value,
};
pub use reports::{
    ActualLoad, ActualLoadRecord, DayAheadSpp, DayAheadSppRecord, LoadForecast, LoadForecastRecord,
    RealTimeLmp, RealTimeLmpRecord, RealTimeSpp, RealTimeSppRecord, SettlementPointType,
    SolarActual5Min, SolarActual5MinRecord, SystemGeneration, SystemGenerationRecord,
    WeatherZoneLoad, WindActual5Min, WindActual5MinRecord,
};
pub use request::{fmt_date, fmt_datetime, Query, Report, ReportRequest, SortDir};

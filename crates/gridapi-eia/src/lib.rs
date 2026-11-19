//! Async Rust client for the U.S. Energy Information Administration's Open
//! Data API v2 (`https://api.eia.gov/v2`), plus the keyless EIA-860M and
//! EIA-930 bulk files.
//!
//! * **Browse**: [`Client::root`], [`Client::route`], [`Client::facet_values`],
//!   [`Client::datasets_under`] return route metadata (frequencies, facets,
//!   data columns, period range).
//! * **Query**: [`DataQuery`] builds any `/data` request; [`Client::data_page`]
//!   fetches one page, [`Client::data_stream`] / [`Client::data_all`] page
//!   past EIA's 5000-row cap with `offset`.
//! * **Typed helpers** in [`datasets`]: EIA-930 `electricity/rto`
//!   (region, fuel type, interchange, subregion), EIA-860M
//!   `operating-generator-capacity`, `retail-sales`, natural-gas spot prices.
//! * **Bulk files** in [`bulk`] (feature `bulk`, on by default): the
//!   EIA-860M workbook with its Planned/Retired/Canceled sheets, and the
//!   EIA-930 six-month CSVs.
//!
//! Transient failures (HTTP 429, 5xx, timeouts) are retried with jittered
//! exponential backoff ([`RetryPolicy`]). The client holds no global state and
//! never prints the API key.
//!
//! ```no_run
//! use gridapi_eia::datasets::rto::{self, RegionDataRow, RegionMetric};
//! use gridapi_eia::{Client, Direction};
//!
//! # async fn demo() -> Result<(), gridapi_eia::Error> {
//! let client = Client::from_env()?; // reads EIA_API_KEY
//!
//! // Latest 24 hours of ERCOT demand.
//! let q = rto::region_data_for("ERCO", RegionMetric::Demand)
//!     .sort("period", Direction::Desc)
//!     .length(24);
//! let rows: Vec<RegionDataRow> = client.data_page(&q).await?.rows;
//!
//! // Everything since a date, across as many 5000-row pages as it takes.
//! let q = rto::region_data_for("PJM", RegionMetric::Demand).start("2026-01-01T00");
//! let all: Vec<RegionDataRow> = client.data_all(&q).await?;
//! # let _ = (rows, all);
//! # Ok(()) }
//! ```

#![cfg_attr(test, allow(clippy::unwrap_used, clippy::expect_used))]

#[cfg(feature = "bulk")]
pub mod bulk;
mod client;
mod data;
pub mod datasets;
mod de;
mod error;
mod meta;
mod query;
mod retry;

pub use client::{
    Client, ClientBuilder, Warning, API_KEY_ENV, DEFAULT_BASE_URL, DEFAULT_BULK_BASE_URL,
    DEFAULT_USER_AGENT,
};
pub use data::{DataPage, Row};
pub use error::{Error, Result};
pub use meta::{ChildRoute, DataColumn, FacetMeta, FacetValue, FrequencyMeta, RouteMeta};
pub use query::{DataQuery, Direction, Frequency, MAX_PAGE_LEN};
pub use retry::RetryPolicy;

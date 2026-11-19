//! Async client for the [AESO](https://www.aeso.ca) (Alberta) public API.
//!
//! The API lives behind `https://apimgw.aeso.ca/public/<api>/<version>/...` and
//! needs an `API-KEY` header: register at <https://developer-apim.aeso.ca/> and
//! subscribe to the "AESO Public API" product. AESO's terms limit the data to
//! non-commercial, personal or educational use unless you have written permission.
//!
//! ```no_run
//! # async fn demo() -> gridapi_aeso::Result<()> {
//! use chrono::NaiveDate;
//! use gridapi_aeso::{Client, DateRange};
//!
//! let client = Client::from_env()?; // reads AESO_API_KEY
//! let day = NaiveDate::from_ymd_opt(2026, 11, 16).expect("valid date");
//! let prices = client.pool_price(DateRange::day(day)).await?;
//! println!("pool price at {}: {:?} CAD/MWh", prices[0].begin_mpt, prices[0].pool_price);
//! # Ok(()) }
//! ```
//!
//! Every typed method has a pure counterpart in [`decode`], so you can decode a
//! response you fetched or cached yourself.

pub mod client;
pub mod decode;
pub mod error;
pub mod model;
pub mod query;
pub mod retry;
mod serde_util;

pub use client::{
    merit_order_latest_date, Client, ClientBuilder, AIES_CAPACITY_MAX_DAYS, API_KEY_ENV,
    DEFAULT_BASE_URL, DEFAULT_USER_AGENT, MERIT_ORDER_LAG_DAYS, POOL_PRICE_MAX_DAYS,
};
pub use error::{Error, Result};
pub use query::{
    AssetListQuery, DataType, DateRange, InterchangeQuery, Intertie, MeteredVolumeQuery,
    OutageQuery, Revisions, TransferType,
};
pub use retry::RetryPolicy;

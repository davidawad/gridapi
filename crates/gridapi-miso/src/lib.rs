//! Async client for MISO (Midcontinent Independent System Operator) public market data.
//!
//! Three data sources, one [`Client`]:
//!
//! * the keyless **public API** at `public-api.misoenergy.org` ([`Client::ex_ante_lmp`],
//!   [`Client::lmp_consolidated_table`], [`Client::five_minute_ex_post_lmp`],
//!   [`Client::fuel_mix`], [`Client::total_load`], wind and solar, interchange, constraints);
//! * the keyless **market report files** at `docs.misoenergy.org/marketreports`
//!   ([`Client::lmp_report`]: day-ahead ex-post and ex-ante, real-time prelim and final hourly LMPs);
//! * behind the non-default cargo feature `miso-dataexchange`: the keyed **Data Exchange API** at
//!   `apim.misoenergy.org` (`Client::pricing_page`), which needs `MISO_API_KEY`. **Untested: no
//!   account**, so it is not part of the default build.
//!
//! The legacy `MISORTWDDataBroker` feed was retired by MISO on 2025-12-12; see [`Client::data_broker`].
//!
//! Requests retry on HTTP 429, 5xx and connection errors with jittered exponential backoff
//! ([`RetryPolicy`]). MISO publishes timestamps on a fixed EST clock; see [`est_to_utc`].
//! MISO asks clients to poll no more than once a minute.
//!
//! # Examples
//!
//! ```no_run
//! # async fn run() -> Result<(), gridapi_miso::Error> {
//! use gridapi_miso::Client;
//!
//! let miso = Client::new()?;
//! let prices = miso.ex_ante_lmp().await?;
//! if let Some(hub) = prices.hub("ARKANSAS.HUB") {
//!     println!("{} {}: ${:.2}/MWh", prices.ref_id, hub.name, hub.lmp);
//! }
//! # Ok(())
//! # }
//! ```

mod broker;
mod client;
#[cfg(feature = "miso-dataexchange")]
mod data_exchange;
mod error;
mod lmp;
mod reports;
mod retry;
mod serde_util;
mod system;
mod time;
mod transport;

pub use broker::REPLACEMENTS as DATA_BROKER_REPLACEMENTS;
pub use client::{
    Client, ClientBuilder, Endpoints, DATA_BROKER_BASE, PUBLIC_API_BASE, REPORTS_BASE,
};
#[cfg(feature = "miso-dataexchange")]
pub use client::{API_KEY_ENV, DATA_EXCHANGE_BASE};
#[cfg(feature = "miso-dataexchange")]
pub use data_exchange::{DataExchangeQuery, Page, PageInfo, PricingProduct};
pub use error::{Error, Result};
pub use lmp::{
    AncillaryMcp, ExAnteLmp, FiveMinuteLmp, FiveMinuteWindow, HubLmp, LmpConsolidatedTable,
    NodePrice, PriceBlock, ZoneMcp,
};
pub use reports::{Component, LmpReport, LmpReportKind, NodeType, ReportRow};
pub use retry::RetryPolicy;
pub use system::{
    AcePoint, BindingConstraint, ForecastVsActual, FuelEntry, FuelMix, FuelMixView, HourlyMw,
    HourlyPoint, LoadPoint, NetActualInterchange, NetScheduledInterchange, Renewable, SnapshotItem,
    TotalLoad,
};
pub use time::{est, est_to_utc, parse_ref_id, parse_timestamp};

/// Compile-time proof the client and its futures can cross threads.
const _: () = {
    const fn assert_send_sync<T: Send + Sync>() {}
    assert_send_sync::<Client>();
    assert_send_sync::<Error>();
};

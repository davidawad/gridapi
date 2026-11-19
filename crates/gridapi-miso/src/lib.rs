//! Async client for MISO (Midcontinent Independent System Operator) market data.
//!
//! # Which routes are compiled in
//!
//! * **Data Exchange API** (`apim.misoenergy.org`), the route MISO provides for programmatic
//!   access: behind the cargo feature `miso-dataexchange`; needs your own `MISO_API_KEY`
//!   (<https://data-exchange.misoenergy.org/>) and is **untested** (no account).
//! * **Website-hosted routes**, behind the **off-by-default** cargo feature `miso-public-web`:
//!   the keyless public API at `public-api.misoenergy.org` (`Client::ex_ante_lmp`,
//!   `Client::lmp_consolidated_table`, `Client::five_minute_ex_post_lmp`,
//!   `Client::fuel_mix`, `Client::total_load`, wind and solar, interchange, constraints),
//!   the market report files at `docs.misoenergy.org/marketreports` (`Client::lmp_report`) and
//!   the retired `MISORTWDDataBroker` (`Client::data_broker`).
//!
//! MISO's website terms (<https://www.misoenergy.org/meet-miso/legal-and-privacy/>) say: "You
//! agree not to access or use this Website or the App by any means other than through the
//! interface that is provided by MISO for use in accessing or using this Website or the App.
//! You agree not use any automated means, including, without limitation, agents, robots,
//! scripts, or spiders, to access, monitor, or copy any part of this Website or the App." Whether
//! those hosts are "the Website" is arguable, which is why they are opt-in. Enable
//! `miso-public-web` only if you have MISO's permission or accept that risk yourself.
//!
//! Requests retry on HTTP 429, 5xx and connection errors with jittered exponential backoff
//! ([`RetryPolicy`]). MISO publishes timestamps on a fixed EST clock; see [`est_to_utc`].
//! MISO asks clients to poll no more than once a minute.
//!
//! # Examples
//!
//! ```no_run
//! # #[cfg(feature = "miso-public-web")]
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

// With neither opt-in feature there is no request-making method, so the shared plumbing is
// (deliberately) unused.
#![cfg_attr(
    not(any(feature = "miso-public-web", feature = "miso-dataexchange")),
    allow(dead_code)
)]

#[cfg(feature = "miso-public-web")]
mod broker;
mod client;
#[cfg(feature = "miso-dataexchange")]
mod data_exchange;
mod error;
#[cfg(feature = "miso-public-web")]
mod lmp;
#[cfg(feature = "miso-public-web")]
mod reports;
mod retry;
#[cfg(feature = "miso-public-web")]
mod serde_util;
#[cfg(feature = "miso-public-web")]
mod system;
mod time;
mod transport;

#[cfg(feature = "miso-public-web")]
pub use broker::REPLACEMENTS as DATA_BROKER_REPLACEMENTS;
pub use client::{Client, ClientBuilder, Endpoints};
#[cfg(feature = "miso-dataexchange")]
pub use client::{API_KEY_ENV, DATA_EXCHANGE_BASE};
#[cfg(feature = "miso-public-web")]
pub use client::{DATA_BROKER_BASE, PUBLIC_API_BASE, REPORTS_BASE};
#[cfg(feature = "miso-dataexchange")]
pub use data_exchange::{DataExchangeQuery, Page, PageInfo, PricingProduct};
pub use error::{Error, Result};
#[cfg(feature = "miso-public-web")]
pub use lmp::{
    AncillaryMcp, ExAnteLmp, FiveMinuteLmp, FiveMinuteWindow, HubLmp, LmpConsolidatedTable,
    NodePrice, PriceBlock, ZoneMcp,
};
#[cfg(feature = "miso-public-web")]
pub use reports::{Component, LmpReport, LmpReportKind, NodeType, ReportRow};
pub use retry::RetryPolicy;
#[cfg(feature = "miso-public-web")]
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

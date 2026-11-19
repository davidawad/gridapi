//! Async client for SPP (Southwest Power Pool) public market data, no account needed.
//!
//! SPP publishes market data through the file browser on its public portal,
//! `portal.spp.org` (the successor of `marketplace.spp.org`). Its web UI sends an
//! `X-SPP-UI-Token` header, but that token only exists for signed-in users: for the public
//! tree the header is empty and anonymous requests work, so this client sends none.
//!
//! * [`Client::list`] / [`Client::download`] / [`Client::newest_file`]: the raw file browser,
//!   with [`Client::file_browser_pages`] to discover every public folder tree;
//! * [`Client::da_lmp_by_location`], [`Client::rt_lmp_by_location`] and the `latest_*`
//!   variants: day-ahead hourly and real-time five-minute LMPs by settlement location,
//!   which includes the hubs ([`SPP_HUBS`], [`LocationalPrice::is_hub`]);
//! * [`Client::hourly_load_daily`], [`Client::hourly_load_monthly`]: load by control zone;
//! * [`Client::generation_mix`]: five-minute generation by fuel with system load;
//! * [`Client::chart`]: the dashboard chart feeds (generation mix, load forecast, ACE).
//!
//! Requests retry on HTTP 429, 5xx and connection errors with jittered exponential backoff
//! ([`RetryPolicy`]). SPP stamps LMP intervals in Central Prevailing Time and also gives a
//! GMT column; this crate keeps both ([`LocationalPrice::interval_end`] and
//! [`LocationalPrice::interval_end_utc`]).
//!
//! # Examples
//!
//! ```no_run
//! # async fn run() -> Result<(), gridapi_spp::Error> {
//! use gridapi_spp::{at_location, Client};
//!
//! let spp = Client::new()?;
//! let (file, prices) = spp.latest_rt_lmp_by_location().await?;
//! for p in at_location(&prices, "SPPNORTH_HUB") {
//!     println!("{} {}: ${:.2}/MWh", file.name, p.settlement_location, p.lmp);
//! }
//! # Ok(())
//! # }
//! ```

mod browser;
mod chart;
mod client;
mod error;
mod genmix;
mod load;
mod prices;
mod retry;
mod transport;

pub use browser::{
    BrowserPage, Entry, EntryKind, FileSource, DA_LMP_BY_LOCATION, GENERATION_MIX, HOURLY_LOAD,
    RTBM_LMP_BY_LOCATION,
};
pub use chart::{parse_chart, Chart, ChartData, Series};
pub use client::{Client, ClientBuilder, PORTAL_BASE};
pub use error::{Error, Result};
pub use genmix::{parse_gen_mix, BalancingArea, FuelOutput, GenMixSample, GenMixWindow};
pub use load::{daily_load_path, monthly_load_path, parse_hourly_load, ZoneLoad};
pub use prices::{
    at_location, da_lmp_path, parse_lmp_csv, rt_lmp_daily_path, rt_lmp_path, LocationalPrice,
    SPP_HUBS,
};
pub use retry::RetryPolicy;

/// Compile-time proof the client and its errors can cross threads.
const _: () = {
    const fn assert_send_sync<T: Send + Sync>() {}
    assert_send_sync::<Client>();
    assert_send_sync::<Error>();
};

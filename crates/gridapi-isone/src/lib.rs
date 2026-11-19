//! Async client for ISO New England market data.
//!
//! Two routes, one crate:
//!
//! * [`express`]: the **keyless** public ISO Express CSV reports on `www.iso-ne.com`
//!   (day-ahead and real-time LMP, five-minute LMP, fuel mix, system load, ...).
//!   No account needed. The crate follows the redirect the static reports use and
//!   obtains the session cookie the dynamic reports require.
//! * `webservices` (behind the non-default cargo feature `isone-webservices`): the
//!   **authenticated** Web Services API v1.1 (`webservices.iso-ne.com/api/v1.1`), which needs
//!   an ISO Express account. Credentials come from `ISONE_USERNAME` and `ISONE_PASSWORD`.
//!   **Untested: no account**, so it is not part of the default build.
//!
//! ```no_run
//! use gridapi_isone::{
//!     chrono::NaiveDate,
//!     express::{Express, Revision},
//!     LocationFilter, INTERNAL_HUB_ID,
//! };
//!
//! # async fn demo() -> Result<(), gridapi_isone::Error> {
//! let express = Express::new()?;
//! let day = NaiveDate::from_ymd_opt(2026, 11, 17).expect("valid date");
//!
//! // Day-ahead hourly LMP at the Internal Hub (4000).
//! let hub = LocationFilter::id(INTERNAL_HUB_ID);
//! let da = express.day_ahead_lmp(day).locations(hub.clone()).fetch_strict().await?;
//! println!("{} {} ${}", da[0].interval_start, da[0].hour_ending, da[0].lmp);
//!
//! // The same hub, five-minute final real-time prices for a past day.
//! let rt = express.five_minute_lmp(day, Revision::Final).locations(hub).fetch().await?;
//! println!("{} rows", rt.rows.len());
//! # Ok(())
//! # }
//! ```
//!
//! # Time
//!
//! Timestamps are [`DateTime<FixedOffset>`](chrono::DateTime) in Eastern time:
//! offset `-05:00` in winter, `-04:00` in summer. ISO-NE labels hourly rows "hour
//! ending" (`01`..`24`, with `02X` for the repeated hour on the fall-back day);
//! the crate converts that to the interval start. Reports that carry local clock
//! times resolve the repeated hour by row order within each series.
//!
//! # Errors
//!
//! Every fallible function returns [`Error`]. Transient failures (transport
//! errors, HTTP 429 and 5xx) are retried with jittered exponential backoff, see
//! [`RetryPolicy`]. A day ISO-NE never published is not an error for
//! [`express::Request::fetch`]; it is listed in [`express::Series::missing`].

mod error;
pub mod express;
mod http;
mod report;
mod rows;
#[cfg(feature = "isone-webservices")]
pub mod webservices;

mod time;

pub use chrono;
pub use error::Error;
pub use http::RetryPolicy;
#[cfg(feature = "isone-webservices")]
pub use http::{Credentials, PASSWORD_VARIABLE, USERNAME_VARIABLE};
pub use rows::{
    FiveMinuteLmp, FiveMinuteSystemLoad, FuelMix, HourlyDemand, HourlyLmp, LocationFilter,
    RegulationPrice, ReserveRequirement, ZonalLoad, INTERNAL_HUB_ID, INTERNAL_HUB_NAME,
    ZONE_CONNECTICUT, ZONE_MAINE, ZONE_NEMASS_BOSTON, ZONE_NEW_HAMPSHIRE, ZONE_RHODE_ISLAND,
    ZONE_SEMASS, ZONE_VERMONT, ZONE_WCMASS,
};

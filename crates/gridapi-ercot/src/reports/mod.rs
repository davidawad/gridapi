//! Typed helpers for the reports most users want.
//!
//! Each report is a marker type implementing [`Report`](crate::Report); pair it with
//! [`Query`](crate::Query) and [`Client::fetch`](crate::Client::fetch).
//!
//! | Report | EMIL id | Marker type |
//! |---|---|---|
//! | Real-time settlement point prices (15-min) | NP6-905-CD | [`RealTimeSpp`] |
//! | Day-ahead settlement point prices | NP4-190-CD | [`DayAheadSpp`] |
//! | Real-time LMPs by node/zone/hub (SCED) | NP6-788-CD | [`RealTimeLmp`] |
//! | Actual load by weather zone | NP6-345-CD | [`ActualLoad`] |
//! | Seven-day load forecast by model and weather zone | NP3-565-CD | [`LoadForecast`] |
//! | Wind actual, 5-minute | NP4-733-CD | [`WindActual5Min`] |
//! | Solar actual, 5-minute | NP4-738-CD | [`SolarActual5Min`] |
//! | State-estimator total generation | NP6-625-CD | [`SystemGeneration`] |

mod generation;
mod load;
mod prices;
mod renewables;

pub use generation::{SystemGeneration, SystemGenerationRecord};
pub use load::{ActualLoad, ActualLoadRecord, LoadForecast, LoadForecastRecord, WeatherZoneLoad};
pub use prices::{
    DayAheadSpp, DayAheadSppRecord, RealTimeLmp, RealTimeLmpRecord, RealTimeSpp, RealTimeSppRecord,
    SettlementPointType,
};
pub use renewables::{
    SolarActual5Min, SolarActual5MinRecord, WindActual5Min, WindActual5MinRecord,
};

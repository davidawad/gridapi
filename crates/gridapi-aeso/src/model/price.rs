//! Prices and load: pool price, system marginal price, Alberta internal load.

use chrono::{DateTime, NaiveDateTime, Utc};
use serde::Deserialize;

use crate::serde_util::{local, opt_f64, utc};

/// One settled (or forecast) hour of the Alberta pool price, CAD/MWh.
#[derive(Debug, Clone, PartialEq, Deserialize)]
pub struct PoolPrice {
    /// Start of the hour (UTC).
    #[serde(rename = "begin_datetime_utc", deserialize_with = "utc")]
    pub begin_utc: DateTime<Utc>,
    /// Start of the hour (Mountain prevailing time).
    #[serde(rename = "begin_datetime_mpt", deserialize_with = "local")]
    pub begin_mpt: NaiveDateTime,
    /// Settled pool price in CAD/MWh; `None` until the hour has settled.
    #[serde(default, deserialize_with = "opt_f64")]
    pub pool_price: Option<f64>,
    /// AESO's forecast pool price in CAD/MWh.
    #[serde(default, deserialize_with = "opt_f64")]
    pub forecast_pool_price: Option<f64>,
    /// Rolling 30-day average pool price in CAD/MWh.
    #[serde(rename = "rolling_30day_avg", default, deserialize_with = "opt_f64")]
    pub rolling_30day_average: Option<f64>,
}

/// Wire shape of the pool price payload.
#[derive(Debug, Deserialize)]
pub(crate) struct PoolPriceReport {
    #[serde(rename = "Pool Price Report", default)]
    pub(crate) rows: Vec<PoolPrice>,
}

/// One system marginal price interval, CAD/MWh. Intervals are irregular:
/// a new one starts whenever the marginal offer changes.
#[derive(Debug, Clone, PartialEq, Deserialize)]
pub struct SystemMarginalPrice {
    /// Interval start (UTC).
    #[serde(rename = "begin_datetime_utc", deserialize_with = "utc")]
    pub begin_utc: DateTime<Utc>,
    /// Interval end (UTC).
    #[serde(rename = "end_datetime_utc", deserialize_with = "utc")]
    pub end_utc: DateTime<Utc>,
    /// Interval start (Mountain prevailing time).
    #[serde(rename = "begin_datetime_mpt", deserialize_with = "local")]
    pub begin_mpt: NaiveDateTime,
    /// Interval end (Mountain prevailing time).
    #[serde(rename = "end_datetime_mpt", deserialize_with = "local")]
    pub end_mpt: NaiveDateTime,
    /// System marginal price in CAD/MWh.
    #[serde(default, deserialize_with = "opt_f64")]
    pub system_marginal_price: Option<f64>,
    /// Volume of the marginal block in MW.
    #[serde(default, deserialize_with = "opt_f64")]
    pub volume: Option<f64>,
}

/// The current-interval variant carries no end time.
#[derive(Debug, Clone, PartialEq, Deserialize)]
pub struct CurrentSystemMarginalPrice {
    /// Interval start (UTC).
    #[serde(rename = "begin_datetime_utc", deserialize_with = "utc")]
    pub begin_utc: DateTime<Utc>,
    /// Interval start (Mountain prevailing time).
    #[serde(rename = "begin_datetime_mpt", deserialize_with = "local")]
    pub begin_mpt: NaiveDateTime,
    /// System marginal price in CAD/MWh.
    #[serde(default, deserialize_with = "opt_f64")]
    pub system_marginal_price: Option<f64>,
    /// Volume of the marginal block in MW.
    #[serde(default, deserialize_with = "opt_f64")]
    pub volume: Option<f64>,
}

/// Wire shape of the system marginal price payload.
#[derive(Debug, Deserialize)]
pub(crate) struct SmpReport<T> {
    #[serde(rename = "System Marginal Price Report", default = "Vec::new")]
    pub(crate) rows: Vec<T>,
}

/// One hour of Alberta internal load (AIL), MW.
#[derive(Debug, Clone, PartialEq, Deserialize)]
pub struct InternalLoad {
    /// Start of the hour (UTC).
    #[serde(rename = "begin_datetime_utc", deserialize_with = "utc")]
    pub begin_utc: DateTime<Utc>,
    /// Start of the hour (Mountain prevailing time).
    #[serde(rename = "begin_datetime_mpt", deserialize_with = "local")]
    pub begin_mpt: NaiveDateTime,
    /// Actual load in MW; `None` for hours that have not happened.
    #[serde(default, deserialize_with = "opt_f64")]
    pub alberta_internal_load: Option<f64>,
    /// AESO's load forecast in MW.
    #[serde(default, deserialize_with = "opt_f64")]
    pub forecast_alberta_internal_load: Option<f64>,
}

/// Wire shape of the actual/forecast payload.
#[derive(Debug, Deserialize)]
pub(crate) struct ActualForecastReport {
    #[serde(rename = "Actual Forecast Report", default)]
    pub(crate) rows: Vec<InternalLoad>,
}

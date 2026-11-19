//! Generation capacity and outage outlooks.

use chrono::{DateTime, NaiveDateTime, Utc};
use serde::Deserialize;

use crate::serde_util::{local, opt_f64, opt_text, utc};

/// Capacity and outage volumes for a fuel group in one hour, MW.
#[derive(Debug, Clone, PartialEq, Deserialize)]
pub struct OutageGrouping {
    /// Maximum capability.
    #[serde(rename = "MC", default, deserialize_with = "opt_f64")]
    pub maximum_capability: Option<f64>,
    /// Mothballed capacity.
    #[serde(rename = "MBO OUT", default, deserialize_with = "opt_f64")]
    pub mothball_outage: Option<f64>,
    /// Operational outage.
    #[serde(rename = "OP OUT", default, deserialize_with = "opt_f64")]
    pub operational_outage: Option<f64>,
    /// Available capability: `MC - (MBO OUT + OP OUT)`.
    #[serde(rename = "AC", default, deserialize_with = "opt_f64")]
    pub available_capability: Option<f64>,
}

/// One hour of a fuel group's capacity picture.
#[derive(Debug, Clone, PartialEq, Deserialize)]
pub struct CapacityHour {
    /// Start of the hour (UTC).
    #[serde(rename = "begin_datetime_utc", deserialize_with = "utc")]
    pub begin_utc: DateTime<Utc>,
    /// Start of the hour (Mountain prevailing time).
    #[serde(rename = "begin_datetime_mpt", deserialize_with = "local")]
    pub begin_mpt: NaiveDateTime,
    /// Capacity and outage volumes.
    pub outage_grouping: OutageGrouping,
}

/// AIES generation capacity and outages for one fuel / sub-fuel group
/// (`aiesgencapacity-api`). Available from 2011 to 24 months ahead; 31 days per request.
#[derive(Debug, Clone, PartialEq, Deserialize)]
pub struct FuelCapacity {
    /// Fuel type, for example `GAS`.
    pub fuel_type: String,
    /// Sub-fuel type, for example `COGENERATION`.
    #[serde(default, deserialize_with = "opt_text")]
    pub sub_fuel_type: Option<String>,
    /// Hourly series.
    #[serde(rename = "Hours", default)]
    pub hours: Vec<CapacityHour>,
}

/// One hour of forecast load outages (`loadoutageforecast-api`), MW.
#[derive(Debug, Clone, PartialEq, Deserialize)]
pub struct LoadOutageHour {
    /// Start of the hour (UTC).
    #[serde(rename = "begin_datetime_utc", deserialize_with = "utc")]
    pub begin_utc: DateTime<Utc>,
    /// Start of the hour (Mountain prevailing time).
    #[serde(rename = "begin_datetime_mpt", deserialize_with = "local")]
    pub begin_mpt: NaiveDateTime,
    /// Aggregated forecast load outage in MW.
    #[serde(default, deserialize_with = "opt_f64")]
    pub load_outage_forecast: Option<f64>,
}

/// Wire shape of the load outage payload.
#[derive(Debug, Deserialize)]
pub(crate) struct LoadOutagePayload {
    #[serde(rename = "loadOutagePerHourVO", default)]
    pub(crate) hours: Vec<LoadOutageHour>,
}

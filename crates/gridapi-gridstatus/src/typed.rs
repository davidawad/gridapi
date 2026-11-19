//! Typed rows and helpers for the commonly used datasets.
//!
//! Every helper is a thin wrapper over [`Client::query`] with a fixed dataset
//! id and row type; use [`Client::query_records`] for any other dataset.

use std::collections::BTreeMap;

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::client::Client;
use crate::error::{Error, Result};
use crate::models::QueryPage;
use crate::query::Query;

/// Dataset ids used by the typed helpers.
pub mod ids {
    /// ERCOT settlement point prices, day-ahead market, hourly.
    pub const ERCOT_SPP_DAY_AHEAD_HOURLY: &str = "ercot_spp_day_ahead_hourly";
    /// ERCOT settlement point prices, real-time market, 15-minute.
    /// (The catalog id is `..._15_min`, not `..._15_minute`.)
    pub const ERCOT_SPP_REAL_TIME_15_MIN: &str = "ercot_spp_real_time_15_min";
    /// PJM LMPs, day-ahead, hourly.
    pub const PJM_LMP_DAY_AHEAD_HOURLY: &str = "pjm_lmp_day_ahead_hourly";
    /// CAISO LMPs, day-ahead, hourly.
    pub const CAISO_LMP_DAY_AHEAD_HOURLY: &str = "caiso_lmp_day_ahead_hourly";
}

/// A North American grid operator with `{iso}_fuel_mix` and `{iso}_load` datasets.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
#[non_exhaustive]
pub enum Iso {
    /// Electric Reliability Council of Texas.
    Ercot,
    /// PJM Interconnection.
    Pjm,
    /// California ISO.
    Caiso,
    /// Midcontinent ISO.
    Miso,
    /// New York ISO.
    Nyiso,
    /// ISO New England.
    Isone,
    /// Southwest Power Pool.
    Spp,
}

impl Iso {
    /// Every operator with the standard dataset trio.
    pub const ALL: [Self; 7] = [
        Self::Ercot,
        Self::Pjm,
        Self::Caiso,
        Self::Miso,
        Self::Nyiso,
        Self::Isone,
        Self::Spp,
    ];

    /// Dataset id prefix, e.g. `pjm`.
    pub fn prefix(self) -> &'static str {
        match self {
            Self::Ercot => "ercot",
            Self::Pjm => "pjm",
            Self::Caiso => "caiso",
            Self::Miso => "miso",
            Self::Nyiso => "nyiso",
            Self::Isone => "isone",
            Self::Spp => "spp",
        }
    }

    /// `{iso}_fuel_mix`.
    pub fn fuel_mix_dataset(self) -> String {
        format!("{}_fuel_mix", self.prefix())
    }

    /// `{iso}_load`.
    pub fn load_dataset(self) -> String {
        format!("{}_load", self.prefix())
    }

    /// `{iso}_lmp_day_ahead_hourly`, or `None` for ERCOT, which publishes
    /// settlement point prices ([`ids::ERCOT_SPP_DAY_AHEAD_HOURLY`]) instead.
    pub fn lmp_day_ahead_hourly_dataset(self) -> Option<String> {
        match self {
            Self::Ercot => None,
            other => Some(format!("{}_lmp_day_ahead_hourly", other.prefix())),
        }
    }
}

/// ERCOT settlement point price row (day-ahead hourly and real-time 15-minute).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct SppRow {
    /// Interval start (UTC).
    pub interval_start_utc: DateTime<Utc>,
    /// Interval end (UTC).
    pub interval_end_utc: DateTime<Utc>,
    /// Settlement point, e.g. `HB_HOUSTON`.
    pub location: String,
    /// `Trading Hub`, `Load Zone`, `Resource Node`, ...
    #[serde(default)]
    pub location_type: Option<String>,
    /// `DAY_AHEAD_HOURLY` or `REAL_TIME_15_MIN`.
    #[serde(default)]
    pub market: Option<String>,
    /// Settlement point price, USD/MWh.
    #[serde(default)]
    pub spp: Option<f64>,
}

/// Locational marginal price row (PJM, CAISO, and the other ISO LMP datasets).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct LmpRow {
    /// Interval start (UTC).
    pub interval_start_utc: DateTime<Utc>,
    /// Interval end (UTC).
    pub interval_end_utc: DateTime<Utc>,
    /// Market name, e.g. `DAY_AHEAD_HOURLY`.
    #[serde(default)]
    pub market: Option<String>,
    /// Pricing node name.
    pub location: String,
    /// Numeric node id (PJM).
    #[serde(default)]
    pub location_id: Option<i64>,
    /// Short node name (PJM).
    #[serde(default)]
    pub location_short_name: Option<String>,
    /// Node type, e.g. `LOAD`, `HUB`.
    #[serde(default)]
    pub location_type: Option<String>,
    /// Total LMP, USD/MWh.
    #[serde(default)]
    pub lmp: Option<f64>,
    /// Energy component.
    #[serde(default)]
    pub energy: Option<f64>,
    /// Congestion component.
    #[serde(default)]
    pub congestion: Option<f64>,
    /// Loss component.
    #[serde(default)]
    pub loss: Option<f64>,
    /// Greenhouse-gas component (CAISO only).
    #[serde(default)]
    pub ghg: Option<f64>,
}

/// Generation by fuel for one interval. Fuel names differ per ISO
/// (`natural_gas` vs `gas`, `coal_and_lignite` vs `coal`), so they are kept as
/// a map rather than fixed fields.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct FuelMixRow {
    /// Interval start (UTC).
    pub interval_start_utc: DateTime<Utc>,
    /// Interval end (UTC).
    pub interval_end_utc: DateTime<Utc>,
    #[serde(flatten)]
    columns: BTreeMap<String, Value>,
}

impl FuelMixRow {
    /// Megawatts by fuel (numeric columns only), sorted by fuel name.
    pub fn megawatts(&self) -> BTreeMap<&str, f64> {
        numeric(&self.columns)
    }

    /// Megawatts for one fuel.
    pub fn fuel(&self, name: &str) -> Option<f64> {
        self.columns.get(name).and_then(Value::as_f64)
    }

    /// Sum over all fuels. Note that storage and imports can be negative.
    pub fn total_megawatts(&self) -> f64 {
        self.megawatts().values().sum()
    }
}

/// System load for one interval, plus per-zone columns where the ISO has them
/// (PJM publishes `pjm_rto`, `comed`, `dom`, ...).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct LoadRow {
    /// Interval start (UTC).
    pub interval_start_utc: DateTime<Utc>,
    /// Interval end (UTC).
    pub interval_end_utc: DateTime<Utc>,
    /// System load, MW.
    #[serde(default)]
    pub load: Option<f64>,
    #[serde(flatten)]
    columns: BTreeMap<String, Value>,
}

impl LoadRow {
    /// Load for a named zone column (PJM and others).
    pub fn zone(&self, name: &str) -> Option<f64> {
        self.columns.get(name).and_then(Value::as_f64)
    }

    /// All numeric zone columns, sorted by name.
    pub fn zones(&self) -> BTreeMap<&str, f64> {
        numeric(&self.columns)
    }
}

fn numeric(columns: &BTreeMap<String, Value>) -> BTreeMap<&str, f64> {
    columns
        .iter()
        .filter_map(|(k, v)| v.as_f64().map(|n| (k.as_str(), n)))
        .collect()
}

impl Client {
    /// ERCOT day-ahead hourly settlement point prices.
    pub async fn ercot_spp_day_ahead_hourly(&self, query: &Query) -> Result<QueryPage<SppRow>> {
        self.query(ids::ERCOT_SPP_DAY_AHEAD_HOURLY, query).await
    }

    /// ERCOT real-time 15-minute settlement point prices.
    pub async fn ercot_spp_real_time_15_minute(&self, query: &Query) -> Result<QueryPage<SppRow>> {
        self.query(ids::ERCOT_SPP_REAL_TIME_15_MIN, query).await
    }

    /// PJM day-ahead hourly LMPs.
    pub async fn pjm_lmp_day_ahead_hourly(&self, query: &Query) -> Result<QueryPage<LmpRow>> {
        self.query(ids::PJM_LMP_DAY_AHEAD_HOURLY, query).await
    }

    /// CAISO day-ahead hourly LMPs.
    pub async fn caiso_lmp_day_ahead_hourly(&self, query: &Query) -> Result<QueryPage<LmpRow>> {
        self.query(ids::CAISO_LMP_DAY_AHEAD_HOURLY, query).await
    }

    /// Day-ahead hourly LMPs for any ISO that publishes them (not ERCOT).
    pub async fn lmp_day_ahead_hourly(&self, iso: Iso, query: &Query) -> Result<QueryPage<LmpRow>> {
        let id = iso.lmp_day_ahead_hourly_dataset().ok_or_else(|| {
            Error::InvalidRequest(
                "ERCOT has no LMP hub dataset here; use ercot_spp_day_ahead_hourly".to_owned(),
            )
        })?;
        self.query(&id, query).await
    }

    /// Generation by fuel (`{iso}_fuel_mix`).
    pub async fn fuel_mix(&self, iso: Iso, query: &Query) -> Result<QueryPage<FuelMixRow>> {
        self.query(&iso.fuel_mix_dataset(), query).await
    }

    /// System load (`{iso}_load`).
    pub async fn load(&self, iso: Iso, query: &Query) -> Result<QueryPage<LoadRow>> {
        self.query(&iso.load_dataset(), query).await
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn dataset_ids_follow_catalog_naming() {
        assert_eq!(Iso::Pjm.fuel_mix_dataset(), "pjm_fuel_mix");
        assert_eq!(Iso::Caiso.load_dataset(), "caiso_load");
        assert_eq!(
            Iso::Isone.lmp_day_ahead_hourly_dataset().as_deref(),
            Some("isone_lmp_day_ahead_hourly")
        );
        assert_eq!(Iso::Ercot.lmp_day_ahead_hourly_dataset(), None);
        assert_eq!(Iso::ALL.len(), 7);
    }
}

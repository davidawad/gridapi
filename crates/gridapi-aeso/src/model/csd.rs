//! Current supply and demand (CSD): the live system snapshot and asset-level generation.

use chrono::{DateTime, NaiveDateTime, Utc};
use serde::Deserialize;

use crate::serde_util::{local, opt_f64, opt_text, utc};

/// Aggregated generation for one fuel group.
#[derive(Debug, Clone, PartialEq, Deserialize)]
pub struct FuelGroup {
    /// Fuel group, for example `WIND`, `COGENERATION`, `COMBINED CYCLE`.
    pub fuel_type: String,
    /// Sum of maximum capability in MW.
    #[serde(default, deserialize_with = "opt_f64")]
    pub aggregated_maximum_capability: Option<f64>,
    /// Sum of net generation in MW.
    #[serde(default, deserialize_with = "opt_f64")]
    pub aggregated_net_generation: Option<f64>,
    /// Sum of dispatched contingency reserve in MW.
    #[serde(default, deserialize_with = "opt_f64")]
    pub aggregated_dispatched_contingency_reserve: Option<f64>,
}

/// Actual flow on one intertie path.
#[derive(Debug, Clone, PartialEq, Deserialize)]
pub struct InterchangeFlow {
    /// Path name: `British Columbia`, `Montana` or `Saskatchewan`.
    pub path: String,
    /// Actual flow in MW; positive is an import into Alberta.
    #[serde(default, deserialize_with = "opt_f64")]
    pub actual_flow: Option<f64>,
}

/// The CSD summary (`currentsupplydemand-api/v2/csd/summary/current`).
#[derive(Debug, Clone, PartialEq, Deserialize)]
pub struct SupplyDemandSummary {
    /// Time the snapshot is effective (UTC).
    #[serde(rename = "effective_datetime_utc", deserialize_with = "utc")]
    pub effective_utc: DateTime<Utc>,
    /// Time the snapshot is effective (Mountain prevailing time).
    #[serde(rename = "effective_datetime_mpt", deserialize_with = "local")]
    pub effective_mpt: NaiveDateTime,
    /// Total maximum generation capability in MW.
    #[serde(default, deserialize_with = "opt_f64")]
    pub total_max_generation_capability: Option<f64>,
    /// Total net generation in MW.
    #[serde(default, deserialize_with = "opt_f64")]
    pub total_net_generation: Option<f64>,
    /// Net-to-grid generation in MW.
    #[serde(default, deserialize_with = "opt_f64")]
    pub net_to_grid_generation: Option<f64>,
    /// Net actual interchange in MW; positive is net import.
    #[serde(default, deserialize_with = "opt_f64")]
    pub net_actual_interchange: Option<f64>,
    /// Alberta internal load in MW.
    #[serde(default, deserialize_with = "opt_f64")]
    pub alberta_internal_load: Option<f64>,
    /// Required contingency reserve in MW.
    #[serde(default, deserialize_with = "opt_f64")]
    pub contingency_reserve_required: Option<f64>,
    /// Total dispatched contingency reserve in MW. (The API spells the field
    /// `dispatched_contigency_reserve_total`; this crate fixes the name.)
    #[serde(
        rename = "dispatched_contigency_reserve_total",
        alias = "dispatched_contingency_reserve_total",
        default,
        deserialize_with = "opt_f64"
    )]
    pub dispatched_contingency_reserve_total: Option<f64>,
    /// Dispatched contingency reserve from generation in MW.
    #[serde(default, deserialize_with = "opt_f64")]
    pub dispatched_contingency_reserve_gen: Option<f64>,
    /// Dispatched contingency reserve from other sources in MW.
    #[serde(default, deserialize_with = "opt_f64")]
    pub dispatched_contingency_reserve_other: Option<f64>,
    /// Fast frequency response armed for dispatch in MW.
    #[serde(default, deserialize_with = "opt_f64")]
    pub ffr_armed_dispatch: Option<f64>,
    /// Fast frequency response offered volume in MW.
    #[serde(default, deserialize_with = "opt_f64")]
    pub ffr_offered_volume: Option<f64>,
    /// Long lead time volume in MW.
    #[serde(default, deserialize_with = "opt_f64")]
    pub long_lead_time_volume: Option<f64>,
    /// Generation by fuel group.
    #[serde(default)]
    pub generation_data_list: Vec<FuelGroup>,
    /// Actual interchange by path.
    #[serde(default)]
    pub interchange_list: Vec<InterchangeFlow>,
}

impl SupplyDemandSummary {
    /// Flow on the named path, if reported.
    pub fn flow(&self, path: &str) -> Option<f64> {
        self.interchange_list
            .iter()
            .find(|f| f.path.eq_ignore_ascii_case(path))
            .and_then(|f| f.actual_flow)
    }

    /// The fuel group named `fuel_type` (case-insensitive), if reported.
    pub fn fuel(&self, fuel_type: &str) -> Option<&FuelGroup> {
        self.generation_data_list
            .iter()
            .find(|g| g.fuel_type.eq_ignore_ascii_case(fuel_type))
    }
}

/// One generating asset's current output.
#[derive(Debug, Clone, PartialEq, Deserialize)]
pub struct GenerationAsset {
    /// Asset id, for example `AST1`.
    pub asset: String,
    /// Fuel type: `GAS`, `WIND`, `SOLAR`, `HYDRO`, `ENERGY STORAGE`, `OTHER`.
    pub fuel_type: String,
    /// Sub-fuel type (`COGENERATION`, `SIMPLE_CYCLE`, `COMBINED_CYCLE`,
    /// `GAS_FIRED_STEAM`) when the fuel has one.
    #[serde(default, deserialize_with = "opt_text")]
    pub sub_fuel_type: Option<String>,
    /// Maximum capability in MW.
    #[serde(default, deserialize_with = "opt_f64")]
    pub maximum_capability: Option<f64>,
    /// Net generation in MW.
    #[serde(default, deserialize_with = "opt_f64")]
    pub net_generation: Option<f64>,
    /// Dispatched contingency reserve in MW.
    #[serde(default, deserialize_with = "opt_f64")]
    pub dispatched_contingency_reserve: Option<f64>,
}

/// The asset-level CSD generation list.
#[derive(Debug, Clone, PartialEq, Deserialize)]
pub struct GenerationAssets {
    /// When the list was last updated (UTC).
    #[serde(rename = "last_updated_datetime_utc", deserialize_with = "utc")]
    pub last_updated_utc: DateTime<Utc>,
    /// When the list was last updated (Mountain prevailing time).
    #[serde(rename = "last_updated_datetime_mpt", deserialize_with = "local")]
    pub last_updated_mpt: NaiveDateTime,
    /// One entry per asset.
    #[serde(default)]
    pub asset_list: Vec<GenerationAsset>,
}

impl GenerationAssets {
    /// Total net generation in MW over assets of `fuel_type` (case-insensitive).
    pub fn net_generation_by_fuel(&self, fuel_type: &str) -> f64 {
        self.asset_list
            .iter()
            .filter(|a| a.fuel_type.eq_ignore_ascii_case(fuel_type))
            .filter_map(|a| a.net_generation)
            .sum()
    }
}

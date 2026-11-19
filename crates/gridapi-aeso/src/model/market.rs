//! Market microdata: the energy merit order, operating reserve offers,
//! metered volumes and unit commitment directives.

use chrono::{DateTime, NaiveDateTime, Utc};
use serde::{Deserialize, Deserializer};

use crate::serde_util::{local, opt_f64, opt_text, u8_lenient, utc, yes_no};

/// Import or export marker on a merit order block.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Direction {
    /// `I`: an import offer.
    Import,
    /// `E`: an export bid.
    Export,
}

fn direction<'de, D: Deserializer<'de>>(d: D) -> Result<Option<Direction>, D::Error> {
    match Option::<String>::deserialize(d)?.as_deref().map(str::trim) {
        None | Some("") => Ok(None),
        Some("I") => Ok(Some(Direction::Import)),
        Some("E") => Ok(Some(Direction::Export)),
        Some(other) => Err(serde::de::Error::custom(format!(
            "expected I, E or empty, got {other:?}"
        ))),
    }
}

/// One offer block in the energy merit order.
#[derive(Debug, Clone, PartialEq, Deserialize)]
pub struct EnergyBlock {
    /// Import/export marker; `None` for in-province generation and load.
    #[serde(default, deserialize_with = "direction")]
    pub import_or_export: Option<Direction>,
    /// Asset id, for example `AST3`.
    #[serde(rename = "asset_ID")]
    pub asset_id: String,
    /// Block number, `0..=6`.
    #[serde(deserialize_with = "u8_lenient")]
    pub block_number: u8,
    /// Offer price in CAD/MWh.
    #[serde(default, deserialize_with = "opt_f64")]
    pub block_price: Option<f64>,
    /// Cumulative MW before this block.
    #[serde(rename = "from_MW", default, deserialize_with = "opt_f64")]
    pub from_mw: Option<f64>,
    /// Cumulative MW through this block.
    #[serde(rename = "to_MW", default, deserialize_with = "opt_f64")]
    pub to_mw: Option<f64>,
    /// Block size in MW.
    #[serde(default, deserialize_with = "opt_f64")]
    pub block_size: Option<f64>,
    /// MW available to the system controller for dispatch.
    #[serde(rename = "available_MW", default, deserialize_with = "opt_f64")]
    pub available_mw: Option<f64>,
    /// Whether the block was dispatched.
    #[serde(rename = "dispatched?", default, deserialize_with = "yes_no")]
    pub dispatched: bool,
    /// Dispatched MW.
    #[serde(rename = "dispatched_MW", default, deserialize_with = "opt_f64")]
    pub dispatched_mw: Option<f64>,
    /// Whether the block can be partially dispatched.
    #[serde(rename = "flexible?", default, deserialize_with = "yes_no")]
    pub flexible: bool,
    /// Offer control parties, `;`-separated.
    #[serde(default)]
    pub offer_control: String,
}

/// The merit order for one settlement hour.
#[derive(Debug, Clone, PartialEq, Deserialize)]
pub struct MeritOrderHour {
    /// Start of the hour (UTC).
    #[serde(rename = "begin_dateTime_utc", deserialize_with = "utc")]
    pub begin_utc: DateTime<Utc>,
    /// Start of the hour (Mountain prevailing time).
    #[serde(rename = "begin_dateTime_mpt", deserialize_with = "local")]
    pub begin_mpt: NaiveDateTime,
    /// Every offer block, in the order AESO lists them.
    #[serde(default)]
    pub energy_blocks: Vec<EnergyBlock>,
}

impl MeritOrderHour {
    /// The highest-priced dispatched block: the marginal offer. `None` when
    /// nothing was dispatched or no block carried a price.
    pub fn marginal_block(&self) -> Option<&EnergyBlock> {
        self.energy_blocks
            .iter()
            .filter(|b| b.dispatched && b.block_price.is_some())
            .max_by(|a, b| {
                a.block_price
                    .partial_cmp(&b.block_price)
                    .unwrap_or(std::cmp::Ordering::Equal)
            })
    }
}

/// Wire shape of the merit order payload.
#[derive(Debug, Deserialize)]
pub(crate) struct MeritOrderPayload {
    #[serde(default)]
    pub(crate) data: Vec<MeritOrderHour>,
}

/// One operating reserve offer block.
#[derive(Debug, Clone, PartialEq, Deserialize)]
pub struct ReserveBlock {
    /// `ACTIVE` or `STANDBY`.
    pub commodity: String,
    /// Product: `RR` (regulating), `SR` (spinning), `SUPL`/`SUPG` (supplemental).
    pub product: String,
    /// Asset id.
    #[serde(rename = "asset_ID")]
    pub asset_id: String,
    /// Offered volume in MW.
    #[serde(default, deserialize_with = "opt_f64")]
    pub volume: Option<f64>,
    /// Active price in CAD/MW.
    #[serde(default, deserialize_with = "opt_f64")]
    pub active_price: Option<f64>,
    /// Premium price in CAD/MW.
    #[serde(default, deserialize_with = "opt_f64")]
    pub premium_price: Option<f64>,
    /// Activation price in CAD/MWh.
    #[serde(default, deserialize_with = "opt_f64")]
    pub activation_price: Option<f64>,
    /// Offer control parties.
    #[serde(default)]
    pub offer_control: String,
}

/// Operating reserve offers for one settlement hour.
#[derive(Debug, Clone, PartialEq, Deserialize)]
pub struct ReserveHour {
    /// Start of the hour (UTC).
    #[serde(rename = "begin_datetime_utc", deserialize_with = "utc")]
    pub begin_utc: DateTime<Utc>,
    /// Start of the hour (Mountain prevailing time).
    #[serde(rename = "begin_datetime_mpt", deserialize_with = "local")]
    pub begin_mpt: NaiveDateTime,
    /// Offer blocks.
    #[serde(default)]
    pub operating_reserve_blocks: Vec<ReserveBlock>,
}

/// Wire shape of the operating reserve payload.
#[derive(Debug, Deserialize)]
pub(crate) struct ReservePayload {
    #[serde(rename = "Operating Reserve Trade Merit Order", default)]
    pub(crate) hours: Vec<ReserveHour>,
}

/// One metered hour.
#[derive(Debug, Clone, PartialEq, Deserialize)]
pub struct MeteredHour {
    /// Start of the hour (UTC).
    #[serde(rename = "begin_date_utc", deserialize_with = "utc")]
    pub begin_utc: DateTime<Utc>,
    /// Start of the hour (Mountain prevailing time).
    #[serde(rename = "begin_date_mpt", deserialize_with = "local")]
    pub begin_mpt: NaiveDateTime,
    /// Metered volume in `MWh`.
    #[serde(default, deserialize_with = "opt_f64")]
    pub metered_volume: Option<f64>,
}

/// One asset's metered volumes.
#[derive(Debug, Clone, PartialEq, Deserialize)]
pub struct MeteredAsset {
    /// Asset id.
    #[serde(rename = "asset_ID")]
    pub asset_id: String,
    /// Asset class (`IPP`, `GEN`, `LOAD`, ...).
    #[serde(default, deserialize_with = "opt_text")]
    pub asset_class: Option<String>,
    /// Hourly volumes.
    #[serde(default)]
    pub metered_volume_list: Vec<MeteredHour>,
}

/// One pool participant's metered volumes.
#[derive(Debug, Clone, PartialEq, Deserialize)]
pub struct MeteredParticipant {
    /// Pool participant id.
    #[serde(rename = "pool_participant_ID")]
    pub pool_participant_id: String,
    /// The participant's assets.
    #[serde(default)]
    pub asset_list: Vec<MeteredAsset>,
}

/// A unit commitment directive issued by the system controller.
#[derive(Debug, Clone, PartialEq, Deserialize)]
pub struct UnitCommitment {
    /// Asset id.
    #[serde(rename = "asset_ID")]
    pub asset_id: String,
    /// When the directive was issued (UTC).
    #[serde(rename = "issued_time_utc", deserialize_with = "utc")]
    pub issued_utc: DateTime<Utc>,
    /// When the directive was issued (Mountain prevailing time).
    #[serde(rename = "issued_time_mpt", deserialize_with = "local")]
    pub issued_mpt: NaiveDateTime,
    /// When the asset is expected to begin responding (UTC).
    #[serde(rename = "begin_time_utc", deserialize_with = "utc")]
    pub begin_utc: DateTime<Utc>,
    /// When the asset is expected to begin responding (Mountain prevailing time).
    #[serde(rename = "begin_time_mpt", deserialize_with = "local")]
    pub begin_mpt: NaiveDateTime,
    /// When the asset is told to synchronise (UTC).
    #[serde(rename = "operation_start_time_utc", deserialize_with = "utc")]
    pub operation_start_utc: DateTime<Utc>,
    /// When the asset is told to synchronise (Mountain prevailing time).
    #[serde(rename = "operation_start_time_mpt", deserialize_with = "local")]
    pub operation_start_mpt: NaiveDateTime,
    /// When the asset is no longer required (UTC).
    #[serde(rename = "operation_end_time_utc", deserialize_with = "utc")]
    pub operation_end_utc: DateTime<Utc>,
    /// When the asset is no longer required (Mountain prevailing time).
    #[serde(rename = "operation_end_time_mpt", deserialize_with = "local")]
    pub operation_end_mpt: NaiveDateTime,
}

/// Wire shape of the unit commitment payload.
#[derive(Debug, Deserialize)]
pub(crate) struct UnitCommitmentPayload {
    #[serde(default)]
    pub(crate) unit_commitment: Vec<UnitCommitment>,
}

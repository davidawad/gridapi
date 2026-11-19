//! Intertie data from the ITC API: transfer capability and the outages that
//! limit it. (Live actual flows are in [`super::csd::SupplyDemandSummary::interchange_list`].)

use chrono::{NaiveDate, NaiveDateTime};
use serde::{Deserialize, Deserializer};

use crate::serde_util::{local, opt_f64, opt_text, u8_lenient};

/// One transfer-capability series for one hour and direction, in MW.
#[derive(Debug, Clone, PartialEq, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Transfer {
    /// For example `BC_IMPORT`, `MATL_EXPORT`, `SYSTEM_IMPORT`.
    pub transfer_type: String,
    /// Why the limit is below nominal, if AESO says (for example
    /// `ISO rules Section 205.9 Limit`).
    #[serde(default, deserialize_with = "opt_text")]
    pub reason: Option<String>,
    /// Available transfer capability.
    #[serde(default, deserialize_with = "opt_f64")]
    pub atc: Option<f64>,
    /// Total transmission reliability margin.
    #[serde(default, deserialize_with = "opt_f64")]
    pub trm_total: Option<f64>,
    /// Total transfer capability.
    #[serde(default, deserialize_with = "opt_f64")]
    pub ttc: Option<f64>,
    /// System transmission reliability margin.
    #[serde(default, deserialize_with = "opt_f64")]
    pub trm_system: Option<f64>,
    /// Allocated transmission reliability margin (absent on flowgates).
    #[serde(default, deserialize_with = "opt_f64")]
    pub trm_allocation: Option<f64>,
    /// Gross offer.
    #[serde(default, deserialize_with = "opt_f64")]
    pub gross_offer: Option<f64>,
    /// When the values take effect, as AESO writes it (`2026-11-16 00:00:00 MST`).
    #[serde(default)]
    pub effective_local_time: String,
    /// When the values were last updated, as AESO writes it.
    #[serde(default)]
    pub updated_local_time: String,
    /// Earlier revisions, present only when requested with [`crate::Revisions::All`].
    #[serde(default)]
    pub version: Vec<TransferVersion>,
}

/// A superseded revision of a [`Transfer`].
#[derive(Debug, Clone, PartialEq, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TransferVersion {
    /// Reason text.
    #[serde(default, deserialize_with = "opt_text")]
    pub reason: Option<String>,
    /// Available transfer capability.
    #[serde(default, deserialize_with = "opt_f64")]
    pub atc: Option<f64>,
    /// Total transmission reliability margin.
    #[serde(default, deserialize_with = "opt_f64")]
    pub trm_total: Option<f64>,
    /// Total transfer capability.
    #[serde(default, deserialize_with = "opt_f64")]
    pub ttc: Option<f64>,
    /// System transmission reliability margin.
    #[serde(default, deserialize_with = "opt_f64")]
    pub trm_system: Option<f64>,
    /// Allocated transmission reliability margin.
    #[serde(default, deserialize_with = "opt_f64")]
    pub trm_allocation: Option<f64>,
    /// Gross offer.
    #[serde(default, deserialize_with = "opt_f64")]
    pub gross_offer: Option<f64>,
    /// When this revision was written, as AESO writes it.
    #[serde(default, rename = "versionUpdatedLocaltime")]
    pub version_updated_local_time: String,
}

/// One hour's import and export capability on an intertie or flowgate.
#[derive(Debug, Clone, PartialEq, Deserialize)]
pub struct Allocation {
    /// True for a flowgate (a combination of interties), false for an intertie.
    #[serde(default)]
    pub flowgate: bool,
    /// Hour ending, `1..=24`.
    #[serde(deserialize_with = "u8_lenient")]
    pub he: u8,
    /// Delivery date.
    pub date: NaiveDate,
    /// Import capability.
    #[serde(default)]
    pub import: Option<Transfer>,
    /// Export capability.
    #[serde(default)]
    pub export: Option<Transfer>,
}

/// All hours for one intertie or flowgate.
#[derive(Debug, Clone, PartialEq, Default, Deserialize)]
pub struct Allocations {
    /// One entry per hour (and per revision when revisions were requested).
    #[serde(rename = "Allocations", default)]
    pub allocations: Vec<Allocation>,
}

/// The interchange capability report.
#[derive(Debug, Clone, PartialEq, Default, Deserialize)]
pub struct InterchangeReport {
    /// Alberta-British Columbia intertie.
    #[serde(rename = "BcIntertie", default)]
    pub bc: Option<Allocations>,
    /// Alberta-Montana (MATL) intertie.
    #[serde(rename = "MatlIntertie", default)]
    pub matl: Option<Allocations>,
    /// Alberta-Saskatchewan intertie.
    #[serde(rename = "SkIntertie", default)]
    pub sk: Option<Allocations>,
    /// Combined BC + MATL flowgate.
    #[serde(rename = "BcMatlFlowgate", default)]
    pub bc_matl_flowgate: Option<Allocations>,
    /// System flowgate. (The API spells the key `SystemlFlowgate`.)
    #[serde(rename = "SystemlFlowgate", alias = "SystemFlowgate", default)]
    pub system_flowgate: Option<Allocations>,
}

impl InterchangeReport {
    /// Every allocation across all interties and flowgates.
    pub fn all(&self) -> impl Iterator<Item = &Allocation> {
        [
            &self.bc,
            &self.matl,
            &self.sk,
            &self.bc_matl_flowgate,
            &self.system_flowgate,
        ]
        .into_iter()
        .flatten()
        .flat_map(|a| a.allocations.iter())
    }
}

/// A line affected by an outage, for example display `BC/MATL` over lines `["BC_MATL"]`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AffectedLine {
    /// Human label.
    pub display_value: String,
    /// Machine names of the affected lines (`BC`, `MATL`, `SK`, `BC_MATL`).
    pub affected_lines: Vec<String>,
}

impl<'de> Deserialize<'de> for AffectedLine {
    fn deserialize<D: Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        #[derive(Deserialize)]
        #[serde(untagged)]
        enum Raw {
            Detailed {
                #[serde(rename = "displayValue")]
                display_value: String,
                #[serde(rename = "affectedLines", default)]
                affected_lines: Vec<String>,
            },
            Plain(String),
        }
        Ok(match Raw::deserialize(d)? {
            Raw::Detailed {
                display_value,
                affected_lines,
            } => Self {
                display_value,
                affected_lines,
            },
            Raw::Plain(name) => Self {
                display_value: name.clone(),
                affected_lines: vec![name],
            },
        })
    }
}

/// An outage that reduces transfer capability.
#[derive(Debug, Clone, PartialEq, Deserialize)]
pub struct IntertieOutage {
    /// The transmission element, for example `Path 1 Outage - AIES Islanding`.
    pub element: String,
    /// Lines the outage affects.
    #[serde(rename = "affectedIntertieOrFlowgate", default)]
    pub affected: Vec<AffectedLine>,
    /// Outage start (Mountain prevailing time).
    #[serde(rename = "fromInLocalTime", deserialize_with = "local")]
    pub from_local: NaiveDateTime,
    /// Outage end (Mountain prevailing time).
    #[serde(rename = "toInLocalTime", deserialize_with = "local")]
    pub to_local: NaiveDateTime,
}

/// Wire shape of the outage payload.
#[derive(Debug, Default, Deserialize)]
pub(crate) struct OutagesPayload {
    #[serde(rename = "Outages", default)]
    pub(crate) outages: OutageList,
}

/// Wire shape of the outage list.
#[derive(Debug, Default, Deserialize)]
pub(crate) struct OutageList {
    #[serde(rename = "Outage", default)]
    pub(crate) outage: Vec<IntertieOutage>,
}

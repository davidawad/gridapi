//! `IntertieScheduleFlow`: scheduled and actual flows on Ontario's interties.

use chrono::NaiveDate;
use serde::Deserialize;

use crate::data::xml::{parse_document, Document};
use crate::error::Result;
use crate::serde_util::{children, opt_f64};

/// Hourly scheduled transfer in MW.
#[derive(Debug, Clone, PartialEq, Deserialize)]
pub struct IntertieSchedule {
    /// Delivery hour, hour-ending `1..=24`.
    #[serde(rename = "Hour")]
    pub hour: u8,
    /// Scheduled import into Ontario in MW.
    #[serde(rename = "Import", default, deserialize_with = "opt_f64")]
    pub import_mw: Option<f64>,
    /// Scheduled export out of Ontario in MW.
    #[serde(rename = "Export", default, deserialize_with = "opt_f64")]
    pub export_mw: Option<f64>,
}

/// Actual five-minute flow in MW (positive is import, negative export).
#[derive(Debug, Clone, PartialEq, Deserialize)]
pub struct IntertieFlow {
    /// Delivery hour, hour-ending `1..=24`.
    #[serde(rename = "Hour")]
    pub hour: u8,
    /// Interval within the hour, `1..=12`.
    #[serde(rename = "Interval")]
    pub interval: u8,
    /// Flow in MW.
    #[serde(rename = "Flow", default, deserialize_with = "opt_f64")]
    pub flow_mw: Option<f64>,
}

/// One intertie (for example `MICHIGAN`, `NEW-YORK`, `PQ.AT`).
#[derive(Debug, Clone, PartialEq, Deserialize)]
pub struct IntertieZone {
    /// Intertie zone name.
    #[serde(rename = "IntertieZoneName")]
    pub name: String,
    /// Hourly schedules.
    #[serde(rename = "Schedules", default, deserialize_with = "children")]
    pub schedules: Vec<IntertieSchedule>,
    /// Five-minute actual flows.
    #[serde(rename = "Actuals", default, deserialize_with = "children")]
    pub actuals: Vec<IntertieFlow>,
}

/// Sum over all interties.
#[derive(Debug, Clone, PartialEq, Deserialize)]
pub struct IntertieTotals {
    /// Hourly total schedules.
    #[serde(rename = "Schedules", default, deserialize_with = "children")]
    pub schedules: Vec<IntertieSchedule>,
    /// Five-minute total actual flows.
    #[serde(rename = "Actuals", default, deserialize_with = "children")]
    pub actuals: Vec<IntertieFlow>,
}

/// `IntertieScheduleFlow` for one delivery date.
#[derive(Debug, Clone, PartialEq, Deserialize)]
pub struct IntertieScheduleFlow {
    /// The delivery date.
    #[serde(rename = "Date")]
    pub date: NaiveDate,
    /// One entry per intertie.
    #[serde(rename = "IntertieZone", default)]
    pub zones: Vec<IntertieZone>,
    /// Totals over all interties, when present.
    #[serde(rename = "Totals", default)]
    pub totals: Option<IntertieTotals>,
}

impl IntertieScheduleFlow {
    /// The intertie called `name` (exact match), if present.
    pub fn zone(&self, name: &str) -> Option<&IntertieZone> {
        self.zones.iter().find(|z| z.name == name)
    }
}

/// Decode `IntertieScheduleFlow`.
pub fn parse_intertie_schedule_flow(xml: &str) -> Result<Document<IntertieScheduleFlow>> {
    parse_document(xml)
}

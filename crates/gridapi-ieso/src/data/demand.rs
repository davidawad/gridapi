//! Hourly demand report (CSV, one file per year).
//!
//! ```text
//! \\Hourly Demand Report,,,
//! \\Created at 2026-11-17 07:30:13,,,
//! \\For 2026,,,
//! Date,Hour,Market Demand,Ontario Demand
//! 2026-01-01,1,15424,19643
//! ```

use chrono::{NaiveDate, NaiveDateTime};
use serde::Deserialize;

use crate::error::{Error, Result};

/// One hour of demand in MW.
#[derive(Debug, Clone, PartialEq, Deserialize)]
pub struct DemandRow {
    /// Delivery date.
    #[serde(rename = "Date")]
    pub date: NaiveDate,
    /// Delivery hour, hour-ending `1..=24`.
    #[serde(rename = "Hour")]
    pub hour: u8,
    /// Market demand in MW (Ontario demand plus exports).
    #[serde(rename = "Market Demand")]
    pub market_demand: Option<f64>,
    /// Ontario demand in MW.
    #[serde(rename = "Ontario Demand")]
    pub ontario_demand: Option<f64>,
}

/// A decoded demand file.
#[derive(Debug, Clone, PartialEq)]
pub struct DemandReport {
    /// When IESO produced the file, from the `\\Created at` comment line.
    pub created_at: Option<NaiveDateTime>,
    /// One row per delivery hour.
    pub rows: Vec<DemandRow>,
}

/// Decode a `Demand` yearly CSV.
pub fn parse_demand(csv_text: &str) -> Result<DemandReport> {
    let text = csv_text.trim_start_matches('\u{feff}');
    let created_at = text
        .lines()
        .find_map(|l| l.strip_prefix("\\\\Created at "))
        .and_then(|rest| rest.trim_end_matches(',').trim().get(..19))
        .and_then(|s| NaiveDateTime::parse_from_str(s, "%Y-%m-%d %H:%M:%S").ok());
    // Byte offset of the header row; searching for the text (not summing line
    // lengths) keeps this correct for both LF and CRLF files.
    let header_at = if text.starts_with("Date,") {
        Some(0)
    } else {
        text.find("\nDate,").map(|i| i + 1)
    }
    .ok_or_else(|| Error::Malformed("demand file has no `Date,Hour,...` header".into()))?;
    let rows = csv::ReaderBuilder::new()
        .trim(csv::Trim::All)
        .from_reader(&text.as_bytes()[header_at..])
        .deserialize::<DemandRow>()
        .collect::<std::result::Result<Vec<_>, _>>()?;
    Ok(DemandReport { created_at, rows })
}

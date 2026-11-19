//! Wind and solar actual production at 5-minute resolution.

use chrono::NaiveDateTime;

use crate::error::Result;
use crate::page::{FromRow, Row};
use crate::request::{Query, Report};

// ---------------------------------------------------------------------------
// NP4-733-CD

/// NP4-733-CD: wind power production, actual 5-minute averaged values.
#[derive(Debug, Clone, Copy)]
pub struct WindActual5Min;

/// Wind production for one 5-minute interval.
#[derive(Debug, Clone, PartialEq)]
pub struct WindActual5MinRecord {
    /// When ERCOT posted the value (Central time).
    pub posted: NaiveDateTime,
    /// End of the 5-minute interval (Central time).
    pub interval_ending: NaiveDateTime,
    /// System-wide generation, MW.
    pub gen_system_wide: f64,
    /// Generation in the South/Houston region, MW.
    pub lz_south_houston: Option<f64>,
    /// Generation in the West region, MW.
    pub lz_west: Option<f64>,
    /// Generation in the North region, MW.
    pub lz_north: Option<f64>,
    /// System-wide high sustained limit, MW.
    pub hsl_system_wide: Option<f64>,
    /// True during the repeated hour at the autumn clock change.
    pub dst_flag: bool,
}

impl FromRow for WindActual5MinRecord {
    fn from_row(row: &Row<'_>) -> Result<Self> {
        Ok(Self {
            posted: row.datetime("postedDatetime")?,
            interval_ending: row.datetime("intervalEnding")?,
            gen_system_wide: row.f64("genSystemWide")?,
            lz_south_houston: row.opt_f64("LZSouthHouston")?,
            lz_west: row.opt_f64("LZWest")?,
            lz_north: row.opt_f64("LZNorth")?,
            hsl_system_wide: row.opt_f64("HSLSystemWide")?,
            dst_flag: row.bool("DSTFlag")?,
        })
    }
}

impl Report for WindActual5Min {
    type Record = WindActual5MinRecord;
    const EMIL_ID: &'static str = "NP4-733-CD";
    const ARTIFACT: &'static str = "wpp_actual_5min_avg_values";
}

impl Query<WindActual5Min> {
    /// Restricts to intervals ending within `from..=to`.
    #[must_use]
    pub fn interval_ending(self, from: NaiveDateTime, to: NaiveDateTime) -> Self {
        self.datetime_range("intervalEnding", from, to)
    }

    /// Restricts to values posted within `from..=to`.
    #[must_use]
    pub fn posted(self, from: NaiveDateTime, to: NaiveDateTime) -> Self {
        self.datetime_range("postedDatetime", from, to)
    }
}

// ---------------------------------------------------------------------------
// NP4-738-CD

/// NP4-738-CD: solar power production, actual 5-minute averaged values.
#[derive(Debug, Clone, Copy)]
pub struct SolarActual5Min;

/// Solar production for one 5-minute interval.
#[derive(Debug, Clone, PartialEq)]
pub struct SolarActual5MinRecord {
    /// When ERCOT posted the value (Central time).
    pub posted: NaiveDateTime,
    /// End of the 5-minute interval (Central time).
    pub interval_ending: NaiveDateTime,
    /// System-wide generation, MW.
    pub gen_system_wide: f64,
    /// System-wide high sustained limit, MW.
    pub hsl_system_wide: Option<f64>,
    /// True during the repeated hour at the autumn clock change.
    pub dst_flag: bool,
}

impl FromRow for SolarActual5MinRecord {
    fn from_row(row: &Row<'_>) -> Result<Self> {
        Ok(Self {
            posted: row.datetime("postedDatetime")?,
            interval_ending: row.datetime("intervalEnding")?,
            gen_system_wide: row.f64("genSystemWide")?,
            hsl_system_wide: row.opt_f64("HSLSystemWide")?,
            dst_flag: row.bool("DSTFlag")?,
        })
    }
}

impl Report for SolarActual5Min {
    type Record = SolarActual5MinRecord;
    const EMIL_ID: &'static str = "NP4-738-CD";
    const ARTIFACT: &'static str = "spp_actual_5min_avg_values";
}

impl Query<SolarActual5Min> {
    /// Restricts to intervals ending within `from..=to`.
    #[must_use]
    pub fn interval_ending(self, from: NaiveDateTime, to: NaiveDateTime) -> Self {
        self.datetime_range("intervalEnding", from, to)
    }

    /// Restricts to values posted within `from..=to`.
    #[must_use]
    pub fn posted(self, from: NaiveDateTime, to: NaiveDateTime) -> Self {
        self.datetime_range("postedDatetime", from, to)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::page::ReportPage;

    #[test]
    fn wind_fixture_parses() {
        let recs = ReportPage::from_json(include_bytes!("../../tests/fixtures/wind_5min.json"))
            .expect("page")
            .records::<WindActual5MinRecord>()
            .expect("records");
        assert_eq!(recs.len(), 2);
        assert!((recs[0].gen_system_wide - 21_735.09).abs() < 1e-6);
        assert_eq!(recs[0].lz_west, Some(13_694.38));
        assert_eq!(recs[0].interval_ending.to_string(), "2026-11-17 23:50:00");
    }

    #[test]
    fn solar_fixture_parses() {
        let recs = ReportPage::from_json(include_bytes!("../../tests/fixtures/solar_5min.json"))
            .expect("page")
            .records::<SolarActual5MinRecord>()
            .expect("records");
        assert_eq!(recs.len(), 2);
        assert!(
            (recs[0].gen_system_wide - 0.22).abs() < 1e-9,
            "night-time solar is near zero"
        );
        assert_eq!(recs[0].hsl_system_wide, Some(31.87));
    }

    #[test]
    fn null_optional_columns_become_none() {
        let body = br#"{"_meta":{"totalRecords":1,"pageSize":1,"totalPages":1,"currentPage":1},
          "report":{"reportName":"r","reportDisplayName":"R","reportEMIL":"NP4-738-CD","downloadLimit":1},
          "fields":[
           {"name":"postedDatetime","label":"","cardinality":1,"dataType":"DATETIME","searchable":true,"sortable":true,"hasRange":true},
           {"name":"intervalEnding","label":"","cardinality":2,"dataType":"DATETIME","searchable":true,"sortable":true,"hasRange":true},
           {"name":"genSystemWide","label":"","cardinality":3,"dataType":"DOUBLE","searchable":true,"sortable":true,"hasRange":true},
           {"name":"HSLSystemWide","label":"","cardinality":4,"dataType":"DOUBLE","searchable":true,"sortable":true,"hasRange":true},
           {"name":"DSTFlag","label":"","cardinality":5,"dataType":"BOOLEAN","searchable":true,"sortable":true,"hasRange":false}],
          "data":[["2026-11-18T00:00:10","2026-11-17T23:55:00",1.5,null,false]]}"#;
        let recs = ReportPage::from_json(body)
            .expect("page")
            .records::<SolarActual5MinRecord>()
            .expect("null HSL tolerated");
        assert_eq!(recs[0].hsl_system_wide, None);
    }
}

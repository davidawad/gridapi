//! Actual and forecast system load by weather zone.

use chrono::{NaiveDate, NaiveDateTime};

use crate::error::Result;
use crate::page::{FromRow, HourEnding, Row};
use crate::request::{Query, Report};

/// Load in megawatts for each ERCOT weather zone.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct WeatherZoneLoad {
    /// Coast.
    pub coast: f64,
    /// East.
    pub east: f64,
    /// Far West.
    pub far_west: f64,
    /// North.
    pub north: f64,
    /// North Central.
    pub north_central: f64,
    /// South Central.
    pub south_central: f64,
    /// Southern.
    pub southern: f64,
    /// West.
    pub west: f64,
}

impl WeatherZoneLoad {
    /// Sum of the eight zones, MW.
    #[must_use]
    pub fn sum(&self) -> f64 {
        self.coast
            + self.east
            + self.far_west
            + self.north
            + self.north_central
            + self.south_central
            + self.southern
            + self.west
    }

    /// Reads the eight zone columns; the two central zones are named differently per report.
    fn from_row(row: &Row<'_>, north_central: &str, south_central: &str) -> Result<Self> {
        Ok(Self {
            coast: row.f64("coast")?,
            east: row.f64("east")?,
            far_west: row.f64("farWest")?,
            north: row.f64("north")?,
            north_central: row.f64(north_central)?,
            south_central: row.f64(south_central)?,
            southern: row.f64("southern")?,
            west: row.f64("west")?,
        })
    }
}

// ---------------------------------------------------------------------------
// NP6-345-CD

/// NP6-345-CD: actual hourly system load by weather zone.
#[derive(Debug, Clone, Copy)]
pub struct ActualLoad;

/// Actual load for one hour.
#[derive(Debug, Clone, PartialEq)]
pub struct ActualLoadRecord {
    /// Operating day.
    pub operating_day: NaiveDate,
    /// Hour ending.
    pub hour_ending: HourEnding,
    /// Load per weather zone, MW.
    pub zones: WeatherZoneLoad,
    /// ERCOT-reported total, MW.
    pub total: f64,
    /// True during the repeated hour at the autumn clock change.
    pub dst_flag: bool,
}

impl FromRow for ActualLoadRecord {
    fn from_row(row: &Row<'_>) -> Result<Self> {
        Ok(Self {
            operating_day: row.date("operatingDay")?,
            hour_ending: row.hour_ending("hourEnding")?,
            zones: WeatherZoneLoad::from_row(row, "northC", "southC")?,
            total: row.f64("total")?,
            dst_flag: row.bool("DSTFlag")?,
        })
    }
}

impl Report for ActualLoad {
    type Record = ActualLoadRecord;
    const EMIL_ID: &'static str = "NP6-345-CD";
    const ARTIFACT: &'static str = "act_sys_load_by_wzn";
}

impl Query<ActualLoad> {
    /// Restricts to operating days `from..=to`.
    #[must_use]
    pub fn operating_day(self, from: NaiveDate, to: NaiveDate) -> Self {
        self.date_range("operatingDay", from, to)
    }

    /// Restricts to one hour ending (exact match only).
    #[must_use]
    pub fn hour_ending(self, hour: HourEnding) -> Self {
        self.filter("hourEnding", hour.label())
    }
}

// ---------------------------------------------------------------------------
// NP3-565-CD

/// NP3-565-CD: seven-day hourly load forecast by model and weather zone.
#[derive(Debug, Clone, Copy)]
pub struct LoadForecast;

/// One forecast hour from one forecast model.
#[derive(Debug, Clone, PartialEq)]
pub struct LoadForecastRecord {
    /// When ERCOT posted this forecast run (Central time).
    pub posted: NaiveDateTime,
    /// Day being forecast.
    pub delivery_date: NaiveDate,
    /// Hour ending being forecast.
    pub hour_ending: HourEnding,
    /// Forecast per weather zone, MW.
    pub zones: WeatherZoneLoad,
    /// System-wide forecast, MW.
    pub system_total: f64,
    /// Forecast model id (a letter such as `E` or `X`).
    pub model: String,
    /// True when ERCOT was using this model's forecast at posting time.
    pub in_use: bool,
    /// True during the repeated hour at the autumn clock change.
    pub dst_flag: bool,
}

impl FromRow for LoadForecastRecord {
    fn from_row(row: &Row<'_>) -> Result<Self> {
        Ok(Self {
            posted: row.datetime("postedDatetime")?,
            delivery_date: row.date("deliveryDate")?,
            hour_ending: row.hour_ending("hourEnding")?,
            zones: WeatherZoneLoad::from_row(row, "northCentral", "southCentral")?,
            system_total: row.f64("systemTotal")?,
            model: row.str("model")?.to_owned(),
            in_use: row.bool("inUseFlag")?,
            dst_flag: row.bool("DSTFlag")?,
        })
    }
}

impl Report for LoadForecast {
    type Record = LoadForecastRecord;
    const EMIL_ID: &'static str = "NP3-565-CD";
    const ARTIFACT: &'static str = "lf_by_model_weather_zone";
}

impl Query<LoadForecast> {
    /// Restricts to forecast runs posted within `from..=to`.
    #[must_use]
    pub fn posted(self, from: NaiveDateTime, to: NaiveDateTime) -> Self {
        self.datetime_range("postedDatetime", from, to)
    }

    /// Restricts to forecast days `from..=to`.
    #[must_use]
    pub fn delivery_date(self, from: NaiveDate, to: NaiveDate) -> Self {
        self.date_range("deliveryDate", from, to)
    }

    /// Restricts to one forecast model id.
    #[must_use]
    pub fn model(self, model: &str) -> Self {
        self.filter("model", model)
    }

    /// Keeps only the forecast ERCOT was actually using (`inUseFlag=true`).
    #[must_use]
    pub fn in_use_only(self) -> Self {
        self.filter("inUseFlag", true)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::page::ReportPage;

    #[test]
    fn actual_load_fixture_parses_and_zones_sum_to_total() {
        let recs = ReportPage::from_json(include_bytes!("../../tests/fixtures/load_actual.json"))
            .expect("page")
            .records::<ActualLoadRecord>()
            .expect("records");
        assert_eq!(recs.len(), 3);
        assert_eq!(recs[0].hour_ending.get(), 1);
        assert!((recs[0].total - 48_052.44).abs() < 1e-6);
        for r in &recs {
            // ERCOT rounds each zone to 2 dp, so allow a small tolerance.
            assert!(
                (r.zones.sum() - r.total).abs() < 0.4,
                "{} vs {}",
                r.zones.sum(),
                r.total
            );
        }
    }

    #[test]
    fn forecast_fixture_parses_with_nonpadded_hour_labels() {
        let recs = ReportPage::from_json(include_bytes!("../../tests/fixtures/load_forecast.json"))
            .expect("page")
            .records::<LoadForecastRecord>()
            .expect("records");
        assert_eq!(recs.len(), 2);
        assert_eq!(recs[0].hour_ending.get(), 1, "label was `1:00`");
        assert_eq!(recs[1].hour_ending.get(), 10);
        assert!(recs[0].in_use);
        assert_eq!(recs[0].model, "X");
        assert_eq!(recs[0].posted.to_string(), "2026-11-17 20:30:00");
    }

    #[test]
    fn typed_filters_use_the_api_parameter_names() {
        let t = chrono::NaiveDate::from_ymd_opt(2026, 11, 17).expect("date");
        let q = Query::<LoadForecast>::new()
            .posted(
                t.and_hms_opt(0, 0, 0).expect("t"),
                t.and_hms_opt(1, 0, 0).expect("t"),
            )
            .model("E")
            .in_use_only();
        let params = q.request().params();
        assert!(params.contains(&("postedDatetimeFrom".into(), "2026-11-17T00:00:00".into())));
        assert!(params.contains(&("inUseFlag".into(), "true".into())));
        assert!(params.contains(&("model".into(), "E".into())));
    }
}

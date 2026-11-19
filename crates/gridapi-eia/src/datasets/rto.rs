//! `electricity/rto`: EIA-930 Hourly Electric Grid Monitor, by balancing
//! authority (BA). Hourly series are in UTC unless you ask for
//! [`Frequency::LocalHourly`].
//!
//! ```no_run
//! use gridapi_eia::{Client, Direction};
//! use gridapi_eia::datasets::rto::{self, RegionDataRow, RegionMetric};
//!
//! # async fn demo() -> Result<(), gridapi_eia::Error> {
//! let client = Client::from_env()?;
//! let q = rto::region_data_for("ERCO", RegionMetric::Demand)
//!     .sort("period", Direction::Desc)
//!     .length(24);
//! let rows: Vec<RegionDataRow> = client.data_page(&q).await?.rows;
//! # Ok(()) }
//! ```

use serde::Deserialize;

use crate::de::opt_f64;
use crate::query::{DataQuery, Frequency};

/// Route: demand, forecast, net generation and total interchange per BA.
pub const REGION_DATA: &str = "electricity/rto/region-data";
/// Route: net generation by BA and energy source.
pub const FUEL_TYPE_DATA: &str = "electricity/rto/fuel-type-data";
/// Route: interchange between neighbouring BAs.
pub const INTERCHANGE_DATA: &str = "electricity/rto/interchange-data";
/// Route: demand by BA subregion.
pub const REGION_SUB_BA_DATA: &str = "electricity/rto/region-sub-ba-data";
/// Route: daily demand, forecast, generation, interchange per BA.
pub const DAILY_REGION_DATA: &str = "electricity/rto/daily-region-data";
/// Route: daily net generation by BA and energy source.
pub const DAILY_FUEL_TYPE_DATA: &str = "electricity/rto/daily-fuel-type-data";
/// Route: daily interchange between neighbouring BAs.
pub const DAILY_INTERCHANGE_DATA: &str = "electricity/rto/daily-interchange-data";

/// The `type` facet of region-data.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum RegionMetric {
    /// `D`: demand.
    Demand,
    /// `DF`: day-ahead demand forecast.
    DayAheadForecast,
    /// `NG`: net generation.
    NetGeneration,
    /// `TI`: total interchange.
    TotalInterchange,
}

impl RegionMetric {
    /// The facet value EIA expects.
    #[must_use]
    pub const fn code(self) -> &'static str {
        match self {
            Self::Demand => "D",
            Self::DayAheadForecast => "DF",
            Self::NetGeneration => "NG",
            Self::TotalInterchange => "TI",
        }
    }
}

/// The `fueltype` facet of fuel-type-data (EIA-930 energy-source codes).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum FuelType {
    /// `COL` coal.
    Coal,
    /// `NG` natural gas.
    NaturalGas,
    /// `NUC` nuclear.
    Nuclear,
    /// `OIL` petroleum.
    Oil,
    /// `WAT` hydro.
    Hydro,
    /// `PS` pumped storage.
    PumpedStorage,
    /// `SUN` solar.
    Solar,
    /// `WND` wind.
    Wind,
    /// `BAT` battery storage.
    Battery,
    /// `GEO` geothermal.
    Geothermal,
    /// `OTH` other.
    Other,
    /// `UNK` unknown.
    Unknown,
}

impl FuelType {
    /// The facet value EIA expects.
    #[must_use]
    pub const fn code(self) -> &'static str {
        match self {
            Self::Coal => "COL",
            Self::NaturalGas => "NG",
            Self::Nuclear => "NUC",
            Self::Oil => "OIL",
            Self::Hydro => "WAT",
            Self::PumpedStorage => "PS",
            Self::Solar => "SUN",
            Self::Wind => "WND",
            Self::Battery => "BAT",
            Self::Geothermal => "GEO",
            Self::Other => "OTH",
            Self::Unknown => "UNK",
        }
    }
}

fn hourly(route: &str) -> DataQuery {
    DataQuery::new(route)
        .frequency(Frequency::Hourly)
        .data_column("value")
}

fn daily(route: &str) -> DataQuery {
    DataQuery::new(route)
        .frequency(Frequency::Daily)
        .data_column("value")
}

/// Hourly region-data (all BAs, all metrics); narrow with `.facet(..)`.
pub fn region_data() -> DataQuery {
    hourly(REGION_DATA)
}

/// Hourly region-data for one BA and one metric.
pub fn region_data_for(respondent: &str, metric: RegionMetric) -> DataQuery {
    region_data()
        .facet("respondent", [respondent])
        .facet("type", [metric.code()])
}

/// Hourly net generation by energy source (all BAs); narrow with `.facet(..)`.
pub fn fuel_type_data() -> DataQuery {
    hourly(FUEL_TYPE_DATA)
}

/// Hourly net generation for one BA, optionally one energy source.
pub fn fuel_type_data_for(respondent: &str, fuel: Option<FuelType>) -> DataQuery {
    let q = fuel_type_data().facet("respondent", [respondent]);
    match fuel {
        Some(f) => q.facet("fueltype", [f.code()]),
        None => q,
    }
}

/// Hourly interchange (all BA pairs); narrow with `.facet("fromba", ..)` and
/// `.facet("toba", ..)`.
pub fn interchange_data() -> DataQuery {
    hourly(INTERCHANGE_DATA)
}

/// Hourly interchange out of one BA (to every neighbour).
pub fn interchange_from(fromba: &str) -> DataQuery {
    interchange_data().facet("fromba", [fromba])
}

/// Hourly demand by BA subregion.
pub fn region_sub_ba_data() -> DataQuery {
    hourly(REGION_SUB_BA_DATA)
}

/// Daily region-data. Rows carry a `timezone` (the day boundary's zone).
pub fn daily_region_data() -> DataQuery {
    daily(DAILY_REGION_DATA)
}

/// Daily net generation by energy source.
pub fn daily_fuel_type_data() -> DataQuery {
    daily(DAILY_FUEL_TYPE_DATA)
}

/// Daily interchange.
pub fn daily_interchange_data() -> DataQuery {
    daily(DAILY_INTERCHANGE_DATA)
}

/// A row of `region-data` / `daily-region-data`.
#[derive(Debug, Clone, PartialEq, Deserialize)]
pub struct RegionDataRow {
    /// Period, `YYYY-MM-DDTHH` (hourly) or `YYYY-MM-DD` (daily).
    pub period: String,
    /// BA or region code, for example `ERCO`.
    pub respondent: String,
    /// BA or region name.
    #[serde(default, rename = "respondent-name")]
    pub respondent_name: Option<String>,
    /// Metric code: `D`, `DF`, `NG` or `TI`.
    #[serde(rename = "type")]
    pub metric: String,
    /// Metric name.
    #[serde(default, rename = "type-name")]
    pub metric_name: Option<String>,
    /// Day boundary time zone (daily routes only).
    #[serde(default)]
    pub timezone: Option<String>,
    /// Megawatt-hours; `None` when EIA publishes no value.
    #[serde(default, deserialize_with = "opt_f64")]
    pub value: Option<f64>,
    /// Units, `megawatthours`.
    #[serde(default, rename = "value-units")]
    pub value_units: Option<String>,
}

/// A row of `fuel-type-data` / `daily-fuel-type-data`.
#[derive(Debug, Clone, PartialEq, Deserialize)]
pub struct FuelTypeRow {
    /// Period.
    pub period: String,
    /// BA code.
    pub respondent: String,
    /// BA name.
    #[serde(default, rename = "respondent-name")]
    pub respondent_name: Option<String>,
    /// Energy-source code, for example `NG`, `SUN`, `WND`.
    pub fueltype: String,
    /// Energy-source name (EIA labels this column `type-name`).
    #[serde(default, rename = "type-name")]
    pub fuel_name: Option<String>,
    /// Day boundary time zone (daily routes only).
    #[serde(default)]
    pub timezone: Option<String>,
    /// Net generation in megawatt-hours.
    #[serde(default, deserialize_with = "opt_f64")]
    pub value: Option<f64>,
    /// Units.
    #[serde(default, rename = "value-units")]
    pub value_units: Option<String>,
}

/// A row of `interchange-data` / `daily-interchange-data`. The sign of
/// `value` follows EIA's EIA-930 convention; read EIA's EIA-930 documentation
/// before summing across BAs.
#[derive(Debug, Clone, PartialEq, Deserialize)]
pub struct InterchangeRow {
    /// Period.
    pub period: String,
    /// Reporting BA.
    pub fromba: String,
    /// Reporting BA name.
    #[serde(default, rename = "fromba-name")]
    pub fromba_name: Option<String>,
    /// Neighbouring BA.
    pub toba: String,
    /// Neighbouring BA name.
    #[serde(default, rename = "toba-name")]
    pub toba_name: Option<String>,
    /// Day boundary time zone (daily routes only).
    #[serde(default)]
    pub timezone: Option<String>,
    /// Interchange in megawatt-hours.
    #[serde(default, deserialize_with = "opt_f64")]
    pub value: Option<f64>,
    /// Units.
    #[serde(default, rename = "value-units")]
    pub value_units: Option<String>,
}

/// A row of `region-sub-ba-data` / `daily-region-sub-ba-data`.
#[derive(Debug, Clone, PartialEq, Deserialize)]
pub struct SubBaRow {
    /// Period.
    pub period: String,
    /// Subregion code, for example `PGAE`.
    #[serde(default)]
    pub subba: Option<String>,
    /// Subregion name.
    #[serde(default, rename = "subba-name")]
    pub subba_name: Option<String>,
    /// Parent BA code, for example `CISO`.
    #[serde(default)]
    pub parent: Option<String>,
    /// Parent BA name.
    #[serde(default, rename = "parent-name")]
    pub parent_name: Option<String>,
    /// Demand in megawatt-hours.
    #[serde(default, deserialize_with = "opt_f64")]
    pub value: Option<f64>,
    /// Units.
    #[serde(default, rename = "value-units")]
    pub value_units: Option<String>,
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::data::DataPage;

    fn page<R: serde::de::DeserializeOwned>(json: &str) -> Vec<R> {
        let v: serde_json::Value = serde_json::from_str(json).unwrap();
        serde_json::from_value(v["response"]["data"].clone()).unwrap()
    }

    #[test]
    fn region_rows_decode_string_numbers() {
        let rows: Vec<RegionDataRow> = page(include_str!("../../tests/fixtures/data_region.json"));
        assert_eq!(rows.len(), 3);
        let r = &rows[0];
        assert_eq!(r.respondent, "ERCO");
        assert_eq!(r.metric, "D");
        assert_eq!(r.metric_name.as_deref(), Some("Demand"));
        assert!(r.value.is_some_and(|v| v > 0.0));
        assert_eq!(r.value_units.as_deref(), Some("megawatthours"));
        assert!(
            r.period.len() == 13 && r.period.as_bytes()[10] == b'T',
            "{}",
            r.period
        );
    }

    #[test]
    fn fuel_rows_decode() {
        let rows: Vec<FuelTypeRow> = page(include_str!("../../tests/fixtures/data_fuel_type.json"));
        assert_eq!(rows[0].fueltype, "NG");
        assert_eq!(rows[0].fuel_name.as_deref(), Some("Natural Gas"));
        assert!(rows[0].value.is_some());
    }

    #[test]
    fn interchange_rows_decode_negative_values() {
        let rows: Vec<InterchangeRow> =
            page(include_str!("../../tests/fixtures/data_interchange.json"));
        assert_eq!(rows[0].fromba, "ERCO");
        assert_eq!(rows[0].toba, "CEN");
        assert_eq!(rows[0].value, Some(-1.0));
    }

    #[test]
    fn builders_set_route_frequency_and_facets() {
        let q = region_data_for("ERCO", RegionMetric::DayAheadForecast);
        let p = q.params();
        assert_eq!(q.route(), "electricity/rto/region-data");
        assert!(p.contains(&("frequency".into(), "hourly".into())));
        assert!(p.contains(&("data[0]".into(), "value".into())));
        assert!(p.contains(&("facets[respondent][]".into(), "ERCO".into())));
        assert!(p.contains(&("facets[type][]".into(), "DF".into())));

        let f = fuel_type_data_for("PJM", Some(FuelType::Solar)).params();
        assert!(f.contains(&("facets[fueltype][]".into(), "SUN".into())));
        assert!(!fuel_type_data_for("PJM", None)
            .params()
            .iter()
            .any(|(k, _)| k.starts_with("facets[fueltype")));

        assert!(daily_region_data()
            .params()
            .contains(&("frequency".into(), "daily".into())));
        assert!(interchange_from("MISO")
            .params()
            .contains(&("facets[fromba][]".into(), "MISO".into())));
    }

    #[test]
    fn empty_page_has_zero_rows_and_total() {
        let v: serde_json::Value =
            serde_json::from_str(include_str!("../../tests/fixtures/data_empty.json")).unwrap();
        assert_eq!(v["response"]["total"], "0");
        let rows: Vec<RegionDataRow> = page(include_str!("../../tests/fixtures/data_empty.json"));
        assert_eq!(rows, Vec::<RegionDataRow>::new());
        let _: Option<DataPage<RegionDataRow>> = None;
    }
}

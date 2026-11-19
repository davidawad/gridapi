//! `electricity/operating-generator-capacity`: the API's view of the EIA-860
//! / EIA-860M inventory of operable generators (monthly snapshots). For the
//! planned, retired and cancelled sheets of the 860M workbook see
//! [`crate::bulk::eia860m`].

use serde::Deserialize;

use crate::de::opt_f64;
use crate::query::{DataQuery, Frequency};

/// Route of the generator inventory.
pub const OPERATING_GENERATOR_CAPACITY: &str = "electricity/operating-generator-capacity";

/// Every numeric/date data column the route offers.
pub const DATA_COLUMNS: [&str; 12] = [
    "nameplate-capacity-mw",
    "net-summer-capacity-mw",
    "net-winter-capacity-mw",
    "operating-year-month",
    "planned-retirement-year-month",
    "planned-derate-year-month",
    "planned-derate-summer-cap-mw",
    "planned-uprate-year-month",
    "planned-uprate-summer-cap-mw",
    "county",
    "latitude",
    "longitude",
];

/// Monthly generator inventory with every data column selected. Narrow with
/// facets: `stateid`, `sector`, `entityid`, `plantid`, `generatorid`,
/// `technology`, `energy_source_code`, `prime_mover_code`,
/// `balancing_authority_code`, `status`. Add `.start("2026-08")` to pin one
/// monthly snapshot, otherwise every month since 2008 matches.
pub fn operating_generator_capacity() -> DataQuery {
    DataQuery::new(OPERATING_GENERATOR_CAPACITY)
        .frequency(Frequency::Monthly)
        .data_columns(DATA_COLUMNS)
}

/// One generator in one monthly snapshot.
#[derive(Debug, Clone, PartialEq, Deserialize)]
pub struct GeneratorCapacityRow {
    /// Snapshot month, `YYYY-MM`.
    pub period: String,
    /// State postal code.
    #[serde(default)]
    pub stateid: Option<String>,
    /// State name.
    #[serde(default, rename = "stateName")]
    pub state_name: Option<String>,
    /// Sector id, for example `electric-utility`.
    #[serde(default)]
    pub sector: Option<String>,
    /// Sector name.
    #[serde(default, rename = "sectorName")]
    pub sector_name: Option<String>,
    /// Owner/operator entity id.
    #[serde(default)]
    pub entityid: Option<String>,
    /// Owner/operator name.
    #[serde(default, rename = "entityName")]
    pub entity_name: Option<String>,
    /// Plant id (EIA plant code).
    #[serde(default)]
    pub plantid: Option<String>,
    /// Plant name.
    #[serde(default, rename = "plantName")]
    pub plant_name: Option<String>,
    /// Generator id within the plant.
    #[serde(default)]
    pub generatorid: Option<String>,
    /// Technology, for example `Natural Gas Steam Turbine`.
    #[serde(default)]
    pub technology: Option<String>,
    /// Energy source code, for example `NG`.
    #[serde(default)]
    pub energy_source_code: Option<String>,
    /// Energy source description.
    #[serde(default, rename = "energy-source-desc")]
    pub energy_source_desc: Option<String>,
    /// Prime mover code, for example `ST`.
    #[serde(default)]
    pub prime_mover_code: Option<String>,
    /// Balancing authority code, for example `PJM`.
    #[serde(default)]
    pub balancing_authority_code: Option<String>,
    /// Balancing authority name.
    #[serde(default, rename = "balancing-authority-name")]
    pub balancing_authority_name: Option<String>,
    /// Status code, for example `OP`.
    #[serde(default)]
    pub status: Option<String>,
    /// Status description.
    #[serde(default, rename = "statusDescription")]
    pub status_description: Option<String>,
    /// Nameplate capacity, MW.
    #[serde(
        default,
        rename = "nameplate-capacity-mw",
        deserialize_with = "opt_f64"
    )]
    pub nameplate_capacity_mw: Option<f64>,
    /// Net summer capacity, MW.
    #[serde(
        default,
        rename = "net-summer-capacity-mw",
        deserialize_with = "opt_f64"
    )]
    pub net_summer_capacity_mw: Option<f64>,
    /// Net winter capacity, MW.
    #[serde(
        default,
        rename = "net-winter-capacity-mw",
        deserialize_with = "opt_f64"
    )]
    pub net_winter_capacity_mw: Option<f64>,
    /// Commercial operation month, `YYYY-MM`.
    #[serde(default, rename = "operating-year-month")]
    pub operating_year_month: Option<String>,
    /// Planned retirement month, `YYYY-MM`.
    #[serde(default, rename = "planned-retirement-year-month")]
    pub planned_retirement_year_month: Option<String>,
    /// County.
    #[serde(default)]
    pub county: Option<String>,
    /// Latitude, degrees.
    #[serde(default, deserialize_with = "opt_f64")]
    pub latitude: Option<f64>,
    /// Longitude, degrees.
    #[serde(default, deserialize_with = "opt_f64")]
    pub longitude: Option<f64>,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn row_decodes_from_recorded_reply() {
        let v: serde_json::Value = serde_json::from_str(include_str!(
            "../../tests/fixtures/data_generator_capacity.json"
        ))
        .unwrap();
        let rows: Vec<GeneratorCapacityRow> =
            serde_json::from_value(v["response"]["data"].clone()).unwrap();
        assert_eq!(rows.len(), 2);
        let g = &rows[0];
        assert_eq!(g.stateid.as_deref(), Some("VA"));
        assert_eq!(g.plant_name.as_deref(), Some("Clinch River"));
        assert_eq!(g.energy_source_code.as_deref(), Some("NG"));
        assert_eq!(g.balancing_authority_code.as_deref(), Some("PJM"));
        assert_eq!(g.status.as_deref(), Some("OP"));
        assert_eq!(g.nameplate_capacity_mw, Some(237.5));
        assert!(g.latitude.is_some_and(|l| (36.0..38.0).contains(&l)));
        assert_eq!(g.county, None);
    }

    #[test]
    fn query_selects_all_columns_monthly() {
        let p = operating_generator_capacity().params();
        assert!(p.contains(&("frequency".into(), "monthly".into())));
        assert_eq!(
            p.iter().filter(|(k, _)| k.starts_with("data[")).count(),
            DATA_COLUMNS.len()
        );
        assert!(p.contains(&("data[11]".into(), "longitude".into())));
    }
}

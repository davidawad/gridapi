//! `electricity/retail-sales`: sales, revenue, price and customer counts by
//! state and sector (EIA-826 / EIA-861 / EIA-861M).

use serde::Deserialize;

use crate::de::opt_f64;
use crate::query::{DataQuery, Frequency};

/// Route of retail electricity sales.
pub const RETAIL_SALES: &str = "electricity/retail-sales";

/// Customer sector, the `sectorid` facet.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Sector {
    /// `RES` residential.
    Residential,
    /// `COM` commercial.
    Commercial,
    /// `IND` industrial.
    Industrial,
    /// `TRA` transportation.
    Transportation,
    /// `OTH` other.
    Other,
    /// `ALL` all sectors.
    All,
}

impl Sector {
    /// The facet value EIA expects.
    #[must_use]
    pub const fn code(self) -> &'static str {
        match self {
            Self::Residential => "RES",
            Self::Commercial => "COM",
            Self::Industrial => "IND",
            Self::Transportation => "TRA",
            Self::Other => "OTH",
            Self::All => "ALL",
        }
    }
}

/// Retail sales with price, sales, revenue and customers selected, at the
/// given frequency (`monthly`, `quarterly` or `annual`). Facets: `stateid`
/// (two-letter code, `US` for the nation, plus census regions) and `sectorid`.
pub fn retail_sales(frequency: Frequency) -> DataQuery {
    DataQuery::new(RETAIL_SALES)
        .frequency(frequency)
        .data_columns(["price", "sales", "revenue", "customers"])
}

/// Monthly retail sales for one state and sector.
pub fn monthly_for(state: &str, sector: Sector) -> DataQuery {
    retail_sales(Frequency::Monthly)
        .facet("stateid", [state])
        .facet("sectorid", [sector.code()])
}

/// One state/sector/period observation.
#[derive(Debug, Clone, PartialEq, Deserialize)]
pub struct RetailSalesRow {
    /// Period: `YYYY-MM`, `YYYY-Qn` or `YYYY`.
    pub period: String,
    /// State (or region) code.
    #[serde(default)]
    pub stateid: Option<String>,
    /// State (or region) name.
    #[serde(default, rename = "stateDescription")]
    pub state_description: Option<String>,
    /// Sector code.
    #[serde(default)]
    pub sectorid: Option<String>,
    /// Sector name.
    #[serde(default, rename = "sectorName")]
    pub sector_name: Option<String>,
    /// Average price, cents per kWh.
    #[serde(default, deserialize_with = "opt_f64")]
    pub price: Option<f64>,
    /// Sales, million `kWh` (equal to `GWh`).
    #[serde(default, deserialize_with = "opt_f64")]
    pub sales: Option<f64>,
    /// Revenue, million dollars.
    #[serde(default, deserialize_with = "opt_f64")]
    pub revenue: Option<f64>,
    /// Customer count.
    #[serde(default, deserialize_with = "opt_f64")]
    pub customers: Option<f64>,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn row_decodes_and_missing_columns_are_none() {
        let v: serde_json::Value =
            serde_json::from_str(include_str!("../../tests/fixtures/data_retail_sales.json"))
                .unwrap();
        let rows: Vec<RetailSalesRow> =
            serde_json::from_value(v["response"]["data"].clone()).unwrap();
        assert_eq!(rows[0].stateid.as_deref(), Some("TX"));
        assert_eq!(rows[0].sectorid.as_deref(), Some("COM"));
        assert_eq!(rows[0].price, Some(8.96));
        assert!(rows[0].sales.is_some_and(|s| s > 10_000.0));
        assert_eq!(
            rows[0].revenue, None,
            "the recorded query did not select revenue"
        );
    }

    #[test]
    fn builder_sets_facets() {
        let p = monthly_for("CA", Sector::Residential).params();
        assert!(p.contains(&("facets[stateid][]".into(), "CA".into())));
        assert!(p.contains(&("facets[sectorid][]".into(), "RES".into())));
        assert!(p.contains(&("frequency".into(), "monthly".into())));
    }
}

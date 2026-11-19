//! Natural gas prices. EIA publishes the Henry Hub spot price and NYMEX
//! futures contracts 1-4 under `natural-gas/pri/fut`; the `series` facet
//! selects which. (The route is named `fut` but also carries the spot.)

use serde::Deserialize;

use crate::de::opt_f64;
use crate::query::{DataQuery, Frequency};

/// Route holding spot and futures prices.
pub const PRICES_FUT: &str = "natural-gas/pri/fut";
/// Route with summary prices (city gate, residential, ... by state).
pub const PRICES_SUMMARY: &str = "natural-gas/pri/sum";
/// Series id of the Henry Hub natural gas spot price, dollars per `MMBtu`.
pub const HENRY_HUB_SPOT: &str = "RNGWHHD";

/// A priced series under `natural-gas/pri/fut`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum FutSeries {
    /// `RNGWHHD` Henry Hub spot.
    HenryHubSpot,
    /// `RNGC1` NYMEX futures contract 1.
    Contract1,
    /// `RNGC2` contract 2.
    Contract2,
    /// `RNGC3` contract 3.
    Contract3,
    /// `RNGC4` contract 4.
    Contract4,
}

impl FutSeries {
    /// The `series` facet value.
    #[must_use]
    pub const fn code(self) -> &'static str {
        match self {
            Self::HenryHubSpot => HENRY_HUB_SPOT,
            Self::Contract1 => "RNGC1",
            Self::Contract2 => "RNGC2",
            Self::Contract3 => "RNGC3",
            Self::Contract4 => "RNGC4",
        }
    }
}

/// Spot/futures prices for one series at `frequency` (`daily`, `weekly`,
/// `monthly` or `annual`).
pub fn prices(series: FutSeries, frequency: Frequency) -> DataQuery {
    DataQuery::new(PRICES_FUT)
        .frequency(frequency)
        .data_column("value")
        .facet("series", [series.code()])
}

/// Daily Henry Hub spot price, dollars per `MMBtu`.
pub fn henry_hub_spot_daily() -> DataQuery {
    prices(FutSeries::HenryHubSpot, Frequency::Daily)
}

/// One price observation.
#[derive(Debug, Clone, PartialEq, Deserialize)]
pub struct PriceRow {
    /// Period: `YYYY-MM-DD` (daily), `YYYY-MM` (monthly), `YYYY`.
    pub period: String,
    /// Series id, for example `RNGWHHD`.
    #[serde(default)]
    pub series: Option<String>,
    /// Series description.
    #[serde(default, rename = "series-description")]
    pub series_description: Option<String>,
    /// Pricing-area code, `RGC` for the national Henry Hub series.
    #[serde(default)]
    pub duoarea: Option<String>,
    /// Process code, for example `PS0` (spot price).
    #[serde(default)]
    pub process: Option<String>,
    /// Process name, for example `Spot Price`.
    #[serde(default, rename = "process-name")]
    pub process_name: Option<String>,
    /// Price. EIA publishes spot prices rounded to two decimals.
    #[serde(default, deserialize_with = "opt_f64")]
    pub value: Option<f64>,
    /// Units, `$/MMBTU`.
    #[serde(default)]
    pub units: Option<String>,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn spot_rows_decode() {
        let v: serde_json::Value =
            serde_json::from_str(include_str!("../../tests/fixtures/data_henry_hub.json")).unwrap();
        let rows: Vec<PriceRow> = serde_json::from_value(v["response"]["data"].clone()).unwrap();
        assert_eq!(rows.len(), 3);
        assert_eq!(rows[0].series.as_deref(), Some("RNGWHHD"));
        assert_eq!(rows[0].process_name.as_deref(), Some("Spot Price"));
        assert_eq!(rows[0].units.as_deref(), Some("$/MMBTU"));
        assert_eq!(rows[2].value, Some(2.89));
        assert_eq!(rows[0].period, "2026-11-11");
    }

    #[test]
    fn builder_targets_henry_hub() {
        let q = henry_hub_spot_daily();
        assert_eq!(q.route(), "natural-gas/pri/fut");
        let p = q.params();
        assert!(p.contains(&("facets[series][]".into(), "RNGWHHD".into())));
        assert!(p.contains(&("frequency".into(), "daily".into())));
    }
}

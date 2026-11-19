//! Locational marginal price rows (`PRC_LMP`, `PRC_INTVL_LMP`, `PRC_RTPD_LMP`).

use std::collections::BTreeMap;

use chrono::{DateTime, NaiveDate, Utc};

use crate::error::Error;
use crate::record::{FromRecord, Record};

/// One component of an LMP, from the `LMP_TYPE` column.
#[derive(Debug, Clone, PartialEq, Eq, Hash, PartialOrd, Ord)]
#[non_exhaustive]
pub enum LmpComponent {
    /// The total LMP (`LMP`, data item `LMP_PRC`).
    Lmp,
    /// Marginal cost of congestion (`MCC`, `LMP_CONG_PRC`).
    Congestion,
    /// Marginal cost of energy (`MCE`, `LMP_ENE_PRC`).
    Energy,
    /// Marginal cost of losses (`MCL`, `LMP_LOSS_PRC`).
    Loss,
    /// Marginal greenhouse-gas cost (`MGHG`, `LMP_GHG_PRC`).
    Ghg,
    /// A component code this crate does not know yet.
    Other(String),
}

impl LmpComponent {
    /// Maps an `LMP_TYPE` code or an `XML_DATA_ITEM` name to a component.
    #[must_use]
    pub fn from_code(code: &str) -> Self {
        match code.trim().to_ascii_uppercase().as_str() {
            "LMP" | "LMP_PRC" => Self::Lmp,
            "MCC" | "LMP_CONG_PRC" => Self::Congestion,
            "MCE" | "LMP_ENE_PRC" => Self::Energy,
            "MCL" | "LMP_LOSS_PRC" => Self::Loss,
            "MGHG" | "LMP_GHG_PRC" => Self::Ghg,
            other => Self::Other(other.to_owned()),
        }
    }
}

/// One price component at one node and interval, in US$/MWh.
#[derive(Debug, Clone, PartialEq)]
pub struct LmpRow {
    /// Interval start, UTC (inclusive).
    pub interval_start: DateTime<Utc>,
    /// Interval end, UTC (exclusive).
    pub interval_end: DateTime<Utc>,
    /// The CAISO operating date (Pacific), when reported.
    pub operating_date: Option<NaiveDate>,
    /// The pricing node or hub id.
    pub node: String,
    /// The market run (`DAM`, `RTM`, `RTPD`), when reported.
    pub market_run_id: Option<String>,
    /// Which part of the LMP this row carries.
    pub component: LmpComponent,
    /// The raw OASIS data item, e.g. `LMP_CONG_PRC`.
    pub data_item: String,
    /// The price in US$/MWh.
    pub price: f64,
}

impl FromRecord for LmpRow {
    fn from_record(r: &Record) -> Result<Self, Error> {
        let data_item = r
            .first(&["XML_DATA_ITEM", "DATA_ITEM"])
            .unwrap_or_default()
            .to_owned();
        let component = r
            .first(&["LMP_TYPE"])
            .or(Some(data_item.as_str()).filter(|d| !d.is_empty()))
            .map(LmpComponent::from_code)
            .ok_or_else(|| Error::MissingField {
                field: "LMP_TYPE|XML_DATA_ITEM".into(),
            })?;
        Ok(Self {
            interval_start: r.timestamp(&["INTERVALSTARTTIME_GMT", "INTERVAL_START_GMT"])?,
            interval_end: r.timestamp(&["INTERVALENDTIME_GMT", "INTERVAL_END_GMT"])?,
            operating_date: r.optional_date(&["OPR_DT", "OPR_DATE"])?,
            node: r
                .require(&["NODE", "NODE_ID", "NODE_ID_XML", "RESOURCE_NAME"])?
                .to_owned(),
            market_run_id: r.first(&["MARKET_RUN_ID", "MKT_TYPE"]).map(str::to_owned),
            component,
            data_item,
            // The value column is MW in PRC_LMP, VALUE in PRC_INTVL_LMP, PRC in PRC_RTPD_LMP.
            price: r.number(&["MW", "VALUE", "PRC"])?,
        })
    }
}

/// All LMP components for one node and interval, pivoted into columns.
#[derive(Debug, Clone, PartialEq)]
pub struct LmpPoint {
    /// The pricing node or hub id.
    pub node: String,
    /// Interval start, UTC.
    pub interval_start: DateTime<Utc>,
    /// Interval end, UTC.
    pub interval_end: DateTime<Utc>,
    /// Total LMP, US$/MWh.
    pub lmp: Option<f64>,
    /// Energy component.
    pub energy: Option<f64>,
    /// Congestion component.
    pub congestion: Option<f64>,
    /// Loss component.
    pub loss: Option<f64>,
    /// Greenhouse-gas component.
    pub ghg: Option<f64>,
}

/// Pivots component rows into one [`LmpPoint`] per `(interval_start, node)`.
///
/// The result is ordered by interval start, then node. Duplicate components
/// keep the last value seen.
#[must_use]
pub fn pivot_lmp<I>(rows: I) -> Vec<LmpPoint>
where
    I: IntoIterator<Item = LmpRow>,
{
    rows.into_iter()
        .fold(
            BTreeMap::<(DateTime<Utc>, String), LmpPoint>::new(),
            |mut points, row| {
                let point = points
                    .entry((row.interval_start, row.node.clone()))
                    .or_insert_with(|| LmpPoint {
                        node: row.node.clone(),
                        interval_start: row.interval_start,
                        interval_end: row.interval_end,
                        lmp: None,
                        energy: None,
                        congestion: None,
                        loss: None,
                        ghg: None,
                    });
                let slot = match row.component {
                    LmpComponent::Lmp => Some(&mut point.lmp),
                    LmpComponent::Energy => Some(&mut point.energy),
                    LmpComponent::Congestion => Some(&mut point.congestion),
                    LmpComponent::Loss => Some(&mut point.loss),
                    LmpComponent::Ghg => Some(&mut point.ghg),
                    LmpComponent::Other(_) => None,
                };
                if let Some(slot) = slot {
                    *slot = Some(row.price);
                }
                points
            },
        )
        .into_values()
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::record::parse_csv;

    #[test]
    fn component_codes_map_from_csv_and_xml_names() {
        assert_eq!(LmpComponent::from_code("MCC"), LmpComponent::Congestion);
        assert_eq!(LmpComponent::from_code("lmp_ene_prc"), LmpComponent::Energy);
        assert_eq!(
            LmpComponent::from_code("XYZ"),
            LmpComponent::Other("XYZ".into())
        );
    }

    #[test]
    fn missing_price_column_is_an_error() -> Result<(), Error> {
        let rows = parse_csv(
            b"INTERVALSTARTTIME_GMT,INTERVALENDTIME_GMT,NODE,LMP_TYPE\n2026-11-10T08:00:00-00:00,2026-11-10T09:00:00-00:00,N,LMP\n",
        )?;
        assert!(matches!(
            LmpRow::from_record(&rows[0]),
            Err(Error::MissingField { .. })
        ));
        Ok(())
    }
}

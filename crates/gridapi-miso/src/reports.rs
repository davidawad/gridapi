//! Market report files published at `docs.misoenergy.org/marketreports`.
//!
//! The four LMP reports share one layout: a title, the market date, then a table with
//! columns `Node, Type, Value, HE 1 .. HE 24` and three rows (`LMP`, `MCC`, `MLC`) per node.

use chrono::NaiveDate;
use serde::{Deserialize, Serialize};

use crate::client::{join, Client};
use crate::error::{Error, Result};

/// Which hourly LMP report.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[non_exhaustive]
pub enum LmpReportKind {
    /// Day-ahead ex-post LMPs (`YYYYMMDD_da_expost_lmp.csv`), published for the operating day.
    DayAheadExPost,
    /// Day-ahead ex-ante LMPs (`YYYYMMDD_da_exante_lmp.csv`).
    DayAheadExAnte,
    /// Real-time hourly LMPs, preliminary (`YYYYMMDD_rt_lmp_prelim.csv`), available for roughly the last week.
    RealTimePrelim,
    /// Real-time hourly LMPs, final (`YYYYMMDD_rt_lmp_final.csv`), available after settlement.
    RealTimeFinal,
}

impl LmpReportKind {
    /// The file name MISO uses for `date`.
    pub fn file_name(self, date: NaiveDate) -> String {
        let stem = match self {
            Self::DayAheadExPost => "da_expost_lmp",
            Self::DayAheadExAnte => "da_exante_lmp",
            Self::RealTimePrelim => "rt_lmp_prelim",
            Self::RealTimeFinal => "rt_lmp_final",
        };
        format!("{}_{stem}.csv", date.format("%Y%m%d"))
    }
}

/// The kind of pricing node a report row is for.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[non_exhaustive]
pub enum NodeType {
    /// A trading hub, e.g. `ARKANSAS.HUB`.
    Hub,
    /// A load zone.
    LoadZone,
    /// A generator node.
    GenNode,
    /// An interface with a neighbouring system.
    Interface,
    /// A type this crate does not know yet, kept verbatim.
    Other(String),
}

impl NodeType {
    /// Classify the `Type` column of a report row; unknown labels are kept in [`NodeType::Other`].
    pub fn from_label(label: &str) -> Self {
        match label.trim().to_ascii_lowercase().as_str() {
            "hub" => Self::Hub,
            "loadzone" => Self::LoadZone,
            "gennode" => Self::GenNode,
            "interface" => Self::Interface,
            _ => Self::Other(label.trim().to_owned()),
        }
    }
}

/// Which price component a row carries.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[non_exhaustive]
pub enum Component {
    /// Locational marginal price.
    Lmp,
    /// Marginal congestion component.
    Mcc,
    /// Marginal loss component.
    Mlc,
}

impl Component {
    fn parse(s: &str) -> Option<Self> {
        match s.trim().to_ascii_uppercase().as_str() {
            "LMP" => Some(Self::Lmp),
            "MCC" => Some(Self::Mcc),
            "MLC" => Some(Self::Mlc),
            _ => None,
        }
    }
}

/// One node's hourly values for one price component.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ReportRow {
    /// Node name, e.g. `ARKANSAS.HUB`.
    pub node: String,
    /// What kind of node it is.
    pub node_type: NodeType,
    /// Which component the values are.
    pub component: Component,
    /// $/MWh for HE 1 through HE 24 (blank cells are `None`).
    pub hourly: Vec<Option<f64>>,
}

/// A parsed hourly LMP report.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct LmpReport {
    /// The report title line, e.g. `Day Ahead Market ExPost LMPs`.
    pub title: String,
    /// The market date printed in the file.
    pub market_date: NaiveDate,
    /// Every row, in file order.
    pub rows: Vec<ReportRow>,
}

impl LmpReport {
    /// Parse the CSV text of an LMP report.
    pub fn parse(csv_bytes: &[u8]) -> Result<Self> {
        parse_report(csv_bytes)
    }

    /// The hourly values for `node` and `component`, if the report has them.
    pub fn series(&self, node: &str, component: Component) -> Option<&[Option<f64>]> {
        self.rows
            .iter()
            .find(|r| r.node == node && r.component == component)
            .map(|r| r.hourly.as_slice())
    }

    /// The hourly LMP for `node` at `hour_ending` (1 through 24).
    pub fn lmp(&self, node: &str, hour_ending: usize) -> Option<f64> {
        let idx = hour_ending.checked_sub(1)?;
        self.series(node, Component::Lmp)?
            .get(idx)
            .copied()
            .flatten()
    }

    /// The LMP rows of every node of `node_type`.
    pub fn lmp_rows(&self, node_type: &NodeType) -> impl Iterator<Item = &ReportRow> {
        let node_type = node_type.clone();
        self.rows
            .iter()
            .filter(move |r| r.component == Component::Lmp && r.node_type == node_type)
    }
}

fn parse_report(bytes: &[u8]) -> Result<LmpReport> {
    let what = "MISO LMP report";
    let mut reader = csv::ReaderBuilder::new()
        .has_headers(false)
        .flexible(true)
        .from_reader(bytes);
    let mut preamble: Vec<String> = Vec::new();
    let mut hour_cols: Option<usize> = None;
    let mut rows = Vec::new();
    for (n, record) in reader.records().enumerate() {
        let record = record.map_err(|e| Error::decode(what, format!("record {n}: {e}")))?;
        let first = record.get(0).unwrap_or_default().trim();
        match hour_cols {
            None if first.eq_ignore_ascii_case("node") => {
                hour_cols = Some(
                    record
                        .iter()
                        .skip(3)
                        .filter(|h| h.trim().starts_with("HE"))
                        .count(),
                );
            }
            None => {
                if !first.is_empty() {
                    preamble.push(first.to_owned());
                }
            }
            Some(cols) => {
                let Some(component) = record.get(2).and_then(Component::parse) else {
                    continue;
                };
                let hourly = (3..3 + cols)
                    .map(|i| match record.get(i).map(str::trim) {
                        None | Some("") => Ok(None),
                        Some(cell) => cell.parse::<f64>().map(Some).map_err(|e| {
                            Error::decode(what, format!("row {first}: {cell:?}: {e}"))
                        }),
                    })
                    .collect::<Result<Vec<_>>>()?;
                rows.push(ReportRow {
                    node: first.to_owned(),
                    node_type: NodeType::from_label(record.get(1).unwrap_or_default()),
                    component,
                    hourly,
                });
            }
        }
    }
    if hour_cols.is_none() {
        return Err(Error::decode(
            what,
            "no `Node,Type,Value,HE 1..` header row",
        ));
    }
    let title = preamble.first().cloned().unwrap_or_default();
    let market_date = preamble
        .iter()
        .find_map(|l| NaiveDate::parse_from_str(l, "%m/%d/%Y").ok())
        .ok_or_else(|| Error::decode(what, "no market date line"))?;
    Ok(LmpReport {
        title,
        market_date,
        rows,
    })
}

impl Client {
    /// Fetch and parse an hourly LMP report for `date`.
    ///
    /// A 404 [`Error::Status`] means MISO has not published that file (yet, or any more).
    pub async fn lmp_report(&self, kind: LmpReportKind, date: NaiveDate) -> Result<LmpReport> {
        let bytes = self.market_report(&kind.file_name(date)).await?;
        parse_report(&bytes)
    }

    /// Download any file from the market report host by name, e.g. `20261116_rt_irsf.csv`.
    pub async fn market_report(&self, file_name: &str) -> Result<Vec<u8>> {
        let url = join(&self.endpoints().reports, file_name);
        self.get_bytes(&url, &[], &[]).await
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const SAMPLE: &str = "Day Ahead Market ExPost LMPs\n11/16/2026\n\n,,,All Hours-Ending are Eastern Standard Time (EST)\nNode,Type,Value,HE 1,HE 2,HE 3\nAECI,Interface,LMP,14.59,30.49,\nAECI,Interface,MCC,-.48,.67,1\nARKANSAS.HUB,Hub,LMP,22.20,17.42,12.77\n";

    #[test]
    fn parses_preamble_header_and_rows() {
        let r = LmpReport::parse(SAMPLE.as_bytes()).expect("parse");
        assert_eq!(r.title, "Day Ahead Market ExPost LMPs");
        assert_eq!(r.market_date.to_string(), "2026-11-16");
        assert_eq!(r.rows.len(), 3);
        assert_eq!(
            r.series("AECI", Component::Lmp),
            Some(&[Some(14.59), Some(30.49), None][..])
        );
        assert_eq!(
            r.series("AECI", Component::Mcc),
            Some(&[Some(-0.48), Some(0.67), Some(1.0)][..])
        );
        assert_eq!(r.lmp("ARKANSAS.HUB", 1), Some(22.20));
        assert_eq!(r.lmp("ARKANSAS.HUB", 4), None);
        assert_eq!(r.lmp("ARKANSAS.HUB", 0), None);
        assert_eq!(r.lmp_rows(&NodeType::Hub).count(), 1);
    }

    #[test]
    fn rejects_files_without_a_header_or_date() {
        assert!(LmpReport::parse(b"<html>nope</html>").is_err());
        assert!(LmpReport::parse(b"Title\n\nNode,Type,Value,HE 1\nA,Hub,LMP,1\n").is_err());
        assert!(
            LmpReport::parse(b"Title\n11/16/2026\nNode,Type,Value,HE 1\nA,Hub,LMP,abc\n").is_err()
        );
    }

    #[test]
    fn file_names_follow_miso_convention() {
        let d = NaiveDate::from_ymd_opt(2026, 11, 7).expect("date");
        assert_eq!(
            LmpReportKind::DayAheadExPost.file_name(d),
            "20261107_da_expost_lmp.csv"
        );
        assert_eq!(
            LmpReportKind::RealTimeFinal.file_name(d),
            "20261107_rt_lmp_final.csv"
        );
    }

    #[test]
    fn unknown_node_types_survive() {
        assert_eq!(
            NodeType::from_label("Whatever"),
            NodeType::Other("Whatever".into())
        );
    }
}

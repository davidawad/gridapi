//! Locational marginal prices and market clearing prices from the keyless public API.

use chrono::{DateTime, NaiveDateTime, Utc};
use serde::{Deserialize, Serialize};

use crate::client::{join, Client};
use crate::error::{Error, Result};
use crate::serde_util::{f64_loose, opt_f64_loose, u32_loose};
use crate::time::{est_to_utc, parse_ref_id, parse_timestamp};

// ---------------------------------------------------------------- ex-ante hubs

/// One hub's ex-ante price.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct HubLmp {
    /// Hub name, e.g. `ARKANSAS.HUB`.
    pub name: String,
    /// Locational marginal price, $/MWh.
    #[serde(rename = "LMP", deserialize_with = "f64_loose")]
    pub lmp: f64,
    /// Marginal loss component, $/MWh.
    #[serde(rename = "loss", deserialize_with = "f64_loose")]
    pub loss: f64,
    /// Marginal congestion component, $/MWh.
    #[serde(rename = "congestion", deserialize_with = "f64_loose")]
    pub congestion: f64,
}

/// The latest five-minute ex-ante LMP at MISO's hubs (`/api/MarketPricing/GetExAnteLmp`).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(from = "ExAnteWire")]
pub struct ExAnteLmp {
    /// MISO's interval stamp, e.g. `18-Nov-2026 - Interval 01:10 EST`.
    pub ref_id: String,
    /// The interval's `HH:MM` on the EST market clock.
    pub hour_and_min: String,
    /// One entry per hub.
    pub hubs: Vec<HubLmp>,
}

impl ExAnteLmp {
    /// Look up a hub by exact name.
    pub fn hub(&self, name: &str) -> Option<&HubLmp> {
        self.hubs.iter().find(|h| h.name == name)
    }

    /// The interval this snapshot is for, in UTC.
    pub fn interval_utc(&self) -> Option<DateTime<Utc>> {
        parse_ref_id(&self.ref_id).map(est_to_utc)
    }
}

#[derive(Deserialize)]
struct ExAnteWire {
    #[serde(rename = "LMPData")]
    data: ExAnteData,
}
#[derive(Deserialize)]
struct ExAnteData {
    #[serde(rename = "RefId")]
    ref_id: String,
    #[serde(rename = "ExAnteLMP")]
    ex_ante: ExAnteInner,
}
#[derive(Deserialize)]
struct ExAnteInner {
    #[serde(rename = "HourAndMin")]
    hour_and_min: String,
    #[serde(rename = "Hub")]
    hubs: Vec<HubLmp>,
}
impl From<ExAnteWire> for ExAnteLmp {
    fn from(w: ExAnteWire) -> Self {
        Self {
            ref_id: w.data.ref_id,
            hour_and_min: w.data.ex_ante.hour_and_min,
            hubs: w.data.ex_ante.hubs,
        }
    }
}

// ------------------------------------------------------- consolidated table

/// A price at one pricing node, with whatever components the table carries.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct NodePrice {
    /// Pricing node name, e.g. `NIPS.NIPS`.
    pub name: String,
    /// MISO region: `North`, `Central`, `South`, `Midwest`, ...
    pub region: String,
    /// Locational marginal price, $/MWh.
    #[serde(rename = "LMP", deserialize_with = "f64_loose")]
    pub lmp: f64,
    /// Marginal loss component, $/MWh.
    #[serde(rename = "MLC", deserialize_with = "f64_loose")]
    pub mlc: f64,
    /// Marginal congestion component, $/MWh.
    #[serde(rename = "MCC", deserialize_with = "f64_loose")]
    pub mcc: f64,
    /// Regulation market clearing price (five-minute table only).
    #[serde(rename = "REGMCP", default, deserialize_with = "opt_f64_loose")]
    pub reg_mcp: Option<f64>,
    /// Regulation mileage market clearing price (five-minute table only).
    #[serde(rename = "REGMILEAGEMCP", default, deserialize_with = "opt_f64_loose")]
    pub reg_mileage_mcp: Option<f64>,
    /// Spinning reserve market clearing price (five-minute table only).
    #[serde(rename = "SPINMCP", default, deserialize_with = "opt_f64_loose")]
    pub spin_mcp: Option<f64>,
    /// Supplemental reserve market clearing price (five-minute table only).
    #[serde(rename = "SUPPMCP", default, deserialize_with = "opt_f64_loose")]
    pub supp_mcp: Option<f64>,
    /// Short-term reserve market clearing price (five-minute table only).
    #[serde(rename = "STRMCP", default, deserialize_with = "opt_f64_loose")]
    pub str_mcp: Option<f64>,
    /// Ramp capability up market clearing price (five-minute table only).
    #[serde(rename = "RCUPMCP", default, deserialize_with = "opt_f64_loose")]
    pub rcup_mcp: Option<f64>,
    /// Ramp capability down market clearing price (five-minute table only).
    #[serde(rename = "RCDOWNMCP", default, deserialize_with = "opt_f64_loose")]
    pub rcdown_mcp: Option<f64>,
}

/// One labelled set of node prices.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct PriceBlock {
    /// `HH:MM` for the five-minute block, `HE n` for the hourly blocks.
    #[serde(rename = "HourAndMin")]
    pub label: String,
    /// The prices.
    #[serde(rename = "PricingNode")]
    pub nodes: Vec<NodePrice>,
}

impl PriceBlock {
    /// Look up a node by exact name.
    pub fn node(&self, name: &str) -> Option<&NodePrice> {
        self.nodes.iter().find(|n| n.name == name)
    }
}

/// Real-time, hourly-integrated and day-ahead LMPs for every published node
/// (`/api/MarketPricing/GetLmpConsolidatedTable`).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(from = "ConsolidatedWire")]
pub struct LmpConsolidatedTable {
    /// MISO's interval stamp.
    pub ref_id: String,
    /// The latest five-minute real-time prices, with ancillary clearing prices.
    pub five_minute: PriceBlock,
    /// The current hour's integrated real-time prices.
    pub hourly_integrated: PriceBlock,
    /// Day-ahead ex-ante prices for the current hour.
    pub day_ahead_ex_ante: PriceBlock,
    /// Day-ahead ex-post prices for the current hour.
    pub day_ahead_ex_post: PriceBlock,
}

#[derive(Deserialize)]
struct ConsolidatedWire {
    #[serde(rename = "LMPData")]
    data: ConsolidatedData,
}
#[derive(Deserialize)]
struct ConsolidatedData {
    #[serde(rename = "RefId")]
    ref_id: String,
    #[serde(rename = "FiveMinLMP")]
    five_minute: PriceBlock,
    #[serde(rename = "HourlyIntegratedLMP")]
    hourly_integrated: PriceBlock,
    #[serde(rename = "DayAheadExAnteLMP")]
    day_ahead_ex_ante: PriceBlock,
    #[serde(rename = "DayAheadExPostLMP")]
    day_ahead_ex_post: PriceBlock,
}
impl From<ConsolidatedWire> for LmpConsolidatedTable {
    fn from(w: ConsolidatedWire) -> Self {
        let d = w.data;
        Self {
            ref_id: d.ref_id,
            five_minute: d.five_minute,
            hourly_integrated: d.hourly_integrated,
            day_ahead_ex_ante: d.day_ahead_ex_ante,
            day_ahead_ex_post: d.day_ahead_ex_post,
        }
    }
}

// ------------------------------------------------------ five-minute ex-post

/// Which window of five-minute ex-post LMPs to fetch.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[non_exhaustive]
pub enum FiveMinuteWindow {
    /// The latest interval only.
    Current,
    /// The interval before the latest.
    Previous,
    /// Every interval since the start of the market day.
    Rolling,
}

impl FiveMinuteWindow {
    fn path(self) -> &'static str {
        match self {
            Self::Current => "api/MarketPricing/GetRealTimeFiveMinExPost/Current",
            Self::Previous => "api/MarketPricing/GetRealTimeFiveMinExPost/Previous",
            Self::Rolling => "api/MarketPricing/GetRealTimeFiveMinExPost/Rolling",
        }
    }
}

/// One node's real-time five-minute ex-post LMP.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct FiveMinuteLmp {
    /// Interval start on the EST market clock (see [`crate::est_to_utc`]).
    pub interval: NaiveDateTime,
    /// Commercial pricing node, e.g. `AMIL.GRTPLAINS`.
    pub node: String,
    /// Locational marginal price, $/MWh.
    pub lmp: f64,
    /// Marginal loss component, $/MWh.
    pub mlc: f64,
    /// Marginal congestion component, $/MWh.
    pub mcc: f64,
}

impl FiveMinuteLmp {
    /// The interval start in UTC.
    pub fn interval_utc(&self) -> DateTime<Utc> {
        est_to_utc(self.interval)
    }
}

#[derive(Deserialize)]
struct FiveMinuteWire {
    headers: Vec<String>,
    data: Vec<Vec<String>>,
}

/// Decode the `{"headers": [...], "data": [[...]]}` table the five-minute endpoints return.
pub(crate) fn decode_five_minute(body: &[u8], what: &str) -> Result<Vec<FiveMinuteLmp>> {
    let wire: FiveMinuteWire = serde_json::from_slice(body).map_err(|e| Error::decode(what, e))?;
    let col = |name: &str| {
        wire.headers
            .iter()
            .position(|h| h.eq_ignore_ascii_case(name))
            .ok_or_else(|| Error::decode(what, format!("missing column {name}")))
    };
    let (c_time, c_node, c_lmp, c_loss, c_cong) = (
        col("INTERVAL")?,
        col("CPNODE")?,
        col("LMP")?,
        col("MLC")?,
        col("MCC")?,
    );
    wire.data
        .iter()
        .enumerate()
        .map(|(n, row)| {
            let cell = |i: usize| {
                row.get(i)
                    .map(String::as_str)
                    .ok_or_else(|| Error::decode(what, format!("row {n} is short")))
            };
            let num = |i: usize| {
                cell(i).and_then(|s| {
                    s.trim()
                        .parse::<f64>()
                        .map_err(|e| Error::decode(what, format!("row {n}: {s:?}: {e}")))
                })
            };
            let stamp = cell(c_time)?;
            Ok(FiveMinuteLmp {
                interval: parse_timestamp(stamp).ok_or_else(|| {
                    Error::decode(what, format!("row {n}: bad interval {stamp:?}"))
                })?,
                node: cell(c_node)?.to_owned(),
                lmp: num(c_lmp)?,
                mlc: num(c_loss)?,
                mcc: num(c_cong)?,
            })
        })
        .collect()
}

// ------------------------------------------------------------- ancillary MCP

/// Real-time ancillary-service market clearing prices for one reserve zone.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ZoneMcp {
    /// Reserve zone number.
    #[serde(rename = "number", deserialize_with = "u32_loose")]
    pub zone: u32,
    /// Generation regulation MCP, $/MW.
    #[serde(rename = "GenRegMCP", deserialize_with = "f64_loose")]
    pub gen_reg: f64,
    /// Generation spinning reserve MCP, $/MW.
    #[serde(rename = "GenSpinMCP", deserialize_with = "f64_loose")]
    pub gen_spin: f64,
    /// Generation supplemental reserve MCP, $/MW.
    #[serde(rename = "GenSuppMCP", deserialize_with = "f64_loose")]
    pub gen_supp: f64,
    /// Short-term reserve MCP, $/MW.
    #[serde(rename = "StrMcp", deserialize_with = "f64_loose")]
    pub short_term_reserve: f64,
    /// Demand-response regulation MCP, $/MW.
    #[serde(rename = "DemandRegMcp", deserialize_with = "f64_loose")]
    pub demand_reg: f64,
    /// Demand-response spinning MCP, $/MW.
    #[serde(rename = "DemandSpinMcp", deserialize_with = "f64_loose")]
    pub demand_spin: f64,
    /// Demand-response supplemental MCP, $/MW.
    #[serde(rename = "DemandSuppMcp", deserialize_with = "f64_loose")]
    pub demand_supp: f64,
    /// Ramp capability up MCP, $/MW.
    #[serde(rename = "RcpUpMcp", deserialize_with = "f64_loose")]
    pub ramp_up: f64,
    /// Ramp capability down MCP, $/MW.
    #[serde(rename = "RcpDownMcp", deserialize_with = "f64_loose")]
    pub ramp_down: f64,
}

/// Real-time ancillary-service prices by reserve zone (`/api/MarketPricing/GetAncillaryServicesMcp`).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(from = "McpWire")]
pub struct AncillaryMcp {
    /// MISO's interval stamp.
    pub ref_id: String,
    /// The market day, `MM-DD-YYYY`.
    pub market_day: String,
    /// The interval's `HH:MM` on the EST market clock.
    pub hour_and_min: String,
    /// One entry per reserve zone.
    pub zones: Vec<ZoneMcp>,
}

#[derive(Deserialize)]
struct McpWire {
    #[serde(rename = "MCPData")]
    data: McpData,
}
#[derive(Deserialize)]
struct McpData {
    #[serde(rename = "RefId")]
    ref_id: String,
    #[serde(rename = "MktDay")]
    market_day: String,
    #[serde(rename = "RealTimeMCP")]
    real_time: McpInner,
}
#[derive(Deserialize)]
struct McpInner {
    #[serde(rename = "HourAndMin")]
    hour_and_min: String,
    #[serde(rename = "Zone")]
    zones: Vec<ZoneMcp>,
}
impl From<McpWire> for AncillaryMcp {
    fn from(w: McpWire) -> Self {
        Self {
            ref_id: w.data.ref_id,
            market_day: w.data.market_day,
            hour_and_min: w.data.real_time.hour_and_min,
            zones: w.data.real_time.zones,
        }
    }
}

// ------------------------------------------------------------------- client

impl Client {
    /// The latest five-minute ex-ante LMP at each MISO hub.
    pub async fn ex_ante_lmp(&self) -> Result<ExAnteLmp> {
        let url = join(
            &self.endpoints().public_api,
            "api/MarketPricing/GetExAnteLmp",
        );
        self.get_json(&url, &[], &[]).await
    }

    /// Real-time, hourly and day-ahead LMPs for every published pricing node.
    pub async fn lmp_consolidated_table(&self) -> Result<LmpConsolidatedTable> {
        let url = join(
            &self.endpoints().public_api,
            "api/MarketPricing/GetLmpConsolidatedTable",
        );
        self.get_json(&url, &[], &[]).await
    }

    /// Five-minute ex-post LMPs by commercial pricing node for the chosen window.
    ///
    /// [`FiveMinuteWindow::Rolling`] is several megabytes (every node, every interval so far today).
    pub async fn five_minute_ex_post_lmp(
        &self,
        window: FiveMinuteWindow,
    ) -> Result<Vec<FiveMinuteLmp>> {
        let url = join(&self.endpoints().public_api, window.path());
        let body = self.get_bytes(&url, &[], &[]).await?;
        decode_five_minute(&body, &url)
    }

    /// Real-time ancillary-service clearing prices by reserve zone.
    pub async fn ancillary_services_mcp(&self) -> Result<AncillaryMcp> {
        let url = join(
            &self.endpoints().public_api,
            "api/MarketPricing/GetAncillaryServicesMcp",
        );
        self.get_json(&url, &[], &[]).await
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn five_minute_rejects_a_table_without_the_lmp_column() {
        let body = br#"{"headers":["INTERVAL","CPNODE"],"data":[]}"#;
        let err = decode_five_minute(body, "t").expect_err("missing column");
        assert!(err.to_string().contains("missing column LMP"), "{err}");
    }

    #[test]
    fn five_minute_rejects_short_rows_and_bad_numbers() {
        let hdr = r#""headers":["INTERVAL","CPNODE","LMP","MLC","MCC"]"#;
        let short = format!(r#"{{{hdr},"data":[["2026-11-18T01:05:00","N"]]}}"#);
        assert!(decode_five_minute(short.as_bytes(), "t").is_err());
        let bad = format!(r#"{{{hdr},"data":[["2026-11-18T01:05:00","N","x","0","0"]]}}"#);
        assert!(decode_five_minute(bad.as_bytes(), "t").is_err());
    }
}

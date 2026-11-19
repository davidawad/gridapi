//! The generic OASIS `SingleZip` query and its builder.

use std::collections::BTreeMap;
use std::fmt;

use chrono::{DateTime, Utc};

use crate::error::Error;
use crate::time::format_oasis_time;

/// An OASIS report name, the `queryname` parameter.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
#[non_exhaustive]
pub enum QueryName {
    /// Day-ahead (and other hourly) locational marginal prices.
    PrcLmp,
    /// Real-time market 5-minute interval LMPs.
    PrcIntvlLmp,
    /// Real-time pre-dispatch (FMM) 15-minute LMPs.
    PrcRtpdLmp,
    /// System load forecasts by TAC area.
    SldFcst,
    /// System load and resource schedules.
    EneSlrs,
    /// Ancillary service clearing prices.
    PrcAs,
    /// Pricing node list.
    AtlPnode,
    /// Aggregate pricing node list.
    AtlApnode,
    /// Any other OASIS report by its raw name.
    Other(String),
}

impl QueryName {
    /// The wire name sent as `queryname`.
    #[must_use]
    pub fn as_str(&self) -> &str {
        match self {
            Self::PrcLmp => "PRC_LMP",
            Self::PrcIntvlLmp => "PRC_INTVL_LMP",
            Self::PrcRtpdLmp => "PRC_RTPD_LMP",
            Self::SldFcst => "SLD_FCST",
            Self::EneSlrs => "ENE_SLRS",
            Self::PrcAs => "PRC_AS",
            Self::AtlPnode => "ATL_PNODE",
            Self::AtlApnode => "ATL_APNODE",
            Self::Other(name) => name,
        }
    }

    /// The report version this crate was verified against, when known.
    #[must_use]
    pub fn default_version(&self) -> Option<u32> {
        match self {
            Self::PrcLmp => Some(12),
            Self::PrcIntvlLmp | Self::PrcRtpdLmp => Some(3),
            Self::SldFcst | Self::EneSlrs | Self::PrcAs | Self::AtlPnode | Self::AtlApnode => {
                Some(1)
            }
            Self::Other(_) => None,
        }
    }
}

impl fmt::Display for QueryName {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

/// The `market_run_id` parameter.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
#[non_exhaustive]
pub enum MarketRun {
    /// Day-ahead market.
    Dam,
    /// Real-time market (5-minute dispatch).
    Rtm,
    /// Real-time pre-dispatch / fifteen-minute market.
    Rtpd,
    /// Residual unit commitment.
    Ruc,
    /// Hour-ahead scheduling process.
    Hasp,
    /// Two-day-ahead forecast run.
    TwoDayAhead,
    /// Seven-day-ahead forecast run.
    SevenDayAhead,
    /// Any other run id by its raw name.
    Other(String),
}

impl MarketRun {
    /// The wire name sent as `market_run_id`.
    #[must_use]
    pub fn as_str(&self) -> &str {
        match self {
            Self::Dam => "DAM",
            Self::Rtm => "RTM",
            Self::Rtpd => "RTPD",
            Self::Ruc => "RUC",
            Self::Hasp => "HASP",
            Self::TwoDayAhead => "2DA",
            Self::SevenDayAhead => "7DA",
            Self::Other(name) => name,
        }
    }
}

impl fmt::Display for MarketRun {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

/// The `resultformat` parameter.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
#[non_exhaustive]
pub enum ResultFormat {
    /// Zipped CSV, `resultformat=6`. Flat, compact, and the default here.
    #[default]
    Csv,
    /// Zipped XML, OASIS's own default (`resultformat=1`).
    Xml,
}

impl ResultFormat {
    /// The numeric code sent as `resultformat`.
    #[must_use]
    pub fn code(self) -> u8 {
        match self {
            Self::Csv => 6,
            Self::Xml => 1,
        }
    }
}

/// Parameter names the builder owns; `extra` may not shadow them.
const RESERVED: [&str; 8] = [
    "queryname",
    "startdatetime",
    "enddatetime",
    "version",
    "market_run_id",
    "node",
    "grp_type",
    "resultformat",
];

/// One validated `SingleZip` request.
///
/// Build it with [`Query::builder`]. Timestamps are UTC instants; the window
/// is half-open, `[start, end)`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Query {
    name: QueryName,
    start: DateTime<Utc>,
    end: DateTime<Utc>,
    version: Option<u32>,
    market_run: Option<MarketRun>,
    nodes: Vec<String>,
    grp_type: Option<String>,
    extra: BTreeMap<String, String>,
    format: ResultFormat,
}

impl Query {
    /// Starts a query for `name` over `[start, end)`.
    ///
    /// # Examples
    ///
    /// ```
    /// use chrono::{TimeZone, Utc};
    /// use gridapi_caiso::{MarketRun, Query, QueryName};
    ///
    /// # fn main() -> Result<(), gridapi_caiso::Error> {
    /// let start = Utc.with_ymd_and_hms(2026, 11, 10, 8, 0, 0).single().unwrap_or_default();
    /// let end = Utc.with_ymd_and_hms(2026, 11, 11, 8, 0, 0).single().unwrap_or_default();
    /// let query = Query::builder(QueryName::PrcLmp, start, end)
    ///     .market_run(MarketRun::Dam)
    ///     .node("TH_NP15_GEN-APND")
    ///     .build()?;
    /// assert!(query.params().contains(&("version".to_string(), "12".to_string())));
    /// # Ok(())
    /// # }
    /// ```
    #[must_use]
    pub fn builder(name: QueryName, start: DateTime<Utc>, end: DateTime<Utc>) -> QueryBuilder {
        QueryBuilder {
            query: Self {
                version: name.default_version(),
                name,
                start,
                end,
                market_run: None,
                nodes: Vec::new(),
                grp_type: None,
                extra: BTreeMap::new(),
                format: ResultFormat::default(),
            },
        }
    }

    /// The report being requested.
    #[must_use]
    pub fn name(&self) -> &QueryName {
        &self.name
    }

    /// Inclusive start of the window.
    #[must_use]
    pub fn start(&self) -> DateTime<Utc> {
        self.start
    }

    /// Exclusive end of the window.
    #[must_use]
    pub fn end(&self) -> DateTime<Utc> {
        self.end
    }

    /// The requested wire format.
    #[must_use]
    pub fn format(&self) -> ResultFormat {
        self.format
    }

    /// The same query over a different window (used by range chunking).
    #[must_use]
    pub fn with_window(&self, start: DateTime<Utc>, end: DateTime<Utc>) -> Self {
        Self {
            start,
            end,
            ..self.clone()
        }
    }

    /// The query string pairs, in OASIS's documented order, unencoded.
    #[must_use]
    pub fn params(&self) -> Vec<(String, String)> {
        let fixed = [
            Some(("queryname", self.name.as_str().to_owned())),
            Some(("startdatetime", format_oasis_time(self.start))),
            Some(("enddatetime", format_oasis_time(self.end))),
            self.version.map(|v| ("version", v.to_string())),
            self.market_run
                .as_ref()
                .map(|m| ("market_run_id", m.as_str().to_owned())),
            (!self.nodes.is_empty()).then(|| ("node", self.nodes.join(","))),
            self.grp_type.clone().map(|g| ("grp_type", g)),
        ];
        fixed
            .into_iter()
            .flatten()
            .map(|(k, v)| (k.to_owned(), v))
            .chain(self.extra.iter().map(|(k, v)| (k.clone(), v.clone())))
            .chain(std::iter::once((
                "resultformat".to_owned(),
                self.format.code().to_string(),
            )))
            .collect()
    }
}

/// Builder for [`Query`]; setters are infallible and `build` validates.
#[derive(Debug, Clone)]
pub struct QueryBuilder {
    query: Query,
}

impl QueryBuilder {
    /// Overrides the report version (defaults to [`QueryName::default_version`]).
    pub fn version(&mut self, version: u32) -> &mut Self {
        self.query.version = Some(version);
        self
    }

    /// Sets `market_run_id`.
    pub fn market_run(&mut self, run: MarketRun) -> &mut Self {
        self.query.market_run = Some(run);
        self
    }

    /// Adds one pricing node (or trading hub, or `APNode`) to the `node` list.
    pub fn node(&mut self, node: impl Into<String>) -> &mut Self {
        self.query.nodes.push(node.into());
        self
    }

    /// Adds several nodes at once.
    pub fn nodes<I, S>(&mut self, nodes: I) -> &mut Self
    where
        I: IntoIterator<Item = S>,
        S: Into<String>,
    {
        self.query.nodes.extend(nodes.into_iter().map(Into::into));
        self
    }

    /// Sets `grp_type` (for example `ALL`, `ALL_APNODES`).
    pub fn grp_type(&mut self, grp_type: impl Into<String>) -> &mut Self {
        self.query.grp_type = Some(grp_type.into());
        self
    }

    /// Sets any report-specific parameter, e.g. `anc_type` or `tac_zone_name`.
    pub fn param(&mut self, key: impl Into<String>, value: impl Into<String>) -> &mut Self {
        self.query.extra.insert(key.into(), value.into());
        self
    }

    /// Chooses zipped CSV (default) or zipped XML.
    pub fn format(&mut self, format: ResultFormat) -> &mut Self {
        self.query.format = format;
        self
    }

    /// Validates and returns the query.
    ///
    /// # Errors
    ///
    /// [`Error::InvalidQuery`] when `end <= start`, a node name is empty or
    /// contains a comma, or an extra parameter shadows a builder-owned one.
    pub fn build(&self) -> Result<Query, Error> {
        let query = &self.query;
        if query.end <= query.start {
            return Err(Error::InvalidQuery(format!(
                "end {} is not after start {}",
                query.end, query.start
            )));
        }
        if let Some(bad) = query
            .nodes
            .iter()
            .find(|n| n.trim().is_empty() || n.contains(','))
        {
            return Err(Error::InvalidQuery(format!("invalid node name {bad:?}")));
        }
        if let Some(key) = query
            .extra
            .keys()
            .find(|k| RESERVED.contains(&k.to_ascii_lowercase().as_str()))
        {
            return Err(Error::InvalidQuery(format!(
                "parameter {key:?} is set through its dedicated builder method"
            )));
        }
        Ok(query.clone())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::TimeZone;

    fn at(h: u32) -> DateTime<Utc> {
        Utc.with_ymd_and_hms(2026, 11, 10, h, 0, 0)
            .single()
            .unwrap_or_default()
    }

    #[test]
    fn params_follow_documented_order() -> Result<(), Error> {
        let q = Query::builder(QueryName::PrcLmp, at(8), at(9))
            .market_run(MarketRun::Dam)
            .nodes(["TH_NP15_GEN-APND", "TH_SP15_GEN-APND"])
            .build()?;
        let keys: Vec<String> = q.params().into_iter().map(|(k, _)| k).collect();
        assert_eq!(
            keys,
            [
                "queryname",
                "startdatetime",
                "enddatetime",
                "version",
                "market_run_id",
                "node",
                "resultformat"
            ]
        );
        let node = q.params().into_iter().find(|(k, _)| k == "node");
        assert_eq!(
            node.map(|(_, v)| v).as_deref(),
            Some("TH_NP15_GEN-APND,TH_SP15_GEN-APND")
        );
        Ok(())
    }

    #[test]
    fn window_and_format_are_encoded() -> Result<(), Error> {
        let q = Query::builder(QueryName::SldFcst, at(8), at(10)).build()?;
        let p = q.params();
        assert!(p.contains(&("startdatetime".into(), "20261110T08:00-0000".into())));
        assert!(p.contains(&("enddatetime".into(), "20261110T10:00-0000".into())));
        assert!(p.contains(&("resultformat".into(), "6".into())));
        Ok(())
    }

    #[test]
    fn rejects_inverted_window_bad_nodes_and_shadowed_params() {
        let inverted = Query::builder(QueryName::PrcLmp, at(9), at(8)).build();
        assert!(matches!(inverted, Err(Error::InvalidQuery(_))));
        let comma = Query::builder(QueryName::PrcLmp, at(8), at(9))
            .node("A,B")
            .build();
        assert!(matches!(comma, Err(Error::InvalidQuery(_))));
        let blank = Query::builder(QueryName::PrcLmp, at(8), at(9))
            .node(" ")
            .build();
        assert!(matches!(blank, Err(Error::InvalidQuery(_))));
        let shadow = Query::builder(QueryName::PrcLmp, at(8), at(9))
            .param("Version", "9")
            .build();
        assert!(matches!(shadow, Err(Error::InvalidQuery(_))));
    }

    #[test]
    fn extra_params_and_overrides() -> Result<(), Error> {
        let q = Query::builder(QueryName::PrcAs, at(8), at(9))
            .version(2)
            .param("anc_type", "RU")
            .grp_type("ALL")
            .format(ResultFormat::Xml)
            .build()?;
        let p = q.params();
        assert!(p.contains(&("version".into(), "2".into())));
        assert!(p.contains(&("anc_type".into(), "RU".into())));
        assert!(p.contains(&("grp_type".into(), "ALL".into())));
        assert!(p.contains(&("resultformat".into(), "1".into())));
        Ok(())
    }

    #[test]
    fn with_window_keeps_everything_else() -> Result<(), Error> {
        let q = Query::builder(QueryName::PrcLmp, at(8), at(9))
            .node("X")
            .build()?;
        let moved = q.with_window(at(10), at(11));
        assert_eq!(moved.start(), at(10));
        assert_eq!(moved.end(), at(11));
        assert_eq!(moved.name(), q.name());
        Ok(())
    }
}

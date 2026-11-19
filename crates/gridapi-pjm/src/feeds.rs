//! Typed rows and request builders for the feeds most users want.
//!
//! Everything here is sugar over [`Client::query`](crate::Client::query);
//! feeds not covered are reachable through that generic call with
//! [`Row`](crate::Row) rows.

use chrono::{Datelike, NaiveDateTime};
use serde::Deserialize;

use crate::client::Client;
use crate::error::Error;
use crate::query::{DateFilter, Query, VersionFilter};

/// Feed names, for [`Client::query`] and friends.
pub mod feed {
    /// Day-ahead hourly LMPs.
    pub const DA_HRL_LMPS: &str = "da_hrl_lmps";
    /// Real-time hourly (verified) LMPs.
    pub const RT_HRL_LMPS: &str = "rt_hrl_lmps";
    /// Real-time five-minute (verified) LMPs.
    pub const RT_FIVEMIN_HRL_LMPS: &str = "rt_fivemin_hrl_lmps";
    /// Hourly metered load by zone and load area.
    pub const HRL_LOAD_METERED: &str = "hrl_load_metered";
    /// Seven-day hourly load forecast by area.
    pub const LOAD_FRCSTD_7_DAY: &str = "load_frcstd_7_day";
    /// Two-hour, five-minute load forecast by area.
    pub const VERY_SHORT_LOAD_FRCST: &str = "very_short_load_frcst";
    /// Hourly generation by fuel type.
    pub const GEN_BY_FUEL: &str = "gen_by_fuel";
    /// Pricing node master data.
    pub const PNODE: &str = "pnode";
}

/// A day-ahead hourly LMP row (`da_hrl_lmps`). Prices are USD/MWh.
///
/// Datetimes are wall-clock without an offset: `_ept` is Eastern prevailing
/// time, `_utc` is UTC. The interval *begins* at the stamp.
#[derive(Debug, Clone, PartialEq, Deserialize)]
pub struct DaHourlyLmp {
    /// Interval start, UTC.
    pub datetime_beginning_utc: NaiveDateTime,
    /// Interval start, Eastern prevailing time.
    pub datetime_beginning_ept: NaiveDateTime,
    /// Pricing node id.
    pub pnode_id: u64,
    /// Pricing node name.
    pub pnode_name: String,
    /// Voltage class, for bus nodes.
    pub voltage: Option<String>,
    /// Equipment description.
    pub equipment: Option<String>,
    /// Node type (`HUB`, `ZONE`, `GEN`, ...).
    #[serde(rename = "type")]
    pub node_type: Option<String>,
    /// Transmission zone.
    pub zone: Option<String>,
    /// System energy component.
    pub system_energy_price_da: f64,
    /// Total LMP.
    pub total_lmp_da: f64,
    /// Congestion component.
    pub congestion_price_da: f64,
    /// Marginal loss component.
    pub marginal_loss_price_da: f64,
    /// Whether this is the latest version of the row.
    pub row_is_current: bool,
    /// Version number.
    pub version_nbr: u32,
}

/// A real-time LMP row (`rt_hrl_lmps` and `rt_fivemin_hrl_lmps`). Prices are
/// USD/MWh.
#[derive(Debug, Clone, PartialEq, Deserialize)]
pub struct RtLmp {
    /// Interval start, UTC.
    pub datetime_beginning_utc: NaiveDateTime,
    /// Interval start, Eastern prevailing time.
    pub datetime_beginning_ept: NaiveDateTime,
    /// Pricing node id.
    pub pnode_id: u64,
    /// Pricing node name.
    pub pnode_name: String,
    /// Voltage class, for bus nodes.
    pub voltage: Option<String>,
    /// Equipment description.
    pub equipment: Option<String>,
    /// Node type (`HUB`, `ZONE`, `GEN`, ...).
    #[serde(rename = "type")]
    pub node_type: Option<String>,
    /// Transmission zone.
    pub zone: Option<String>,
    /// System energy component.
    pub system_energy_price_rt: f64,
    /// Total LMP.
    pub total_lmp_rt: f64,
    /// Congestion component.
    pub congestion_price_rt: f64,
    /// Marginal loss component.
    pub marginal_loss_price_rt: f64,
    /// Whether this is the latest version of the row.
    pub row_is_current: bool,
    /// Version number.
    pub version_nbr: u32,
}

/// An hourly metered-load row (`hrl_load_metered`). Load is MW.
#[derive(Debug, Clone, PartialEq, Deserialize)]
pub struct HourlyLoadMetered {
    /// Interval start, UTC.
    pub datetime_beginning_utc: NaiveDateTime,
    /// Interval start, Eastern prevailing time.
    pub datetime_beginning_ept: NaiveDateTime,
    /// NERC region (`RFC`, `SERC`, ...).
    pub nerc_region: Option<String>,
    /// Market region (`MIDATL`, `WEST`, `SOUTH`).
    pub mkt_region: Option<String>,
    /// Transmission zone (`AE`, `DOM`, ...).
    pub zone: Option<String>,
    /// Load area.
    pub load_area: Option<String>,
    /// Metered load, MW.
    pub mw: f64,
    /// Whether the distribution company verified the value. PJM keeps
    /// adjusting these for up to 90 days.
    pub is_verified: bool,
}

/// A seven-day load forecast row (`load_frcstd_7_day`). Load is MW.
#[derive(Debug, Clone, PartialEq, Deserialize)]
pub struct LoadForecast7Day {
    /// When the forecast was produced, UTC.
    pub evaluated_at_datetime_utc: NaiveDateTime,
    /// When the forecast was produced, Eastern prevailing time.
    pub evaluated_at_datetime_ept: NaiveDateTime,
    /// Forecast hour start, UTC.
    pub forecast_datetime_beginning_utc: NaiveDateTime,
    /// Forecast hour start, Eastern prevailing time.
    pub forecast_datetime_beginning_ept: NaiveDateTime,
    /// Forecast hour end, UTC.
    pub forecast_datetime_ending_utc: NaiveDateTime,
    /// Forecast hour end, Eastern prevailing time.
    pub forecast_datetime_ending_ept: NaiveDateTime,
    /// Area (`RTO_COMBINED`, `MID_ATLANTIC_REGION`, `AE/MIDATL`, ...).
    pub forecast_area: String,
    /// Forecast load, MW.
    pub forecast_load_mw: f64,
}

/// A five-minute load forecast row (`very_short_load_frcst`). Load is MW.
#[derive(Debug, Clone, PartialEq, Deserialize)]
pub struct FiveMinuteLoadForecast {
    /// When the forecast was produced, UTC.
    pub evaluated_at_utc: NaiveDateTime,
    /// When the forecast was produced, Eastern prevailing time.
    pub evaluated_at_ept: NaiveDateTime,
    /// Forecast interval start, UTC.
    pub forecast_datetime_beginning_utc: NaiveDateTime,
    /// Forecast interval start, Eastern prevailing time.
    pub forecast_datetime_beginning_ept: NaiveDateTime,
    /// Forecast interval end, UTC.
    pub forecast_datetime_ending_utc: NaiveDateTime,
    /// Forecast interval end, Eastern prevailing time.
    pub forecast_datetime_ending_ept: NaiveDateTime,
    /// Area.
    pub forecast_area: String,
    /// Forecast load, MW.
    pub forecast_load_mw: f64,
}

/// A generation-by-fuel row (`gen_by_fuel`).
#[derive(Debug, Clone, PartialEq, Deserialize)]
pub struct GenByFuel {
    /// Interval start, UTC.
    pub datetime_beginning_utc: NaiveDateTime,
    /// Interval start, Eastern prevailing time.
    pub datetime_beginning_ept: NaiveDateTime,
    /// Fuel category (`Coal`, `Gas`, `Nuclear`, `Wind`, ...).
    pub fuel_type: String,
    /// Generation, MW.
    pub mw: f64,
    /// Share of total generation, 0 to 1.
    pub fuel_percentage_of_total: f64,
    /// Whether PJM counts the fuel as renewable.
    pub is_renewable: bool,
}

/// A pricing node record (`pnode`). The feed keeps history: a node can have
/// several rows with different effective periods.
#[derive(Debug, Clone, PartialEq, Deserialize)]
pub struct Pnode {
    /// Pricing node id.
    pub pnode_id: u64,
    /// Pricing node name.
    pub pnode_name: String,
    /// `AGGREGATE`, `BUS`, `GEN`, ...
    pub pnode_type: Option<String>,
    /// `HUB`, `ZONE`, `EHV`, ...
    pub pnode_subtype: Option<String>,
    /// Transmission zone, when PJM assigns one.
    pub zone: Option<String>,
    /// Voltage level, for bus nodes.
    pub voltage_level: Option<String>,
    /// First day the record applies.
    pub effective_date: NaiveDateTime,
    /// Last day the record applies; `9999-12-31` for a current record.
    pub termination_date: NaiveDateTime,
}

impl Pnode {
    /// True for a current (unterminated) record.
    #[must_use]
    pub fn is_active(&self) -> bool {
        self.termination_date.year() >= 9999
    }
}

/// An LMP query: a date filter plus optional node, version and zone filters.
/// Works for `da_hrl_lmps`, `rt_hrl_lmps` and `rt_fivemin_hrl_lmps`; PJM
/// requires the date.
///
/// Dates are filtered on `datetime_beginning_ept`. Converts into a [`Query`]
/// for sorting, projection or paging overrides.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LmpRequest {
    when: DateFilter,
    pnode_ids: Vec<u64>,
    version: VersionFilter,
    zone: Option<String>,
}

impl LmpRequest {
    /// Latest-version LMPs in `when` for every node (add nodes with
    /// [`LmpRequest::pnode_id`], or the response is the whole system).
    #[must_use]
    pub fn new(when: impl Into<DateFilter>) -> Self {
        Self {
            when: when.into(),
            pnode_ids: Vec::new(),
            version: VersionFilter::Current,
            zone: None,
        }
    }

    /// Add one pricing node.
    #[must_use]
    pub fn pnode_id(mut self, id: u64) -> Self {
        self.pnode_ids.push(id);
        self
    }

    /// Add several pricing nodes.
    #[must_use]
    pub fn pnode_ids(mut self, ids: impl IntoIterator<Item = u64>) -> Self {
        self.pnode_ids.extend(ids);
        self
    }

    /// Select row versions (default: current only).
    #[must_use]
    pub fn version(mut self, version: VersionFilter) -> Self {
        self.version = version;
        self
    }

    /// Restrict to a transmission zone (not available for five-minute LMPs).
    #[must_use]
    pub fn zone(mut self, zone: impl Into<String>) -> Self {
        self.zone = Some(zone.into());
        self
    }
}

impl From<LmpRequest> for Query {
    fn from(r: LmpRequest) -> Self {
        let q = Query::new()
            .datetime_beginning_ept(r.when)
            .version(r.version);
        let q = if r.pnode_ids.is_empty() {
            q
        } else {
            q.pnode_ids(r.pnode_ids)
        };
        match r.zone {
            Some(zone) => q.filter("zone", zone),
            None => q,
        }
    }
}

/// A metered-load query.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LoadRequest {
    when: DateFilter,
    zone: Option<String>,
    load_area: Option<String>,
}

impl LoadRequest {
    /// Metered load in `when`, every zone.
    #[must_use]
    pub fn new(when: impl Into<DateFilter>) -> Self {
        Self {
            when: when.into(),
            zone: None,
            load_area: None,
        }
    }

    /// Restrict to a transmission zone (`AE`, `DOM`, ...).
    #[must_use]
    pub fn zone(mut self, zone: impl Into<String>) -> Self {
        self.zone = Some(zone.into());
        self
    }

    /// Restrict to a load area.
    #[must_use]
    pub fn load_area(mut self, area: impl Into<String>) -> Self {
        self.load_area = Some(area.into());
        self
    }
}

impl From<LoadRequest> for Query {
    fn from(r: LoadRequest) -> Self {
        let q = Query::new().datetime_beginning_ept(r.when);
        let q = match r.zone {
            Some(zone) => q.filter("zone", zone),
            None => q,
        };
        match r.load_area {
            Some(area) => q.filter("load_area", area),
            None => q,
        }
    }
}

/// A load-forecast query (seven-day or five-minute).
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct ForecastRequest {
    area: Option<String>,
}

impl ForecastRequest {
    /// Every forecast area.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// One area (`RTO_COMBINED`, `MID_ATLANTIC_REGION`, `DOMINION`, ...). The
    /// match is partial and case-insensitive.
    #[must_use]
    pub fn area(mut self, area: impl Into<String>) -> Self {
        self.area = Some(area.into());
        self
    }
}

impl From<ForecastRequest> for Query {
    fn from(r: ForecastRequest) -> Self {
        match r.area {
            Some(area) => Query::new().filter("forecast_area", area),
            None => Query::new(),
        }
    }
}

/// A pricing-node lookup.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PnodeRequest {
    ids: Vec<u64>,
    name: Option<String>,
    subtype: Option<String>,
    zone: Option<String>,
    active_only: bool,
}

impl Default for PnodeRequest {
    fn default() -> Self {
        Self::new()
    }
}

impl PnodeRequest {
    /// Active pnodes only, no other filter.
    #[must_use]
    pub fn new() -> Self {
        Self {
            ids: Vec::new(),
            name: None,
            subtype: None,
            zone: None,
            active_only: true,
        }
    }

    /// Look up by id.
    #[must_use]
    pub fn ids(mut self, ids: impl IntoIterator<Item = u64>) -> Self {
        self.ids.extend(ids);
        self
    }

    /// Look up by name (PJM matches whole names, case-insensitively).
    #[must_use]
    pub fn name(mut self, name: impl Into<String>) -> Self {
        self.name = Some(name.into());
        self
    }

    /// Restrict to a subtype (`HUB`, `ZONE`, `EHV`, ...).
    #[must_use]
    pub fn subtype(mut self, subtype: impl Into<String>) -> Self {
        self.subtype = Some(subtype.into());
        self
    }

    /// Restrict to a transmission zone.
    #[must_use]
    pub fn zone(mut self, zone: impl Into<String>) -> Self {
        self.zone = Some(zone.into());
        self
    }

    /// Include terminated (historical) records too.
    #[must_use]
    pub fn include_terminated(mut self) -> Self {
        self.active_only = false;
        self
    }
}

impl From<PnodeRequest> for Query {
    fn from(r: PnodeRequest) -> Self {
        let q = if r.ids.is_empty() {
            Query::new()
        } else {
            Query::new().pnode_ids(r.ids)
        };
        let q = r.name.into_iter().fold(q, |q, v| q.filter("pnode_name", v));
        let q = r
            .subtype
            .into_iter()
            .fold(q, |q, v| q.filter("pnode_subtype", v));
        let q = r.zone.into_iter().fold(q, |q, v| q.filter("zone", v));
        if r.active_only {
            q.filter("termination_date", "12/31/9999 exact")
        } else {
            q
        }
    }
}

impl Client {
    /// Day-ahead hourly LMPs, every page.
    pub async fn da_hrl_lmps(&self, req: impl Into<Query>) -> Result<Vec<DaHourlyLmp>, Error> {
        self.query_all(feed::DA_HRL_LMPS, &req.into()).await
    }

    /// Real-time hourly (verified) LMPs, every page. Verified RT prices post
    /// the next business day.
    pub async fn rt_hrl_lmps(&self, req: impl Into<Query>) -> Result<Vec<RtLmp>, Error> {
        self.query_all(feed::RT_HRL_LMPS, &req.into()).await
    }

    /// Real-time five-minute (verified) LMPs, every page. PJM keeps six months
    /// in the standard store; older data is archived with fewer filters.
    pub async fn rt_fivemin_hrl_lmps(&self, req: impl Into<Query>) -> Result<Vec<RtLmp>, Error> {
        self.query_all(feed::RT_FIVEMIN_HRL_LMPS, &req.into()).await
    }

    /// Hourly metered load, every page.
    pub async fn hrl_load_metered(
        &self,
        req: impl Into<Query>,
    ) -> Result<Vec<HourlyLoadMetered>, Error> {
        self.query_all(feed::HRL_LOAD_METERED, &req.into()).await
    }

    /// The current seven-day hourly load forecast, every page.
    pub async fn load_frcstd_7_day(
        &self,
        req: impl Into<Query>,
    ) -> Result<Vec<LoadForecast7Day>, Error> {
        self.query_all(feed::LOAD_FRCSTD_7_DAY, &req.into()).await
    }

    /// The five-minute load forecast. Unfiltered it holds 30 days of history
    /// (200k+ rows), so add an `evaluated_at_ept` filter, for example
    /// `Query::new().filter("evaluated_at_ept", "CurrentHour")`.
    pub async fn very_short_load_frcst(
        &self,
        req: impl Into<Query>,
    ) -> Result<Vec<FiveMinuteLoadForecast>, Error> {
        self.query_all(feed::VERY_SHORT_LOAD_FRCST, &req.into())
            .await
    }

    /// Hourly generation by fuel type for `when`, every page.
    pub async fn gen_by_fuel(&self, when: impl Into<DateFilter>) -> Result<Vec<GenByFuel>, Error> {
        let query = Query::new().datetime_beginning_ept(when);
        self.query_all(feed::GEN_BY_FUEL, &query).await
    }

    /// Pricing-node records matching `req`, every page.
    pub async fn pnodes(&self, req: impl Into<Query>) -> Result<Vec<Pnode>, Error> {
        self.query_all(feed::PNODE, &req.into()).await
    }

    /// The current record of one pricing node, if it exists and is active.
    pub async fn pnode(&self, pnode_id: u64) -> Result<Option<Pnode>, Error> {
        let rows = self.pnodes(PnodeRequest::new().ids([pnode_id])).await?;
        Ok(rows.into_iter().next())
    }
}

#[cfg(test)]
mod tests {
    use chrono::NaiveDate;

    use super::*;
    use crate::page::Page;

    fn decode<T: serde::de::DeserializeOwned>(body: &str) -> Page<T> {
        serde_json::from_str(body).unwrap()
    }

    fn params(q: impl Into<Query>) -> std::collections::BTreeMap<String, String> {
        q.into().to_params().unwrap().into_iter().collect()
    }

    fn nov17() -> NaiveDate {
        NaiveDate::from_ymd_opt(2026, 11, 17).unwrap()
    }

    #[test]
    fn da_fixture_decodes_the_western_hub_row() {
        let page: Page<DaHourlyLmp> = decode(include_str!("../tests/fixtures/da_hrl_lmps.json"));
        assert_eq!(page.total_rows, 24);
        let first = &page.items[0];
        assert_eq!(first.pnode_id, 51288);
        assert_eq!(first.pnode_name, "WESTERN HUB");
        assert_eq!(first.node_type.as_deref(), Some("HUB"));
        assert_eq!(first.zone, None);
        assert_eq!(
            first.datetime_beginning_utc.to_string(),
            "2026-11-17 05:00:00"
        );
        assert_eq!(
            first.datetime_beginning_ept.to_string(),
            "2026-11-17 00:00:00"
        );
        assert!((first.total_lmp_da - 116.425_657).abs() < 1e-9);
        assert!(first.row_is_current && first.version_nbr == 1);
        let parts =
            first.system_energy_price_da + first.congestion_price_da + first.marginal_loss_price_da;
        assert!(
            (parts - first.total_lmp_da).abs() < 1e-3,
            "LMP components sum to total"
        );
    }

    #[test]
    fn rt_fixtures_decode() {
        let hourly: Page<RtLmp> = decode(include_str!("../tests/fixtures/rt_hrl_lmps.json"));
        assert_eq!(hourly.items.len(), 3);
        assert!((hourly.items[0].total_lmp_rt - 130.414_572).abs() < 1e-9);
        let five: Page<RtLmp> = decode(include_str!("../tests/fixtures/rt_fivemin_hrl_lmps.json"));
        assert_eq!(five.total_rows, 288);
        assert_eq!(
            five.items[1].datetime_beginning_utc - five.items[0].datetime_beginning_utc,
            chrono::Duration::minutes(5)
        );
    }

    #[test]
    fn load_forecast_gen_and_pnode_fixtures_decode() {
        let load: Page<HourlyLoadMetered> =
            decode(include_str!("../tests/fixtures/hrl_load_metered.json"));
        assert_eq!(load.items[0].zone.as_deref(), Some("AE"));
        assert!((load.items[0].mw - 877.303).abs() < 1e-9);
        assert!(!load.items[0].is_verified);

        let fc: Page<LoadForecast7Day> =
            decode(include_str!("../tests/fixtures/load_frcstd_7_day.json"));
        assert_eq!(fc.total_rows, 168);
        assert_eq!(fc.items[0].forecast_area, "RTO_COMBINED");
        assert!((fc.items[0].forecast_load_mw - 93_723.0).abs() < 1e-9);

        let vs: Page<FiveMinuteLoadForecast> =
            decode(include_str!("../tests/fixtures/very_short_load_frcst.json"));
        assert_eq!(vs.items[0].forecast_area, "RTO_COMBINED");

        let gen: Page<GenByFuel> = decode(include_str!("../tests/fixtures/gen_by_fuel.json"));
        assert_eq!(gen.items[0].fuel_type, "Coal");
        assert!((gen.items[0].mw - 21_791.4).abs() < 1e-9);
        assert!(!gen.items[0].is_renewable);

        let pn: Page<Pnode> = decode(include_str!("../tests/fixtures/pnode.json"));
        assert_eq!(pn.items[0].pnode_name, "WESTERN HUB");
        assert_eq!(pn.items[0].pnode_subtype.as_deref(), Some("HUB"));
        assert!(pn.items[0].is_active());
    }

    #[test]
    fn terminated_pnode_is_not_active() {
        let body = r#"{"pnode_id":1,"pnode_name":"PJM-RTO","pnode_type":"AGGREGATE","pnode_subtype":"ZONE",
            "zone":"","voltage_level":"","effective_date":"2004-05-01T00:00:00","termination_date":"2017-09-05T00:00:00"}"#;
        let p: Pnode = serde_json::from_str(body).unwrap();
        assert!(!p.is_active());
    }

    #[test]
    fn lmp_request_builds_the_documented_filters() {
        let p = params(
            LmpRequest::new(nov17())
                .pnode_id(51288)
                .pnode_ids([1, 2])
                .zone("PSEG"),
        );
        assert_eq!(
            p["datetime_beginning_ept"],
            "11/17/2026 00:00to11/17/2026 23:59"
        );
        assert_eq!(p["pnode_id"], "51288;1;2");
        assert_eq!(p["row_is_current"], "true");
        assert_eq!(p["zone"], "PSEG");
        assert_eq!(p["rowCount"], "50000");
        assert_eq!(p["startRow"], "1");
    }

    #[test]
    fn lmp_request_without_nodes_has_no_pnode_filter() {
        let p = params(LmpRequest::new(nov17()).version(VersionFilter::All));
        assert!(!p.contains_key("pnode_id"));
        assert_eq!(p["row_is_current"], "all");
    }

    #[test]
    fn load_forecast_and_pnode_requests() {
        let p = params(LoadRequest::new(nov17()).zone("AE").load_area("AECO"));
        assert_eq!(
            (p["zone"].as_str(), p["load_area"].as_str()),
            ("AE", "AECO")
        );
        assert_eq!(
            params(ForecastRequest::new().area("RTO_COMBINED"))["forecast_area"],
            "RTO_COMBINED"
        );
        assert!(!params(ForecastRequest::new()).contains_key("forecast_area"));

        let p = params(PnodeRequest::new().ids([51288]).subtype("HUB"));
        assert_eq!(p["pnode_id"], "51288");
        assert_eq!(p["pnode_subtype"], "HUB");
        assert_eq!(p["termination_date"], "12/31/9999 exact");
        let p = params(PnodeRequest::new().name("WESTERN HUB").include_terminated());
        assert_eq!(p["pnode_name"], "WESTERN HUB");
        assert!(!p.contains_key("termination_date"));
    }
}

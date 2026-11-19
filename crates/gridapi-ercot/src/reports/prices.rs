//! Settlement point prices and locational marginal prices.

use chrono::{Duration, NaiveDate, NaiveDateTime};

use crate::client::Client;
use crate::error::Result;
use crate::page::{FromRow, HourEnding, Row};
use crate::request::{Query, Report};

/// Kind of settlement point, from the report's two-letter code.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
#[non_exhaustive]
pub enum SettlementPointType {
    /// Trading hub (`HU`), e.g. `HB_NORTH`.
    Hub,
    /// Load zone (`LZ`), e.g. `LZ_HOUSTON`.
    LoadZone,
    /// Resource node (`RN`).
    ResourceNode,
    /// Any other code ERCOT publishes (kept verbatim).
    Other(String),
}

impl SettlementPointType {
    /// Maps ERCOT's code (`HU`, `LZ`, `RN`, ...) to a variant.
    #[must_use]
    pub fn from_code(code: &str) -> Self {
        match code {
            "HU" => Self::Hub,
            "LZ" => Self::LoadZone,
            "RN" => Self::ResourceNode,
            other => Self::Other(other.to_owned()),
        }
    }

    /// ERCOT's two-letter code.
    #[must_use]
    pub fn code(&self) -> &str {
        match self {
            Self::Hub => "HU",
            Self::LoadZone => "LZ",
            Self::ResourceNode => "RN",
            Self::Other(code) => code,
        }
    }
}

// ---------------------------------------------------------------------------
// NP6-905-CD: real-time settlement point prices

/// NP6-905-CD: real-time settlement point prices at resource nodes, hubs and load zones.
#[derive(Debug, Clone, Copy)]
pub struct RealTimeSpp;

/// One 15-minute real-time settlement point price.
#[derive(Debug, Clone, PartialEq)]
pub struct RealTimeSppRecord {
    /// Operating day.
    pub delivery_date: NaiveDate,
    /// Hour of the operating day, `1..=24`.
    pub delivery_hour: u8,
    /// 15-minute interval within the hour, `1..=4`.
    pub delivery_interval: u8,
    /// Settlement point name, e.g. `HB_NORTH`.
    pub settlement_point: String,
    /// Settlement point kind.
    pub settlement_point_type: SettlementPointType,
    /// Price in USD per MWh.
    pub price: f64,
    /// True during the repeated hour at the autumn clock change.
    pub dst_flag: bool,
}

impl RealTimeSppRecord {
    /// End of the 15-minute interval, as local clock time of the operating day.
    ///
    /// Returns `None` for an out-of-range hour or interval. The repeated hour on the
    /// autumn clock change maps to the same clock time as its first occurrence; use
    /// [`dst_flag`](Self::dst_flag) to tell them apart.
    #[must_use]
    pub fn interval_ending(&self) -> Option<NaiveDateTime> {
        let valid =
            (1..=24).contains(&self.delivery_hour) && (1..=4).contains(&self.delivery_interval);
        valid.then(|| {
            let minutes =
                (i64::from(self.delivery_hour) - 1) * 60 + i64::from(self.delivery_interval) * 15;
            self.delivery_date.and_time(chrono::NaiveTime::MIN) + Duration::minutes(minutes)
        })
    }
}

impl FromRow for RealTimeSppRecord {
    fn from_row(row: &Row<'_>) -> Result<Self> {
        Ok(Self {
            delivery_date: row.date("deliveryDate")?,
            delivery_hour: row.u8("deliveryHour")?,
            delivery_interval: row.u8("deliveryInterval")?,
            settlement_point: row.str("settlementPoint")?.to_owned(),
            settlement_point_type: SettlementPointType::from_code(row.str("settlementPointType")?),
            price: row.f64("settlementPointPrice")?,
            dst_flag: row.bool("DSTFlag")?,
        })
    }
}

impl Report for RealTimeSpp {
    type Record = RealTimeSppRecord;
    const EMIL_ID: &'static str = "NP6-905-CD";
    const ARTIFACT: &'static str = "spp_node_zone_hub";
}

impl Query<RealTimeSpp> {
    /// Restricts to operating days `from..=to`.
    #[must_use]
    pub fn delivery_date(self, from: NaiveDate, to: NaiveDate) -> Self {
        self.date_range("deliveryDate", from, to)
    }

    /// Restricts to hours `from..=to` (1-based, 1..=24).
    #[must_use]
    pub fn delivery_hour(self, from: u8, to: u8) -> Self {
        self.map(|r| r.range("deliveryHour", from, to))
    }

    /// Restricts to one settlement point, e.g. `HB_NORTH` or `LZ_HOUSTON`.
    #[must_use]
    pub fn settlement_point(self, name: &str) -> Self {
        self.filter("settlementPoint", name)
    }

    /// Restricts to one kind of settlement point.
    #[must_use]
    pub fn settlement_point_type(self, kind: &SettlementPointType) -> Self {
        self.filter("settlementPointType", kind.code())
    }
}

impl Client {
    /// The newest published real-time price for `settlement_point`.
    ///
    /// The report sorts the newest day first but intervals ascending within it, so a
    /// plain `size(1)` query returns the *first* interval of the day. This fetches
    /// `today - 1 ..= today` (at most 192 rows) and returns the latest interval, or
    /// `None` when ERCOT has published nothing for that point in the window.
    ///
    /// `today` is the current operating day in Central time; the library has no clock.
    ///
    /// # Errors
    ///
    /// As [`Client::fetch_all`].
    pub async fn latest_real_time_spp(
        &self,
        settlement_point: &str,
        today: NaiveDate,
    ) -> Result<Option<RealTimeSppRecord>> {
        let query = Query::<RealTimeSpp>::new()
            .delivery_date(today - Duration::days(1), today)
            .settlement_point(settlement_point);
        let rows = self.fetch_all(&query, 2).await?;
        Ok(rows
            .into_iter()
            .max_by_key(|r| (r.delivery_date, r.delivery_hour, r.delivery_interval)))
    }
}

// ---------------------------------------------------------------------------
// NP4-190-CD: day-ahead settlement point prices

/// NP4-190-CD: day-ahead market settlement point prices.
#[derive(Debug, Clone, Copy)]
pub struct DayAheadSpp;

/// One hourly day-ahead settlement point price.
#[derive(Debug, Clone, PartialEq)]
pub struct DayAheadSppRecord {
    /// Operating day.
    pub delivery_date: NaiveDate,
    /// Hour ending.
    pub hour_ending: HourEnding,
    /// Settlement point name.
    pub settlement_point: String,
    /// Price in USD per MWh.
    pub price: f64,
    /// True during the repeated hour at the autumn clock change.
    pub dst_flag: bool,
}

impl FromRow for DayAheadSppRecord {
    fn from_row(row: &Row<'_>) -> Result<Self> {
        Ok(Self {
            delivery_date: row.date("deliveryDate")?,
            hour_ending: row.hour_ending("hourEnding")?,
            settlement_point: row.str("settlementPoint")?.to_owned(),
            price: row.f64("settlementPointPrice")?,
            dst_flag: row.bool("DSTFlag")?,
        })
    }
}

impl Report for DayAheadSpp {
    type Record = DayAheadSppRecord;
    const EMIL_ID: &'static str = "NP4-190-CD";
    const ARTIFACT: &'static str = "dam_stlmnt_pnt_prices";
}

impl Query<DayAheadSpp> {
    /// Restricts to operating days `from..=to`.
    #[must_use]
    pub fn delivery_date(self, from: NaiveDate, to: NaiveDate) -> Self {
        self.date_range("deliveryDate", from, to)
    }

    /// Restricts to one hour ending (this column supports exact match only).
    #[must_use]
    pub fn hour_ending(self, hour: HourEnding) -> Self {
        self.filter("hourEnding", hour.label())
    }

    /// Restricts to one settlement point.
    #[must_use]
    pub fn settlement_point(self, name: &str) -> Self {
        self.filter("settlementPoint", name)
    }
}

// ---------------------------------------------------------------------------
// NP6-788-CD: real-time LMPs by node, zone, hub

/// NP6-788-CD: real-time locational marginal prices from each SCED run.
#[derive(Debug, Clone, Copy)]
pub struct RealTimeLmp;

/// One SCED-interval LMP at a settlement point.
#[derive(Debug, Clone, PartialEq)]
pub struct RealTimeLmpRecord {
    /// SCED run timestamp (Central time).
    pub sced_timestamp: NaiveDateTime,
    /// True for the repeated hour at the autumn clock change.
    pub repeat_hour: bool,
    /// Settlement point name.
    pub settlement_point: String,
    /// Locational marginal price in USD per MWh.
    pub lmp: f64,
}

impl FromRow for RealTimeLmpRecord {
    fn from_row(row: &Row<'_>) -> Result<Self> {
        Ok(Self {
            sced_timestamp: row.datetime("SCEDTimestamp")?,
            repeat_hour: row.bool("repeatHourFlag")?,
            settlement_point: row.str("settlementPoint")?.to_owned(),
            lmp: row.f64("LMP")?,
        })
    }
}

impl Report for RealTimeLmp {
    type Record = RealTimeLmpRecord;
    const EMIL_ID: &'static str = "NP6-788-CD";
    const ARTIFACT: &'static str = "lmp_node_zone_hub";
}

impl Query<RealTimeLmp> {
    /// Restricts to SCED runs within `from..=to`.
    #[must_use]
    pub fn sced_timestamp(self, from: NaiveDateTime, to: NaiveDateTime) -> Self {
        self.datetime_range("SCEDTimestamp", from, to)
    }

    /// Restricts to one settlement point (resource node, load zone or hub).
    #[must_use]
    pub fn settlement_point(self, name: &str) -> Self {
        self.filter("settlementPoint", name)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::page::ReportPage;

    fn d(y: i32, m: u32, day: u32) -> NaiveDate {
        NaiveDate::from_ymd_opt(y, m, day).expect("valid date")
    }

    #[test]
    fn real_time_spp_record_from_fixture_row_shape() {
        let body = br#"{"_meta":{"totalRecords":1,"pageSize":1,"totalPages":1,"currentPage":1},
          "report":{"reportName":"spp_node_zone_hub","reportDisplayName":"x","reportEMIL":"NP6-905-CD","downloadLimit":1},
          "fields":[
            {"name":"deliveryDate","label":"","cardinality":1,"dataType":"DATE","searchable":true,"sortable":true,"hasRange":true},
            {"name":"deliveryHour","label":"","cardinality":2,"dataType":"INTEGER","searchable":true,"sortable":true,"hasRange":true},
            {"name":"deliveryInterval","label":"","cardinality":3,"dataType":"INTEGER","searchable":true,"sortable":true,"hasRange":true},
            {"name":"settlementPoint","label":"","cardinality":4,"dataType":"STRING","searchable":true,"sortable":true,"hasRange":false},
            {"name":"settlementPointType","label":"","cardinality":5,"dataType":"STRING","searchable":true,"sortable":true,"hasRange":false},
            {"name":"settlementPointPrice","label":"","cardinality":6,"dataType":"DOUBLE","searchable":true,"sortable":true,"hasRange":true},
            {"name":"DSTFlag","label":"","cardinality":7,"dataType":"BOOLEAN","searchable":true,"sortable":true,"hasRange":false}],
          "data":[["2026-11-17",1,1,"HB_NORTH","HU",30.43,false],["2026-11-17",24,4,"LZ_HOUSTON","LZ",-3.4,true]]}"#;
        let recs = ReportPage::from_json(body)
            .expect("page")
            .records::<RealTimeSppRecord>()
            .expect("records");
        assert_eq!(recs[0].settlement_point_type, SettlementPointType::Hub);
        assert!((recs[0].price - 30.43).abs() < 1e-9);
        assert_eq!(recs[1].settlement_point_type, SettlementPointType::LoadZone);
        assert!(recs[1].price < 0.0, "negative prices are valid in ERCOT");
        assert!(recs[1].dst_flag);
    }

    #[test]
    fn interval_ending_maps_hour_and_interval_to_clock_time() {
        let rec = |h, i| RealTimeSppRecord {
            delivery_date: d(2026, 11, 17),
            delivery_hour: h,
            delivery_interval: i,
            settlement_point: "HB_NORTH".into(),
            settlement_point_type: SettlementPointType::Hub,
            price: 1.0,
            dst_flag: false,
        };
        let at = |r: &RealTimeSppRecord| r.interval_ending().expect("valid").to_string();
        assert_eq!(at(&rec(1, 1)), "2026-11-17 00:15:00");
        assert_eq!(at(&rec(1, 4)), "2026-11-17 01:00:00");
        assert_eq!(at(&rec(24, 4)), "2026-11-18 00:00:00");
        assert!(rec(0, 1).interval_ending().is_none());
        assert!(rec(25, 1).interval_ending().is_none());
        assert!(rec(1, 5).interval_ending().is_none());
    }

    #[test]
    fn dam_and_lmp_records_parse_real_fixtures() {
        let dam = ReportPage::from_json(include_bytes!("../../tests/fixtures/dam_spp.json"))
            .expect("page")
            .records::<DayAheadSppRecord>()
            .expect("records");
        assert_eq!(dam.len(), 3);
        assert_eq!(dam[0].hour_ending.get(), 1);
        assert_eq!(dam[0].settlement_point, "HB_HOUSTON");

        let lmp = ReportPage::from_json(include_bytes!("../../tests/fixtures/lmp_node.json"))
            .expect("page")
            .records::<RealTimeLmpRecord>()
            .expect("records");
        assert_eq!(lmp.len(), 2);
        assert_eq!(lmp[0].sced_timestamp.to_string(), "2026-11-17 23:05:18");
        assert!((lmp[0].lmp - 11.65).abs() < 1e-9);
    }

    #[test]
    fn typed_filters_use_the_api_parameter_names() {
        let q = Query::<RealTimeSpp>::new()
            .delivery_date(d(2026, 11, 1), d(2026, 11, 2))
            .delivery_hour(3, 5)
            .settlement_point("HB_NORTH")
            .settlement_point_type(&SettlementPointType::Hub);
        let names: Vec<String> = q.request().params().into_iter().map(|(n, _)| n).collect();
        for want in [
            "deliveryDateFrom",
            "deliveryDateTo",
            "deliveryHourFrom",
            "deliveryHourTo",
            "settlementPoint",
            "settlementPointType",
        ] {
            assert!(
                names.iter().any(|n| n == want),
                "missing {want} in {names:?}"
            );
        }
        assert_eq!(q.request().path(), "np6-905-cd/spp_node_zone_hub");

        let dam = Query::<DayAheadSpp>::new().hour_ending(HourEnding::try_new(7).expect("hour"));
        assert!(dam
            .request()
            .params()
            .contains(&("hourEnding".to_owned(), "07:00".to_owned())));
    }

    #[test]
    fn settlement_point_type_round_trips_codes() {
        for code in ["HU", "LZ", "RN", "PCCRN"] {
            assert_eq!(SettlementPointType::from_code(code).code(), code);
        }
    }
}

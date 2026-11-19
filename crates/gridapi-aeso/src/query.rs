//! Typed request builders. Each turns into query-string pairs; nothing here
//! touches the network, so every rule is unit-testable.

use chrono::{Duration, NaiveDate};

use crate::error::{Error, Result};

/// A `startDate` with an optional `endDate`, both inclusive calendar dates in
/// Mountain time. Without an end the API returns the start day only.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct DateRange {
    start: NaiveDate,
    end: Option<NaiveDate>,
}

impl DateRange {
    /// A single day.
    pub fn day(date: NaiveDate) -> Self {
        Self {
            start: date,
            end: None,
        }
    }

    /// `start..=end`. Fails when `end` precedes `start`.
    pub fn new(start: NaiveDate, end: NaiveDate) -> Result<Self> {
        if end < start {
            return Err(Error::InvalidQuery(format!(
                "end date {end} is before start date {start}"
            )));
        }
        Ok(Self {
            start,
            end: Some(end),
        })
    }

    /// First day.
    pub fn start(&self) -> NaiveDate {
        self.start
    }

    /// Last day (the start day when no end was given).
    pub fn end(&self) -> NaiveDate {
        self.end.unwrap_or(self.start)
    }

    /// Number of calendar days covered.
    pub fn days(&self) -> i64 {
        (self.end() - self.start).num_days() + 1
    }

    /// Split into consecutive ranges of at most `max_days` days each, for APIs
    /// that cap the span (pool price: 366 days; AIES capacity: 31 days).
    /// `max_days` below 1 is treated as 1.
    pub fn chunks(&self, max_days: i64) -> Vec<Self> {
        let step = max_days.max(1);
        std::iter::successors(Some(self.start), |s| {
            s.checked_add_signed(Duration::days(step))
                .filter(|next| *next <= self.end())
        })
        .map(|s| {
            let last = s
                .checked_add_signed(Duration::days(step - 1))
                .map_or(self.end(), |e| e.min(self.end()));
            Self {
                start: s,
                end: Some(last),
            }
        })
        .collect()
    }

    /// `startDate`/`endDate` pairs in the `yyyy-MM-dd` form most APIs use.
    pub(crate) fn iso_params(self) -> Vec<(&'static str, String)> {
        self.params("%Y-%m-%d")
    }

    /// `startDate`/`endDate` pairs in the `yyyyMMdd` form the ITC API uses.
    pub(crate) fn compact_params(self) -> Vec<(&'static str, String)> {
        self.params("%Y%m%d")
    }

    fn params(self, fmt: &str) -> Vec<(&'static str, String)> {
        std::iter::once(("startDate", self.start.format(fmt).to_string()))
            .chain(self.end.map(|e| ("endDate", e.format(fmt).to_string())))
            .collect()
    }
}

/// Which transfer data series to return from the interchange capability API.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DataType {
    /// Available transfer capability.
    Atc,
    /// Total transfer capability.
    Ttc,
    /// Gross offer.
    GrossOffer,
    /// System transmission reliability margin.
    TrmSystem,
    /// Allocated transmission reliability margin.
    TrmAllocation,
    /// Total transmission reliability margin.
    TrmTotal,
}

impl DataType {
    const fn param(self) -> &'static str {
        match self {
            Self::Atc => "ATC",
            Self::Ttc => "TTC",
            Self::GrossOffer => "GROSS_OFFER",
            Self::TrmSystem => "TRM_SYSTEM",
            Self::TrmAllocation => "TRM_ALLOCATION",
            Self::TrmTotal => "TRM_TOTAL",
        }
    }
}

/// An Alberta intertie or flowgate.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Intertie {
    /// Alberta-British Columbia.
    Bc,
    /// Alberta-Montana (MATL).
    Matl,
    /// Alberta-Saskatchewan.
    Sk,
    /// The combined BC + MATL flowgate.
    BcMatl,
    /// The system-wide flowgate (interchange API only).
    System,
}

impl Intertie {
    const fn param(self) -> &'static str {
        match self {
            Self::Bc => "BC",
            Self::Matl => "MATL",
            Self::Sk => "SK",
            Self::BcMatl => "BC_MATL",
            Self::System => "SYSTEM",
        }
    }
}

/// Direction of a transfer.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TransferType {
    /// Into Alberta.
    Import,
    /// Out of Alberta.
    Export,
}

/// Whether to include every data revision made in the window.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Revisions {
    /// Only the values in force.
    #[default]
    Current,
    /// All revisions since 60 minutes before the hour began.
    All,
}

/// Query for `itc/v1/interchange` (interchange capability report).
/// The API serves at most 100 days at a time.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct InterchangeQuery {
    range: DateRange,
    start_he: u8,
    end_he: u8,
    revisions: Revisions,
    data_type: Option<DataType>,
    intertie: Option<Intertie>,
    transfer: Option<TransferType>,
}

impl InterchangeQuery {
    /// All hours of `range`, all series, current values only.
    pub fn new(range: DateRange) -> Self {
        Self {
            range,
            start_he: 1,
            end_he: 24,
            revisions: Revisions::Current,
            data_type: None,
            intertie: None,
            transfer: None,
        }
    }

    /// Restrict to hour-ending `start..=end` (each `1..=24`, `start <= end`).
    pub fn hours(mut self, start: u8, end: u8) -> Result<Self> {
        if !(1..=24).contains(&start) || !(1..=24).contains(&end) || start > end {
            return Err(Error::InvalidQuery(format!(
                "hour ending range {start}..={end} must lie in 1..=24 and be ordered"
            )));
        }
        self.start_he = start;
        self.end_he = end;
        Ok(self)
    }

    /// Include or exclude superseded revisions.
    #[must_use]
    pub fn revisions(mut self, revisions: Revisions) -> Self {
        self.revisions = revisions;
        self
    }

    /// Return only one data series.
    #[must_use]
    pub fn data_type(mut self, data_type: DataType) -> Self {
        self.data_type = Some(data_type);
        self
    }

    /// Return only one intertie or flowgate.
    #[must_use]
    pub fn intertie(mut self, intertie: Intertie) -> Self {
        self.intertie = Some(intertie);
        self
    }

    /// Return only imports or only exports.
    #[must_use]
    pub fn transfer(mut self, transfer: TransferType) -> Self {
        self.transfer = Some(transfer);
        self
    }

    pub(crate) fn params(&self) -> Vec<(&'static str, String)> {
        let mut p = self.range.compact_params();
        // The API requires an explicit endDate for this report.
        if self.range.end.is_none() {
            p.push(("endDate", self.range.start.format("%Y%m%d").to_string()));
        }
        p.push(("startHE", self.start_he.to_string()));
        p.push(("endHE", self.end_he.to_string()));
        p.push((
            "version",
            matches!(self.revisions, Revisions::All).to_string(),
        ));
        if let Some(t) = self.data_type {
            p.push(("dataType", t.param().to_owned()));
        }
        if let Some(i) = self.intertie {
            p.push(("intertieOrFlowgate", i.param().to_owned()));
        }
        if let Some(t) = self.transfer {
            p.push((
                "transferType",
                match t {
                    TransferType::Import => "IMPORT",
                    TransferType::Export => "EXPORT",
                }
                .to_owned(),
            ));
        }
        p
    }
}

/// Query for `itc/v1/outage` (outages affecting transfer capability; 13 months at a time).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct OutageQuery {
    range: DateRange,
    intertie: Option<Intertie>,
}

impl OutageQuery {
    /// All interties over `range`.
    pub fn new(range: DateRange) -> Self {
        Self {
            range,
            intertie: None,
        }
    }

    /// Only outages affecting one line. The API accepts `BC`, `MATL`, `SK` and `BC_MATL`.
    pub fn intertie(mut self, intertie: Intertie) -> Result<Self> {
        if matches!(intertie, Intertie::System) {
            return Err(Error::InvalidQuery(
                "the outage report has no SYSTEM filter (use BC, MATL, SK or BC_MATL)".into(),
            ));
        }
        self.intertie = Some(intertie);
        Ok(self)
    }

    pub(crate) fn params(&self) -> Vec<(&'static str, String)> {
        let mut p = self.range.compact_params();
        if self.range.end.is_none() {
            p.push(("endDate", self.range.start.format("%Y%m%d").to_string()));
        }
        if let Some(i) = self.intertie {
            p.push(("affectedIntertieOrFlowgate", i.param().to_owned()));
        }
        p
    }
}

/// Filters for the asset list (`assetlist-api/v1/assetlist`).
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct AssetListQuery {
    asset_id: Option<String>,
    pool_participant_id: Option<String>,
    operating_status: Option<String>,
    asset_type: Option<String>,
}

impl AssetListQuery {
    /// No filters: every asset.
    pub fn all() -> Self {
        Self::default()
    }

    /// One asset by its 3-4 character id, for example `AST1`.
    #[must_use]
    pub fn asset_id(mut self, id: impl Into<String>) -> Self {
        self.asset_id = Some(id.into());
        self
    }

    /// Assets of one pool participant, for example `PPI1`.
    #[must_use]
    pub fn pool_participant_id(mut self, id: impl Into<String>) -> Self {
        self.pool_participant_id = Some(id.into());
        self
    }

    /// Filter on operating status, for example `Active` or `Retired`.
    #[must_use]
    pub fn operating_status(mut self, status: impl Into<String>) -> Self {
        self.operating_status = Some(status.into());
        self
    }

    /// Filter on asset type, for example `SOURCE` or `SINK`.
    #[must_use]
    pub fn asset_type(mut self, asset_type: impl Into<String>) -> Self {
        self.asset_type = Some(asset_type.into());
        self
    }

    pub(crate) fn params(&self) -> Vec<(&'static str, String)> {
        [
            ("asset_ID", &self.asset_id),
            ("pool_participant_ID", &self.pool_participant_id),
            ("operating_status", &self.operating_status),
            ("asset_type", &self.asset_type),
        ]
        .into_iter()
        .filter_map(|(k, v)| v.as_ref().map(|v| (k, v.clone())))
        .collect()
    }
}

/// Query for the metered volume report. Up to 20 asset / participant ids per filter.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MeteredVolumeQuery {
    range: DateRange,
    asset_ids: Vec<String>,
    pool_participant_ids: Vec<String>,
}

impl MeteredVolumeQuery {
    /// Every asset over `range`.
    pub fn new(range: DateRange) -> Self {
        Self {
            range,
            asset_ids: Vec::new(),
            pool_participant_ids: Vec::new(),
        }
    }

    /// Restrict to these assets (at most 20).
    pub fn assets<I, S>(mut self, ids: I) -> Result<Self>
    where
        I: IntoIterator<Item = S>,
        S: Into<String>,
    {
        self.asset_ids = ids.into_iter().map(Into::into).collect();
        check_id_count("asset", self.asset_ids.len())?;
        Ok(self)
    }

    /// Restrict to these pool participants (at most 20).
    pub fn pool_participants<I, S>(mut self, ids: I) -> Result<Self>
    where
        I: IntoIterator<Item = S>,
        S: Into<String>,
    {
        self.pool_participant_ids = ids.into_iter().map(Into::into).collect();
        check_id_count("pool participant", self.pool_participant_ids.len())?;
        Ok(self)
    }

    pub(crate) fn params(&self) -> Vec<(&'static str, String)> {
        let mut p = self.range.iso_params();
        if !self.asset_ids.is_empty() {
            p.push(("asset_ID", self.asset_ids.join(",")));
        }
        if !self.pool_participant_ids.is_empty() {
            p.push(("pool_participant_ID", self.pool_participant_ids.join(",")));
        }
        p
    }
}

fn check_id_count(what: &str, n: usize) -> Result<()> {
    if n > 20 {
        Err(Error::InvalidQuery(format!(
            "at most 20 {what} ids per request, got {n}"
        )))
    } else {
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn d(y: i32, m: u32, day: u32) -> NaiveDate {
        NaiveDate::from_ymd_opt(y, m, day).expect("valid date")
    }

    #[test]
    fn single_day_sends_only_start_date() {
        assert_eq!(
            DateRange::day(d(2026, 11, 16)).iso_params(),
            vec![("startDate", "2026-11-16".to_owned())]
        );
    }

    #[test]
    fn range_sends_both_dates_in_the_requested_format() {
        let r = DateRange::new(d(2026, 11, 1), d(2026, 11, 16)).expect("ordered");
        assert_eq!(
            r.compact_params(),
            vec![
                ("startDate", "20261101".to_owned()),
                ("endDate", "20261116".to_owned())
            ]
        );
        assert_eq!(r.days(), 16);
    }

    #[test]
    fn an_end_before_the_start_is_rejected() {
        let err = DateRange::new(d(2026, 11, 17), d(2026, 11, 16)).expect_err("reversed");
        assert!(matches!(err, Error::InvalidQuery(_)));
    }

    #[test]
    fn chunks_cover_the_range_exactly_without_overlap() {
        let r = DateRange::new(d(2026, 1, 1), d(2026, 3, 10)).expect("ordered"); // 69 days
        let chunks = r.chunks(31);
        let spans: Vec<_> = chunks.iter().map(|c| (c.start(), c.end())).collect();
        assert_eq!(
            spans,
            vec![
                (d(2026, 1, 1), d(2026, 1, 31)),
                (d(2026, 2, 1), d(2026, 3, 3)),
                (d(2026, 3, 4), d(2026, 3, 10)),
            ]
        );
        assert_eq!(chunks.iter().map(DateRange::days).sum::<i64>(), r.days());
    }

    #[test]
    fn chunks_of_a_short_or_single_day_range_is_one_chunk() {
        assert_eq!(DateRange::day(d(2026, 5, 5)).chunks(366).len(), 1);
        let r = DateRange::new(d(2026, 1, 1), d(2026, 1, 31)).expect("ordered");
        assert_eq!(r.chunks(31), vec![r]);
        assert_eq!(
            r.chunks(0).len(),
            31,
            "a nonsensical cap degrades to one day"
        );
    }

    #[test]
    fn interchange_query_always_sends_end_date_hours_and_version() {
        let q = InterchangeQuery::new(DateRange::day(d(2026, 11, 16)));
        assert_eq!(
            q.params(),
            vec![
                ("startDate", "20261116".to_owned()),
                ("endDate", "20261116".to_owned()),
                ("startHE", "1".to_owned()),
                ("endHE", "24".to_owned()),
                ("version", "false".to_owned()),
            ]
        );
    }

    #[test]
    fn interchange_filters_render_the_documented_enum_values() {
        let q = InterchangeQuery::new(DateRange::day(d(2026, 11, 16)))
            .hours(1, 3)
            .expect("valid hours")
            .revisions(Revisions::All)
            .data_type(DataType::GrossOffer)
            .intertie(Intertie::BcMatl)
            .transfer(TransferType::Export);
        let p = q.params();
        for want in [
            ("version", "true"),
            ("dataType", "GROSS_OFFER"),
            ("intertieOrFlowgate", "BC_MATL"),
            ("transferType", "EXPORT"),
            ("endHE", "3"),
        ] {
            assert!(
                p.contains(&(want.0, want.1.to_owned())),
                "{want:?} in {p:?}"
            );
        }
    }

    #[test]
    fn bad_hour_ranges_are_rejected() {
        let q = InterchangeQuery::new(DateRange::day(d(2026, 11, 16)));
        assert!(q.clone().hours(0, 5).is_err());
        assert!(q.clone().hours(5, 25).is_err());
        assert!(q.hours(7, 6).is_err());
    }

    #[test]
    fn outage_query_rejects_the_system_filter() {
        let q = OutageQuery::new(DateRange::day(d(2026, 11, 16)));
        assert!(q.intertie(Intertie::System).is_err());
        let ok = q.intertie(Intertie::Sk).expect("SK is valid");
        assert!(ok
            .params()
            .contains(&("affectedIntertieOrFlowgate", "SK".to_owned())));
    }

    #[test]
    fn asset_list_sends_only_the_filters_that_were_set() {
        assert_eq!(AssetListQuery::all().params(), Vec::<(&str, String)>::new());
        let q = AssetListQuery::all()
            .asset_id("AST1")
            .operating_status("Active");
        assert_eq!(
            q.params(),
            vec![
                ("asset_ID", "AST1".to_owned()),
                ("operating_status", "Active".to_owned())
            ]
        );
    }

    #[test]
    fn metered_volume_joins_ids_and_caps_them_at_twenty() {
        let q = MeteredVolumeQuery::new(DateRange::day(d(2026, 11, 1)))
            .assets(["AST1", "AST2"])
            .expect("two ids");
        assert!(q.params().contains(&("asset_ID", "AST1,AST2".to_owned())));
        let many: Vec<String> = (0..21).map(|i| format!("A{i}")).collect();
        assert!(MeteredVolumeQuery::new(DateRange::day(d(2026, 11, 1)))
            .assets(many)
            .is_err());
    }
}

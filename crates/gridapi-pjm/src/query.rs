//! Typed request builders for Data Miner 2 feed queries.

use std::fmt::Display;

use chrono::{Duration, NaiveDate, NaiveDateTime};

use crate::error::Error;

/// Largest `rowCount` PJM accepts per request.
pub const MAX_ROW_COUNT: u32 = 50_000;

/// Longest datetime range PJM accepts for one query, in days.
pub const MAX_RANGE_DAYS: i64 = 366;

/// Query-string parameters the client sets itself; `Query::filter` rejects
/// them so a filter can never shadow paging or projection.
const RESERVED: [&str; 6] = [
    "rowcount", "startrow", "sort", "order", "fields", "download",
];

/// `strftime` pattern of PJM date filters: `MM/DD/YYYY HH:MM`.
const FILTER_FORMAT: &str = "%m/%d/%Y %H:%M";

/// Sort direction.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Order {
    /// Ascending (PJM's default).
    #[default]
    Asc,
    /// Descending.
    Desc,
}

impl Order {
    fn as_param(self) -> &'static str {
        match self {
            Self::Asc => "Asc",
            Self::Desc => "Desc",
        }
    }
}

/// Which versions of versioned rows (LMPs) to return (`row_is_current`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum VersionFilter {
    /// Latest version only (`true`).
    #[default]
    Current,
    /// Superseded versions only (`false`).
    Superseded,
    /// Every version (`all`).
    All,
}

impl VersionFilter {
    fn as_param(self) -> &'static str {
        match self {
            Self::Current => "true",
            Self::Superseded => "false",
            Self::All => "all",
        }
    }
}

/// An inclusive datetime range, formatted the way PJM wants it.
///
/// Times are wall-clock in the field the filter is applied to (EPT for
/// `datetime_beginning_ept`, UTC for `datetime_beginning_utc`). A row whose
/// interval *begins* inside the range matches, so `00:00` to `23:59` is a full
/// day of hourly rows.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct DateRange {
    start: NaiveDateTime,
    end: NaiveDateTime,
}

impl DateRange {
    /// A range from `start` to `end`.
    ///
    /// # Errors
    /// [`Error::InvalidQuery`] when `end < start` or the span exceeds
    /// [`MAX_RANGE_DAYS`].
    pub fn new(start: NaiveDateTime, end: NaiveDateTime) -> Result<Self, Error> {
        if end < start {
            return Err(Error::InvalidQuery(format!(
                "date range ends before it starts ({start} > {end})"
            )));
        }
        if end - start > Duration::days(MAX_RANGE_DAYS) {
            return Err(Error::InvalidQuery(format!(
                "date range exceeds {MAX_RANGE_DAYS} days ({start} to {end})"
            )));
        }
        Ok(Self { start, end })
    }

    /// The whole of one calendar day, `00:00` to `23:59`.
    #[must_use]
    pub fn day(date: NaiveDate) -> Self {
        Self {
            start: date.and_time(chrono::NaiveTime::MIN),
            end: date.and_time(chrono::NaiveTime::MIN) + Duration::minutes(24 * 60 - 1),
        }
    }

    /// Whole calendar days `first..=last`.
    ///
    /// # Errors
    /// As [`DateRange::new`].
    pub fn days(first: NaiveDate, last: NaiveDate) -> Result<Self, Error> {
        Self::new(Self::day(first).start, Self::day(last).end)
    }

    /// The range start.
    #[must_use]
    pub fn start(&self) -> NaiveDateTime {
        self.start
    }

    /// The range end.
    #[must_use]
    pub fn end(&self) -> NaiveDateTime {
        self.end
    }

    fn to_param(self) -> String {
        format!(
            "{}to{}",
            self.start.format(FILTER_FORMAT),
            self.end.format(FILTER_FORMAT)
        )
    }
}

/// PJM's named relative date values.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub enum RelativeDate {
    /// Today.
    Today,
    /// Yesterday.
    Yesterday,
    /// Tomorrow.
    Tomorrow,
    /// The last calendar week.
    LastWeek,
    /// The last calendar month.
    LastMonth,
    /// The current calendar week, including today.
    CurrentWeek,
    /// The current calendar month, including today.
    CurrentMonth,
    /// The current calendar year, including today.
    CurrentYear,
    /// The last calendar year.
    LastYear,
    /// The next calendar week.
    NextWeek,
    /// The next calendar month.
    NextMonth,
    /// The next calendar year.
    NextYear,
    /// The current hour (EPT feeds that update through the day).
    CurrentHour,
    /// The previous hour.
    LastHour,
    /// 15 seconds ago (dispatch-rate feeds, `_ept` columns only).
    FifteenSecondsAgo,
    /// 5 minutes ago (unverified LMP feeds, `_ept` columns only).
    FiveMinutesAgo,
}

impl RelativeDate {
    fn as_param(self) -> &'static str {
        match self {
            Self::Today => "Today",
            Self::Yesterday => "Yesterday",
            Self::Tomorrow => "Tomorrow",
            Self::LastWeek => "LastWeek",
            Self::LastMonth => "LastMonth",
            Self::CurrentWeek => "CurrentWeek",
            Self::CurrentMonth => "CurrentMonth",
            Self::CurrentYear => "CurrentYear",
            Self::LastYear => "LastYear",
            Self::NextWeek => "NextWeek",
            Self::NextMonth => "NextMonth",
            Self::NextYear => "NextYear",
            Self::CurrentHour => "CurrentHour",
            Self::LastHour => "LastHour",
            Self::FifteenSecondsAgo => "15SecondsAgo",
            Self::FiveMinutesAgo => "5MinutesAgo",
        }
    }
}

/// A value for a datetime filter such as `datetime_beginning_ept`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub enum DateFilter {
    /// `MM/DD/YYYY HH:MMtoMM/DD/YYYY HH:MM`.
    Range(DateRange),
    /// Exactly one instant (`MM/DD/YYYY HH:MM exact`).
    Exact(NaiveDateTime),
    /// A named relative period.
    Relative(RelativeDate),
}

impl DateFilter {
    /// The query-string value.
    #[must_use]
    pub fn to_param(&self) -> String {
        match self {
            Self::Range(r) => r.to_param(),
            Self::Exact(t) => format!("{} exact", t.format(FILTER_FORMAT)),
            Self::Relative(r) => r.as_param().to_owned(),
        }
    }
}

impl From<DateRange> for DateFilter {
    fn from(r: DateRange) -> Self {
        Self::Range(r)
    }
}

impl From<RelativeDate> for DateFilter {
    fn from(r: RelativeDate) -> Self {
        Self::Relative(r)
    }
}

impl From<NaiveDate> for DateFilter {
    /// The whole day.
    fn from(d: NaiveDate) -> Self {
        Self::Range(DateRange::day(d))
    }
}

/// A feed query: paging, projection, sort and filters.
///
/// Built by chaining; every method takes and returns `self`. Nothing is sent
/// until a [`Client`](crate::Client) method runs the query, and
/// [`Query::to_params`] validates it first.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Query {
    row_count: u32,
    start_row: u32,
    fields: Vec<String>,
    sort: Option<(String, Order)>,
    filters: Vec<(String, String)>,
}

impl Default for Query {
    fn default() -> Self {
        Self::new()
    }
}

impl Query {
    /// An empty query: first row, [`MAX_ROW_COUNT`] rows per page (the fewest
    /// requests for a given amount of data, which matters under the rate
    /// limit), every field, PJM's default order, no filters.
    #[must_use]
    pub fn new() -> Self {
        Self {
            row_count: MAX_ROW_COUNT,
            start_row: 1,
            fields: Vec::new(),
            sort: None,
            filters: Vec::new(),
        }
    }

    /// Rows per page (`rowCount`, 1 to [`MAX_ROW_COUNT`]).
    #[must_use]
    pub fn row_count(mut self, n: u32) -> Self {
        self.row_count = n;
        self
    }

    /// One-based index of the first row (`startRow`).
    #[must_use]
    pub fn start_row(mut self, n: u32) -> Self {
        self.start_row = n;
        self
    }

    /// Restrict the returned columns (`fields`).
    #[must_use]
    pub fn fields<I, S>(mut self, fields: I) -> Self
    where
        I: IntoIterator<Item = S>,
        S: Into<String>,
    {
        self.fields = fields.into_iter().map(Into::into).collect();
        self
    }

    /// Sort by a column. PJM rejects custom sorting on archived data (older
    /// than 731 days for hourly LMPs, 186 days for five-minute LMPs).
    #[must_use]
    pub fn sort(mut self, field: impl Into<String>, order: Order) -> Self {
        self.sort = Some((field.into(), order));
        self
    }

    /// Set a filter, replacing an earlier value for the same field. The value
    /// is sent as-is; PJM text filters are case-insensitive partial matches
    /// unless the feed documents otherwise.
    #[must_use]
    pub fn filter(mut self, field: impl Into<String>, value: impl Into<String>) -> Self {
        let field = field.into();
        self.filters.retain(|(f, _)| *f != field);
        self.filters.push((field, value.into()));
        self
    }

    /// Set a multi-value filter: PJM takes `a;b;c`.
    #[must_use]
    pub fn filter_any<I>(self, field: impl Into<String>, values: I) -> Self
    where
        I: IntoIterator,
        I::Item: Display,
    {
        let joined = values
            .into_iter()
            .map(|v| v.to_string())
            .collect::<Vec<_>>()
            .join(";");
        self.filter(field, joined)
    }

    /// Filter on `datetime_beginning_ept`.
    #[must_use]
    pub fn datetime_beginning_ept(self, when: impl Into<DateFilter>) -> Self {
        self.filter("datetime_beginning_ept", when.into().to_param())
    }

    /// Filter on `datetime_beginning_utc`.
    #[must_use]
    pub fn datetime_beginning_utc(self, when: impl Into<DateFilter>) -> Self {
        self.filter("datetime_beginning_utc", when.into().to_param())
    }

    /// Filter on one or more `pnode_id`s.
    #[must_use]
    pub fn pnode_ids<I>(self, ids: I) -> Self
    where
        I: IntoIterator<Item = u64>,
    {
        self.filter_any("pnode_id", ids)
    }

    /// Filter on `row_is_current` (LMP version selection).
    #[must_use]
    pub fn version(self, version: VersionFilter) -> Self {
        self.filter("row_is_current", version.as_param())
    }

    /// The `rowCount` currently set.
    #[must_use]
    pub fn page_size(&self) -> u32 {
        self.row_count
    }

    /// The `startRow` currently set.
    #[must_use]
    pub fn first_row(&self) -> u32 {
        self.start_row
    }

    /// Validate and render the query-string parameters, in a stable order.
    ///
    /// # Errors
    /// [`Error::InvalidQuery`] for a `rowCount` outside `1..=50000`, a zero
    /// `startRow`, an empty or reserved filter name, or an empty value.
    pub fn to_params(&self) -> Result<Vec<(String, String)>, Error> {
        if !(1..=MAX_ROW_COUNT).contains(&self.row_count) {
            return Err(Error::InvalidQuery(format!(
                "rowCount must be 1..={MAX_ROW_COUNT}, got {}",
                self.row_count
            )));
        }
        if self.start_row == 0 {
            return Err(Error::InvalidQuery("startRow is one-based".into()));
        }
        let mut params = vec![
            ("rowCount".to_owned(), self.row_count.to_string()),
            ("startRow".to_owned(), self.start_row.to_string()),
        ];
        if let Some((field, order)) = &self.sort {
            check_name(field)?;
            params.push(("sort".to_owned(), field.clone()));
            params.push(("order".to_owned(), order.as_param().to_owned()));
        }
        if !self.fields.is_empty() {
            self.fields.iter().try_for_each(|f| check_name(f))?;
            params.push(("fields".to_owned(), self.fields.join(",")));
        }
        for (field, value) in &self.filters {
            check_name(field)?;
            if RESERVED.contains(&field.to_ascii_lowercase().as_str()) {
                return Err(Error::InvalidQuery(format!(
                    "`{field}` is a paging/projection parameter, not a filter"
                )));
            }
            if value.trim().is_empty() {
                return Err(Error::InvalidQuery(format!("filter `{field}` is empty")));
            }
            params.push((field.clone(), value.clone()));
        }
        Ok(params)
    }
}

/// Field names are ASCII identifiers; anything else is a caller bug that would
/// otherwise surface as an opaque 400.
fn check_name(name: &str) -> Result<(), Error> {
    if !name.is_empty() && name.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'_') {
        Ok(())
    } else {
        Err(Error::InvalidQuery(format!(
            "`{name}` is not a valid field name"
        )))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn dt(s: &str) -> NaiveDateTime {
        NaiveDateTime::parse_from_str(s, "%Y-%m-%d %H:%M").unwrap()
    }

    fn map(q: &Query) -> std::collections::BTreeMap<String, String> {
        q.to_params().unwrap().into_iter().collect()
    }

    #[test]
    fn range_uses_pjm_format_without_spaces_around_to() {
        let r = DateRange::new(dt("2026-11-17 00:00"), dt("2026-11-17 23:59")).unwrap();
        assert_eq!(
            DateFilter::from(r).to_param(),
            "11/17/2026 00:00to11/17/2026 23:59"
        );
    }

    #[test]
    fn day_covers_the_full_day() {
        let d = NaiveDate::from_ymd_opt(2026, 11, 17).unwrap();
        assert_eq!(
            DateFilter::from(d).to_param(),
            "11/17/2026 00:00to11/17/2026 23:59"
        );
    }

    #[test]
    fn range_validation() {
        assert!(DateRange::new(dt("2026-11-18 00:00"), dt("2026-11-17 00:00")).is_err());
        assert!(DateRange::new(dt("2024-01-01 00:00"), dt("2026-01-01 00:00")).is_err());
        assert!(DateRange::new(dt("2026-01-01 00:00"), dt("2026-12-31 00:00")).is_ok());
    }

    #[test]
    fn exact_and_relative_filters() {
        assert_eq!(
            DateFilter::Exact(dt("2026-11-17 05:00")).to_param(),
            "11/17/2026 05:00 exact"
        );
        assert_eq!(
            DateFilter::from(RelativeDate::CurrentHour).to_param(),
            "CurrentHour"
        );
        assert_eq!(
            DateFilter::from(RelativeDate::FiveMinutesAgo).to_param(),
            "5MinutesAgo"
        );
    }

    #[test]
    fn full_query_renders_all_parameters() {
        let q = Query::new()
            .row_count(3)
            .start_row(4)
            .fields(["pnode_id", "total_lmp_da"])
            .sort("datetime_beginning_ept", Order::Desc)
            .pnode_ids([51288, 1])
            .version(VersionFilter::All)
            .datetime_beginning_ept(DateRange::day(
                NaiveDate::from_ymd_opt(2026, 11, 17).unwrap(),
            ));
        let p = map(&q);
        assert_eq!(p["rowCount"], "3");
        assert_eq!(p["startRow"], "4");
        assert_eq!(p["fields"], "pnode_id,total_lmp_da");
        assert_eq!(p["sort"], "datetime_beginning_ept");
        assert_eq!(p["order"], "Desc");
        assert_eq!(p["pnode_id"], "51288;1");
        assert_eq!(p["row_is_current"], "all");
        assert_eq!(
            p["datetime_beginning_ept"],
            "11/17/2026 00:00to11/17/2026 23:59"
        );
    }

    #[test]
    fn filter_replaces_earlier_value() {
        let q = Query::new().filter("zone", "AE").filter("zone", "PSEG");
        assert_eq!(map(&q)["zone"], "PSEG");
        assert_eq!(
            q.to_params()
                .unwrap()
                .iter()
                .filter(|(k, _)| k == "zone")
                .count(),
            1
        );
    }

    #[test]
    fn invalid_queries_are_rejected_locally() {
        assert!(Query::new().row_count(0).to_params().is_err());
        assert!(Query::new()
            .row_count(MAX_ROW_COUNT + 1)
            .to_params()
            .is_err());
        assert!(Query::new().start_row(0).to_params().is_err());
        assert!(Query::new().filter("rowCount", "5").to_params().is_err());
        assert!(Query::new().filter("STARTROW", "5").to_params().is_err());
        assert!(Query::new().filter("a b", "5").to_params().is_err());
        assert!(Query::new().filter("zone", "  ").to_params().is_err());
        assert!(Query::new().fields(["ok", "no;pe"]).to_params().is_err());
        assert!(Query::new().sort("x&y", Order::Asc).to_params().is_err());
    }

    #[test]
    fn defaults_are_maximum_page_from_row_one() {
        let q = Query::default();
        assert_eq!((q.page_size(), q.first_row()), (MAX_ROW_COUNT, 1));
        assert_eq!(q.to_params().unwrap().len(), 2);
    }
}

//! Typed builder for `GET /datasets/{id}/query`.

use std::fmt;

use chrono::{DateTime, NaiveDate, SecondsFormat, Utc};

use crate::error::{Error, Result};

/// A time argument: an RFC 3016.8 instant, a calendar date, or one of the
/// server's magic words (`latest`, `latest_report`, `latest_before:-6 hours`, ...).
///
/// The API accepts any of these as a string; this wrapper only exists so call
/// sites can pass `DateTime<Utc>` or `NaiveDate` directly.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ApiTime(String);

impl ApiTime {
    /// Pass a raw string through unchanged (for `latest`, offsets, naive timestamps).
    pub fn raw(s: impl Into<String>) -> Self {
        Self(s.into())
    }

    /// The wire value.
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl fmt::Display for ApiTime {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

impl From<DateTime<Utc>> for ApiTime {
    fn from(t: DateTime<Utc>) -> Self {
        Self(t.to_rfc3339_opts(SecondsFormat::Secs, true))
    }
}

impl From<NaiveDate> for ApiTime {
    fn from(d: NaiveDate) -> Self {
        Self(d.format("%Y-%m-%d").to_string())
    }
}

impl From<&str> for ApiTime {
    fn from(s: &str) -> Self {
        Self::raw(s)
    }
}

impl From<String> for ApiTime {
    fn from(s: String) -> Self {
        Self(s)
    }
}

/// Sort order of returned rows (by the dataset's time index).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Order {
    /// Oldest first (the server default for queries).
    #[default]
    Asc,
    /// Newest first.
    Desc,
}

impl Order {
    fn as_str(self) -> &'static str {
        match self {
            Self::Asc => "asc",
            Self::Desc => "desc",
        }
    }
}

/// Comparison used with `filter_column` / `filter_value`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum FilterOperator {
    /// `=` (server default).
    #[default]
    Eq,
    /// `!=`
    Ne,
    /// `>`
    Gt,
    /// `>=`
    Ge,
    /// `<`
    Lt,
    /// `<=`
    Le,
    /// `in`, with a [`FilterValue::List`].
    In,
}

impl FilterOperator {
    fn as_str(self) -> &'static str {
        match self {
            Self::Eq => "=",
            Self::Ne => "!=",
            Self::Gt => ">",
            Self::Ge => ">=",
            Self::Lt => "<",
            Self::Le => "<=",
            Self::In => "in",
        }
    }
}

/// Right-hand side of a column filter.
#[derive(Debug, Clone, PartialEq)]
pub enum FilterValue {
    /// A string, e.g. a location name.
    Text(String),
    /// An integer.
    Int(i64),
    /// A float.
    Float(f64),
    /// Several values, sent comma-joined (for [`FilterOperator::In`]).
    List(Vec<String>),
}

impl FilterValue {
    fn to_wire(&self) -> String {
        match self {
            Self::Text(s) => s.clone(),
            Self::Int(i) => i.to_string(),
            Self::Float(f) => f.to_string(),
            Self::List(v) => v.join(","),
        }
    }
}

impl From<&str> for FilterValue {
    fn from(s: &str) -> Self {
        Self::Text(s.to_owned())
    }
}
impl From<String> for FilterValue {
    fn from(s: String) -> Self {
        Self::Text(s)
    }
}
impl From<i64> for FilterValue {
    fn from(i: i64) -> Self {
        Self::Int(i)
    }
}
impl From<f64> for FilterValue {
    fn from(f: f64) -> Self {
        Self::Float(f)
    }
}
impl<S: Into<String>> From<Vec<S>> for FilterValue {
    fn from(v: Vec<S>) -> Self {
        Self::List(v.into_iter().map(Into::into).collect())
    }
}

/// Bucket size for server-side resampling.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ResampleFrequency {
    /// `1 minute` (max 31 days per query).
    Minute1,
    /// `5 minutes` (max 100 days).
    Minutes5,
    /// `10 minutes` (max 120 days).
    Minutes10,
    /// `15 minutes` (max 180 days).
    Minutes15,
    /// `1 hour` (max 365 days).
    Hour1,
    /// `1 day` (max 1825 days).
    Day1,
    /// `1 week`.
    Week1,
    /// `1 month`.
    Month1,
    /// `1 year`.
    Year1,
}

impl ResampleFrequency {
    fn as_str(self) -> &'static str {
        match self {
            Self::Minute1 => "1 minute",
            Self::Minutes5 => "5 minutes",
            Self::Minutes10 => "10 minutes",
            Self::Minutes15 => "15 minutes",
            Self::Hour1 => "1 hour",
            Self::Day1 => "1 day",
            Self::Week1 => "1 week",
            Self::Month1 => "1 month",
            Self::Year1 => "1 year",
        }
    }
}

/// Aggregation applied per resample bucket.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum ResampleFunction {
    /// Mean (server default).
    #[default]
    Mean,
    /// Sum.
    Sum,
    /// Minimum.
    Min,
    /// Maximum.
    Max,
    /// Row count.
    Count,
    /// Standard deviation.
    Stddev,
    /// Variance.
    Variance,
}

impl ResampleFunction {
    fn as_str(self) -> &'static str {
        match self {
            Self::Mean => "mean",
            Self::Sum => "sum",
            Self::Min => "min",
            Self::Max => "max",
            Self::Count => "count",
            Self::Stddev => "stddev",
            Self::Variance => "variance",
        }
    }
}

/// Server-side resampling request.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Resample {
    /// Bucket size.
    pub frequency: ResampleFrequency,
    /// Aggregation; `None` uses the server default (mean).
    pub function: Option<ResampleFunction>,
    /// Extra group-by columns (in addition to the time index).
    pub by: Vec<String>,
}

impl Resample {
    /// Resample to `frequency` with the server-default aggregation.
    pub fn new(frequency: ResampleFrequency) -> Self {
        Self {
            frequency,
            function: None,
            by: Vec::new(),
        }
    }

    /// Choose the aggregation function.
    #[must_use]
    pub fn function(mut self, function: ResampleFunction) -> Self {
        self.function = Some(function);
        self
    }

    /// Group by these columns before resampling.
    #[must_use]
    pub fn by<I, S>(mut self, columns: I) -> Self
    where
        I: IntoIterator<Item = S>,
        S: Into<String>,
    {
        self.by = columns.into_iter().map(Into::into).collect();
        self
    }
}

/// Parameters of a dataset query. Build with the chained setters; every field
/// is optional, matching the API.
///
/// ```
/// use gridapi_gridstatus::{Order, Query};
/// let q = Query::new()
///     .start_time("2026-11-17")
///     .end_time("2026-11-18")
///     .location("HB_HOUSTON")
///     .order(Order::Desc)
///     .limit(48);
/// assert!(q.validate().is_ok());
/// ```
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Query {
    start_time: Option<ApiTime>,
    end_time: Option<ApiTime>,
    time: Option<ApiTime>,
    time_comparison: Option<String>,
    publish_time: Option<ApiTime>,
    publish_time_start: Option<ApiTime>,
    publish_time_end: Option<ApiTime>,
    filter_column: Option<String>,
    filter_value: Option<FilterValue>,
    filter_operator: Option<FilterOperator>,
    columns: Vec<String>,
    resample: Option<Resample>,
    timezone: Option<String>,
    order: Option<Order>,
    limit: Option<u64>,
    page: Option<u64>,
    page_size: Option<u64>,
    cursor: Option<String>,
}

impl Query {
    /// An empty query (server defaults: all columns, first page).
    pub fn new() -> Self {
        Self::default()
    }

    /// Rows with time index `>= t`.
    #[must_use]
    pub fn start_time(mut self, t: impl Into<ApiTime>) -> Self {
        self.start_time = Some(t.into());
        self
    }

    /// Rows with time index `< t`.
    #[must_use]
    pub fn end_time(mut self, t: impl Into<ApiTime>) -> Self {
        self.end_time = Some(t.into());
        self
    }

    /// `time=latest` or an exact timestamp. Mutually exclusive with start/end.
    #[must_use]
    pub fn time(mut self, t: impl Into<ApiTime>) -> Self {
        self.time = Some(t.into());
        self
    }

    /// Operator for `time`: one of `=`, `>`, `>=`, `<`, `<=`.
    /// **Unverified, behind the `unverified` feature**: no live request has used it (not run live
    /// inside the 15-request budget).
    #[cfg(feature = "unverified")]
    #[must_use]
    pub fn time_comparison(mut self, op: impl Into<String>) -> Self {
        self.time_comparison = Some(op.into());
        self
    }

    /// Publish-time selector (`latest_report`, `latest`, a timestamp, `latest_before:...`).
    /// **Unverified, behind the `unverified` feature**: no live request has used it (the
    /// datasets the 15-request live budget reached have no publish-time column).
    #[cfg(feature = "unverified")]
    #[must_use]
    pub fn publish_time(mut self, t: impl Into<ApiTime>) -> Self {
        self.publish_time = Some(t.into());
        self
    }

    /// Lower bound on the publish time column.
    /// **Unverified, behind the `unverified` feature**: no live request has used it (the
    /// datasets the 15-request live budget reached have no publish-time column).
    #[cfg(feature = "unverified")]
    #[must_use]
    pub fn publish_time_start(mut self, t: impl Into<ApiTime>) -> Self {
        self.publish_time_start = Some(t.into());
        self
    }

    /// Upper bound on the publish time column.
    /// **Unverified, behind the `unverified` feature**: no live request has used it (the
    /// datasets the 15-request live budget reached have no publish-time column).
    #[cfg(feature = "unverified")]
    #[must_use]
    pub fn publish_time_end(mut self, t: impl Into<ApiTime>) -> Self {
        self.publish_time_end = Some(t.into());
        self
    }

    /// Filter rows with `column <op> value` (operator defaults to `=`).
    #[must_use]
    pub fn filter(mut self, column: impl Into<String>, value: impl Into<FilterValue>) -> Self {
        self.filter_column = Some(column.into());
        self.filter_value = Some(value.into());
        self
    }

    /// Operator for [`Query::filter`].
    #[must_use]
    pub fn filter_operator(mut self, op: FilterOperator) -> Self {
        self.filter_operator = Some(op);
        self
    }

    /// Shorthand for `filter("location", name)`, the subseries column of the
    /// price datasets.
    #[must_use]
    pub fn location(self, name: impl Into<String>) -> Self {
        self.filter("location", name.into())
    }

    /// Return only these columns.
    #[must_use]
    pub fn columns<I, S>(mut self, columns: I) -> Self
    where
        I: IntoIterator<Item = S>,
        S: Into<String>,
    {
        self.columns = columns.into_iter().map(Into::into).collect();
        self
    }

    /// Resample server-side. Requires `start_time` and `end_time`.
    #[must_use]
    pub fn resample(mut self, resample: Resample) -> Self {
        self.resample = Some(resample);
        self
    }

    /// IANA zone, or `market` for the ISO's own zone. Adds `*_local` columns.
    #[must_use]
    pub fn timezone(mut self, tz: impl Into<String>) -> Self {
        self.timezone = Some(tz.into());
        self
    }

    /// Sort order.
    #[must_use]
    pub fn order(mut self, order: Order) -> Self {
        self.order = Some(order);
        self
    }

    /// Maximum rows across all pages.
    #[must_use]
    pub fn limit(mut self, n: u64) -> Self {
        self.limit = Some(n);
        self
    }

    /// 1-based page number.
    #[must_use]
    pub fn page(mut self, n: u64) -> Self {
        self.page = Some(n);
        self
    }

    /// Rows per page (the free tier caps a response at 50 000 rows).
    #[must_use]
    pub fn page_size(mut self, n: u64) -> Self {
        self.page_size = Some(n);
        self
    }

    /// Cursor returned in the previous page's `meta.cursor`.
    #[must_use]
    pub fn cursor(mut self, cursor: impl Into<String>) -> Self {
        self.cursor = Some(cursor.into());
        self
    }

    /// Check combinations the API rejects, before spending a request on them.
    pub fn validate(&self) -> Result<()> {
        let bad = |msg: &str| Err(Error::InvalidRequest(msg.to_owned()));
        if self.time.is_some() && (self.start_time.is_some() || self.end_time.is_some()) {
            return bad("`time` cannot be combined with start_time or end_time");
        }
        if self.publish_time.is_some()
            && (self.publish_time_start.is_some() || self.publish_time_end.is_some())
        {
            return bad("`publish_time` cannot be combined with publish_time_start/end");
        }
        if self.filter_column.is_some() != self.filter_value.is_some() {
            return bad("filter column and value must be set together");
        }
        if self.filter_operator.is_some() && self.filter_column.is_none() {
            return bad("filter_operator without a filter");
        }
        if matches!(self.filter_operator, Some(FilterOperator::In))
            && !matches!(self.filter_value, Some(FilterValue::List(_)))
        {
            return bad("operator `in` needs FilterValue::List");
        }
        if self.resample.is_some() {
            let times_ok = self.start_time.is_some() && self.end_time.is_some();
            let publish_ok = self.publish_time.is_some();
            if !times_ok && !publish_ok {
                return bad("resampling requires both start_time and end_time");
            }
        }
        if self.limit == Some(0) || self.page_size == Some(0) || self.page == Some(0) {
            return bad("limit, page and page_size must be at least 1");
        }
        Ok(())
    }

    /// The row limit, if set.
    pub fn row_limit(&self) -> Option<u64> {
        self.limit
    }

    /// The cursor, if set.
    pub fn cursor_value(&self) -> Option<&str> {
        self.cursor.as_deref()
    }

    /// A copy positioned at the page after `cursor`.
    #[must_use]
    pub(crate) fn with_next(&self, cursor: Option<String>, next_page: u64) -> Self {
        let mut q = self.clone();
        match cursor {
            Some(c) => {
                q.cursor = Some(c);
                q.page = None;
            }
            None => q.page = Some(next_page),
        }
        q
    }

    /// The page number requested (1 when unset).
    pub(crate) fn page_number(&self) -> u64 {
        self.page.unwrap_or(1)
    }

    /// Serialise to `(name, value)` query-string pairs, in a stable order.
    pub fn to_params(&self) -> Vec<(&'static str, String)> {
        let time =
            |name: &'static str, v: &Option<ApiTime>| v.as_ref().map(|t| (name, t.to_string()));
        let res = self.resample.as_ref();
        [
            time("start_time", &self.start_time),
            time("end_time", &self.end_time),
            time("time", &self.time),
            self.time_comparison.clone().map(|c| ("time_comparison", c)),
            time("publish_time", &self.publish_time),
            time("publish_time_start", &self.publish_time_start),
            time("publish_time_end", &self.publish_time_end),
            self.filter_column.clone().map(|c| ("filter_column", c)),
            self.filter_value
                .as_ref()
                .map(|v| ("filter_value", v.to_wire())),
            self.filter_operator
                .map(|o| ("filter_operator", o.as_str().to_owned())),
            (!self.columns.is_empty()).then(|| ("columns", self.columns.join(","))),
            res.map(|r| ("resample_frequency", r.frequency.as_str().to_owned())),
            res.and_then(|r| r.function)
                .map(|f| ("resample_function", f.as_str().to_owned())),
            res.filter(|r| !r.by.is_empty())
                .map(|r| ("resample_by", r.by.join(","))),
            self.timezone.clone().map(|z| ("timezone", z)),
            self.order.map(|o| ("order", o.as_str().to_owned())),
            self.limit.map(|n| ("limit", n.to_string())),
            self.page.map(|n| ("page", n.to_string())),
            self.page_size.map(|n| ("page_size", n.to_string())),
            self.cursor.clone().map(|c| ("cursor", c)),
        ]
        .into_iter()
        .flatten()
        .collect()
    }
}

#[cfg(test)]
mod tests {
    use chrono::TimeZone;

    use super::*;

    fn p(q: &Query) -> Vec<(&'static str, String)> {
        q.to_params()
    }

    #[test]
    fn empty_query_has_no_params() {
        assert_eq!(p(&Query::new()).len(), 0);
    }

    #[test]
    fn datetime_and_date_serialise_as_api_expects() {
        let t = Utc
            .with_ymd_and_hms(2026, 11, 17, 0, 0, 0)
            .single()
            .expect("valid");
        let q = Query::new()
            .start_time(t)
            .end_time(NaiveDate::from_ymd_opt(2026, 11, 18).expect("valid"));
        assert_eq!(
            p(&q),
            vec![
                ("start_time", "2026-11-17T00:00:00Z".to_owned()),
                ("end_time", "2026-11-18".to_owned()),
            ]
        );
    }

    #[test]
    fn full_query_serialises_every_field() {
        let q = Query::new()
            .start_time("2026-11-17")
            .end_time("2026-11-18")
            .filter("location", "HB_HOUSTON")
            .filter_operator(FilterOperator::Ne)
            .columns(["interval_start_utc", "spp"])
            .resample(
                Resample::new(ResampleFrequency::Hour1)
                    .function(ResampleFunction::Max)
                    .by(["location"]),
            )
            .timezone("market")
            .order(Order::Desc)
            .limit(10)
            .page(2)
            .page_size(5)
            .cursor("abc");
        let got: std::collections::BTreeMap<_, _> = p(&q).into_iter().collect();
        assert_eq!(got["filter_operator"], "!=");
        assert_eq!(got["columns"], "interval_start_utc,spp");
        assert_eq!(got["resample_frequency"], "1 hour");
        assert_eq!(got["resample_function"], "max");
        assert_eq!(got["resample_by"], "location");
        assert_eq!(got["timezone"], "market");
        assert_eq!(got["order"], "desc");
        assert_eq!(got["limit"], "10");
        assert_eq!(got["page"], "2");
        assert_eq!(got["page_size"], "5");
        assert_eq!(got["cursor"], "abc");
        assert!(q.validate().is_ok());
    }

    #[test]
    fn in_operator_joins_list() {
        let q = Query::new()
            .filter("location", vec!["HB_HOUSTON", "HB_NORTH"])
            .filter_operator(FilterOperator::In);
        assert!(q.validate().is_ok());
        let got: std::collections::BTreeMap<_, _> = p(&q).into_iter().collect();
        assert_eq!(got["filter_value"], "HB_HOUSTON,HB_NORTH");
    }

    #[test]
    fn numeric_filter_values_render_plainly() {
        let q = Query::new().filter("location_id", 98_369_975_i64);
        let got: std::collections::BTreeMap<_, _> = p(&q).into_iter().collect();
        assert_eq!(got["filter_value"], "98369975");
    }

    #[test]
    fn rejects_combinations_the_api_rejects() {
        #[allow(unused_mut)]
        let mut cases = vec![
            Query::new().time("latest").start_time("2026-01-01"),
            Query::new().resample(Resample::new(ResampleFrequency::Day1)),
            Query::new()
                .start_time("2026-01-01")
                .resample(Resample::new(ResampleFrequency::Day1)),
            Query::new().limit(0),
            Query::new().page_size(0),
            Query::new().filter_operator(FilterOperator::Gt),
            Query::new()
                .filter("location", "X")
                .filter_operator(FilterOperator::In),
        ];
        #[cfg(feature = "unverified")]
        cases.push(
            Query::new()
                .publish_time("latest")
                .publish_time_end("2026-01-01"),
        );
        for q in cases {
            assert!(
                matches!(q.validate(), Err(Error::InvalidRequest(_))),
                "should reject {q:?}"
            );
        }
    }

    #[test]
    fn resample_with_both_times_is_valid() {
        let q = Query::new()
            .start_time("2026-01-01")
            .end_time("2026-01-08")
            .resample(Resample::new(ResampleFrequency::Day1));
        assert!(q.validate().is_ok());
    }

    #[test]
    fn next_page_prefers_cursor() {
        let q = Query::new().page(1).limit(5);
        let by_cursor = q.with_next(Some("c1".into()), 2);
        assert_eq!(by_cursor.cursor_value(), Some("c1"));
        assert_eq!(by_cursor.page_number(), 1);
        let by_page = q.with_next(None, 2);
        assert_eq!(by_page.page_number(), 2);
    }
}

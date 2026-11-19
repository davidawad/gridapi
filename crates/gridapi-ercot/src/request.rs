//! Report query builders: the generic [`ReportRequest`] and the typed [`Query`].

use std::marker::PhantomData;

use chrono::{NaiveDate, NaiveDateTime};

use crate::page::FromRow;

/// Sort direction for [`ReportRequest::sort`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SortDir {
    /// Ascending (`asc`).
    Asc,
    /// Descending (`desc`).
    Desc,
}

impl SortDir {
    fn as_param(self) -> &'static str {
        match self {
            SortDir::Asc => "asc",
            SortDir::Desc => "desc",
        }
    }
}

/// Formats a date the way ERCOT filters expect (`yyyy-MM-dd`).
#[must_use]
pub fn fmt_date(date: NaiveDate) -> String {
    date.format("%Y-%m-%d").to_string()
}

/// Formats a timestamp the way ERCOT filters expect (`yyyy-MM-ddTHH:mm:ss`).
#[must_use]
pub fn fmt_datetime(at: NaiveDateTime) -> String {
    at.format("%Y-%m-%dT%H:%M:%S").to_string()
}

/// A query against any report: product id, artifact name and filter parameters.
///
/// Parameters are passed through verbatim, so every filter the API Explorer
/// shows for a report can be used even when this crate has no typed helper for it.
///
/// ```
/// use gridapi_ercot::{ReportRequest, SortDir};
///
/// let req = ReportRequest::new("NP6-905-CD", "spp_node_zone_hub")
///     .filter("settlementPoint", "HB_NORTH")
///     .range("deliveryDate", "2026-11-01", "2026-11-07")
///     .size(500)
///     .sort("deliveryDate", SortDir::Desc);
/// assert_eq!(req.path(), "np6-905-cd/spp_node_zone_hub");
/// ```
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ReportRequest {
    emil_id: String,
    artifact: String,
    filters: Vec<(String, String)>,
    page: Option<u32>,
    size: Option<u32>,
    sort: Option<Sort>,
}

/// The API sorts either by one field in a chosen direction, or by several fields ascending.
#[derive(Debug, Clone, PartialEq, Eq)]
enum Sort {
    Single(String, SortDir),
    Ascending(Vec<String>),
}

impl ReportRequest {
    /// Targets artifact `artifact` of product `emil_id` (case-insensitive).
    #[must_use]
    pub fn new(emil_id: impl Into<String>, artifact: impl Into<String>) -> Self {
        Self {
            emil_id: emil_id.into(),
            artifact: artifact.into(),
            filters: Vec::new(),
            page: None,
            size: None,
            sort: None,
        }
    }

    /// Adds `name=value`, replacing an earlier value for the same name.
    #[must_use]
    pub fn filter(mut self, name: impl Into<String>, value: impl ToString) -> Self {
        let name = name.into();
        self.filters.retain(|(n, _)| *n != name);
        self.filters.push((name, value.to_string()));
        self
    }

    /// Adds `{name}From=from` and `{name}To=to`.
    #[must_use]
    pub fn range(self, name: &str, from: impl ToString, to: impl ToString) -> Self {
        self.filter(format!("{name}From"), from)
            .filter(format!("{name}To"), to)
    }

    /// Requests 1-based page `page`.
    #[must_use]
    pub fn page(mut self, page: u32) -> Self {
        self.page = Some(page);
        self
    }

    /// Requests `size` rows per page (the server accepts at least 100 000).
    #[must_use]
    pub fn size(mut self, size: u32) -> Self {
        self.size = Some(size);
        self
    }

    /// Sorts by one `field` in `dir`.
    #[must_use]
    pub fn sort(mut self, field: impl Into<String>, dir: SortDir) -> Self {
        self.sort = Some(Sort::Single(field.into(), dir));
        self
    }

    /// Sorts ascending by several fields, in priority order.
    ///
    /// The API allows a sort direction only when sorting by a single field, so
    /// multi-field sorts are always ascending; use [`ReportRequest::sort`] for descending.
    #[must_use]
    pub fn sort_ascending_by<I, S>(mut self, fields: I) -> Self
    where
        I: IntoIterator<Item = S>,
        S: Into<String>,
    {
        self.sort = Some(Sort::Ascending(
            fields.into_iter().map(Into::into).collect(),
        ));
        self
    }

    /// Lower-case `emil/artifact` URL path relative to the API base.
    #[must_use]
    pub fn path(&self) -> String {
        format!("{}/{}", self.emil_id.to_ascii_lowercase(), self.artifact)
    }

    /// The configured page size, if any.
    #[must_use]
    pub fn page_size(&self) -> Option<u32> {
        self.size
    }

    /// Every query-string pair, filters first.
    #[must_use]
    pub fn params(&self) -> Vec<(String, String)> {
        let mut out = self.filters.clone();
        out.extend(self.page.map(|p| ("page".to_owned(), p.to_string())));
        out.extend(self.size.map(|s| ("size".to_owned(), s.to_string())));
        match &self.sort {
            Some(Sort::Single(field, dir)) => {
                out.push(("sort".to_owned(), field.clone()));
                out.push(("dir".to_owned(), dir.as_param().to_owned()));
            }
            Some(Sort::Ascending(fields)) if !fields.is_empty() => {
                out.push(("sort".to_owned(), fields.join(",")));
            }
            Some(Sort::Ascending(_)) | None => {}
        }
        out
    }
}

/// A report this crate knows how to type.
pub trait Report {
    /// Row type produced by the report.
    type Record: FromRow;
    /// EMIL product id, e.g. `NP6-905-CD`.
    const EMIL_ID: &'static str;
    /// Artifact (data endpoint) name, e.g. `spp_node_zone_hub`.
    const ARTIFACT: &'static str;
}

/// A typed query for report `R`.
///
/// Shared setters live here; each report adds its own filter methods in its module.
pub struct Query<R: Report> {
    request: ReportRequest,
    _report: PhantomData<fn() -> R>,
}

impl<R: Report> Query<R> {
    /// An unfiltered query for `R`.
    #[must_use]
    pub fn new() -> Self {
        Self {
            request: ReportRequest::new(R::EMIL_ID, R::ARTIFACT),
            _report: PhantomData,
        }
    }

    /// Adds an arbitrary `name=value` filter (see [`ReportRequest::filter`]).
    #[must_use]
    pub fn filter(self, name: impl Into<String>, value: impl ToString) -> Self {
        self.map(|r| r.filter(name, value))
    }

    /// Requests 1-based page `page`.
    #[must_use]
    pub fn page(self, page: u32) -> Self {
        self.map(|r| r.page(page))
    }

    /// Requests `size` rows per page.
    #[must_use]
    pub fn size(self, size: u32) -> Self {
        self.map(|r| r.size(size))
    }

    /// Sorts by one `field` in `dir`.
    #[must_use]
    pub fn sort(self, field: impl Into<String>, dir: SortDir) -> Self {
        self.map(|r| r.sort(field, dir))
    }

    /// Sorts ascending by several fields (see [`ReportRequest::sort_ascending_by`]).
    #[must_use]
    pub fn sort_ascending_by<I, S>(self, fields: I) -> Self
    where
        I: IntoIterator<Item = S>,
        S: Into<String>,
    {
        self.map(|r| r.sort_ascending_by(fields))
    }

    /// The untyped request this query will send.
    #[must_use]
    pub fn request(&self) -> &ReportRequest {
        &self.request
    }

    pub(crate) fn map(self, f: impl FnOnce(ReportRequest) -> ReportRequest) -> Self {
        Self {
            request: f(self.request),
            _report: PhantomData,
        }
    }

    pub(crate) fn date_range(self, name: &str, from: NaiveDate, to: NaiveDate) -> Self {
        self.map(|r| r.range(name, fmt_date(from), fmt_date(to)))
    }

    pub(crate) fn datetime_range(self, name: &str, from: NaiveDateTime, to: NaiveDateTime) -> Self {
        self.map(|r| r.range(name, fmt_datetime(from), fmt_datetime(to)))
    }
}

impl<R: Report> Default for Query<R> {
    fn default() -> Self {
        Self::new()
    }
}

impl<R: Report> Clone for Query<R> {
    fn clone(&self) -> Self {
        Self {
            request: self.request.clone(),
            _report: PhantomData,
        }
    }
}

impl<R: Report> std::fmt::Debug for Query<R> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Query")
            .field("emil_id", &R::EMIL_ID)
            .field("request", &self.request)
            .finish()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn path_is_lowercase_emil_then_artifact() {
        assert_eq!(
            ReportRequest::new("NP4-190-CD", "dam_stlmnt_pnt_prices").path(),
            "np4-190-cd/dam_stlmnt_pnt_prices"
        );
    }

    #[test]
    fn filter_replaces_and_range_expands() {
        let r = ReportRequest::new("A", "b")
            .filter("settlementPoint", "HB_NORTH")
            .filter("settlementPoint", "HB_WEST")
            .range("deliveryDate", "2026-11-01", "2026-11-02");
        assert_eq!(
            r.params(),
            [
                ("settlementPoint".to_owned(), "HB_WEST".to_owned()),
                ("deliveryDateFrom".to_owned(), "2026-11-01".to_owned()),
                ("deliveryDateTo".to_owned(), "2026-11-02".to_owned()),
            ]
        );
    }

    #[test]
    fn paging_and_sort_are_appended_after_filters() {
        let r = ReportRequest::new("A", "b")
            .filter("x", 1)
            .page(2)
            .size(50)
            .sort("x", SortDir::Desc);
        let names: Vec<_> = r.params().into_iter().map(|(n, _)| n).collect();
        assert_eq!(names, ["x", "page", "size", "sort", "dir"]);
    }

    #[test]
    fn multi_field_sort_has_no_direction_parameter() {
        let r = ReportRequest::new("A", "b").sort_ascending_by(["deliveryDate", "deliveryHour"]);
        assert_eq!(
            r.params(),
            [("sort".to_owned(), "deliveryDate,deliveryHour".to_owned())]
        );
        assert!(ReportRequest::new("A", "b")
            .sort_ascending_by(Vec::<String>::new())
            .params()
            .is_empty());
    }

    #[test]
    fn date_formats_match_the_api() {
        let d = NaiveDate::from_ymd_opt(2026, 1, 5).expect("valid date");
        assert_eq!(fmt_date(d), "2026-01-05");
        let t = d.and_hms_opt(3, 4, 5).expect("valid time");
        assert_eq!(fmt_datetime(t), "2026-01-05T03:04:05");
    }
}

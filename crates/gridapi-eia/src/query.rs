//! Typed request builder for `GET /v2/<route>/data`.

use crate::error::{Error, Result};

/// EIA returns at most this many JSON rows per request.
pub const MAX_PAGE_LEN: u32 = 5000;

/// Time granularity of a data query (the `frequency` parameter).
///
/// Which values a route accepts is listed in its metadata
/// ([`crate::Client::route`]); sending an unsupported one is a 400.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Frequency {
    /// `hourly` (UTC hours, EIA-930).
    Hourly,
    /// `local-hourly` (local-time hours, EIA-930).
    LocalHourly,
    /// `daily`.
    Daily,
    /// `weekly`.
    Weekly,
    /// `monthly`.
    Monthly,
    /// `quarterly`.
    Quarterly,
    /// `annual`.
    Annual,
}

impl Frequency {
    /// The wire value EIA expects.
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Hourly => "hourly",
            Self::LocalHourly => "local-hourly",
            Self::Daily => "daily",
            Self::Weekly => "weekly",
            Self::Monthly => "monthly",
            Self::Quarterly => "quarterly",
            Self::Annual => "annual",
        }
    }
}

/// Sort direction for a `sort` column.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Direction {
    /// Ascending.
    Asc,
    /// Descending.
    Desc,
}

impl Direction {
    const fn as_str(self) -> &'static str {
        match self {
            Self::Asc => "asc",
            Self::Desc => "desc",
        }
    }
}

/// A data query for one route. Build with [`DataQuery::new`] and chain.
///
/// ```
/// use gridapi_eia::{DataQuery, Direction, Frequency};
///
/// let q = DataQuery::new("electricity/rto/region-data")
///     .frequency(Frequency::Hourly)
///     .data_column("value")
///     .facet("respondent", ["ERCO"])
///     .facet("type", ["D"])
///     .start("2026-01-01T00")
///     .sort("period", Direction::Desc)
///     .length(24);
/// assert!(q.params().contains(&("facets[respondent][]".into(), "ERCO".into())));
/// ```
#[derive(Debug, Clone, PartialEq, Eq)]
#[must_use]
pub struct DataQuery {
    route: String,
    frequency: Option<String>,
    data: Vec<String>,
    facets: Vec<(String, Vec<String>)>,
    start: Option<String>,
    end: Option<String>,
    sort: Vec<(String, Direction)>,
    offset: Option<u64>,
    length: Option<u32>,
    auto_sort: bool,
}

impl DataQuery {
    /// New query for `route`, for example `electricity/rto/region-data`.
    /// Leading/trailing slashes and a trailing `/data` are accepted and
    /// normalised away.
    pub fn new(route: impl AsRef<str>) -> Self {
        Self {
            route: normalise_route(route.as_ref()),
            frequency: None,
            data: Vec::new(),
            facets: Vec::new(),
            start: None,
            end: None,
            sort: Vec::new(),
            offset: None,
            length: None,
            auto_sort: true,
        }
    }

    /// Set the aggregation frequency.
    pub fn frequency(mut self, f: Frequency) -> Self {
        self.frequency = Some(f.as_str().to_owned());
        self
    }

    /// Set the frequency by raw wire value, for routes with values the
    /// [`Frequency`] enum does not cover.
    pub fn frequency_raw(mut self, f: impl Into<String>) -> Self {
        self.frequency = Some(f.into());
        self
    }

    /// Request one data column (`data[n]=<column>`), for example `value`.
    pub fn data_column(mut self, column: impl Into<String>) -> Self {
        self.data.push(column.into());
        self
    }

    /// Request several data columns.
    pub fn data_columns<I, S>(mut self, columns: I) -> Self
    where
        I: IntoIterator<Item = S>,
        S: Into<String>,
    {
        self.data.extend(columns.into_iter().map(Into::into));
        self
    }

    /// Filter on a facet (`facets[<name>][]=<v>` repeated). Calling again for
    /// the same facet replaces its values.
    pub fn facet<I, S>(mut self, name: impl Into<String>, values: I) -> Self
    where
        I: IntoIterator<Item = S>,
        S: Into<String>,
    {
        let name = name.into();
        let values: Vec<String> = values.into_iter().map(Into::into).collect();
        self.facets.retain(|(n, _)| *n != name);
        self.facets.push((name, values));
        self
    }

    /// Inclusive lower period bound, in the route's period format
    /// (`2026-01`, `2026-01-01`, `2026-01-01T00`).
    pub fn start(mut self, period: impl Into<String>) -> Self {
        self.start = Some(period.into());
        self
    }

    /// Inclusive upper period bound.
    pub fn end(mut self, period: impl Into<String>) -> Self {
        self.end = Some(period.into());
        self
    }

    /// Append a sort key. Earlier calls take precedence.
    pub fn sort(mut self, column: impl Into<String>, direction: Direction) -> Self {
        self.sort.push((column.into(), direction));
        self
    }

    /// Row offset of the first row to fetch. For a paged fetch this is where
    /// paging starts.
    pub fn offset(mut self, offset: u64) -> Self {
        self.offset = Some(offset);
        self
    }

    /// Rows per request, 1..=5000. For a paged fetch this is the page size.
    pub fn length(mut self, length: u32) -> Self {
        self.length = Some(length);
        self
    }

    /// Do not add the default `sort[0]=period asc` that paged fetches use for
    /// a stable row order when the query has no sort of its own. Needed only
    /// for routes without a `period` column.
    pub fn without_auto_sort(mut self) -> Self {
        self.auto_sort = false;
        self
    }

    /// Normalised route path, no slashes at either end.
    #[must_use]
    pub fn route(&self) -> &str {
        &self.route
    }

    /// Page size a paged fetch will use.
    #[must_use]
    pub fn page_len(&self) -> u32 {
        self.length.unwrap_or(MAX_PAGE_LEN)
    }

    /// Row offset a paged fetch starts at.
    #[must_use]
    pub fn start_offset(&self) -> u64 {
        self.offset.unwrap_or(0)
    }

    /// Check local invariants (route shape, page length bounds).
    pub fn validate(&self) -> Result<()> {
        if self.route.is_empty() {
            return Err(Error::InvalidRequest("route is empty".into()));
        }
        if self
            .route
            .split('/')
            .any(|s| s.is_empty() || s == "." || s == "..")
        {
            return Err(Error::InvalidRequest(format!(
                "route {:?} has an empty or relative segment",
                self.route
            )));
        }
        if let Some(n) = self.length {
            if !(1..=MAX_PAGE_LEN).contains(&n) {
                return Err(Error::InvalidRequest(format!(
                    "length must be 1..={MAX_PAGE_LEN}, got {n}"
                )));
            }
        }
        Ok(())
    }

    /// The query-string pairs EIA expects, without the api key. Pure.
    #[must_use]
    pub fn params(&self) -> Vec<(String, String)> {
        let scalar = |k: &str, v: &Option<String>| v.as_ref().map(|v| (k.to_owned(), v.clone()));
        let data = self
            .data
            .iter()
            .enumerate()
            .map(|(i, c)| (format!("data[{i}]"), c.clone()));
        let facets = self.facets.iter().flat_map(|(name, values)| {
            values
                .iter()
                .map(move |v| (format!("facets[{name}][]"), v.clone()))
        });
        let sort = self.sort.iter().enumerate().flat_map(|(i, (col, dir))| {
            [
                (format!("sort[{i}][column]"), col.clone()),
                (format!("sort[{i}][direction]"), dir.as_str().to_owned()),
            ]
        });
        scalar("frequency", &self.frequency)
            .into_iter()
            .chain(data)
            .chain(facets)
            .chain(scalar("start", &self.start))
            .chain(scalar("end", &self.end))
            .chain(sort)
            .chain(self.offset.map(|o| ("offset".to_owned(), o.to_string())))
            .chain(self.length.map(|l| ("length".to_owned(), l.to_string())))
            .collect()
    }

    /// The query for the page starting at `offset`, with the page size set
    /// and, unless disabled, a deterministic default sort.
    pub(crate) fn page_at(&self, offset: u64) -> Self {
        let mut q = self.clone();
        q.offset = Some(offset);
        q.length = Some(self.page_len());
        if q.sort.is_empty() && q.auto_sort {
            q.sort.push(("period".into(), Direction::Asc));
        }
        q
    }
}

fn normalise_route(route: &str) -> String {
    let r = route.trim().trim_matches('/');
    r.strip_suffix("/data").unwrap_or(r).to_owned()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn pairs(q: &DataQuery) -> Vec<String> {
        q.params()
            .into_iter()
            .map(|(k, v)| format!("{k}={v}"))
            .collect()
    }

    #[test]
    fn route_is_normalised() {
        for raw in [
            "electricity/rto/region-data",
            "/electricity/rto/region-data/",
            "electricity/rto/region-data/data",
            " /electricity/rto/region-data/data/ ",
        ] {
            assert_eq!(DataQuery::new(raw).route(), "electricity/rto/region-data");
        }
    }

    #[test]
    fn full_query_serialises_in_eia_bracket_syntax() {
        let q = DataQuery::new("electricity/rto/region-data")
            .frequency(Frequency::Hourly)
            .data_column("value")
            .facet("respondent", ["ERCO", "PJM"])
            .facet("type", ["D"])
            .start("2026-01-01T00")
            .end("2026-01-02T00")
            .sort("period", Direction::Desc)
            .sort("respondent", Direction::Asc)
            .offset(5000)
            .length(100);
        assert_eq!(
            pairs(&q),
            [
                "frequency=hourly",
                "data[0]=value",
                "facets[respondent][]=ERCO",
                "facets[respondent][]=PJM",
                "facets[type][]=D",
                "start=2026-01-01T00",
                "end=2026-01-02T00",
                "sort[0][column]=period",
                "sort[0][direction]=desc",
                "sort[1][column]=respondent",
                "sort[1][direction]=asc",
                "offset=5000",
                "length=100",
            ]
        );
    }

    #[test]
    fn repeated_facet_replaces_values() {
        let q = DataQuery::new("a/b")
            .facet("x", ["1"])
            .facet("x", ["2", "3"]);
        assert_eq!(pairs(&q), ["facets[x][]=2", "facets[x][]=3"]);
    }

    #[test]
    fn empty_query_has_no_params() {
        assert_eq!(
            DataQuery::new("a/b").params(),
            Vec::<(String, String)>::new()
        );
    }

    #[test]
    fn validation_rejects_bad_routes_and_lengths() {
        assert!(DataQuery::new("").validate().is_err());
        assert!(DataQuery::new("a//b").validate().is_err());
        assert!(DataQuery::new("a/../b").validate().is_err());
        assert!(DataQuery::new("a/b").length(0).validate().is_err());
        assert!(DataQuery::new("a/b").length(5001).validate().is_err());
        assert!(DataQuery::new("a/b").length(5000).validate().is_ok());
    }

    #[test]
    fn page_at_sets_offset_length_and_default_sort() {
        let q = DataQuery::new("a/b").length(250).page_at(750);
        assert_eq!(
            pairs(&q),
            [
                "sort[0][column]=period",
                "sort[0][direction]=asc",
                "offset=750",
                "length=250"
            ]
        );
    }

    #[test]
    fn page_at_respects_user_sort_and_opt_out() {
        let sorted = DataQuery::new("a/b")
            .sort("value", Direction::Desc)
            .page_at(0);
        assert_eq!(sorted.sort, [("value".to_owned(), Direction::Desc)]);
        let off = DataQuery::new("a/b").without_auto_sort().page_at(0);
        assert_eq!(off.sort, Vec::<(String, Direction)>::new());
        assert_eq!(off.page_len(), MAX_PAGE_LEN);
    }
}

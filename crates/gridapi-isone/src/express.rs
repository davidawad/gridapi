//! The keyless ISO Express client: public CSV reports from `www.iso-ne.com`.
//!
//! Two kinds of URL serve these reports, and they behave differently:
//!
//! * `/static-transform/csv/histRpts/...` answers with an HTTP 302 to
//!   `/histRpts/...`, which holds the file. Redirects must be followed; nothing
//!   else is needed. A day with no file is an HTTP 200 whose body is
//!   `No data exists for this period.`
//! * `/transform/csv/...` is a dynamic report generator. It answers HTTP 403
//!   unless the request carries the `isox_token` cookie that any page under
//!   `/isoexpress/web/reports/` sets. This client fetches that page once, keeps
//!   the cookie in its jar, and fetches it again if a 403 ever comes back.

use std::{
    sync::{
        atomic::{AtomicBool, Ordering},
        Arc,
    },
    time::Duration,
};

use bytes::Bytes;
use chrono::{NaiveDate, Utc};
use chrono_tz::America::New_York;

use crate::{
    error::Error,
    http::{Call, RetryPolicy, Transport},
    report::is_no_data,
    rows::{
        self, Decoded, Decoder, FiveMinuteLmp, FiveMinuteSystemLoad, FuelMix, HourlyDemand,
        HourlyLmp, LocationFilter, RegulationPrice, ReserveRequirement, ZonalLoad,
    },
};

/// ISO New England's public web root.
pub const DEFAULT_SITE_URL: &str = "https://www.iso-ne.com";

/// Page whose response sets the `isox_token` session cookie.
const SESSION_PAGE: &str = "/isoexpress/web/reports/";

/// A five-minute LMP day is split into six four-hour files.
const FIVE_MINUTE_BLOCKS: [&str; 6] = ["00-04", "04-08", "08-12", "12-16", "16-20", "20-24"];

/// Whether a price is the first, preliminary calculation or the settled final.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Revision {
    /// Preliminary prices, available within the hour.
    Preliminary,
    /// Final prices, available a few days after the operating day.
    Final,
}

impl Revision {
    const fn word(self) -> &'static str {
        match self {
            Self::Preliminary => "prelim",
            Self::Final => "final",
        }
    }
}

/// Which energy market an hourly LMP feed covers.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Market {
    /// The day-ahead market.
    DayAhead,
    /// The real-time market.
    RealTime,
}

impl Market {
    const fn word(self) -> &'static str {
        match self {
            Self::DayAhead => "da",
            Self::RealTime => "rt",
        }
    }
}

/// How a path is reached.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Access {
    /// A static file behind a redirect; no session needed.
    Public,
    /// A dynamic report; needs the `isox_token` cookie.
    Session,
}

/// One URL path a day's report needs, and how to reach it.
type Path = (String, Access);

/// Settings for an [`Express`] client.
#[derive(Debug, Clone)]
pub struct ExpressBuilder {
    site_url: String,
    user_agent: String,
    retry: RetryPolicy,
    timeout: Duration,
    connect_timeout: Duration,
}

impl Default for ExpressBuilder {
    fn default() -> Self {
        Self {
            site_url: DEFAULT_SITE_URL.to_owned(),
            user_agent: format!("gridapi-isone/{}", env!("CARGO_PKG_VERSION")),
            retry: RetryPolicy::default(),
            timeout: Duration::from_secs(180),
            connect_timeout: Duration::from_secs(15),
        }
    }
}

impl ExpressBuilder {
    /// Point the client at a different root, e.g. a mirror or a test server.
    pub fn site_url(&mut self, site_url: impl Into<String>) -> &mut Self {
        self.site_url = site_url.into();
        self
    }

    /// The `User-Agent` header sent with every request.
    pub fn user_agent(&mut self, user_agent: impl Into<String>) -> &mut Self {
        self.user_agent = user_agent.into();
        self
    }

    /// How transient failures are retried.
    pub fn retry(&mut self, retry: RetryPolicy) -> &mut Self {
        self.retry = retry;
        self
    }

    /// Whole-request timeout, including reading the body.
    pub fn timeout(&mut self, timeout: Duration) -> &mut Self {
        self.timeout = timeout;
        self
    }

    /// Validate the settings and build the client.
    pub fn build(&self) -> Result<Express, Error> {
        let site_url = self.site_url.trim_end_matches('/').to_owned();
        if !(site_url.starts_with("https://") || site_url.starts_with("http://")) {
            return Err(Error::InvalidBaseUrl(self.site_url.clone()));
        }
        let http = reqwest::Client::builder()
            .user_agent(&self.user_agent)
            .cookie_store(true)
            .timeout(self.timeout)
            .connect_timeout(self.connect_timeout)
            .build()
            .map_err(|e| Error::ClientBuild(Box::new(e)))?;
        Ok(Express {
            inner: Arc::new(Inner {
                transport: Transport::new(http, self.retry),
                site_url,
                session_ready: AtomicBool::new(false),
            }),
        })
    }
}

#[derive(Debug)]
struct Inner {
    transport: Transport,
    site_url: String,
    /// Whether this client has fetched the session page. Plain flag, no lock:
    /// racing callers may each fetch the page, which is harmless.
    session_ready: AtomicBool,
}

/// A handle to ISO Express's public CSV reports. Cheap to clone; clones share
/// one connection pool and one cookie jar.
#[derive(Debug, Clone)]
pub struct Express {
    inner: Arc<Inner>,
}

const _: () = {
    const fn assert_send_sync<T: Send + Sync>() {}
    assert_send_sync::<Express>();
};

/// Rows fetched for a date range, plus the days for which ISO-NE had nothing.
#[derive(Debug, Clone, PartialEq)]
pub struct Series<T> {
    /// Rows from every day that was found, in day order then file order.
    pub rows: Vec<T>,
    /// Requested days that had no report, or an empty one.
    pub missing: Vec<NaiveDate>,
}

impl<T> Default for Series<T> {
    fn default() -> Self {
        Self {
            rows: Vec::new(),
            missing: Vec::new(),
        }
    }
}

/// A pending fetch of one report over one or more days.
///
/// Build it with an [`Express`] method, optionally narrow it with
/// [`through`](Request::through) and [`locations`](Request::locations), then call
/// [`fetch`](Request::fetch).
#[derive(Debug)]
#[must_use = "a Request does nothing until `.fetch()` is awaited"]
pub struct Request<'c, T> {
    client: &'c Express,
    label: &'static str,
    paths: fn(NaiveDate) -> Vec<Path>,
    decode: Decoder<T>,
    start: NaiveDate,
    end: NaiveDate,
    filter: LocationFilter,
}

impl<T> Request<'_, T> {
    /// Extend the request to every day from the first through `end`, inclusive.
    pub fn through(mut self, end: NaiveDate) -> Self {
        self.end = end;
        self
    }

    /// Keep only rows for these locations; see [`LocationFilter`].
    pub fn locations(mut self, filter: LocationFilter) -> Self {
        self.filter = filter;
        self
    }

    /// Fetch and decode, one day at a time. Days with no report are listed in
    /// [`Series::missing`] rather than failing the whole request.
    pub async fn fetch(self) -> Result<Series<T>, Error> {
        if self.end < self.start {
            return Err(Error::EmptyRange {
                start: self.start,
                end: self.end,
            });
        }
        let mut series = Series::default();
        for day in self.start.iter_days().take_while(|d| *d <= self.end) {
            match self.fetch_day(day).await? {
                Some(rows) => series.rows.extend(rows),
                None => series.missing.push(day),
            }
        }
        tracing::info!(
            report = self.label,
            start = %self.start,
            end = %self.end,
            rows = series.rows.len(),
            missing_days = series.missing.len(),
            "fetched iso-ne report"
        );
        Ok(series)
    }

    /// Like [`fetch`](Request::fetch), but any missing day is an error.
    pub async fn fetch_strict(self) -> Result<Vec<T>, Error> {
        let label = self.label;
        let series = self.fetch().await?;
        match series.missing.first() {
            Some(first) => Err(Error::MissingDays {
                report: label.to_owned(),
                count: series.missing.len(),
                first: *first,
            }),
            None => Ok(series.rows),
        }
    }

    /// One day's rows, or `None` when ISO-NE had no report for it.
    async fn fetch_day(&self, day: NaiveDate) -> Result<Option<Vec<T>>, Error> {
        let mut rows = Vec::new();
        let mut file_rows = 0;
        let mut found = false;
        for (path, access) in (self.paths)(day) {
            let Some(body) = self.client.get(&path, access).await? else {
                continue;
            };
            found = true;
            let decoded = (self.decode)(&body, day, &self.filter)?;
            file_rows += decoded.file_rows;
            rows.extend(decoded.rows);
        }
        Ok((found && file_rows > 0).then_some(rows))
    }
}

fn ymd(day: NaiveDate) -> String {
    day.format("%Y%m%d").to_string()
}

fn static_path(directory: &str, file: &str) -> String {
    format!("/static-transform/csv/histRpts/{directory}/{file}")
}

fn dynamic_range(report: &str, day: NaiveDate) -> Vec<Path> {
    let d = ymd(day);
    vec![(
        format!("/transform/csv/{report}?start={d}&end={d}"),
        Access::Session,
    )]
}

impl Express {
    /// A client with default settings.
    pub fn new() -> Result<Self, Error> {
        ExpressBuilder::default().build()
    }

    /// Settings for a customised client.
    #[must_use]
    pub fn builder() -> ExpressBuilder {
        ExpressBuilder::default()
    }

    fn request<T>(
        &self,
        label: &'static str,
        paths: fn(NaiveDate) -> Vec<Path>,
        decode: Decoder<T>,
        day: NaiveDate,
    ) -> Request<'_, T> {
        Request {
            client: self,
            label,
            paths,
            decode,
            start: day,
            end: day,
            filter: LocationFilter::all(),
        }
    }

    /// Day-ahead hourly LMP for every location (about 29,000 rows a day), from
    /// the day-ahead report `WW_DALMP_ISO_<day>.csv`. Published the day before.
    pub fn day_ahead_lmp(&self, day: NaiveDate) -> Request<'_, HourlyLmp> {
        self.request(
            "day-ahead hourly lmp",
            |d| {
                vec![(
                    static_path("da-lmp", &format!("WW_DALMP_ISO_{}.csv", ymd(d))),
                    Access::Public,
                )]
            },
            rows::hourly_lmp,
            day,
        )
    }

    /// Real-time hourly LMP. `Final` reads `lmp_rt_final_<day>.csv` (id, name and
    /// type per row, available a few days late); `Preliminary` reads
    /// `lmp_rt_prelim_<day>.csv` (names only).
    pub fn real_time_hourly_lmp(
        &self,
        day: NaiveDate,
        revision: Revision,
    ) -> Request<'_, HourlyLmp> {
        let paths: fn(NaiveDate) -> Vec<Path> = match revision {
            Revision::Final => |d| {
                vec![(
                    static_path("rt-lmp", &format!("lmp_rt_final_{}.csv", ymd(d))),
                    Access::Public,
                )]
            },
            Revision::Preliminary => |d| {
                vec![(
                    static_path("rt-lmp", &format!("lmp_rt_prelim_{}.csv", ymd(d))),
                    Access::Public,
                )]
            },
        };
        self.request("real-time hourly lmp", paths, rows::hourly_lmp, day)
    }

    /// Real-time five-minute LMP for the whole day, from the six four-hour files
    /// `lmp_5min_<day>_<HH-HH>.csv`. A full day for every location is about
    /// 350,000 rows, so narrow it with [`Request::locations`].
    pub fn five_minute_lmp(
        &self,
        day: NaiveDate,
        revision: Revision,
    ) -> Request<'_, FiveMinuteLmp> {
        let paths: fn(NaiveDate) -> Vec<Path> = match revision {
            Revision::Final => |d| five_minute_paths("5min-rt-final", d),
            Revision::Preliminary => |d| five_minute_paths("5min-rt-prelim", d),
        };
        self.request("five-minute lmp", paths, rows::five_minute_lmp, day)
    }

    /// Dispatch fuel mix samples for the day (about 2,000 rows).
    pub fn fuel_mix(&self, day: NaiveDate) -> Request<'_, FuelMix> {
        self.request(
            "fuel mix",
            |d| {
                vec![(
                    format!("/transform/csv/genfuelmix?start={}", ymd(d)),
                    Access::Session,
                )]
            },
            rows::fuel_mix,
            day,
        )
    }

    /// Real-time hourly system load, in `MWh`.
    pub fn hourly_system_load(&self, day: NaiveDate) -> Request<'_, HourlyDemand> {
        self.request(
            "hourly system load",
            |d| dynamic_range("hourlysystemdemand", d),
            rows::hourly_system_load,
            day,
        )
    }

    /// Day-ahead cleared hourly demand, in `MWh`.
    pub fn day_ahead_demand(&self, day: NaiveDate) -> Request<'_, HourlyDemand> {
        self.request(
            "day-ahead demand",
            |d| dynamic_range("hourlydayaheaddemand", d),
            rows::day_ahead_demand,
            day,
        )
    }

    /// Real-time five-minute system load.
    pub fn five_minute_system_load(&self, day: NaiveDate) -> Request<'_, FiveMinuteSystemLoad> {
        self.request(
            "five-minute system load",
            |d| dynamic_range("fiveminutesystemload", d),
            rows::five_minute_system_load,
            day,
        )
    }

    /// Five-minute estimated load by load zone.
    pub fn zonal_load(&self, day: NaiveDate) -> Request<'_, ZonalLoad> {
        self.request(
            "five-minute zonal load",
            |d| dynamic_range("fiveminuteestimatedzonalload", d),
            rows::zonal_load,
            day,
        )
    }

    /// Five-minute regulation clearing prices, preliminary or final.
    pub fn regulation_prices(
        &self,
        day: NaiveDate,
        revision: Revision,
    ) -> Request<'_, RegulationPrice> {
        let paths: fn(NaiveDate) -> Vec<Path> = match revision {
            Revision::Preliminary => |d| regulation_paths("prelim", d),
            Revision::Final => |d| regulation_paths("final", d),
        };
        self.request(
            "regulation clearing prices",
            paths,
            rows::regulation_price,
            day,
        )
    }

    /// Hourly operating-reserve requirements by reserve zone.
    pub fn reserve_requirements(&self, day: NaiveDate) -> Request<'_, ReserveRequirement> {
        self.request(
            "reserve requirements",
            |d| dynamic_range("hourlyrequirements", d),
            rows::reserve_requirement,
            day,
        )
    }

    /// The latest five-minute LMPs for every location (about 1,200 rows).
    pub async fn five_minute_lmp_latest(
        &self,
        revision: Revision,
        filter: &LocationFilter,
    ) -> Result<Vec<FiveMinuteLmp>, Error> {
        let path = format!("/transform/csv/fiveminlmp/current?type={}", revision.word());
        self.latest(&path, rows::five_minute_lmp, filter).await
    }

    /// The rolling window of recent five-minute LMPs (about 2.8 MB of CSV).
    pub async fn five_minute_lmp_rolling(
        &self,
        filter: &LocationFilter,
    ) -> Result<Vec<FiveMinuteLmp>, Error> {
        self.latest(
            "/transform/csv/fiveminlmp/currentrollinginterval",
            rows::five_minute_lmp,
            filter,
        )
        .await
    }

    /// The latest hourly LMPs for every location, for one market and revision.
    /// Rows carry ids only, no names.
    ///
    /// ISO-NE serves only two pairs here (checked live 2026-11-18): day-ahead as
    /// [`Revision::Final`] and real-time as [`Revision::Preliminary`]. Any other pair is
    /// refused with [`Error::UnsupportedQuery`] before a request is sent; ISO-NE answers
    /// those with an opaque HTTP 500.
    pub async fn hourly_lmp_latest(
        &self,
        market: Market,
        revision: Revision,
        filter: &LocationFilter,
    ) -> Result<Vec<HourlyLmp>, Error> {
        if !matches!(
            (market, revision),
            (Market::DayAhead, Revision::Final) | (Market::RealTime, Revision::Preliminary)
        ) {
            return Err(Error::UnsupportedQuery(format!(
                "the latest hourly LMP report has day-ahead final and real-time preliminary prices only, not {market:?} {revision:?}"
            )));
        }
        let path = format!(
            "/transform/csv/hourlylmp/current?type={}&market={}",
            revision.word(),
            market.word()
        );
        self.latest(&path, rows::hourly_lmp, filter).await
    }

    /// The rolling window of recent hourly LMPs for one market and revision.
    ///
    /// ISO-NE serves only real-time [`Revision::Preliminary`] here (checked live
    /// 2026-11-18); any other pair is refused with [`Error::UnsupportedQuery`] before a
    /// request is sent, where ISO-NE answers HTTP 500.
    pub async fn hourly_lmp_rolling(
        &self,
        market: Market,
        revision: Revision,
        filter: &LocationFilter,
    ) -> Result<Vec<HourlyLmp>, Error> {
        if (market, revision) != (Market::RealTime, Revision::Preliminary) {
            return Err(Error::UnsupportedQuery(format!(
                "the rolling hourly LMP report has real-time preliminary prices only, not {market:?} {revision:?}"
            )));
        }
        let path = format!(
            "/transform/csv/hourlylmp/currentrollinginterval?type={}&market={}",
            revision.word(),
            market.word()
        );
        self.latest(&path, rows::hourly_lmp, filter).await
    }

    /// The raw CSV body at a site-relative path, such as
    /// `/transform/csv/genfuelmix?start=20261117` or
    /// `/static-transform/csv/histRpts/da-lmp/WW_DALMP_ISO_20261117.csv`.
    /// The session cookie is handled for `/transform/` paths. `None` means ISO-NE
    /// has no such report.
    pub async fn raw_csv(&self, path: &str) -> Result<Option<Bytes>, Error> {
        let access = if path.starts_with("/transform/") {
            Access::Session
        } else {
            Access::Public
        };
        self.get(path, access).await
    }

    async fn latest<T>(
        &self,
        path: &str,
        decode: Decoder<T>,
        filter: &LocationFilter,
    ) -> Result<Vec<T>, Error> {
        let body = self
            .get(path, Access::Session)
            .await?
            .ok_or_else(|| Error::NotFound {
                url: format!("{}{path}", self.inner.site_url),
            })?;
        let today = Utc::now().with_timezone(&New_York).date_naive();
        let Decoded { rows, .. } = decode(&body, today, filter)?;
        tracing::info!(path, rows = rows.len(), "fetched latest iso-ne report");
        Ok(rows)
    }

    /// GET a site-relative path. `Ok(None)` means ISO-NE has no such report.
    async fn get(&self, path: &str, access: Access) -> Result<Option<Bytes>, Error> {
        let url = format!("{}{path}", self.inner.site_url);
        let body = match access {
            Access::Public => self.inner.transport.get(&url, Call::default()).await,
            Access::Session => self.get_with_session(&url).await,
        };
        match body {
            Ok(bytes) if is_no_data(&bytes) => Ok(None),
            Ok(bytes) => Ok(Some(bytes)),
            Err(Error::NotFound { .. }) => Ok(None),
            Err(e) => Err(e),
        }
    }

    /// GET a dynamic report: prime the session if this client has not yet, and
    /// re-prime and retry once if the report still answers 403.
    async fn get_with_session(&self, url: &str) -> Result<Bytes, Error> {
        if !self.inner.session_ready.load(Ordering::Acquire) {
            self.start_session().await?;
        }
        match self.inner.transport.get(url, Call::default()).await {
            Err(Error::Status { status: 403, .. }) => {
                tracing::warn!(
                    url,
                    "iso express answered 403; refreshing the session token"
                );
                self.start_session().await?;
                match self.inner.transport.get(url, Call::default()).await {
                    Err(Error::Status { status: 403, .. }) => Err(Error::SessionRejected {
                        url: url.to_owned(),
                    }),
                    other => other,
                }
            }
            other => other,
        }
    }

    /// Fetch the session page so the cookie jar holds a fresh `isox_token`.
    async fn start_session(&self) -> Result<(), Error> {
        let url = format!("{}{SESSION_PAGE}", self.inner.site_url);
        self.inner.transport.get(&url, Call::default()).await?;
        self.inner.session_ready.store(true, Ordering::Release);
        tracing::debug!(url, "iso express session started");
        Ok(())
    }
}

fn regulation_paths(revision: &str, day: NaiveDate) -> Vec<Path> {
    let d = ymd(day);
    vec![(
        format!("/transform/csv/fiveminrcp?type={revision}&start={d}&end={d}"),
        Access::Session,
    )]
}

fn five_minute_paths(directory: &str, day: NaiveDate) -> Vec<Path> {
    FIVE_MINUTE_BLOCKS
        .iter()
        .map(|block| {
            (
                static_path(directory, &format!("lmp_5min_{}_{block}.csv", ymd(day))),
                Access::Public,
            )
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn day() -> NaiveDate {
        NaiveDate::from_ymd_opt(2026, 11, 17).expect("date")
    }

    #[test]
    fn five_minute_paths_cover_the_six_blocks() {
        let paths = five_minute_paths("5min-rt-final", day());
        assert_eq!(paths.len(), 6);
        assert_eq!(
            paths[0].0,
            "/static-transform/csv/histRpts/5min-rt-final/lmp_5min_20261117_00-04.csv"
        );
        assert_eq!(
            paths[5].0,
            "/static-transform/csv/histRpts/5min-rt-final/lmp_5min_20261117_20-24.csv"
        );
        assert!(paths.iter().all(|(_, a)| *a == Access::Public));
    }

    #[test]
    fn dynamic_ranges_use_start_and_end() {
        let paths = dynamic_range("hourlysystemdemand", day());
        assert_eq!(
            paths,
            [(
                "/transform/csv/hourlysystemdemand?start=20261117&end=20261117".to_owned(),
                Access::Session
            )]
        );
    }

    #[test]
    fn rejects_a_site_url_without_a_scheme() {
        let err = Express::builder()
            .site_url("www.iso-ne.com")
            .build()
            .expect_err("no scheme");
        assert!(matches!(err, Error::InvalidBaseUrl(_)), "got {err:?}");
    }

    #[test]
    fn revision_and_market_words_match_the_query_strings() {
        assert_eq!(Revision::Preliminary.word(), "prelim");
        assert_eq!(Revision::Final.word(), "final");
        assert_eq!(Market::DayAhead.word(), "da");
        assert_eq!(Market::RealTime.word(), "rt");
    }
}

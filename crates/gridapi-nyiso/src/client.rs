//! The client: URL layout, daily-versus-archive routing, and typed requests.

use std::{
    collections::{hash_map::Entry, HashMap},
    io::Cursor,
    io::Read,
    marker::PhantomData,
    sync::Arc,
    time::Duration,
};

use bytes::Bytes;
use chrono::{Datelike, NaiveDate, Utc};
use chrono_tz::America::New_York;
use zip::{result::ZipError, ZipArchive};

use crate::{
    dataset::{Dataset, Level},
    error::Error,
    http::{RetryPolicy, Transport},
    records::{AncillaryPrice, FuelMix, InterfaceFlow, Lbmp, Load, LoadForecast, Record},
};

/// NYISO's public CSV root.
pub const DEFAULT_BASE_URL: &str = "https://mis.nyiso.com/public/csv";

/// Days (Eastern) a daily file is assumed to stay on the server. Measured
/// 2026-11-18: day -8 still served, day -13 gone. Files older than this are
/// fetched from the monthly archive first. A wrong guess only costs one extra
/// request, because a miss on either route falls through to the other.
pub const DEFAULT_DAILY_RETENTION_DAYS: u32 = 7;

/// Largest single CSV accepted out of an archive, as a guard against zip bombs.
const MAX_ENTRY_BYTES: u64 = 512 * 1024 * 1024;

/// Which published form of a day's file to read.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Source {
    /// Recent days from the daily file, older days from the monthly archive;
    /// a miss on one route falls through to the other.
    #[default]
    Auto,
    /// Only the daily file.
    DailyOnly,
    /// Only the monthly archive.
    ArchiveOnly,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Route {
    Daily,
    Archive,
}

impl Source {
    /// The routes to try, in order, for a day that is `age_days` old.
    fn routes(self, age_days: i64, retention_days: i64) -> &'static [Route] {
        match self {
            Self::DailyOnly => &[Route::Daily],
            Self::ArchiveOnly => &[Route::Archive],
            Self::Auto if age_days <= retention_days => &[Route::Daily, Route::Archive],
            Self::Auto => &[Route::Archive, Route::Daily],
        }
    }
}

/// Settings for a [`Client`].
#[derive(Debug, Clone)]
pub struct ClientBuilder {
    base_url: String,
    user_agent: String,
    retry: RetryPolicy,
    timeout: Duration,
    connect_timeout: Duration,
    retention_days: u32,
}

impl Default for ClientBuilder {
    fn default() -> Self {
        Self {
            base_url: DEFAULT_BASE_URL.to_owned(),
            user_agent: format!("gridapi-nyiso/{}", env!("CARGO_PKG_VERSION")),
            retry: RetryPolicy::default(),
            timeout: Duration::from_secs(120),
            connect_timeout: Duration::from_secs(15),
            retention_days: DEFAULT_DAILY_RETENTION_DAYS,
        }
    }
}

impl ClientBuilder {
    /// Point the client at a different root, e.g. a mirror or a test server.
    pub fn base_url(&mut self, base_url: impl Into<String>) -> &mut Self {
        self.base_url = base_url.into();
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

    /// Age in days beyond which [`Source::Auto`] reads the monthly archive first.
    pub fn daily_retention_days(&mut self, days: u32) -> &mut Self {
        self.retention_days = days;
        self
    }

    /// Validate the settings and build the client.
    pub fn build(&self) -> Result<Client, Error> {
        let base_url = self.base_url.trim_end_matches('/').to_owned();
        if !(base_url.starts_with("https://") || base_url.starts_with("http://")) {
            return Err(Error::InvalidBaseUrl(self.base_url.clone()));
        }
        let http = reqwest::Client::builder()
            .user_agent(&self.user_agent)
            .timeout(self.timeout)
            .connect_timeout(self.connect_timeout)
            .build()
            .map_err(|e| Error::ClientBuild(Box::new(e)))?;
        Ok(Client {
            inner: Arc::new(Inner {
                transport: Transport::new(http, self.retry),
                base_url,
                retention_days: i64::from(self.retention_days),
            }),
        })
    }
}

#[derive(Debug)]
struct Inner {
    transport: Transport,
    base_url: String,
    retention_days: i64,
}

/// A handle to NYISO's public CSV data. Cheap to clone; clones share one
/// connection pool.
#[derive(Debug, Clone)]
pub struct Client {
    inner: Arc<Inner>,
}

const _: () = {
    const fn assert_send_sync<T: Send + Sync>() {}
    assert_send_sync::<Client>();
};

/// Rows fetched for a date range, plus the days for which NYISO published nothing.
#[derive(Debug, Clone, PartialEq)]
pub struct Series<T> {
    /// Rows from every day that was found, in day order then file order.
    pub rows: Vec<T>,
    /// Requested days that had neither a daily file nor an archive entry.
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

/// A pending fetch of one dataset over one or more days.
///
/// Build it with a [`Client`] method, optionally extend it with
/// [`through`](Request::through) and [`source`](Request::source), then call
/// [`fetch`](Request::fetch).
#[derive(Debug)]
#[must_use = "a Request does nothing until `.fetch()` is awaited"]
pub struct Request<'c, T> {
    client: &'c Client,
    dataset: Dataset,
    start: NaiveDate,
    end: NaiveDate,
    source: Source,
    row: PhantomData<fn() -> T>,
}

impl<'c, T: Record> Request<'c, T> {
    fn new(client: &'c Client, dataset: Dataset, day: NaiveDate) -> Self {
        Self {
            client,
            dataset,
            start: day,
            end: day,
            source: Source::Auto,
            row: PhantomData,
        }
    }

    /// Extend the request to every day from the first through `end`, inclusive.
    pub fn through(mut self, end: NaiveDate) -> Self {
        self.end = end;
        self
    }

    /// Choose daily files, monthly archives, or the automatic switch.
    pub fn source(mut self, source: Source) -> Self {
        self.source = source;
        self
    }

    /// Fetch and decode. Days with no published file are listed in
    /// [`Series::missing`] rather than failing the whole request.
    pub async fn fetch(self) -> Result<Series<T>, Error> {
        if self.end < self.start {
            return Err(Error::EmptyRange {
                start: self.start,
                end: self.end,
            });
        }
        let today = eastern_today();
        let mut archives = Archives::default();
        let mut series = Series::default();
        for day in self.start.iter_days().take_while(|d| *d <= self.end) {
            let age = (today - day).num_days();
            match self
                .client
                .day_csv(self.dataset, day, self.source, age, &mut archives)
                .await?
            {
                Some(csv) => series.rows.extend(T::decode(self.dataset, &csv)?),
                None => series.missing.push(day),
            }
        }
        tracing::info!(
            dataset = %self.dataset,
            start = %self.start,
            end = %self.end,
            rows = series.rows.len(),
            missing_days = series.missing.len(),
            "fetched nyiso dataset"
        );
        Ok(series)
    }

    /// Like [`fetch`](Request::fetch), but any missing day is an error.
    pub async fn fetch_strict(self) -> Result<Vec<T>, Error> {
        let series = self.fetch().await?;
        match series.missing.first() {
            Some(first) => Err(Error::MissingDays {
                count: series.missing.len(),
                first: *first,
            }),
            None => Ok(series.rows),
        }
    }
}

fn eastern_today() -> NaiveDate {
    Utc::now().with_timezone(&New_York).date_naive()
}

/// Monthly archives opened during one fetch, keyed by the first of the month.
/// `None` records a month whose archive does not exist, so it is asked for once.
type Archives = HashMap<NaiveDate, Option<ZipArchive<Cursor<Bytes>>>>;

impl Client {
    /// A client with default settings.
    pub fn new() -> Result<Self, Error> {
        ClientBuilder::default().build()
    }

    /// Settings for a customised client.
    #[must_use]
    pub fn builder() -> ClientBuilder {
        ClientBuilder::default()
    }

    /// Day-ahead hourly LBMP (`damlbmp`), starting at `day`.
    pub fn day_ahead_lbmp(&self, level: Level, day: NaiveDate) -> Request<'_, Lbmp> {
        Request::new(self, Dataset::DayAheadLbmp(level), day)
    }

    /// Real-time five-minute LBMP (`realtime`), starting at `day`.
    pub fn real_time_lbmp(&self, level: Level, day: NaiveDate) -> Request<'_, Lbmp> {
        Request::new(self, Dataset::RealTimeLbmp(level), day)
    }

    /// Real-time hourly integrated LBMP (`rtlbmp`), starting at `day`.
    pub fn integrated_real_time_lbmp(&self, level: Level, day: NaiveDate) -> Request<'_, Lbmp> {
        Request::new(self, Dataset::IntegratedRealTimeLbmp(level), day)
    }

    /// Five-minute actual load by zone (`pal`), starting at `day`.
    pub fn actual_load(&self, day: NaiveDate) -> Request<'_, Load> {
        Request::new(self, Dataset::ActualLoad, day)
    }

    /// Hourly integrated actual load by zone (`palIntegrated`), starting at `day`.
    pub fn integrated_load(&self, day: NaiveDate) -> Request<'_, Load> {
        Request::new(self, Dataset::IntegratedLoad, day)
    }

    /// Hourly zonal load forecast (`isolf`), starting at `day`.
    pub fn load_forecast(&self, day: NaiveDate) -> Request<'_, LoadForecast> {
        Request::new(self, Dataset::LoadForecast, day)
    }

    /// Five-minute generation by fuel category (`rtfuelmix`), starting at `day`.
    pub fn fuel_mix(&self, day: NaiveDate) -> Request<'_, FuelMix> {
        Request::new(self, Dataset::FuelMix, day)
    }

    /// Day-ahead hourly ancillary service prices (`damasp`), starting at `day`.
    /// Only the per-zone layout (2017 onward) is decoded.
    pub fn day_ahead_ancillary(&self, day: NaiveDate) -> Request<'_, AncillaryPrice> {
        Request::new(self, Dataset::DayAheadAncillary, day)
    }

    /// Real-time five-minute ancillary service prices (`rtasp`), starting at `day`.
    /// Only the per-zone layout (2017 onward) is decoded.
    pub fn real_time_ancillary(&self, day: NaiveDate) -> Request<'_, AncillaryPrice> {
        Request::new(self, Dataset::RealTimeAncillary, day)
    }

    /// Interface flows and limits (`ExternalLimitsFlows`), starting at `day`.
    pub fn interface_flows(&self, day: NaiveDate) -> Request<'_, InterfaceFlow> {
        Request::new(self, Dataset::InterfaceFlows, day)
    }

    /// The URL of the daily file for `dataset` on `day`.
    #[must_use]
    pub fn daily_url(&self, dataset: Dataset, day: NaiveDate) -> String {
        format!(
            "{}/{}/{}",
            self.inner.base_url,
            dataset.directory(),
            dataset.daily_file(day)
        )
    }

    /// The URL of the monthly archive that holds `day`.
    #[must_use]
    pub fn archive_url(&self, dataset: Dataset, day: NaiveDate) -> String {
        format!(
            "{}/{}/{}",
            self.inner.base_url,
            dataset.directory(),
            dataset.archive_file(day)
        )
    }

    /// The raw CSV text of one day, or `None` when NYISO published nothing.
    pub async fn raw_csv(
        &self,
        dataset: Dataset,
        day: NaiveDate,
        source: Source,
    ) -> Result<Option<Bytes>, Error> {
        let age = (eastern_today() - day).num_days();
        self.day_csv(dataset, day, source, age, &mut Archives::default())
            .await
    }

    async fn day_csv(
        &self,
        dataset: Dataset,
        day: NaiveDate,
        source: Source,
        age_days: i64,
        archives: &mut Archives,
    ) -> Result<Option<Bytes>, Error> {
        for route in source.routes(age_days, self.inner.retention_days) {
            let found = match route {
                Route::Daily => self.daily_csv(dataset, day).await?,
                Route::Archive => self.archived_csv(dataset, day, archives).await?,
            };
            tracing::debug!(%dataset, %day, ?route, found = found.is_some(), "route tried");
            if found.is_some() {
                return Ok(found);
            }
        }
        Ok(None)
    }

    async fn daily_csv(&self, dataset: Dataset, day: NaiveDate) -> Result<Option<Bytes>, Error> {
        match self
            .inner
            .transport
            .get(&self.daily_url(dataset, day))
            .await
        {
            Ok(bytes) => Ok(Some(bytes)),
            Err(Error::NotFound { .. }) => Ok(None),
            Err(e) => Err(e),
        }
    }

    async fn archived_csv(
        &self,
        dataset: Dataset,
        day: NaiveDate,
        archives: &mut Archives,
    ) -> Result<Option<Bytes>, Error> {
        let month = day.with_day(1).unwrap_or(day);
        let url = self.archive_url(dataset, day);
        let slot = match archives.entry(month) {
            Entry::Occupied(occupied) => occupied.into_mut(),
            Entry::Vacant(vacant) => {
                let opened = match self.inner.transport.get(&url).await {
                    Ok(zip) => Some(open_archive(&url, zip)?),
                    Err(Error::NotFound { .. }) => None,
                    Err(e) => return Err(e),
                };
                vacant.insert(opened)
            }
        };
        match slot {
            Some(archive) => read_entry(&url, archive, &dataset.daily_file(day)),
            None => Ok(None),
        }
    }
}

fn archive_error(url: &str, source: impl std::error::Error + Send + Sync + 'static) -> Error {
    Error::Archive {
        archive: url.to_owned(),
        source: Box::new(source),
    }
}

fn open_archive(url: &str, zip: Bytes) -> Result<ZipArchive<Cursor<Bytes>>, Error> {
    ZipArchive::new(Cursor::new(zip)).map_err(|e| archive_error(url, e))
}

fn read_entry(
    url: &str,
    archive: &mut ZipArchive<Cursor<Bytes>>,
    name: &str,
) -> Result<Option<Bytes>, Error> {
    match archive.by_name(name) {
        Ok(entry) => {
            let mut buffer = Vec::new();
            entry
                .take(MAX_ENTRY_BYTES)
                .read_to_end(&mut buffer)
                .map_err(|e| archive_error(url, e))?;
            Ok(Some(Bytes::from(buffer)))
        }
        Err(ZipError::FileNotFound) => Ok(None),
        Err(e) => Err(archive_error(url, e)),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn auto_prefers_daily_for_recent_days_and_archive_for_old() {
        assert_eq!(
            Source::Auto.routes(0, 7),
            [Route::Daily, Route::Archive],
            "today"
        );
        assert_eq!(Source::Auto.routes(7, 7), [Route::Daily, Route::Archive]);
        assert_eq!(Source::Auto.routes(8, 7), [Route::Archive, Route::Daily]);
        assert_eq!(
            Source::Auto.routes(-1, 7),
            [Route::Daily, Route::Archive],
            "tomorrow's day-ahead file"
        );
    }

    #[test]
    fn explicit_sources_pin_the_route() {
        assert_eq!(Source::DailyOnly.routes(400, 7), [Route::Daily]);
        assert_eq!(Source::ArchiveOnly.routes(0, 7), [Route::Archive]);
    }

    #[test]
    fn urls_follow_the_published_layout() {
        let client = Client::builder()
            .base_url("https://example.test/public/csv/")
            .build()
            .expect("builds");
        let day = NaiveDate::from_ymd_opt(2026, 11, 17).expect("date");
        assert_eq!(
            client.daily_url(Dataset::DayAheadLbmp(Level::Zone), day),
            "https://example.test/public/csv/damlbmp/20261117damlbmp_zone.csv"
        );
        assert_eq!(
            client.archive_url(Dataset::FuelMix, day),
            "https://example.test/public/csv/rtfuelmix/20261101rtfuelmix_csv.zip"
        );
    }

    #[test]
    fn rejects_a_base_url_without_a_scheme() {
        let err = Client::builder()
            .base_url("mis.nyiso.com/public/csv")
            .build()
            .expect_err("no scheme");
        assert!(matches!(err, Error::InvalidBaseUrl(_)), "got {err:?}");
    }
}

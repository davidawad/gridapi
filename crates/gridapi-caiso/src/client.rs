//! The async HTTP client: pacing, retries, chunking, and the typed helpers.

use std::sync::Arc;
use std::time::Duration;

use chrono::{DateTime, Days, NaiveDate, TimeDelta, Utc};
use chrono_tz::America::Los_Angeles;

use crate::ancillary::{AncillaryPriceRow, AncillaryType};
use crate::error::Error;
use crate::lmp::LmpRow;
use crate::load::{LoadForecastRow, SystemScheduleRow};
use crate::node::NodeRow;
use crate::outlook::{parse_outlook_csv, OutlookDataset, OutlookSeries};
use crate::pacer::Pacer;
use crate::payload::decode_response;
use crate::query::{MarketRun, Query, QueryName};
use crate::record::{FromRecord, Record};
use crate::retry::RetryPolicy;
use crate::time::{chunk_range, OASIS_MAX_SPAN_DAYS};

/// The OASIS API root; `/SingleZip` is appended per request.
pub const DEFAULT_OASIS_BASE_URL: &str = "https://oasis.caiso.com/oasisapi";

/// The Today's Outlook root; `/current/<file>.csv` and `/history/<date>/<file>.csv` hang off it.
pub const DEFAULT_OUTLOOK_BASE_URL: &str = "https://www.caiso.com/outlook";

/// Default gap between OASIS requests: CAISO documents one request per five
/// seconds, plus a margin because the limiter counts at the server.
pub const DEFAULT_MIN_INTERVAL: Duration = Duration::from_millis(5_250);

/// Default per-request timeout. OASIS routinely takes tens of seconds, and
/// requests from cloud IP ranges have been seen to stall far longer.
pub const DEFAULT_REQUEST_TIMEOUT: Duration = Duration::from_secs(180);

/// Default connect timeout.
pub const DEFAULT_CONNECT_TIMEOUT: Duration = Duration::from_secs(30);

/// Default window size used when a long range is split into requests. One day
/// under the hard 31-day limit, so boundary rounding can never trip it.
pub const DEFAULT_CHUNK_DAYS: i64 = 30;

/// Default cap on a single response body.
pub const DEFAULT_MAX_RESPONSE_BYTES: u64 = 256 * 1024 * 1024;

/// Builds a [`Client`].
#[derive(Debug, Clone)]
pub struct ClientBuilder {
    oasis_base_url: String,
    outlook_base_url: String,
    user_agent: String,
    request_timeout: Duration,
    connect_timeout: Duration,
    min_interval: Duration,
    retry: RetryPolicy,
    chunk_span: TimeDelta,
    max_response_bytes: u64,
}

impl Default for ClientBuilder {
    fn default() -> Self {
        Self {
            oasis_base_url: DEFAULT_OASIS_BASE_URL.to_owned(),
            outlook_base_url: DEFAULT_OUTLOOK_BASE_URL.to_owned(),
            user_agent: concat!("gridapi-caiso/", env!("CARGO_PKG_VERSION")).to_owned(),
            request_timeout: DEFAULT_REQUEST_TIMEOUT,
            connect_timeout: DEFAULT_CONNECT_TIMEOUT,
            min_interval: DEFAULT_MIN_INTERVAL,
            retry: RetryPolicy::default(),
            chunk_span: TimeDelta::days(DEFAULT_CHUNK_DAYS),
            max_response_bytes: DEFAULT_MAX_RESPONSE_BYTES,
        }
    }
}

impl ClientBuilder {
    /// Overrides the OASIS API root (for a proxy or a test server).
    pub fn oasis_base_url(&mut self, url: impl Into<String>) -> &mut Self {
        self.oasis_base_url = url.into();
        self
    }

    /// Overrides the Today's Outlook root.
    pub fn outlook_base_url(&mut self, url: impl Into<String>) -> &mut Self {
        self.outlook_base_url = url.into();
        self
    }

    /// Sets the `User-Agent` header. CAISO may block anonymous default agents,
    /// so put a contact in it for production use.
    pub fn user_agent(&mut self, user_agent: impl Into<String>) -> &mut Self {
        self.user_agent = user_agent.into();
        self
    }

    /// Sets the whole-request timeout.
    pub fn request_timeout(&mut self, timeout: Duration) -> &mut Self {
        self.request_timeout = timeout;
        self
    }

    /// Sets the connect timeout.
    pub fn connect_timeout(&mut self, timeout: Duration) -> &mut Self {
        self.connect_timeout = timeout;
        self
    }

    /// Sets the minimum gap between OASIS requests from this client.
    /// `Duration::ZERO` disables pacing.
    pub fn min_interval(&mut self, interval: Duration) -> &mut Self {
        self.min_interval = interval;
        self
    }

    /// Sets the retry policy.
    pub fn retry(&mut self, policy: RetryPolicy) -> &mut Self {
        self.retry = policy;
        self
    }

    /// Sets the window size for long-range chunking (at most 31 days).
    pub fn chunk_span(&mut self, span: TimeDelta) -> &mut Self {
        self.chunk_span = span;
        self
    }

    /// Caps the size of any single response body.
    pub fn max_response_bytes(&mut self, limit: u64) -> &mut Self {
        self.max_response_bytes = limit;
        self
    }

    /// Validates the configuration and builds the client.
    ///
    /// # Errors
    ///
    /// [`Error::Config`] for a chunk span outside `(0, 31 days]`, zero
    /// `max_attempts`, or an HTTP client that cannot be constructed.
    pub fn build(&self) -> Result<Client, Error> {
        if self.chunk_span <= TimeDelta::zero() || self.chunk_span > max_span() {
            return Err(Error::Config(format!(
                "chunk span must be within (0, {OASIS_MAX_SPAN_DAYS} days]"
            )));
        }
        if self.retry.max_attempts == 0 {
            return Err(Error::Config(
                "retry max_attempts must be at least 1".into(),
            ));
        }
        let http = reqwest::Client::builder()
            .user_agent(&self.user_agent)
            .timeout(self.request_timeout)
            .connect_timeout(self.connect_timeout)
            .build()
            .map_err(|e| Error::Config(e.without_url().to_string()))?;
        Ok(Client {
            inner: Arc::new(Inner {
                http,
                oasis_base_url: self.oasis_base_url.trim_end_matches('/').to_owned(),
                outlook_base_url: self.outlook_base_url.trim_end_matches('/').to_owned(),
                retry: self.retry,
                pacer: Pacer::new(self.min_interval),
                chunk_span: self.chunk_span,
                max_response_bytes: self.max_response_bytes,
            }),
        })
    }
}

fn max_span() -> TimeDelta {
    TimeDelta::days(OASIS_MAX_SPAN_DAYS)
}

#[derive(Debug)]
struct Inner {
    http: reqwest::Client,
    oasis_base_url: String,
    outlook_base_url: String,
    retry: RetryPolicy,
    pacer: Pacer,
    chunk_span: TimeDelta,
    max_response_bytes: u64,
}

/// A response body plus the headers this crate reads.
struct Body {
    bytes: Vec<u8>,
    last_modified: Option<DateTime<Utc>>,
}

/// Whether a request counts against the OASIS pacing budget.
#[derive(Clone, Copy)]
enum Pacing {
    Oasis,
    Unpaced,
}

/// An async CAISO client. Cloning is cheap and clones share one pacer.
///
/// Pacing is per client: each [`Client`] (and its clones) spaces its own OASIS
/// requests, with no process-wide lock. Run several clients if you want several
/// independent budgets, for example behind different egress IPs.
#[derive(Debug, Clone)]
pub struct Client {
    inner: Arc<Inner>,
}

const _: () = {
    const fn assert_send_sync<T: Send + Sync>() {}
    assert_send_sync::<Client>();
};

impl Client {
    /// A client with default settings.
    ///
    /// # Errors
    ///
    /// [`Error::Config`] when the HTTP stack cannot initialise.
    pub fn new() -> Result<Self, Error> {
        Self::builder().build()
    }

    /// Starts configuring a client.
    #[must_use]
    pub fn builder() -> ClientBuilder {
        ClientBuilder::default()
    }

    // ----- generic OASIS access -------------------------------------------------

    /// Runs one OASIS request and returns its rows.
    ///
    /// The window must not exceed 31 days; use [`Client::fetch_all`] for longer ones.
    ///
    /// # Errors
    ///
    /// [`Error::InvalidQuery`] for an over-long window (nothing is sent),
    /// [`Error::Oasis`] when OASIS returns an error document (including "no
    /// data"), plus transport, status and decoding errors once retries are spent.
    pub async fn fetch_records(&self, query: &Query) -> Result<Vec<Record>, Error> {
        if query.end() - query.start() > max_span() {
            return Err(Error::InvalidQuery(format!(
                "window {} to {} exceeds the {OASIS_MAX_SPAN_DAYS}-day OASIS limit; use fetch_all",
                query.start(),
                query.end()
            )));
        }
        let url = format!("{}/SingleZip", self.inner.oasis_base_url);
        self.request(&url, &query.params(), Pacing::Oasis, |body| {
            decode_response(&body.bytes)
        })
        .await
    }

    /// Runs a query over any window, splitting it into requests of at most the
    /// configured chunk span and concatenating the rows in time order.
    ///
    /// Windows OASIS reports as "no data" are skipped. If every window is
    /// empty the "no data" error is returned, which also catches a mistyped
    /// node name.
    ///
    /// # Errors
    ///
    /// As [`Client::fetch_records`], from the first window that fails.
    pub async fn fetch_all(&self, query: &Query) -> Result<Vec<Record>, Error> {
        let windows = chunk_range(query.start(), query.end(), self.inner.chunk_span);
        let mut rows = Vec::new();
        let mut no_data = None;
        for (start, end) in windows {
            match self.fetch_records(&query.with_window(start, end)).await {
                Ok(part) => rows.extend(part),
                Err(e) if e.is_no_data() => no_data = Some(e),
                Err(e) => return Err(e),
            }
        }
        match (rows.is_empty(), no_data) {
            (true, Some(e)) => Err(e),
            _ => Ok(rows),
        }
    }

    /// [`Client::fetch_all`] with each row converted to `T`.
    ///
    /// # Errors
    ///
    /// As [`Client::fetch_all`], plus the conversion errors of `T::from_record`.
    pub async fn fetch<T: FromRecord>(&self, query: &Query) -> Result<Vec<T>, Error> {
        self.fetch_all(query)
            .await?
            .iter()
            .map(T::from_record)
            .collect()
    }

    // ----- typed OASIS helpers --------------------------------------------------

    /// Day-ahead LMPs (`PRC_LMP`, hourly) for one or more nodes or hubs.
    ///
    /// Each interval yields one row per component (LMP, energy, congestion,
    /// loss, GHG); see [`pivot_lmp`](crate::pivot_lmp). Use
    /// [`TradingHub::all_node_ids`](crate::TradingHub::all_node_ids) for the hubs.
    ///
    /// # Errors
    ///
    /// [`Error::InvalidQuery`] for an empty node list or a bad window, plus
    /// the errors of [`Client::fetch`].
    pub async fn day_ahead_lmp(
        &self,
        nodes: &[&str],
        start: DateTime<Utc>,
        end: DateTime<Utc>,
    ) -> Result<Vec<LmpRow>, Error> {
        self.lmp(QueryName::PrcLmp, MarketRun::Dam, nodes, start, end)
            .await
    }

    /// Real-time market 5-minute LMPs (`PRC_INTVL_LMP`).
    ///
    /// # Errors
    ///
    /// As [`Client::day_ahead_lmp`].
    pub async fn real_time_lmp(
        &self,
        nodes: &[&str],
        start: DateTime<Utc>,
        end: DateTime<Utc>,
    ) -> Result<Vec<LmpRow>, Error> {
        self.lmp(QueryName::PrcIntvlLmp, MarketRun::Rtm, nodes, start, end)
            .await
    }

    /// Real-time pre-dispatch 15-minute LMPs (`PRC_RTPD_LMP`).
    ///
    /// # Errors
    ///
    /// As [`Client::day_ahead_lmp`].
    pub async fn rtpd_lmp(
        &self,
        nodes: &[&str],
        start: DateTime<Utc>,
        end: DateTime<Utc>,
    ) -> Result<Vec<LmpRow>, Error> {
        self.lmp(QueryName::PrcRtpdLmp, MarketRun::Rtpd, nodes, start, end)
            .await
    }

    async fn lmp(
        &self,
        name: QueryName,
        run: MarketRun,
        nodes: &[&str],
        start: DateTime<Utc>,
        end: DateTime<Utc>,
    ) -> Result<Vec<LmpRow>, Error> {
        if nodes.is_empty() {
            return Err(Error::InvalidQuery("at least one node is required".into()));
        }
        let query = Query::builder(name, start, end)
            .market_run(run)
            .nodes(nodes.iter().copied())
            .build()?;
        self.fetch(&query).await
    }

    /// System load forecast (`SLD_FCST`) for a market run (`Dam`, `TwoDayAhead`,
    /// `SevenDayAhead`, `Rtm`), optionally restricted to one TAC area such as
    /// `"CA ISO-TAC"`.
    ///
    /// # Errors
    ///
    /// As [`Client::fetch`].
    pub async fn load_forecast(
        &self,
        run: MarketRun,
        tac_area: Option<&str>,
        start: DateTime<Utc>,
        end: DateTime<Utc>,
    ) -> Result<Vec<LoadForecastRow>, Error> {
        let mut builder = Query::builder(QueryName::SldFcst, start, end);
        builder.market_run(run);
        if let Some(area) = tac_area {
            builder.param("tac_area_name", area);
        }
        self.fetch(&builder.build()?).await
    }

    /// System load and resource schedules (`ENE_SLRS`): totals of generation,
    /// imports and exports, optionally restricted to one TAC zone.
    ///
    /// # Errors
    ///
    /// As [`Client::fetch`].
    pub async fn system_schedules(
        &self,
        run: MarketRun,
        tac_zone: Option<&str>,
        start: DateTime<Utc>,
        end: DateTime<Utc>,
    ) -> Result<Vec<SystemScheduleRow>, Error> {
        let mut builder = Query::builder(QueryName::EneSlrs, start, end);
        builder.market_run(run);
        if let Some(zone) = tac_zone {
            builder.param("tac_zone_name", zone);
        }
        self.fetch(&builder.build()?).await
    }

    /// Ancillary service clearing prices (`PRC_AS`).
    ///
    /// `region` is an AS region such as `AS_CAISO_EXP`, or `"ALL"`. When either
    /// the type or the region is "ALL", OASIS redirects to a whole-day group
    /// file; this method then requests one Pacific day at a time and filters the
    /// rows back to `[start, end)`.
    ///
    /// # Errors
    ///
    /// As [`Client::fetch`].
    pub async fn ancillary_prices(
        &self,
        run: MarketRun,
        anc_type: AncillaryType,
        region: &str,
        start: DateTime<Utc>,
        end: DateTime<Utc>,
    ) -> Result<Vec<AncillaryPriceRow>, Error> {
        let grouped = anc_type == AncillaryType::All || region.eq_ignore_ascii_case("ALL");
        let build = |from, to| {
            Query::builder(QueryName::PrcAs, from, to)
                .market_run(run.clone())
                .param("anc_type", anc_type.code())
                .param("anc_region", region)
                .build()
        };
        if !grouped {
            return self.fetch(&build(start, end)?).await;
        }
        let mut rows = Vec::new();
        for (day_start, day_end) in pacific_days(start, end) {
            match self
                .fetch::<AncillaryPriceRow>(&build(day_start, day_end)?)
                .await
            {
                Ok(part) => rows.extend(part),
                Err(e) if e.is_no_data() => {}
                Err(e) => return Err(e),
            }
        }
        rows.retain(|r| r.interval_start >= start && r.interval_start < end);
        Ok(rows)
    }

    /// The pricing node list (`ATL_PNODE`) in force at `as_of`.
    ///
    /// The response is large (about 3 MB zipped CSV).
    ///
    /// # Errors
    ///
    /// As [`Client::fetch`].
    pub async fn pnodes(&self, as_of: DateTime<Utc>) -> Result<Vec<NodeRow>, Error> {
        self.node_list(QueryName::AtlPnode, as_of).await
    }

    /// The aggregate pricing node list (`ATL_APNODE`: hubs, load zones, ...) in force at `as_of`.
    ///
    /// # Errors
    ///
    /// As [`Client::fetch`].
    pub async fn apnodes(&self, as_of: DateTime<Utc>) -> Result<Vec<NodeRow>, Error> {
        self.node_list(QueryName::AtlApnode, as_of).await
    }

    async fn node_list(
        &self,
        name: QueryName,
        as_of: DateTime<Utc>,
    ) -> Result<Vec<NodeRow>, Error> {
        let end = as_of + TimeDelta::hours(1);
        self.fetch(&Query::builder(name, as_of, end).build()?).await
    }

    // ----- Today's Outlook -----------------------------------------------------

    /// The live Today's Outlook feed for the current Pacific day.
    ///
    /// The row dates come from the file's `Last-Modified` header, so a fetch
    /// just after Pacific midnight is dated correctly.
    ///
    /// # Errors
    ///
    /// Transport, status and CSV decoding errors.
    pub async fn outlook(&self, dataset: OutlookDataset) -> Result<OutlookSeries, Error> {
        let url = format!(
            "{}/current/{}.csv",
            self.inner.outlook_base_url,
            dataset.file_stem()
        );
        self.request(&url, &[], Pacing::Unpaced, |body| {
            let date = pacific_date(body.last_modified.unwrap_or_else(Utc::now));
            outlook_series(dataset, date, &body.bytes)
        })
        .await
    }

    /// An archived Today's Outlook feed for a past Pacific day.
    ///
    /// # Errors
    ///
    /// As [`Client::outlook`]; a missing day is a 404 [`Error::Status`].
    pub async fn outlook_on(
        &self,
        dataset: OutlookDataset,
        date: NaiveDate,
    ) -> Result<OutlookSeries, Error> {
        let url = format!(
            "{}/history/{}/{}.csv",
            self.inner.outlook_base_url,
            date.format("%Y%m%d"),
            dataset.file_stem()
        );
        self.request(&url, &[], Pacing::Unpaced, |body| {
            outlook_series(dataset, date, &body.bytes)
        })
        .await
    }

    // ----- transport -----------------------------------------------------------

    /// Sends a GET with pacing and retries, then decodes the body.
    ///
    /// A retryable failure (transport error, 429, 5xx, non-zip body) is retried
    /// after a jittered exponential backoff, or the server's `Retry-After` if
    /// longer. For paced requests the backoff pushes the shared pacer out, so
    /// every later request from this client honours it too. No lock is held
    /// while waiting.
    async fn request<T, D>(
        &self,
        url: &str,
        params: &[(String, String)],
        pacing: Pacing,
        decode: D,
    ) -> Result<T, Error>
    where
        D: Fn(&Body) -> Result<T, Error>,
    {
        let policy = self.inner.retry;
        let mut attempt = 0;
        loop {
            if matches!(pacing, Pacing::Oasis) {
                self.inner.pacer.wait().await;
            }
            let outcome = match self.get(url, params).await {
                Ok(body) => decode(&body),
                Err(e) => Err(e),
            };
            match outcome {
                Err(e) if e.is_retryable() && attempt + 1 < policy.max_attempts => {
                    let backoff = policy.delay(attempt);
                    let wait = e.retry_after().map_or(backoff, |hint| hint.max(backoff));
                    match pacing {
                        Pacing::Oasis => self.inner.pacer.defer(wait),
                        Pacing::Unpaced => tokio::time::sleep(wait).await,
                    }
                    attempt += 1;
                }
                other => return other,
            }
        }
    }

    async fn get(&self, url: &str, params: &[(String, String)]) -> Result<Body, Error> {
        let mut response = self.inner.http.get(url).query(params).send().await?;
        let status = response.status();
        let retry_after = response
            .headers()
            .get(reqwest::header::RETRY_AFTER)
            .and_then(|v| v.to_str().ok())
            .and_then(|v| v.trim().parse::<u64>().ok())
            .map(Duration::from_secs);
        let last_modified = response
            .headers()
            .get(reqwest::header::LAST_MODIFIED)
            .and_then(|v| v.to_str().ok())
            .and_then(|v| DateTime::parse_from_rfc2822(v).ok())
            .map(|t| t.with_timezone(&Utc));

        let limit = self.inner.max_response_bytes;
        let mut bytes = Vec::new();
        while let Some(chunk) = response.chunk().await? {
            if (bytes.len() as u64).saturating_add(chunk.len() as u64) > limit {
                return Err(Error::ResponseTooLarge { limit });
            }
            bytes.extend_from_slice(&chunk);
        }

        if status.is_success() {
            Ok(Body {
                bytes,
                last_modified,
            })
        } else {
            Err(Error::Status {
                status: status.as_u16(),
                body: String::from_utf8_lossy(&bytes[..bytes.len().min(200)])
                    .trim()
                    .to_owned(),
                retry_after,
            })
        }
    }
}

fn outlook_series(
    dataset: OutlookDataset,
    date: NaiveDate,
    bytes: &[u8],
) -> Result<OutlookSeries, Error> {
    // A 200 that is really an HTML error or maintenance page is worth retrying.
    if bytes.iter().find(|b| !b.is_ascii_whitespace()) == Some(&b'<') {
        return Err(Error::UnexpectedBody(
            String::from_utf8_lossy(&bytes[..bytes.len().min(200)]).into_owned(),
        ));
    }
    parse_outlook_csv(dataset, date, bytes)
}

fn pacific_date(instant: DateTime<Utc>) -> NaiveDate {
    instant.with_timezone(&Los_Angeles).date_naive()
}

/// Daily windows starting at Pacific midnight that together cover `[start, end)`.
fn pacific_days(start: DateTime<Utc>, end: DateTime<Utc>) -> Vec<(DateTime<Utc>, DateTime<Utc>)> {
    use chrono::TimeZone;
    let midnight_utc = |day: NaiveDate| {
        day.and_hms_opt(0, 0, 0)
            .and_then(|local| Los_Angeles.from_local_datetime(&local).earliest())
            .map(|t| t.with_timezone(&Utc))
    };
    std::iter::successors(Some(pacific_date(start)), |d| {
        d.checked_add_days(Days::new(1))
    })
    .map_while(|day| {
        let from = midnight_utc(day)?;
        let to = midnight_utc(day.checked_add_days(Days::new(1))?)?;
        Some((from, to))
    })
    .take_while(|(from, _)| *from < end)
    .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::TimeZone;

    fn at(m: u32, d: u32, h: u32) -> DateTime<Utc> {
        Utc.with_ymd_and_hms(2026, m, d, h, 0, 0)
            .single()
            .unwrap_or_default()
    }

    #[test]
    fn pacific_days_cover_the_range_contiguously() {
        // 2026-11-10 12:00Z is 04:00 PST on the 10th; 2026-11-12 09:00Z is 01:00 PST on the 12th.
        let days = pacific_days(at(11, 10, 12), at(11, 12, 9));
        assert_eq!(days.len(), 3);
        assert_eq!(
            days[0].0,
            at(11, 10, 8),
            "Pacific midnight is 08:00Z in November"
        );
        assert!(days.windows(2).all(|w| w[0].1 == w[1].0));
        assert!(days[0].0 <= at(11, 10, 12) && days[2].1 >= at(11, 12, 9));
    }

    #[test]
    fn builder_rejects_bad_config() {
        assert!(matches!(
            Client::builder().chunk_span(TimeDelta::days(40)).build(),
            Err(Error::Config(_))
        ));
        assert!(matches!(
            Client::builder().chunk_span(TimeDelta::zero()).build(),
            Err(Error::Config(_))
        ));
        let none = RetryPolicy {
            max_attempts: 0,
            ..RetryPolicy::default()
        };
        assert!(matches!(
            Client::builder().retry(none).build(),
            Err(Error::Config(_))
        ));
        assert!(Client::new().is_ok());
    }
}

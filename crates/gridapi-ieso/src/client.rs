//! The async HTTP client.

use std::time::Duration;

use chrono::NaiveDate;

use crate::data::adequacy::{parse_adequacy, Adequacy};
use crate::data::demand::{parse_demand, DemandReport};
use crate::data::generation::{
    parse_output_by_fuel_hourly, parse_output_capability, FuelOutputYear, GeneratorOutputCapability,
};
use crate::data::intertie::{parse_intertie_schedule_flow, IntertieScheduleFlow};
use crate::data::lmp::{parse_hourly_lmp, parse_interval_lmp, HourlyLmp, IntervalLmp, LmpFile};
use crate::data::zonal::{
    parse_ontario_zonal_hourly, parse_ontario_zonal_realtime, parse_virtual_zonal_realtime,
    OntarioZonalHourly, OntarioZonalRealtime, VirtualZonalRealtime,
};
use crate::data::Document;
use crate::error::{Error, Result};
use crate::index::{
    latest_version, parse_directory_index, parse_file_index, ReportDirectory, ReportFile,
};
use crate::report::{FileSpec, Report, Stamp, Version};
use crate::retry::{is_retryable_status, parse_retry_after, RetryPolicy};

/// The public IESO reports root.
pub const DEFAULT_BASE_URL: &str = "https://reports-public.ieso.ca/public";

/// `User-Agent` sent unless overridden: crate name and version.
pub const DEFAULT_USER_AGENT: &str = concat!("gridapi-ieso/", env!("CARGO_PKG_VERSION"));

/// A fetched file, undecoded.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Fetched {
    /// The URL that was read.
    pub url: String,
    /// The `_vN` revision that was read, or `None` for the alias file.
    pub revision: Option<u32>,
    /// The `Last-Modified` response header, verbatim.
    pub last_modified: Option<String>,
    /// The body as UTF-8 text with any byte-order mark removed.
    pub body: String,
}

/// Builder for [`Client`]. No global state: every client owns its settings.
#[derive(Debug, Clone)]
pub struct ClientBuilder {
    base_url: String,
    user_agent: String,
    timeout: Duration,
    connect_timeout: Duration,
    retry: RetryPolicy,
    version: Version,
}

impl Default for ClientBuilder {
    fn default() -> Self {
        Self {
            base_url: DEFAULT_BASE_URL.to_owned(),
            user_agent: DEFAULT_USER_AGENT.to_owned(),
            timeout: Duration::from_secs(120),
            connect_timeout: Duration::from_secs(15),
            retry: RetryPolicy::default(),
            version: Version::Newest,
        }
    }
}

impl ClientBuilder {
    /// Override the reports root (a mirror, a test server). A trailing slash is ignored.
    #[must_use]
    pub fn base_url(mut self, base_url: impl Into<String>) -> Self {
        let mut url: String = base_url.into();
        url.truncate(url.trim_end_matches('/').len());
        self.base_url = url;
        self
    }

    /// Override the `User-Agent` header.
    #[must_use]
    pub fn user_agent(mut self, user_agent: impl Into<String>) -> Self {
        self.user_agent = user_agent.into();
        self
    }

    /// Whole-request timeout (the yearly reports are several MB).
    #[must_use]
    pub fn timeout(mut self, timeout: Duration) -> Self {
        self.timeout = timeout;
        self
    }

    /// TCP/TLS connect timeout.
    #[must_use]
    pub fn connect_timeout(mut self, timeout: Duration) -> Self {
        self.connect_timeout = timeout;
        self
    }

    /// Retry policy for `429`/`5xx`/transport failures.
    #[must_use]
    pub fn retry(mut self, retry: RetryPolicy) -> Self {
        self.retry = retry;
        self
    }

    /// Default revision policy used by the typed methods.
    #[must_use]
    pub fn version(mut self, version: Version) -> Self {
        self.version = version;
        self
    }

    /// Build the client.
    pub fn build(self) -> Result<Client> {
        let http = reqwest::Client::builder()
            .user_agent(self.user_agent)
            .timeout(self.timeout)
            .connect_timeout(self.connect_timeout)
            .build()
            .map_err(Error::ClientBuild)?;
        Ok(Client {
            http,
            base_url: self.base_url,
            retry: self.retry,
            version: self.version,
        })
    }
}

/// Client for the IESO public reports site. Cheap to clone.
#[derive(Debug, Clone)]
pub struct Client {
    http: reqwest::Client,
    base_url: String,
    retry: RetryPolicy,
    version: Version,
}

enum Attempt {
    Done(Result<Fetched>),
    Retry {
        after: Option<Duration>,
        exhausted: Error,
    },
}

impl Client {
    /// A client with default settings.
    pub fn new() -> Result<Self> {
        ClientBuilder::default().build()
    }

    /// Start configuring a client.
    pub fn builder() -> ClientBuilder {
        ClientBuilder::default()
    }

    /// The reports root this client reads from.
    pub fn base_url(&self) -> &str {
        &self.base_url
    }

    // ---- raw access ------------------------------------------------------

    /// List every report directory on the site.
    pub async fn list_reports(&self) -> Result<Vec<ReportDirectory>> {
        let page = self.get_text(&format!("{}/", self.base_url), None).await?;
        Ok(parse_directory_index(&page.body))
    }

    /// List the files of one report directory (can be several MB of HTML for
    /// the five-minute reports).
    pub async fn list_files(&self, report: &str) -> Result<Vec<ReportFile>> {
        let page = self
            .get_text(&format!("{}/{report}/", self.base_url), None)
            .await?;
        Ok(parse_file_index(&page.body))
    }

    /// Fetch one file as text, applying the spec's [`Version`] policy.
    pub async fn fetch(&self, spec: &FileSpec) -> Result<Fetched> {
        let revision = match spec.version {
            Version::Newest => None,
            Version::Exact(n) => Some(n),
            Version::HighestListed => Some(self.highest_listed(spec).await?),
        };
        self.get_text(&spec.url(&self.base_url, revision), revision)
            .await
    }

    /// Fetch and decode with an arbitrary decoder.
    pub async fn load<T>(&self, spec: &FileSpec, decode: fn(&str) -> Result<T>) -> Result<T> {
        decode(&self.fetch(spec).await?.body)
    }

    async fn highest_listed(&self, spec: &FileSpec) -> Result<u32> {
        let stamp = spec.stamp.render();
        let files = self.list_files(&spec.report).await?;
        latest_version(&files, &spec.report, stamp.as_deref(), &spec.extension)
            .and_then(|f| f.version)
            .ok_or_else(|| Error::NoVersions {
                report: spec.report.clone(),
                stamp: stamp.unwrap_or_else(|| "latest".to_owned()),
            })
    }

    fn spec(&self, report: Report, stamp: Stamp) -> FileSpec {
        FileSpec::of(report, stamp).version(self.version)
    }

    // ---- zonal prices ----------------------------------------------------

    /// Day-ahead hourly Ontario zonal price for `date`.
    pub async fn da_ontario_zonal_prices(
        &self,
        date: NaiveDate,
    ) -> Result<Document<OntarioZonalHourly>> {
        let spec = self.spec(Report::DaHourlyOntarioZonalPrice, Stamp::Day(date));
        self.load(&spec, parse_ontario_zonal_hourly).await
    }

    /// Pre-dispatch hourly Ontario zonal price for `date`.
    pub async fn predispatch_ontario_zonal_prices(
        &self,
        date: NaiveDate,
    ) -> Result<Document<OntarioZonalHourly>> {
        let spec = self.spec(Report::PredispHourlyOntarioZonalPrice, Stamp::Day(date));
        self.load(&spec, parse_ontario_zonal_hourly).await
    }

    /// Real-time five-minute Ontario zonal prices for one delivery hour.
    pub async fn realtime_ontario_zonal_prices(
        &self,
        date: NaiveDate,
        hour: u8,
    ) -> Result<Document<OntarioZonalRealtime>> {
        let spec = self.spec(Report::RealtimeOntarioZonalPrice, Stamp::Hour(date, hour));
        self.load(&spec, parse_ontario_zonal_realtime).await
    }

    /// The most recently published real-time Ontario zonal prices.
    pub async fn realtime_ontario_zonal_prices_current(
        &self,
    ) -> Result<Document<OntarioZonalRealtime>> {
        let spec = self.spec(Report::RealtimeOntarioZonalPrice, Stamp::Current);
        self.load(&spec, parse_ontario_zonal_realtime).await
    }

    /// Real-time five-minute virtual zonal prices for one delivery hour.
    pub async fn realtime_virtual_zonal_prices(
        &self,
        date: NaiveDate,
        hour: u8,
    ) -> Result<Document<VirtualZonalRealtime>> {
        let spec = self.spec(Report::RealtimeZonalEnergyPrices, Stamp::Hour(date, hour));
        self.load(&spec, parse_virtual_zonal_realtime).await
    }

    /// The most recently published real-time virtual zonal prices.
    pub async fn realtime_virtual_zonal_prices_current(
        &self,
    ) -> Result<Document<VirtualZonalRealtime>> {
        let spec = self.spec(Report::RealtimeZonalEnergyPrices, Stamp::Current);
        self.load(&spec, parse_virtual_zonal_realtime).await
    }

    // ---- nodal LMP -------------------------------------------------------

    /// Day-ahead hourly energy LMP for every node on `date`.
    pub async fn da_lmp(&self, date: NaiveDate) -> Result<LmpFile<HourlyLmp>> {
        let spec = self.spec(Report::DaHourlyEnergyLmp, Stamp::Day(date));
        self.load(&spec, parse_hourly_lmp).await
    }

    /// Pre-dispatch hourly energy LMP for every node on `date`.
    pub async fn predispatch_lmp(&self, date: NaiveDate) -> Result<LmpFile<HourlyLmp>> {
        let spec = self.spec(Report::PredispHourlyEnergyLmp, Stamp::Day(date));
        self.load(&spec, parse_hourly_lmp).await
    }

    /// Real-time five-minute energy LMP for every node in one delivery hour.
    pub async fn realtime_lmp(&self, date: NaiveDate, hour: u8) -> Result<LmpFile<IntervalLmp>> {
        let spec = self.spec(Report::RealtimeEnergyLmp, Stamp::Hour(date, hour));
        self.load(&spec, parse_interval_lmp).await
    }

    /// The most recently published real-time energy LMP file.
    pub async fn realtime_lmp_current(&self) -> Result<LmpFile<IntervalLmp>> {
        let spec = self.spec(Report::RealtimeEnergyLmp, Stamp::Current);
        self.load(&spec, parse_interval_lmp).await
    }

    // ---- demand, generation, interties, adequacy -------------------------

    /// Hourly market and Ontario demand for a calendar year.
    pub async fn demand(&self, year: i32) -> Result<DemandReport> {
        let spec = self.spec(Report::Demand, Stamp::Year(year));
        self.load(&spec, parse_demand).await
    }

    /// Per-generator output, capability and available capacity for `date`.
    pub async fn generator_output_capability(
        &self,
        date: NaiveDate,
    ) -> Result<Document<GeneratorOutputCapability>> {
        let spec = self.spec(Report::GenOutputCapability, Stamp::Day(date));
        self.load(&spec, parse_output_capability).await
    }

    /// Hourly output by fuel type for a calendar year.
    pub async fn generation_by_fuel(&self, year: i32) -> Result<Document<FuelOutputYear>> {
        let spec = self.spec(Report::GenOutputByFuelHourly, Stamp::Year(year));
        self.load(&spec, parse_output_by_fuel_hourly).await
    }

    /// Intertie schedules and flows for `date`.
    pub async fn intertie_schedule_flow(
        &self,
        date: NaiveDate,
    ) -> Result<Document<IntertieScheduleFlow>> {
        let spec = self.spec(Report::IntertieScheduleFlow, Stamp::Day(date));
        self.load(&spec, parse_intertie_schedule_flow).await
    }

    /// The adequacy outlook for `date`.
    pub async fn adequacy(&self, date: NaiveDate) -> Result<Document<Adequacy>> {
        let spec = self.spec(Report::Adequacy3, Stamp::Day(date));
        self.load(&spec, parse_adequacy).await
    }

    // ---- transport -------------------------------------------------------

    async fn get_text(&self, url: &str, revision: Option<u32>) -> Result<Fetched> {
        let mut retry = 0u32;
        loop {
            match self.attempt(url, revision).await {
                Attempt::Done(result) => return result,
                Attempt::Retry { after, exhausted } => {
                    if retry + 1 >= self.retry.max_attempts {
                        return Err(exhausted);
                    }
                    let wait = self.retry.delay(retry, rand::random::<f64>(), after);
                    tracing::warn!(
                        url,
                        attempt = retry + 1,
                        wait_ms = u64::try_from(wait.as_millis()).unwrap_or(u64::MAX),
                        "retrying IESO request"
                    );
                    tokio::time::sleep(wait).await;
                    retry += 1;
                }
            }
        }
    }

    async fn attempt(&self, url: &str, revision: Option<u32>) -> Attempt {
        let transport = |source| Error::Transport {
            url: url.to_owned(),
            source,
        };
        let response = match self.http.get(url).send().await {
            Ok(r) => r,
            Err(e) if is_transient(&e) => {
                return Attempt::Retry {
                    after: None,
                    exhausted: transport(e),
                }
            }
            Err(e) => return Attempt::Done(Err(transport(e))),
        };
        let status = response.status();
        let after = response
            .headers()
            .get(reqwest::header::RETRY_AFTER)
            .and_then(|v| v.to_str().ok())
            .and_then(parse_retry_after);
        let last_modified = response
            .headers()
            .get(reqwest::header::LAST_MODIFIED)
            .and_then(|v| v.to_str().ok())
            .map(str::to_owned);
        if status.as_u16() == 404 {
            return Attempt::Done(Err(Error::NotFound {
                url: url.to_owned(),
            }));
        }
        if !status.is_success() {
            let body = response.text().await.unwrap_or_default();
            let error = Error::Status {
                url: url.to_owned(),
                status: status.as_u16(),
                body: body.chars().take(300).collect(),
            };
            return if is_retryable_status(status.as_u16()) {
                Attempt::Retry {
                    after,
                    exhausted: error,
                }
            } else {
                Attempt::Done(Err(error))
            };
        }
        match response.text().await {
            Ok(text) => {
                tracing::debug!(url, bytes = text.len(), "fetched IESO report");
                Attempt::Done(Ok(Fetched {
                    url: url.to_owned(),
                    revision,
                    last_modified,
                    body: text.trim_start_matches('\u{feff}').to_owned(),
                }))
            }
            Err(e) if is_transient(&e) => Attempt::Retry {
                after: None,
                exhausted: transport(e),
            },
            Err(e) => Attempt::Done(Err(transport(e))),
        }
    }
}

fn is_transient(e: &reqwest::Error) -> bool {
    e.is_timeout() || e.is_connect() || e.is_body()
}

//! The async HTTP client.

use std::fmt;
use std::time::Duration;

use chrono::{Duration as ChronoDuration, NaiveDate};
use reqwest::header::{HeaderMap, HeaderName, HeaderValue};

use crate::decode;
use crate::error::{Error, Result};
use crate::model::assets::{Asset, PoolParticipant};
use crate::model::csd::{GenerationAssets, SupplyDemandSummary};
use crate::model::interchange::{InterchangeReport, IntertieOutage};
use crate::model::market::{MeritOrderHour, MeteredParticipant, ReserveHour, UnitCommitment};
use crate::model::price::{
    CurrentSystemMarginalPrice, InternalLoad, PoolPrice, SystemMarginalPrice,
};
use crate::model::supply::{FuelCapacity, LoadOutageHour};
use crate::query::{AssetListQuery, DateRange, InterchangeQuery, MeteredVolumeQuery, OutageQuery};
use crate::retry::{is_retryable_status, parse_retry_after, RetryPolicy};

/// The AESO public API gateway.
pub const DEFAULT_BASE_URL: &str = "https://apimgw.aeso.ca/public";

/// `User-Agent` sent unless overridden: crate name and version.
pub const DEFAULT_USER_AGENT: &str = concat!("gridapi-aeso/", env!("CARGO_PKG_VERSION"));

/// Environment variable read by [`Client::from_env`].
pub const API_KEY_ENV: &str = "AESO_API_KEY";

/// The energy merit order is published with this many days of delay.
pub const MERIT_ORDER_LAG_DAYS: i64 = 60;

/// Most days the pool price API returns per request.
pub const POOL_PRICE_MAX_DAYS: i64 = 366;

/// Most days the AIES generation capacity API returns per request.
pub const AIES_CAPACITY_MAX_DAYS: i64 = 31;

/// Builder for [`Client`]. No global state: every client owns its settings.
#[derive(Clone)]
pub struct ClientBuilder {
    base_url: String,
    api_key: Option<String>,
    user_agent: String,
    timeout: Duration,
    connect_timeout: Duration,
    retry: RetryPolicy,
}

impl fmt::Debug for ClientBuilder {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("ClientBuilder")
            .field("base_url", &self.base_url)
            .field("api_key", &self.api_key.as_ref().map(|_| "<redacted>"))
            .field("user_agent", &self.user_agent)
            .finish_non_exhaustive()
    }
}

impl Default for ClientBuilder {
    fn default() -> Self {
        Self {
            base_url: DEFAULT_BASE_URL.to_owned(),
            api_key: None,
            user_agent: DEFAULT_USER_AGENT.to_owned(),
            timeout: Duration::from_secs(120),
            connect_timeout: Duration::from_secs(15),
            retry: RetryPolicy::default(),
        }
    }
}

impl ClientBuilder {
    /// The `API-KEY` from your AESO developer portal subscription.
    #[must_use]
    pub fn api_key(mut self, key: impl Into<String>) -> Self {
        self.api_key = Some(key.into().trim().to_owned());
        self
    }

    /// Override the gateway root (a mirror, a test server). A trailing slash is ignored.
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

    /// Whole-request timeout.
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

    /// Build the client. Fails with [`Error::MissingApiKey`] when no key was set.
    pub fn build(self) -> Result<Client> {
        let key = self
            .api_key
            .filter(|k| !k.is_empty())
            .ok_or(Error::MissingApiKey)?;
        let mut value = HeaderValue::from_str(&key).map_err(|_| Error::InvalidApiKey)?;
        value.set_sensitive(true);
        let mut headers = HeaderMap::new();
        headers.insert(HeaderName::from_static("api-key"), value);
        let http = reqwest::Client::builder()
            .user_agent(self.user_agent)
            .default_headers(headers)
            .timeout(self.timeout)
            .connect_timeout(self.connect_timeout)
            .build()
            .map_err(Error::ClientBuild)?;
        Ok(Client {
            http,
            base_url: self.base_url,
            retry: self.retry,
        })
    }
}

/// Client for the AESO public API. Cheap to clone.
#[derive(Clone)]
pub struct Client {
    http: reqwest::Client,
    base_url: String,
    retry: RetryPolicy,
}

impl fmt::Debug for Client {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Client")
            .field("base_url", &self.base_url)
            .field("api_key", &"<redacted>")
            .finish_non_exhaustive()
    }
}

enum Attempt {
    Done(Result<String>),
    Retry {
        after: Option<Duration>,
        exhausted: Error,
    },
}

impl Client {
    /// Start configuring a client.
    pub fn builder() -> ClientBuilder {
        ClientBuilder::default()
    }

    /// A client with default settings, reading the key from `AESO_API_KEY`.
    pub fn from_env() -> Result<Self> {
        let key = std::env::var(API_KEY_ENV).map_err(|_| Error::MissingApiKey)?;
        Self::builder().api_key(key).build()
    }

    /// The gateway root this client calls.
    pub fn base_url(&self) -> &str {
        &self.base_url
    }

    // ---- prices ----------------------------------------------------------

    /// Hourly pool price (CAD/MWh). At most 366 days per request; see
    /// [`DateRange::chunks`] and [`POOL_PRICE_MAX_DAYS`] for longer spans.
    /// Hours that have not settled have `pool_price == None` but may carry a forecast.
    pub async fn pool_price(&self, range: DateRange) -> Result<Vec<PoolPrice>> {
        let body = self
            .get("poolprice-api/v1.1/price/poolPrice", &range.iso_params())
            .await?;
        decode::pool_price(&body)
    }

    /// System marginal price intervals (CAD/MWh).
    pub async fn system_marginal_price(
        &self,
        range: DateRange,
    ) -> Result<Vec<SystemMarginalPrice>> {
        let body = self
            .get(
                "systemmarginalprice-api/v1.1/price/systemMarginalPrice",
                &range.iso_params(),
            )
            .await?;
        decode::system_marginal_price(&body)
    }

    /// The system marginal price in force right now.
    pub async fn system_marginal_price_current(&self) -> Result<Vec<CurrentSystemMarginalPrice>> {
        let body = self
            .get(
                "systemmarginalprice-api/v1.1/price/systemMarginalPrice/current",
                &[],
            )
            .await?;
        decode::current_system_marginal_price(&body)
    }

    // ---- load and supply -------------------------------------------------

    /// Hourly Alberta internal load: actual and forecast (MW).
    pub async fn alberta_internal_load(&self, range: DateRange) -> Result<Vec<InternalLoad>> {
        let body = self
            .get(
                "actualforecast-api/v1/load/albertaInternalLoad",
                &range.iso_params(),
            )
            .await?;
        decode::internal_load(&body)
    }

    /// The live supply and demand summary: generation by fuel, load, reserves
    /// and interchange by path.
    pub async fn current_supply_demand(&self) -> Result<SupplyDemandSummary> {
        let body = self
            .get("currentsupplydemand-api/v2/csd/summary/current", &[])
            .await?;
        decode::supply_demand_summary(&body)
    }

    /// Asset-level current generation (maximum capability and net output).
    pub async fn generation_assets_current(&self) -> Result<GenerationAssets> {
        let body = self
            .get(
                "currentsupplydemand-api/v1/csd/generation/assets/current",
                &[],
            )
            .await?;
        decode::generation_assets(&body)
    }

    /// Generation capacity and outages by fuel group. At most 31 days per request
    /// ([`AIES_CAPACITY_MAX_DAYS`]); data runs from 2011 to 24 months ahead.
    pub async fn aies_gen_capacity(&self, range: DateRange) -> Result<Vec<FuelCapacity>> {
        let body = self
            .get(
                "aiesgencapacity-api/v1/AIESGenCapacity",
                &range.iso_params(),
            )
            .await?;
        decode::aies_gen_capacity(&body)
    }

    /// Hourly forecast load outages (MW), from 2013-09-22 to 24 months ahead.
    pub async fn load_outage_forecast(&self, range: DateRange) -> Result<Vec<LoadOutageHour>> {
        let body = self
            .get(
                "loadoutageforecast-api/v1/loadOutageReport",
                &range.iso_params(),
            )
            .await?;
        decode::load_outage_forecast(&body)
    }

    // ---- interties -------------------------------------------------------

    /// Intertie and flowgate transfer capability (ATC, TTC, TRM, gross offer).
    pub async fn interchange_capability(
        &self,
        query: &InterchangeQuery,
    ) -> Result<InterchangeReport> {
        let body = self.get("itc/v1/interchange", &query.params()).await?;
        decode::interchange(&body)
    }

    /// Outages affecting intertie transfer capability.
    pub async fn intertie_outages(&self, query: &OutageQuery) -> Result<Vec<IntertieOutage>> {
        let body = self.get("itc/v1/outage", &query.params()).await?;
        decode::intertie_outages(&body)
    }

    // ---- market microdata ------------------------------------------------

    /// The energy merit order for `date` (hourly snapshots of every offer
    /// block). AESO publishes it 60 days late, so a date inside that window
    /// fails with an [`Error::Api`] for which [`Error::is_no_data`] is true;
    /// see [`merit_order_latest_date`].
    pub async fn energy_merit_order(&self, date: NaiveDate) -> Result<Vec<MeritOrderHour>> {
        let body = self
            .get(
                "energymeritorder-api/v1/meritOrder/energy",
                &DateRange::day(date).iso_params(),
            )
            .await?;
        decode::merit_order(&body)
    }

    /// Operating reserve offer control data for `date` (also 60 days late).
    pub async fn operating_reserve_offers(&self, date: NaiveDate) -> Result<Vec<ReserveHour>> {
        let body = self
            .get(
                "operatingreserveoffercontrol-api/v1/operatingReserveOfferControl",
                &DateRange::day(date).iso_params(),
            )
            .await?;
        decode::operating_reserve_offers(&body)
    }

    /// Settled metered volumes per participant and asset.
    pub async fn metered_volume(
        &self,
        query: &MeteredVolumeQuery,
    ) -> Result<Vec<MeteredParticipant>> {
        let body = self
            .get(
                "meteredvolume-api/v1/meteredvolume/details",
                &query.params(),
            )
            .await?;
        decode::metered_volume(&body)
    }

    /// Unit commitment directives issued in `range` (data from 2024-07-01).
    pub async fn unit_commitment(&self, range: DateRange) -> Result<Vec<UnitCommitment>> {
        let body = self
            .get(
                "unitcommitmentdata-api/v2/unitCommitment",
                &range.iso_params(),
            )
            .await?;
        decode::unit_commitment(&body)
    }

    // ---- reference data --------------------------------------------------

    /// Pool assets, optionally filtered.
    pub async fn asset_list(&self, query: &AssetListQuery) -> Result<Vec<Asset>> {
        let body = self
            .get("assetlist-api/v1/assetlist", &query.params())
            .await?;
        decode::asset_list(&body)
    }

    /// Every registered pool participant.
    pub async fn pool_participants(&self) -> Result<Vec<PoolParticipant>> {
        let body = self
            .get("poolparticipant-api/v1/poolparticipantlist", &[])
            .await?;
        decode::pool_participants(&body)
    }

    // ---- raw access ------------------------------------------------------

    /// GET any path under the gateway root and return the response text. Use
    /// this for endpoints without a typed method.
    pub async fn get_text(&self, path: &str, params: &[(&str, String)]) -> Result<String> {
        self.get(path, params).await
    }

    /// GET any path under the gateway root and parse the body as JSON.
    pub async fn get_json(
        &self,
        path: &str,
        params: &[(&str, String)],
    ) -> Result<serde_json::Value> {
        let body = self.get(path, params).await?;
        serde_json::from_str(&body).map_err(|source| Error::Decode {
            endpoint: path.to_owned(),
            source,
        })
    }

    // ---- transport -------------------------------------------------------

    async fn get(&self, path: &str, params: &[(&str, String)]) -> Result<String> {
        let url = format!("{}/{}", self.base_url, path.trim_start_matches('/'));
        let mut retry = 0u32;
        loop {
            match self.attempt(&url, params).await {
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
                        "retrying AESO request"
                    );
                    tokio::time::sleep(wait).await;
                    retry += 1;
                }
            }
        }
    }

    async fn attempt(&self, url: &str, params: &[(&str, String)]) -> Attempt {
        let transport = |source| Error::Transport {
            url: url.to_owned(),
            source,
        };
        let response = match self.http.get(url).query(params).send().await {
            Ok(r) => r,
            Err(e) if is_transient(&e) => {
                return Attempt::Retry {
                    after: None,
                    exhausted: transport(e),
                }
            }
            Err(e) => return Attempt::Done(Err(transport(e))),
        };
        let status = response.status().as_u16();
        let after = response
            .headers()
            .get(reqwest::header::RETRY_AFTER)
            .and_then(|v| v.to_str().ok())
            .and_then(parse_retry_after);
        let final_url = response.url().to_string();
        let body = match response.text().await {
            Ok(b) => b,
            Err(e) if is_transient(&e) => {
                return Attempt::Retry {
                    after: None,
                    exhausted: transport(e),
                }
            }
            Err(e) => return Attempt::Done(Err(transport(e))),
        };
        match status {
            200..=299 => {
                tracing::debug!(url = %final_url, bytes = body.len(), "fetched AESO response");
                Attempt::Done(Ok(body))
            }
            404 => Attempt::Done(Err(Error::NotFound { url: final_url })),
            s => {
                let error = decode::api_error(s, &body).unwrap_or_else(|| Error::Status {
                    url: final_url,
                    status: s,
                    body: body.chars().take(300).collect(),
                });
                if is_retryable_status(s) {
                    Attempt::Retry {
                        after,
                        exhausted: error,
                    }
                } else {
                    Attempt::Done(Err(error))
                }
            }
        }
    }
}

/// The latest date whose energy merit order AESO has published as of `today`
/// (today minus [`MERIT_ORDER_LAG_DAYS`]).
pub fn merit_order_latest_date(today: NaiveDate) -> NaiveDate {
    today - ChronoDuration::days(MERIT_ORDER_LAG_DAYS)
}

fn is_transient(e: &reqwest::Error) -> bool {
    e.is_timeout() || e.is_connect() || e.is_body()
}

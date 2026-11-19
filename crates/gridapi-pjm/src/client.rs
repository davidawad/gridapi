//! The HTTP client: auth, throttling, retry and paging.

use std::sync::Arc;
#[cfg(feature = "pjm-public-web-key")]
use std::sync::RwLock;
use std::time::Duration;

use rand::Rng;
use reqwest::header::{HeaderValue, RETRY_AFTER};
use serde::de::DeserializeOwned;
use serde::Deserialize;

use crate::auth::{ApiKey, KeyMode};
use crate::catalog::{FeedMetadata, FeedSummary};
use crate::error::{snippet, BoxError, Error, FieldError};
use crate::page::{Page, Row};
#[cfg(feature = "pjm-public-web-key")]
use crate::public_key::{fetch_public_key_with, DEFAULT_SETTINGS_URL};
use crate::query::Query;
use crate::throttle::{backoff_delay, RateLimit, RetryPolicy, Throttle};

/// Production API root.
pub const DEFAULT_BASE_URL: &str = "https://api.pjm.com/api/v1";

/// The header PJM's API management reads the subscription key from.
const KEY_HEADER: &str = "Ocp-Apim-Subscription-Key";

/// Default per-request timeout. Large `rowCount` queries are slow on PJM's side.
const DEFAULT_TIMEOUT: Duration = Duration::from_secs(120);

/// Configuration for a [`Client`].
#[derive(Debug, Clone)]
pub struct ClientBuilder {
    base_url: String,
    #[cfg(feature = "pjm-public-web-key")]
    settings_url: String,
    #[cfg(feature = "pjm-public-web-key")]
    public_key_acknowledged: bool,
    user_agent: String,
    key_mode: KeyMode,
    rate_limit: Option<RateLimit>,
    retry: RetryPolicy,
    timeout: Duration,
}

impl Default for ClientBuilder {
    fn default() -> Self {
        Self {
            base_url: DEFAULT_BASE_URL.to_owned(),
            #[cfg(feature = "pjm-public-web-key")]
            settings_url: DEFAULT_SETTINGS_URL.to_owned(),
            #[cfg(feature = "pjm-public-web-key")]
            public_key_acknowledged: false,
            user_agent: concat!("gridapi-pjm/", env!("CARGO_PKG_VERSION")).to_owned(),
            key_mode: KeyMode::Auto,
            rate_limit: Some(RateLimit::NON_MEMBER),
            retry: RetryPolicy::default(),
            timeout: DEFAULT_TIMEOUT,
        }
    }
}

impl ClientBuilder {
    /// API root; use `https://api-train.pjm.com/api/v1` for PJM's training
    /// environment, or a mock server in tests.
    #[must_use]
    pub fn base_url(mut self, url: impl Into<String>) -> Self {
        self.base_url = url.into();
        self
    }

    /// Where to fetch the public web-app key from
    /// ([`DEFAULT_SETTINGS_URL`] by default). Only with the
    /// `pjm-public-web-key` feature.
    #[cfg(feature = "pjm-public-web-key")]
    #[must_use]
    pub fn settings_url(mut self, url: impl Into<String>) -> Self {
        self.settings_url = url.into();
        self
    }

    /// Acknowledge that `KeyMode::PublicWebApp` uses the subscription key
    /// PJM's own Data Miner 2 web app ships, which is not issued to you, and
    /// that you accept the risk of using it. [`build`](Self::build) refuses
    /// `KeyMode::PublicWebApp` without this call. Only with the
    /// `pjm-public-web-key` feature; prefer your own key from
    /// <https://apiportal.pjm.com/>.
    ///
    /// PJM's Data Miner 2 terms apply "whether as a guest or a registered
    /// user"; "non-members are specifically prohibited from republishing
    /// data"; you may not access the tools "more than ... six (6) times per
    /// minute for non-members", or "reverse engineer, disassemble, decompile,
    /// decode, adapt, or otherwise attempt to derive or gain access to the
    /// source of the Data or methods used to compile the Data". PJM's
    /// documented route for automated queries is a registered PJM account, and
    /// non-member accounts confirm internal business use only.
    #[cfg(feature = "pjm-public-web-key")]
    #[must_use]
    pub fn i_accept_pjm_public_key_risk(mut self) -> Self {
        self.public_key_acknowledged = true;
        self
    }

    /// The `User-Agent` header.
    #[must_use]
    pub fn user_agent(mut self, ua: impl Into<String>) -> Self {
        self.user_agent = ua.into();
        self
    }

    /// Which key to send; see [`KeyMode`].
    #[must_use]
    pub fn key_mode(mut self, mode: KeyMode) -> Self {
        self.key_mode = mode;
        self
    }

    /// Shorthand for `key_mode(KeyMode::User(key))`.
    #[must_use]
    pub fn api_key(self, key: ApiKey) -> Self {
        self.key_mode(KeyMode::User(key))
    }

    /// The request budget. Defaults to PJM's non-member limit
    /// ([`RateLimit::NON_MEMBER`], 6 per minute): raise it to
    /// [`RateLimit::MEMBER`] only with a member key.
    #[must_use]
    pub fn rate_limit(mut self, limit: RateLimit) -> Self {
        self.rate_limit = Some(limit);
        self
    }

    /// Disable client-side throttling (the server still enforces its limit).
    #[must_use]
    pub fn unthrottled(mut self) -> Self {
        self.rate_limit = None;
        self
    }

    /// Retry policy for 429, 5xx and transient transport errors.
    #[must_use]
    pub fn retry(mut self, policy: RetryPolicy) -> Self {
        self.retry = policy;
        self
    }

    /// Per-request timeout.
    #[must_use]
    pub fn timeout(mut self, timeout: Duration) -> Self {
        self.timeout = timeout;
        self
    }

    /// Build the client.
    ///
    /// # Errors
    /// [`Error::MissingKey`] when `KeyMode::Auto` finds no `PJM_API_KEY`;
    /// [`Error::InvalidKey`] when it finds an unusable one;
    /// [`Error::Transport`] when the HTTP stack cannot start. With the
    /// `pjm-public-web-key` feature, `Error::PublicKeyNotAcknowledged` when
    /// `KeyMode::PublicWebApp` was chosen without `i_accept_pjm_public_key_risk`.
    pub fn build(self) -> Result<Client, Error> {
        let mode = match self.key_mode {
            KeyMode::Auto => KeyMode::from_env()?,
            other => other,
        };
        #[cfg(feature = "pjm-public-web-key")]
        if matches!(mode, KeyMode::PublicWebApp) && !self.public_key_acknowledged {
            return Err(Error::PublicKeyNotAcknowledged);
        }
        let http = reqwest::Client::builder()
            .user_agent(&self.user_agent)
            .timeout(self.timeout)
            .gzip(true)
            .build()
            .map_err(|e| Error::Transport(Box::new(e)))?;
        Ok(Client {
            inner: Arc::new(Inner {
                http,
                base_url: self.base_url.trim_end_matches('/').to_owned(),
                #[cfg(feature = "pjm-public-web-key")]
                settings_url: self.settings_url,
                key: KeySlot::new(mode),
                throttle: Throttle::new(self.rate_limit),
                retry: self.retry,
            }),
        })
    }
}

/// An async PJM Data Miner 2 client. Cheap to clone; clones share the key
/// cache, the connection pool and the rate-limit window.
#[derive(Clone, Debug)]
pub struct Client {
    inner: Arc<Inner>,
}

#[derive(Debug)]
struct Inner {
    http: reqwest::Client,
    base_url: String,
    #[cfg(feature = "pjm-public-web-key")]
    settings_url: String,
    key: KeySlot,
    throttle: Throttle,
    retry: RetryPolicy,
}

const _: () = {
    const fn assert_send_sync<T: Send + Sync>() {}
    assert_send_sync::<Client>();
};

/// The key in use and, for the public key, the cached copy.
#[derive(Debug)]
struct KeySlot {
    user: Option<ApiKey>,
    #[cfg(feature = "pjm-public-web-key")]
    cached: RwLock<Option<ApiKey>>,
}

impl KeySlot {
    fn new(mode: KeyMode) -> Self {
        Self {
            user: match mode {
                KeyMode::User(key) => Some(key),
                _ => None,
            },
            #[cfg(feature = "pjm-public-web-key")]
            cached: RwLock::new(None),
        }
    }

    /// True when a 401 is worth one refetch of the key (public key only).
    fn refreshable(&self) -> bool {
        cfg!(feature = "pjm-public-web-key") && self.user.is_none()
    }

    #[cfg(feature = "pjm-public-web-key")]
    fn cached(&self) -> Option<ApiKey> {
        self.cached
            .read()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .clone()
    }

    #[cfg(feature = "pjm-public-web-key")]
    fn store(&self, key: ApiKey) {
        *self
            .cached
            .write()
            .unwrap_or_else(std::sync::PoisonError::into_inner) = Some(key);
    }

    /// Forget `rejected` unless another task already replaced it.
    #[cfg(feature = "pjm-public-web-key")]
    fn invalidate(&self, rejected: &ApiKey) {
        let mut guard = self
            .cached
            .write()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        if guard.as_ref() == Some(rejected) {
            *guard = None;
        }
    }

    #[cfg(not(feature = "pjm-public-web-key"))]
    #[allow(clippy::unused_self)]
    fn invalidate(&self, _rejected: &ApiKey) {}
}

/// What to do with one HTTP response.
#[derive(Debug)]
enum Outcome {
    Success(Vec<u8>),
    /// 401: the key is bad.
    KeyRejected,
    /// 429, 5xx: try again after a back-off.
    Retry {
        error: Error,
        retry_after: Option<Duration>,
    },
    /// Anything else: give up.
    Fatal(Error),
}

/// Classify a response. Pure, so the status table is unit-testable.
fn classify(status: u16, retry_after: Option<Duration>, body: Vec<u8>) -> Outcome {
    match status {
        200..=299 => Outcome::Success(body),
        401 => Outcome::KeyRejected,
        429 => Outcome::Retry {
            error: Error::RateLimited { retry_after },
            retry_after,
        },
        500..=599 => Outcome::Retry {
            error: Error::Server { status },
            retry_after,
        },
        _ => Outcome::Fatal(rejection(status, &body)),
    }
}

/// The two 4xx body shapes PJM uses.
#[derive(Deserialize)]
struct RejectionBody {
    #[serde(default)]
    errors: Vec<FieldError>,
    code: Option<String>,
    message: Option<String>,
}

fn rejection(status: u16, body: &[u8]) -> Error {
    match serde_json::from_slice::<RejectionBody>(body) {
        Ok(b) if !b.errors.is_empty() || b.code.is_some() || b.message.is_some() => {
            Error::Rejected {
                status,
                errors: b.errors,
                code: b.code,
                message: b.message,
            }
        }
        _ => Error::Status {
            status,
            snippet: snippet(body),
        },
    }
}

fn parse_retry_after(value: &HeaderValue) -> Option<Duration> {
    value
        .to_str()
        .ok()?
        .trim()
        .parse::<u64>()
        .ok()
        .map(Duration::from_secs)
}

fn is_transient(e: &reqwest::Error) -> bool {
    e.is_timeout() || e.is_connect() || e.is_request() || e.is_body()
}

fn transport(e: reqwest::Error) -> Error {
    Error::Transport(Box::new(e) as BoxError)
}

impl Client {
    /// A client with default settings: the key in `PJM_API_KEY`; the
    /// non-member rate limit; 3 retries.
    ///
    /// # Errors
    /// [`Error::MissingKey`] when `PJM_API_KEY` is unset; otherwise as
    /// [`ClientBuilder::build`].
    pub fn new() -> Result<Self, Error> {
        Self::builder().build()
    }

    /// Start configuring a client.
    #[must_use]
    pub fn builder() -> ClientBuilder {
        ClientBuilder::default()
    }

    /// The key to send now (with the `pjm-public-web-key` feature, fetching
    /// the public key on first use).
    #[cfg(feature = "pjm-public-web-key")]
    async fn current_key(&self) -> Result<ApiKey, Error> {
        let slot = &self.inner.key;
        if let Some(user) = &slot.user {
            return Ok(user.clone());
        }
        if let Some(cached) = slot.cached() {
            return Ok(cached);
        }
        let key = fetch_public_key_with(&self.inner.http, &self.inner.settings_url).await?;
        tracing::info!(settings_url = %self.inner.settings_url, "gridapi-pjm: fetched public subscription key");
        slot.store(key.clone());
        Ok(key)
    }

    /// The key to send now: always the user's own key (the public web-app key
    /// needs the `pjm-public-web-key` feature).
    #[cfg(not(feature = "pjm-public-web-key"))]
    fn current_key(&self) -> std::future::Ready<Result<ApiKey, Error>> {
        std::future::ready(self.inner.key.user.clone().ok_or(Error::MissingKey))
    }

    /// GET `url` with the key header, honouring throttle, retry and the
    /// one-shot public-key refresh on 401. Returns the body of a 2xx.
    async fn get(&self, url: &str, params: &[(String, String)]) -> Result<Vec<u8>, Error> {
        let mut retries = 0u32;
        let mut refreshed = false;
        loop {
            let key = self.current_key().await?;
            self.inner.throttle.acquire().await;
            let outcome = self.send_once(url, params, &key).await;
            match outcome {
                Outcome::Success(body) => return Ok(body),
                Outcome::KeyRejected if self.inner.key.refreshable() && !refreshed => {
                    tracing::warn!(
                        "gridapi-pjm: public key rejected (401); refetching settings.json"
                    );
                    self.inner.key.invalidate(&key);
                    refreshed = true;
                }
                Outcome::KeyRejected => return Err(Error::Unauthorized),
                Outcome::Retry { error, retry_after } if retries < self.inner.retry.max_retries => {
                    let jitter = rand::thread_rng().gen::<f64>();
                    let delay = backoff_delay(&self.inner.retry, retries, jitter, retry_after);
                    tracing::warn!(
                        attempt = retries + 1,
                        max_retries = self.inner.retry.max_retries,
                        delay_ms = delay.as_millis(),
                        error = %error,
                        "gridapi-pjm: retrying"
                    );
                    retries += 1;
                    tokio::time::sleep(delay).await;
                }
                Outcome::Retry { error, .. } | Outcome::Fatal(error) => return Err(error),
            }
        }
    }

    async fn send_once(&self, url: &str, params: &[(String, String)], key: &ApiKey) -> Outcome {
        let Ok(mut value) = HeaderValue::from_str(key.expose()) else {
            return Outcome::Fatal(Error::InvalidKey("not a valid header value"));
        };
        value.set_sensitive(true);
        let sent = self
            .inner
            .http
            .get(url)
            .query(params)
            .header(KEY_HEADER, value)
            .send()
            .await;
        let resp = match sent {
            Ok(r) => r,
            Err(e) if is_transient(&e) => {
                return Outcome::Retry {
                    error: transport(e),
                    retry_after: None,
                }
            }
            Err(e) => return Outcome::Fatal(transport(e)),
        };
        let status = resp.status().as_u16();
        let retry_after = resp.headers().get(RETRY_AFTER).and_then(parse_retry_after);
        match resp.bytes().await {
            Ok(body) => classify(status, retry_after, body.to_vec()),
            Err(e) if is_transient(&e) => Outcome::Retry {
                error: transport(e),
                retry_after: None,
            },
            Err(e) => Outcome::Fatal(transport(e)),
        }
    }

    async fn get_json<T: DeserializeOwned>(
        &self,
        url: &str,
        params: &[(String, String)],
    ) -> Result<T, Error> {
        let body = self.get(url, params).await?;
        serde_json::from_slice(&body).map_err(|source| Error::Decode {
            source,
            snippet: snippet(&body),
        })
    }

    /// The feed catalog: every feed PJM publishes through Data Miner 2.
    pub async fn feeds(&self) -> Result<Vec<FeedSummary>, Error> {
        let url = format!("{}/", self.inner.base_url);
        let page: Page<FeedSummary> = self.get_json(&url, &[]).await?;
        Ok(page.items)
    }

    /// A feed's metadata and field definitions.
    ///
    /// # Errors
    /// [`Error::InvalidQuery`] for a malformed feed name, plus the usual
    /// request errors.
    pub async fn feed_metadata(&self, feed: &str) -> Result<FeedMetadata, Error> {
        let url = format!("{}/{}/metadata", self.inner.base_url, check_feed(feed)?);
        self.get_json(&url, &[]).await
    }

    /// Run `query` against `feed` and return one page of rows decoded as `T`
    /// ([`Row`] for untyped access).
    pub async fn query<T: DeserializeOwned>(
        &self,
        feed: &str,
        query: &Query,
    ) -> Result<Page<T>, Error> {
        let url = format!("{}/{}", self.inner.base_url, check_feed(feed)?);
        let params = query.to_params()?;
        self.get_json(&url, &params).await
    }

    /// Run `query` and follow the paging (`startRow` advances by the page
    /// size) until `totalRows` rows have been read. `query`'s `rowCount` is
    /// the page size and its `startRow` the starting point.
    ///
    /// Each page is one request against the rate limit, so a wide query on a
    /// non-member key is slow by design.
    pub async fn query_all<T: DeserializeOwned>(
        &self,
        feed: &str,
        query: &Query,
    ) -> Result<Vec<T>, Error> {
        let page_size = u64::from(query.page_size());
        let mut next = query.clone();
        let mut rows: Vec<T> = Vec::new();
        loop {
            let page: Page<T> = self.query(feed, &next).await?;
            let got = u64::try_from(page.items.len()).unwrap_or(u64::MAX);
            let following = u64::from(next.first_row()) + got;
            let total = page.total_rows;
            rows.extend(page.items);
            if got < page_size || following > total {
                return Ok(rows);
            }
            let Ok(following) = u32::try_from(following) else {
                return Ok(rows);
            };
            next = next.start_row(following);
        }
    }

    /// [`Client::query`] with untyped rows.
    pub async fn query_rows(&self, feed: &str, query: &Query) -> Result<Page<Row>, Error> {
        self.query(feed, query).await
    }
}

/// Feed names are lowercase identifiers; reject anything that could alter the
/// URL path.
fn check_feed(feed: &str) -> Result<&str, Error> {
    if !feed.is_empty()
        && feed
            .bytes()
            .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'_')
    {
        Ok(feed)
    } else {
        Err(Error::InvalidQuery(format!("`{feed}` is not a feed name")))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn classify_status_table() {
        assert!(matches!(
            classify(200, None, b"{}".to_vec()),
            Outcome::Success(_)
        ));
        assert!(matches!(classify(401, None, vec![]), Outcome::KeyRejected));
        assert!(matches!(
            classify(429, Some(Duration::from_secs(7)), vec![]),
            Outcome::Retry {
                error: Error::RateLimited { retry_after: Some(d) },
                retry_after: Some(_)
            } if d == Duration::from_secs(7)
        ));
        assert!(matches!(
            classify(503, None, vec![]),
            Outcome::Retry {
                error: Error::Server { status: 503 },
                ..
            }
        ));
        assert!(matches!(
            classify(404, None, b"nope".to_vec()),
            Outcome::Fatal(Error::Status { status: 404, .. })
        ));
    }

    #[test]
    fn rejection_decodes_both_pjm_shapes() {
        let field_shape = include_bytes!("../tests/fixtures/error_datetime_missing.json");
        match rejection(400, field_shape) {
            Error::Rejected { errors, .. } => {
                assert_eq!(errors.len(), 1);
                assert_eq!(errors[0].field, "Filters");
                assert_eq!(errors[0].detail, vec!["datetime_filter".to_owned()]);
            }
            other => panic!("expected Rejected, got {other:?}"),
        }
        let code_shape = include_bytes!("../tests/fixtures/error_validation.json");
        match rejection(400, code_shape) {
            Error::Rejected { code, message, .. } => {
                assert_eq!(code.as_deref(), Some("API_1005"));
                assert_eq!(
                    message.as_deref(),
                    Some("Search specification validation failed.")
                );
            }
            other => panic!("expected Rejected, got {other:?}"),
        }
        assert!(matches!(rejection(400, b"<html>"), Error::Status { .. }));
    }

    #[test]
    fn retry_after_header_parses_seconds_only() {
        assert_eq!(
            parse_retry_after(&HeaderValue::from_static("12")),
            Some(Duration::from_secs(12))
        );
        assert_eq!(
            parse_retry_after(&HeaderValue::from_static("Wed, 21 Oct 2026 07:28:00 GMT")),
            None
        );
    }

    #[test]
    fn feed_names_cannot_escape_the_path() {
        assert!(check_feed("da_hrl_lmps").is_ok());
        assert!(check_feed("../x").is_err());
        assert!(check_feed("a/b").is_err());
        assert!(check_feed("").is_err());
        assert!(check_feed("Upper").is_err());
    }
}

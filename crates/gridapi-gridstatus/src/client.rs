//! The HTTP client.

use std::fmt;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;
use std::time::Duration;

use reqwest::header::{HeaderMap, HeaderValue};
use serde::de::DeserializeOwned;
use serde_json::Value;

use crate::error::{is_retryable_status, Error, Result};
use crate::models::{
    CatalogEnvelope, Dataset, DatasetList, DatasetUpdate, QueryEnvelope, QueryPage, Record,
    UpdatesEnvelope, Usage,
};
use crate::query::{Order, Query};
use crate::response::{retry_after, ResponseMeta};
use crate::retry::{jitter, RetryPolicy};
use crate::throttle::Throttle;

/// Production base URL.
pub const DEFAULT_BASE_URL: &str = "https://api.gridstatus.io/v1";

/// Environment variable read by [`Client::from_env`].
pub const API_KEY_ENV: &str = "GRIDSTATUS_API_KEY";

/// Free tier allows 1 request per second, and the server counts a window, not an average:
/// requests spaced exactly 1 s apart drew `429 Too Many Requests. Limit: 1 per 1 second`
/// (live, 2026-11-18), so the default leaves headroom.
pub const DEFAULT_MIN_INTERVAL: Duration = Duration::from_millis(1500);

const DEFAULT_TIMEOUT: Duration = Duration::from_secs(60);
const ERROR_BODY_LIMIT: usize = 300;

/// Configures and builds a [`Client`].
#[derive(Clone)]
pub struct ClientBuilder {
    api_key: Option<String>,
    base_url: String,
    user_agent: String,
    timeout: Duration,
    min_interval: Duration,
    retry: RetryPolicy,
    request_budget: Option<u64>,
}

impl Default for ClientBuilder {
    fn default() -> Self {
        Self {
            api_key: None,
            base_url: DEFAULT_BASE_URL.to_owned(),
            user_agent: format!("gridapi-gridstatus/{}", env!("CARGO_PKG_VERSION")),
            timeout: DEFAULT_TIMEOUT,
            min_interval: DEFAULT_MIN_INTERVAL,
            retry: RetryPolicy::DEFAULT,
            request_budget: None,
        }
    }
}

impl fmt::Debug for ClientBuilder {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("ClientBuilder")
            .field("api_key", &self.api_key.as_ref().map(|_| "<redacted>"))
            .field("base_url", &self.base_url)
            .field("user_agent", &self.user_agent)
            .field("timeout", &self.timeout)
            .field("min_interval", &self.min_interval)
            .field("retry", &self.retry)
            .field("request_budget", &self.request_budget)
            .finish()
    }
}

impl ClientBuilder {
    /// API key sent as the `x-api-key` header.
    #[must_use]
    pub fn api_key(mut self, key: impl Into<String>) -> Self {
        self.api_key = Some(key.into());
        self
    }

    /// Read the key from `GRIDSTATUS_API_KEY`.
    #[must_use]
    pub fn api_key_from_env(mut self) -> Self {
        self.api_key = std::env::var(API_KEY_ENV)
            .ok()
            .filter(|k| !k.trim().is_empty());
        self
    }

    /// Override the base URL (default `https://api.gridstatus.io/v1`), e.g. for a mock server.
    #[must_use]
    pub fn base_url(mut self, url: impl Into<String>) -> Self {
        self.base_url = url.into();
        self
    }

    /// Override the `User-Agent` header.
    #[must_use]
    pub fn user_agent(mut self, ua: impl Into<String>) -> Self {
        self.user_agent = ua.into();
        self
    }

    /// Per-request timeout (default 60 s).
    #[must_use]
    pub fn timeout(mut self, timeout: Duration) -> Self {
        self.timeout = timeout;
        self
    }

    /// Minimum spacing between the starts of two requests from this client
    /// (default 1 s, the free-tier rate). `Duration::ZERO` disables spacing.
    #[must_use]
    pub fn min_interval(mut self, interval: Duration) -> Self {
        self.min_interval = interval;
        self
    }

    /// Retry policy for 429/5xx/transport errors.
    #[must_use]
    pub fn retry(mut self, retry: RetryPolicy) -> Self {
        self.retry = retry;
        self
    }

    /// Hard cap on HTTP requests (every attempt, retries included) this client
    /// may send. Further calls fail with [`Error::BudgetExhausted`] without
    /// touching the network. Guards a monthly quota (250 requests on free).
    #[must_use]
    pub fn request_budget(mut self, max_requests: u64) -> Self {
        self.request_budget = Some(max_requests);
        self
    }

    /// Build the client.
    ///
    /// # Errors
    /// [`Error::MissingApiKey`] without a key, [`Error::InvalidRequest`] for a
    /// key that is not a valid header value, or a transport error building TLS.
    pub fn build(self) -> Result<Client> {
        let key = self
            .api_key
            .filter(|k| !k.trim().is_empty())
            .ok_or(Error::MissingApiKey)?;
        let mut value = HeaderValue::from_str(key.trim())
            .map_err(|_| Error::InvalidRequest("api key is not a valid header value".to_owned()))?;
        value.set_sensitive(true);
        let mut headers = HeaderMap::new();
        headers.insert("x-api-key", value);
        let http = reqwest::Client::builder()
            .user_agent(&self.user_agent)
            .default_headers(headers)
            .timeout(self.timeout)
            .build()?;
        Ok(Client {
            inner: Arc::new(Inner {
                http,
                base_url: self.base_url.trim_end_matches('/').to_owned(),
                throttle: Throttle::new(self.min_interval),
                retry: self.retry,
                budget: self.request_budget,
                sent: AtomicU64::new(0),
            }),
        })
    }
}

struct Inner {
    http: reqwest::Client,
    base_url: String,
    throttle: Throttle,
    retry: RetryPolicy,
    budget: Option<u64>,
    sent: AtomicU64,
}

/// Async Grid Status API client. Cheap to clone; clones share one throttle and
/// one request counter, so they behave as a single caller of the API.
#[derive(Clone)]
pub struct Client {
    inner: Arc<Inner>,
}

impl fmt::Debug for Client {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Client")
            .field("base_url", &self.inner.base_url)
            .field("requests_sent", &self.requests_sent())
            .field("api_key", &"<redacted>")
            .finish_non_exhaustive()
    }
}

const _: () = {
    const fn assert_send_sync<T: Send + Sync>() {}
    assert_send_sync::<Client>();
};

/// All rows of a (possibly multi-page) query.
#[derive(Debug, Clone, PartialEq)]
pub struct AllRows<R> {
    /// Rows from every fetched page, truncated to the query's `limit`.
    pub rows: Vec<R>,
    /// HTTP requests spent fetching pages (retries not counted).
    pub pages: u32,
    /// False when `max_pages` stopped the walk while the server still had more.
    pub complete: bool,
}

impl Client {
    /// Start configuring a client.
    pub fn builder() -> ClientBuilder {
        ClientBuilder::default()
    }

    /// A client with the given key and all defaults.
    pub fn new(api_key: impl Into<String>) -> Result<Self> {
        Self::builder().api_key(api_key).build()
    }

    /// A client with the key from `GRIDSTATUS_API_KEY` and all defaults.
    pub fn from_env() -> Result<Self> {
        Self::builder().api_key_from_env().build()
    }

    /// HTTP requests sent so far (every attempt, retries included).
    pub fn requests_sent(&self) -> u64 {
        self.inner.sent.load(Ordering::Relaxed)
    }

    /// `GET /datasets`: the catalog of datasets visible to the key.
    pub async fn list_datasets(&self) -> Result<DatasetList> {
        let (body, response) = self.get_bytes("/datasets", &[]).await?;
        let env: CatalogEnvelope = decode(&body)?;
        Ok(DatasetList {
            datasets: env.data,
            response,
        })
    }

    /// `GET /datasets/{id}`: metadata for one dataset.
    pub async fn dataset(&self, dataset_id: &str) -> Result<Dataset> {
        let path = dataset_path(dataset_id, "")?;
        let (body, _) = self.get_bytes(&path, &[]).await?;
        decode(&body)
    }

    /// `GET /datasets/{id}/query`: one page of rows deserialised as `R`.
    pub async fn query<R: DeserializeOwned>(
        &self,
        dataset_id: &str,
        query: &Query,
    ) -> Result<QueryPage<R>> {
        query.validate()?;
        let path = dataset_path(dataset_id, "/query")?;
        let (body, response) = self.get_bytes(&path, &query.to_params()).await?;
        let env: QueryEnvelope<R> = decode(&body)?;
        Ok(QueryPage {
            rows: env.data,
            meta: env.meta,
            dataset: env.dataset_metadata,
            response,
        })
    }

    /// One page of rows as dynamic [`Record`]s, for any dataset.
    pub async fn query_records(
        &self,
        dataset_id: &str,
        query: &Query,
    ) -> Result<QueryPage<Record>> {
        self.query(dataset_id, query).await
    }

    /// Walk pages (cursor when the server returns one, else page number) until
    /// the server reports no next page, the query's `limit` is reached, or
    /// `max_pages` requests were made. `max_pages` is explicit so a loose
    /// query cannot silently burn the monthly request quota.
    pub async fn query_all<R: DeserializeOwned>(
        &self,
        dataset_id: &str,
        query: &Query,
        max_pages: u32,
    ) -> Result<AllRows<R>> {
        let limit = query.row_limit().and_then(|n| usize::try_from(n).ok());
        let mut rows: Vec<R> = Vec::new();
        let mut current = query.clone();
        let mut pages = 0_u32;
        loop {
            if pages >= max_pages {
                return Ok(AllRows {
                    rows,
                    pages,
                    complete: false,
                });
            }
            let page = self.query::<R>(dataset_id, &current).await?;
            pages += 1;
            let more = page.has_next_page() && !page.rows.is_empty();
            let next = page.meta.cursor.clone();
            let page_no = current.page_number() + 1;
            rows.extend(page.rows);
            if let Some(l) = limit {
                rows.truncate(l);
                if rows.len() >= l {
                    return Ok(AllRows {
                        rows,
                        pages,
                        complete: true,
                    });
                }
            }
            if !more {
                return Ok(AllRows {
                    rows,
                    pages,
                    complete: true,
                });
            }
            current = current.with_next(next, page_no);
        }
    }

    /// `GET /api_usage`: plan limits and consumption this billing period.
    pub async fn usage(&self) -> Result<Usage> {
        let (body, _) = self.get_bytes("/api_usage", &[]).await?;
        decode(&body)
    }

    /// `GET /dataset-updates/{id}`: recent update events for a dataset.
    pub async fn dataset_updates(
        &self,
        dataset_id: &str,
        order: Order,
        limit: Option<u64>,
    ) -> Result<Vec<DatasetUpdate>> {
        let path = dataset_path_for("dataset-updates", dataset_id, "")?;
        let mut params = vec![(
            "order",
            match order {
                Order::Asc => "asc".to_owned(),
                Order::Desc => "desc".to_owned(),
            },
        )];
        params.extend(limit.map(|n| ("limit", n.to_string())));
        let (body, _) = self.get_bytes(&path, &params).await?;
        let env: UpdatesEnvelope = decode(&body)?;
        Ok(env.data)
    }

    /// Escape hatch: `GET {base}{path}` with query pairs, returning the JSON
    /// body untyped. Reaches endpoints this crate has no typed wrapper for
    /// (`/pricing_locations`, `/constraints`, `/outages/...`,
    /// `/reports/daily_peak/{iso}`). Throttling, retry and the budget apply.
    pub async fn get_json(
        &self,
        path: &str,
        params: &[(&str, String)],
    ) -> Result<(Value, ResponseMeta)> {
        if !path.starts_with('/') {
            return Err(Error::InvalidRequest("path must start with '/'".to_owned()));
        }
        let (body, meta) = self.get_bytes(path, params).await?;
        Ok((decode(&body)?, meta))
    }

    /// Send one GET with throttle, budget and retry; return body bytes on 2xx.
    async fn get_bytes(
        &self,
        path: &str,
        params: &[(&str, String)],
    ) -> Result<(Vec<u8>, ResponseMeta)> {
        let url = format!("{}{}", self.inner.base_url, path);
        let policy = self.inner.retry;
        let mut retry = 0_u32;
        loop {
            self.inner.throttle.wait().await;
            self.reserve_request()?;
            let outcome = self.attempt(&url, params).await;
            self.inner.throttle.finish();
            let hint = match &outcome {
                Err(Error::Api { retry_after, .. }) => *retry_after,
                _ => None,
            };
            match outcome {
                Ok(ok) => return Ok(ok),
                // A spent budget returns the real failure, not `BudgetExhausted`.
                Err(e) if e.is_retryable() && retry < policy.max_retries && self.has_budget() => {
                    match policy.delay(retry, jitter(retry), hint) {
                        Some(d) => tokio::time::sleep(d).await,
                        None => return Err(e),
                    }
                    retry += 1;
                }
                Err(e) => return Err(e),
            }
        }
    }

    /// True when another request may be sent under the budget.
    fn has_budget(&self) -> bool {
        self.inner
            .budget
            .is_none_or(|b| self.inner.sent.load(Ordering::Acquire) < b)
    }

    /// Count one request against the budget, lock-free.
    fn reserve_request(&self) -> Result<()> {
        let Some(budget) = self.inner.budget else {
            self.inner.sent.fetch_add(1, Ordering::Relaxed);
            return Ok(());
        };
        let mut used = self.inner.sent.load(Ordering::Acquire);
        loop {
            if used >= budget {
                return Err(Error::BudgetExhausted { budget });
            }
            match self.inner.sent.compare_exchange_weak(
                used,
                used + 1,
                Ordering::AcqRel,
                Ordering::Acquire,
            ) {
                Ok(_) => return Ok(()),
                Err(actual) => used = actual,
            }
        }
    }

    async fn attempt(
        &self,
        url: &str,
        params: &[(&str, String)],
    ) -> Result<(Vec<u8>, ResponseMeta)> {
        let resp = self.inner.http.get(url).query(params).send().await?;
        let status = resp.status();
        let meta = ResponseMeta::from_parts(status, resp.headers());
        let hint = retry_after(resp.headers());
        let body = resp.bytes().await?;
        if status.is_success() {
            return Ok((body.to_vec(), meta));
        }
        // Retry-After may only be on a retryable status; keep it either way.
        let hint = hint.filter(|_| is_retryable_status(status));
        Err(Error::Api {
            status,
            message: error_message(&body),
            retry_after: hint,
        })
    }
}

fn dataset_path(dataset_id: &str, suffix: &str) -> Result<String> {
    dataset_path_for("datasets", dataset_id, suffix)
}

fn dataset_path_for(root: &str, dataset_id: &str, suffix: &str) -> Result<String> {
    let ok = !dataset_id.is_empty()
        && dataset_id
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '-');
    if ok {
        Ok(format!("/{root}/{dataset_id}{suffix}"))
    } else {
        Err(Error::InvalidRequest(format!(
            "dataset id must be [A-Za-z0-9_-]+, got {dataset_id:?}"
        )))
    }
}

fn decode<T: DeserializeOwned>(body: &[u8]) -> Result<T> {
    serde_json::from_slice(body).map_err(|source| Error::Decode {
        source,
        snippet: snippet(body, 200),
    })
}

fn snippet(body: &[u8], max: usize) -> String {
    let text = String::from_utf8_lossy(body);
    text.chars().take(max).collect()
}

/// The `detail` of a `FastAPI` error body when present, else a truncated body.
fn error_message(body: &[u8]) -> String {
    serde_json::from_slice::<Value>(body)
        .ok()
        .and_then(|v| v.get("detail").cloned())
        .map_or_else(
            || snippet(body, ERROR_BODY_LIMIT),
            |d| match d {
                Value::String(s) => s,
                other => other.to_string().chars().take(ERROR_BODY_LIMIT).collect(),
            },
        )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn dataset_ids_are_validated_against_path_injection() {
        assert_eq!(
            dataset_path("ercot_load", "/query").as_deref().ok(),
            Some("/datasets/ercot_load/query")
        );
        for bad in ["", "a/b", "../x", "a b", "a?b", "a#b", "é"] {
            assert!(
                matches!(dataset_path(bad, ""), Err(Error::InvalidRequest(_))),
                "{bad:?} should be rejected"
            );
        }
    }

    /// Regression (): requests spaced exactly 1 s apart drew `429 Too Many Requests.
    /// Limit: 1 per 1 second` from the live API, each retry spending a budgeted request.
    #[test]
    fn default_spacing_leaves_headroom_over_the_servers_one_second_window() {
        assert!(DEFAULT_MIN_INTERVAL > Duration::from_secs(1));
    }

    #[test]
    fn error_message_prefers_detail() {
        assert_eq!(
            error_message(br#"{"detail":"Invalid API key"}"#),
            "Invalid API key"
        );
        assert!(error_message(br#"{"detail":[{"msg":"bad"}]}"#).contains("bad"));
        assert_eq!(error_message(b"plain text"), "plain text");
    }

    #[test]
    fn builder_requires_a_key_and_redacts_debug() {
        assert!(matches!(
            Client::builder().build(),
            Err(Error::MissingApiKey)
        ));
        assert!(matches!(
            Client::builder().api_key("   ").build(),
            Err(Error::MissingApiKey)
        ));
        let b = Client::builder().api_key("super-secret");
        assert!(!format!("{b:?}").contains("super-secret"));
        let c = b.build().expect("builds");
        assert!(!format!("{c:?}").contains("super-secret"));
    }

    #[test]
    fn newline_in_key_is_rejected_not_sent() {
        assert!(matches!(
            Client::new("bad\nkey"),
            Err(Error::InvalidRequest(_))
        ));
    }
}

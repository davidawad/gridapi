//! The HTTP client: configuration, retrying transport, and route browsing.

use std::fmt;
use std::sync::Arc;
use std::time::Duration;

use reqwest::StatusCode;
use serde::de::DeserializeOwned;
use serde::Deserialize;
use serde_json::Value;

use crate::error::{Error, Result};
use crate::meta::{FacetList, FacetValue, RouteMeta};
use crate::retry::{parse_retry_after, RetryPolicy};

/// Base URL of the EIA Open Data API v2.
pub const DEFAULT_BASE_URL: &str = "https://api.eia.gov/v2";
/// Host that serves the keyless bulk files.
pub const DEFAULT_BULK_BASE_URL: &str = "https://www.eia.gov";
/// Default `User-Agent`.
pub const DEFAULT_USER_AGENT: &str = concat!("gridapi-eia/", env!("CARGO_PKG_VERSION"));
/// Environment variable [`Client::from_env`] reads.
pub const API_KEY_ENV: &str = "EIA_API_KEY";

/// A non-fatal note EIA attaches to a response, for example
/// `incomplete return` when more rows exist than one page holds.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
pub struct Warning {
    /// Short label.
    #[serde(default)]
    pub warning: String,
    /// Explanation.
    #[serde(default)]
    pub description: String,
}

/// Builder for [`Client`].
///
/// ```
/// use std::time::Duration;
/// use gridapi_eia::{Client, RetryPolicy};
///
/// let client = Client::builder()
///     .api_key("YOUR_KEY")
///     .timeout(Duration::from_secs(60))
///     .retry(RetryPolicy::default())
///     .user_agent("my-app/1.0")
///     .build()?;
/// # Ok::<(), gridapi_eia::Error>(())
/// ```
#[derive(Clone)]
#[must_use]
pub struct ClientBuilder {
    api_key: Option<String>,
    base_url: String,
    bulk_base_url: String,
    user_agent: String,
    timeout: Duration,
    retry: RetryPolicy,
    http: Option<reqwest::Client>,
}

impl Default for ClientBuilder {
    fn default() -> Self {
        Self {
            api_key: None,
            base_url: DEFAULT_BASE_URL.to_owned(),
            bulk_base_url: DEFAULT_BULK_BASE_URL.to_owned(),
            user_agent: DEFAULT_USER_AGENT.to_owned(),
            timeout: Duration::from_secs(120),
            retry: RetryPolicy::default(),
            http: None,
        }
    }
}

impl fmt::Debug for ClientBuilder {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("ClientBuilder")
            .field("api_key", &self.api_key.as_ref().map(|_| "<redacted>"))
            .field("base_url", &self.base_url)
            .field("bulk_base_url", &self.bulk_base_url)
            .field("user_agent", &self.user_agent)
            .field("timeout", &self.timeout)
            .field("retry", &self.retry)
            .finish_non_exhaustive()
    }
}

impl ClientBuilder {
    /// API key (sent as the `api_key` query parameter).
    pub fn api_key(mut self, key: impl Into<String>) -> Self {
        self.api_key = Some(key.into());
        self
    }

    /// Read the API key from `EIA_API_KEY` if one has not been set.
    pub fn api_key_from_env(mut self) -> Self {
        if self.api_key.is_none() {
            self.api_key = std::env::var(API_KEY_ENV).ok();
        }
        self
    }

    /// Override the API base URL (default [`DEFAULT_BASE_URL`]); for tests,
    /// proxies or mirrors.
    pub fn base_url(mut self, url: impl Into<String>) -> Self {
        self.base_url = url.into();
        self
    }

    /// Override the bulk-file host (default [`DEFAULT_BULK_BASE_URL`]).
    pub fn bulk_base_url(mut self, url: impl Into<String>) -> Self {
        self.bulk_base_url = url.into();
        self
    }

    /// Override the `User-Agent` header.
    pub fn user_agent(mut self, ua: impl Into<String>) -> Self {
        self.user_agent = ua.into();
        self
    }

    /// Timeout (default 120 s): total per API request, and connect/idle-read
    /// for bulk downloads, which may legitimately run longer than this.
    pub fn timeout(mut self, t: Duration) -> Self {
        self.timeout = t;
        self
    }

    /// Retry policy for 429, 5xx and transport errors.
    pub fn retry(mut self, policy: RetryPolicy) -> Self {
        self.retry = policy;
        self
    }

    /// Supply a preconfigured `reqwest::Client` (proxy, extra roots, ...).
    /// Its own user agent and timeout then apply; redirects are your call.
    pub fn http_client(mut self, http: reqwest::Client) -> Self {
        self.http = Some(http);
        self
    }

    /// Build the client. Fails with [`Error::MissingApiKey`] if no non-empty
    /// key was provided.
    pub fn build(self) -> Result<Client> {
        let api_key = self
            .api_key
            .map(|k| k.trim().to_owned())
            .filter(|k| !k.is_empty())
            .ok_or(Error::MissingApiKey)?;
        let http = match self.http {
            Some(h) => h,
            None => reqwest::Client::builder()
                .user_agent(self.user_agent)
                .connect_timeout(self.timeout)
                // Idle-read timeout, not a total one: bulk downloads of
                // 100 MB stay alive while bytes flow. API calls add a total
                // per-request timeout in `Client::get_api`.
                .read_timeout(self.timeout)
                // EIA redirects unpublished bulk files to an HTML page; treat
                // a redirect as a status, never follow it.
                .redirect(reqwest::redirect::Policy::none())
                .build()
                .map_err(Error::from_reqwest)?,
        };
        Ok(Client {
            inner: Arc::new(Inner {
                http,
                api_key,
                base_url: self.base_url.trim_end_matches('/').to_owned(),
                bulk_base_url: self.bulk_base_url.trim_end_matches('/').to_owned(),
                retry: self.retry,
                timeout: self.timeout,
            }),
        })
    }
}

struct Inner {
    http: reqwest::Client,
    api_key: String,
    base_url: String,
    bulk_base_url: String,
    retry: RetryPolicy,
    timeout: Duration,
}

/// Async client for the EIA Open Data API v2. Cheap to clone (shared
/// connection pool); holds no global state.
#[derive(Clone)]
pub struct Client {
    inner: Arc<Inner>,
}

impl fmt::Debug for Client {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Client")
            .field("api_key", &"<redacted>")
            .field("base_url", &self.inner.base_url)
            .field("retry", &self.inner.retry)
            .finish_non_exhaustive()
    }
}

/// Envelope every EIA JSON reply wraps its payload in.
#[derive(Deserialize)]
struct Envelope<T> {
    response: Option<T>,
    #[serde(default)]
    warnings: Vec<Warning>,
    error: Option<Value>,
    code: Option<Value>,
}

impl Client {
    /// Start a builder.
    pub fn builder() -> ClientBuilder {
        ClientBuilder::default()
    }

    /// Client with defaults and an explicit key.
    pub fn new(api_key: impl Into<String>) -> Result<Self> {
        Self::builder().api_key(api_key).build()
    }

    /// Client with defaults and the key from `EIA_API_KEY`.
    pub fn from_env() -> Result<Self> {
        Self::builder().api_key_from_env().build()
    }

    pub(crate) fn bulk_base_url(&self) -> &str {
        &self.inner.bulk_base_url
    }

    /// GET `url` with `params`, retrying 429/5xx/transport errors with
    /// jittered exponential backoff. Returns the first response whose status
    /// is not retryable (including 4xx and 3xx) so callers decide what a
    /// status means; a retryable status that outlasts the retries is
    /// returned as the final response.
    pub(crate) async fn send_with_retry(
        &self,
        url: &str,
        params: &[(String, String)],
        total_timeout: Option<Duration>,
    ) -> Result<reqwest::Response> {
        let policy = self.inner.retry;
        let mut retry = 0u32;
        loop {
            let mut req = self.inner.http.get(url).query(params);
            if let Some(t) = total_timeout {
                req = req.timeout(t);
            }
            let outcome = req.send().await;
            let (wait, last) = match outcome {
                Ok(resp) if crate::retry::is_retryable_status(resp.status().as_u16()) => {
                    let ra = resp
                        .headers()
                        .get(reqwest::header::RETRY_AFTER)
                        .and_then(|v| v.to_str().ok())
                        .and_then(parse_retry_after);
                    (ra, Ok(resp))
                }
                Ok(resp) => return Ok(resp),
                Err(e) if is_transient(&e) => (None, Err(Error::from_reqwest(e))),
                Err(e) => return Err(Error::from_reqwest(e)),
            };
            if retry >= policy.max_retries {
                return last;
            }
            tokio::time::sleep(policy.delay(retry, fastrand::f64(), wait)).await;
            retry += 1;
        }
    }

    /// GET an API path and decode the `response` payload plus warnings.
    pub(crate) async fn get_api<T: DeserializeOwned>(
        &self,
        path: &str,
        params: &[(String, String)],
    ) -> Result<(T, Vec<Warning>)> {
        let url = format!("{}/{}", self.inner.base_url, path.trim_matches('/'));
        let mut all = params.to_vec();
        all.push(("api_key".to_owned(), self.inner.api_key.clone()));
        let resp = self
            .send_with_retry(&url, &all, Some(self.inner.timeout))
            .await?;
        let status = resp.status();
        let body = resp.text().await.map_err(Error::from_reqwest)?;
        decode_envelope(status, &body)
    }

    /// Metadata of the root (`/v2/`): the top-level route list.
    pub async fn root(&self) -> Result<RouteMeta> {
        self.route("").await
    }

    /// Metadata of any route, for example `electricity`,
    /// `electricity/rto` or `electricity/rto/region-data`. Intermediate nodes
    /// list `routes`; dataset nodes list frequencies, facets, data columns
    /// and the available period range.
    pub async fn route(&self, path: &str) -> Result<RouteMeta> {
        let (meta, _) = self.get_api::<RouteMeta>(path, &[]).await?;
        Ok(meta)
    }

    /// Every value of one facet of a dataset, for example the balancing
    /// authorities of `electricity/rto/region-data` facet `respondent`.
    pub async fn facet_values(&self, route: &str, facet: &str) -> Result<Vec<FacetValue>> {
        let path = format!("{}/facet/{facet}", route.trim_matches('/'));
        let (list, _) = self.get_api::<FacetList>(&path, &[]).await?;
        Ok(list.facets)
    }

    /// Depth-first walk below `path` (empty for the root) returning the full
    /// route path and metadata of every dataset node. Issues one request per
    /// node, sequentially, so a crawl of the whole root is a few hundred
    /// calls; scope it with `path` where you can.
    pub async fn datasets_under(&self, path: &str) -> Result<Vec<(String, RouteMeta)>> {
        let mut stack = vec![path.trim_matches('/').to_owned()];
        let mut found = Vec::new();
        while let Some(p) = stack.pop() {
            let meta = self.route(&p).await?;
            if meta.is_dataset() {
                found.push((p.clone(), meta.clone()));
            }
            stack.extend(meta.routes.iter().map(|c| join(&p, &c.id)));
        }
        Ok(found)
    }
}

fn join(parent: &str, child: &str) -> String {
    [parent, child]
        .iter()
        .filter(|s| !s.is_empty())
        .copied()
        .collect::<Vec<_>>()
        .join("/")
}

/// Transport errors worth retrying: timeouts and connection failures.
fn is_transient(e: &reqwest::Error) -> bool {
    e.is_timeout() || e.is_connect() || e.is_request() && !e.is_builder()
}

/// Turn a status and body into a decoded payload or a typed error. Pure.
pub(crate) fn decode_envelope<T: DeserializeOwned>(
    status: StatusCode,
    body: &str,
) -> Result<(T, Vec<Warning>)> {
    let parsed: std::result::Result<Envelope<T>, _> = serde_json::from_str(body);
    match (status.is_success(), parsed) {
        (
            true,
            Ok(Envelope {
                response: Some(r),
                warnings,
                ..
            }),
        ) => Ok((r, warnings)),
        (
            _,
            Ok(Envelope {
                error: Some(err),
                code,
                ..
            }),
        ) => Err(api_error(status, &err, code.as_ref())),
        (true, Ok(_)) => Err(Error::Decode("reply has no `response` object".into())),
        (true, Err(e)) => Err(Error::decode(e)),
        (false, _) => Err(Error::Api {
            status: status.as_u16(),
            code: None,
            message: snippet(body),
        }),
    }
}

/// EIA error bodies come in two shapes: `{"error":"msg","code":400}` and the
/// gateway's `{"error":{"code":"API_KEY_INVALID","message":"..."}}`.
fn api_error(status: StatusCode, err: &Value, top_code: Option<&Value>) -> Error {
    let (code, message) = match err {
        Value::String(msg) => (top_code.map(value_text), msg.clone()),
        Value::Object(o) => (
            o.get("code").map(value_text),
            o.get("message").map_or_else(|| err.to_string(), value_text),
        ),
        other => (None, other.to_string()),
    };
    let status = top_code
        .and_then(Value::as_u64)
        .and_then(|c| u16::try_from(c).ok())
        .filter(|_| status.is_success())
        .unwrap_or_else(|| status.as_u16());
    Error::Api {
        status,
        code,
        message,
    }
}

fn value_text(v: &Value) -> String {
    v.as_str().map_or_else(|| v.to_string(), str::to_owned)
}

fn snippet(body: &str) -> String {
    let s: String = body.chars().take(200).collect();
    if s.is_empty() {
        "empty body".to_owned()
    } else {
        s
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn builder_requires_a_key() {
        assert!(matches!(
            Client::builder().build(),
            Err(Error::MissingApiKey)
        ));
        assert!(matches!(
            Client::builder().api_key("   ").build(),
            Err(Error::MissingApiKey)
        ));
        assert!(Client::new("k").is_ok());
    }

    #[test]
    fn debug_never_prints_the_key() {
        let c = Client::new("SECRET-KEY-123").unwrap();
        assert!(!format!("{c:?}").contains("SECRET-KEY-123"));
        let b = Client::builder().api_key("SECRET-KEY-123");
        assert!(!format!("{b:?}").contains("SECRET-KEY-123"));
    }

    #[test]
    fn invalid_key_body_maps_to_typed_error() {
        let body = include_str!("../tests/fixtures/error_invalid_key.json");
        let err = decode_envelope::<Value>(StatusCode::FORBIDDEN, body).unwrap_err();
        match err {
            Error::Api {
                status,
                code,
                message,
            } => {
                assert_eq!(status, 403);
                assert_eq!(code.as_deref(), Some("API_KEY_INVALID"));
                assert!(message.contains("invalid api_key"));
            }
            other => panic!("unexpected {other:?}"),
        }
    }

    #[test]
    fn flat_error_body_maps_to_typed_error() {
        let body = include_str!("../tests/fixtures/error_bad_frequency.json");
        let err = decode_envelope::<Value>(StatusCode::BAD_REQUEST, body).unwrap_err();
        assert!(
            matches!(&err, Error::Api { status: 400, message, .. } if message.contains("Invalid frequency"))
        );
    }

    #[test]
    fn error_envelope_inside_http_200_is_still_an_error() {
        let err =
            decode_envelope::<Value>(StatusCode::OK, r#"{"error":"boom","code":500}"#).unwrap_err();
        assert!(matches!(err, Error::Api { status: 500, .. }));
    }

    #[test]
    fn non_json_error_body_keeps_status() {
        let err = decode_envelope::<Value>(StatusCode::BAD_GATEWAY, "<html>bad gateway</html>")
            .unwrap_err();
        assert!(
            matches!(&err, Error::Api { status: 502, message, .. } if message.contains("bad gateway"))
        );
    }

    #[test]
    fn success_without_response_object_is_a_decode_error() {
        assert!(matches!(
            decode_envelope::<Value>(StatusCode::OK, "{}"),
            Err(Error::Decode(_))
        ));
        assert!(matches!(
            decode_envelope::<Value>(StatusCode::OK, "not json"),
            Err(Error::Decode(_))
        ));
    }

    #[test]
    fn join_skips_empty_segments() {
        assert_eq!(join("", "electricity"), "electricity");
        assert_eq!(join("electricity", "rto"), "electricity/rto");
        assert_eq!(join("a/b", ""), "a/b");
    }
}

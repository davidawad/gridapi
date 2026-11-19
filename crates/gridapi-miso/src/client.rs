//! The [`Client`] and its builder.

use std::time::Duration;

use serde::de::DeserializeOwned;

use crate::error::{Error, Result};
use crate::retry::RetryPolicy;
use crate::transport::Transport;

/// Default base URL of the keyless public API.
pub const PUBLIC_API_BASE: &str = "https://public-api.misoenergy.org";
/// Default base URL of the market report file host.
pub const REPORTS_BASE: &str = "https://docs.misoenergy.org/marketreports";
/// Default base URL of the Data Exchange (API Management) gateway.
#[cfg(feature = "miso-dataexchange")]
pub const DATA_EXCHANGE_BASE: &str = "https://apim.misoenergy.org";
/// Default base URL of the retired real-time data broker.
pub const DATA_BROKER_BASE: &str = "https://api.misoenergy.org/MISORTWDDataBroker";
/// The environment variable [`ClientBuilder::api_key_from_env`] reads.
#[cfg(feature = "miso-dataexchange")]
pub const API_KEY_ENV: &str = "MISO_API_KEY";

/// Default per-request timeout.
const DEFAULT_TIMEOUT: Duration = Duration::from_secs(60);

/// The base URLs of every MISO host the crate talks to.
///
/// Override them to point at a mirror, a proxy or a test server.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Endpoints {
    /// Keyless public API (`/api/...`).
    pub public_api: String,
    /// Market report files (`YYYYMMDD_da_expost_lmp.csv`, ...).
    pub reports: String,
    /// Data Exchange gateway (needs an API key).
    #[cfg(feature = "miso-dataexchange")]
    pub data_exchange: String,
    /// Retired real-time data broker.
    pub data_broker: String,
}

impl Default for Endpoints {
    fn default() -> Self {
        Self {
            public_api: PUBLIC_API_BASE.to_owned(),
            reports: REPORTS_BASE.to_owned(),
            #[cfg(feature = "miso-dataexchange")]
            data_exchange: DATA_EXCHANGE_BASE.to_owned(),
            data_broker: DATA_BROKER_BASE.to_owned(),
        }
    }
}

impl Endpoints {
    /// Point every host at one base URL (handy with a local test server).
    pub fn all_at(base: &str) -> Self {
        let base = base.trim_end_matches('/');
        Self {
            public_api: base.to_owned(),
            reports: format!("{base}/marketreports"),
            #[cfg(feature = "miso-dataexchange")]
            data_exchange: base.to_owned(),
            data_broker: format!("{base}/MISORTWDDataBroker"),
        }
    }
}

/// Configures and builds a [`Client`].
#[derive(Debug, Clone)]
pub struct ClientBuilder {
    endpoints: Endpoints,
    user_agent: String,
    timeout: Duration,
    retry: RetryPolicy,
    #[cfg(feature = "miso-dataexchange")]
    api_key: Option<String>,
}

impl Default for ClientBuilder {
    fn default() -> Self {
        Self {
            endpoints: Endpoints::default(),
            user_agent: format!("gridapi-miso/{}", env!("CARGO_PKG_VERSION")),
            timeout: DEFAULT_TIMEOUT,
            retry: RetryPolicy::default(),
            #[cfg(feature = "miso-dataexchange")]
            api_key: None,
        }
    }
}

impl ClientBuilder {
    /// Replace the host base URLs.
    pub fn endpoints(&mut self, endpoints: Endpoints) -> &mut Self {
        self.endpoints = endpoints;
        self
    }

    /// Set the `User-Agent` header. MISO asks that clients identify themselves.
    pub fn user_agent(&mut self, user_agent: impl Into<String>) -> &mut Self {
        self.user_agent = user_agent.into();
        self
    }

    /// Set the per-request timeout.
    pub fn timeout(&mut self, timeout: Duration) -> &mut Self {
        self.timeout = timeout;
        self
    }

    /// Set the retry policy (default: 4 attempts, jittered exponential backoff).
    pub fn retry(&mut self, retry: RetryPolicy) -> &mut Self {
        self.retry = retry;
        self
    }

    /// Set the Data Exchange subscription key.
    #[cfg(feature = "miso-dataexchange")]
    pub fn api_key(&mut self, key: impl Into<String>) -> &mut Self {
        self.api_key = Some(key.into());
        self
    }

    /// Read the Data Exchange subscription key from `MISO_API_KEY`, if set and non-empty.
    #[cfg(feature = "miso-dataexchange")]
    pub fn api_key_from_env(&mut self) -> &mut Self {
        if let Some(key) = std::env::var(API_KEY_ENV)
            .ok()
            .filter(|k| !k.trim().is_empty())
        {
            self.api_key = Some(key.trim().to_owned());
        }
        self
    }

    /// Build the client.
    pub fn build(&self) -> Result<Client> {
        let http = reqwest::Client::builder()
            .user_agent(&self.user_agent)
            .timeout(self.timeout)
            .build()
            .map_err(|e| Error::Config(e.to_string()))?;
        Ok(Client {
            transport: Transport::new(http, self.retry),
            endpoints: self.endpoints.clone(),
            #[cfg(feature = "miso-dataexchange")]
            api_key: self.api_key.clone(),
        })
    }
}

/// A MISO client. Cheap to clone; holds no global state.
#[derive(Debug, Clone)]
pub struct Client {
    transport: Transport,
    endpoints: Endpoints,
    #[cfg(feature = "miso-dataexchange")]
    api_key: Option<String>,
}

impl Client {
    /// A client with default endpoints, no API key and the default retry policy.
    pub fn new() -> Result<Self> {
        ClientBuilder::default().build()
    }

    /// A builder for a customised client.
    pub fn builder() -> ClientBuilder {
        ClientBuilder::default()
    }

    /// The hosts this client talks to.
    pub fn endpoints(&self) -> &Endpoints {
        &self.endpoints
    }

    #[cfg(feature = "miso-dataexchange")]
    pub(crate) fn api_key(&self) -> Option<&str> {
        self.api_key.as_deref()
    }

    pub(crate) async fn get_bytes(
        &self,
        url: &str,
        query: &[(String, String)],
        headers: &[(&'static str, String)],
    ) -> Result<Vec<u8>> {
        self.transport.get(url, query, headers).await
    }

    /// GET `url` and decode the JSON body as `T`.
    pub(crate) async fn get_json<T: DeserializeOwned>(
        &self,
        url: &str,
        query: &[(String, String)],
        headers: &[(&'static str, String)],
    ) -> Result<T> {
        let body = self.get_bytes(url, query, headers).await?;
        serde_json::from_slice(&body).map_err(|e| Error::decode(url, e))
    }
}

/// Join a base URL and a path with exactly one `/` between them.
pub(crate) fn join(base: &str, path: &str) -> String {
    format!(
        "{}/{}",
        base.trim_end_matches('/'),
        path.trim_start_matches('/')
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn join_normalises_slashes() {
        assert_eq!(join("https://a.b/", "/api/x"), "https://a.b/api/x");
        assert_eq!(join("https://a.b", "api/x"), "https://a.b/api/x");
    }

    #[test]
    fn builder_defaults_identify_the_crate() {
        let b = ClientBuilder::default();
        assert!(
            b.user_agent.starts_with("gridapi-miso/"),
            "{}",
            b.user_agent
        );
        assert_eq!(b.endpoints.public_api, PUBLIC_API_BASE);
    }

    #[test]
    fn all_at_points_every_host_at_one_base() {
        let e = Endpoints::all_at("http://127.0.0.1:9/");
        assert_eq!(e.public_api, "http://127.0.0.1:9");
        assert_eq!(e.reports, "http://127.0.0.1:9/marketreports");
    }
}

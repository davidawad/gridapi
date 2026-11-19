//! The [`Client`] and its builder.

use std::time::Duration;

use serde::de::DeserializeOwned;

use crate::error::{Error, Result};
use crate::retry::RetryPolicy;
use crate::transport::Transport;

/// Default base URL of the SPP portal (`marketplace.spp.org` redirects here).
pub const PORTAL_BASE: &str = "https://portal.spp.org";

/// Default per-request timeout; the biggest files are tens of megabytes.
const DEFAULT_TIMEOUT: Duration = Duration::from_secs(120);

/// Configures and builds a [`Client`].
#[derive(Debug, Clone)]
pub struct ClientBuilder {
    base_url: String,
    user_agent: String,
    timeout: Duration,
    retry: RetryPolicy,
}

impl Default for ClientBuilder {
    fn default() -> Self {
        Self {
            base_url: PORTAL_BASE.to_owned(),
            user_agent: format!("gridapi-spp/{}", env!("CARGO_PKG_VERSION")),
            timeout: DEFAULT_TIMEOUT,
            retry: RetryPolicy::default(),
        }
    }
}

impl ClientBuilder {
    /// Set the portal base URL (default [`PORTAL_BASE`]); useful for a proxy or a test server.
    pub fn base_url(&mut self, base_url: impl Into<String>) -> &mut Self {
        self.base_url = base_url.into();
        self
    }

    /// Set the `User-Agent` header.
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

    /// Build the client.
    pub fn build(&self) -> Result<Client> {
        let http = reqwest::Client::builder()
            .user_agent(&self.user_agent)
            .timeout(self.timeout)
            .build()
            .map_err(|e| Error::Config(e.to_string()))?;
        Ok(Client {
            transport: Transport::new(http, self.retry),
            base_url: self.base_url.trim_end_matches('/').to_owned(),
        })
    }
}

/// A client for the SPP public portal. Cheap to clone; holds no global state.
///
/// The portal's file browser serves anonymous reads: the `X-SPP-UI-Token` header and
/// session cookies its web UI sends are only used for signed-in users and are not needed.
#[derive(Debug, Clone)]
pub struct Client {
    transport: Transport,
    base_url: String,
}

impl Client {
    /// A client with the default portal URL and retry policy.
    pub fn new() -> Result<Self> {
        ClientBuilder::default().build()
    }

    /// A builder for a customised client.
    pub fn builder() -> ClientBuilder {
        ClientBuilder::default()
    }

    /// The portal base URL in use.
    pub fn base_url(&self) -> &str {
        &self.base_url
    }

    pub(crate) fn url(&self, path: &str) -> String {
        format!("{}/{}", self.base_url, path.trim_start_matches('/'))
    }

    pub(crate) async fn get_bytes(&self, url: &str, query: &[(String, String)]) -> Result<Vec<u8>> {
        self.transport.get(url, query, &[]).await
    }

    pub(crate) async fn get_json<T: DeserializeOwned>(
        &self,
        url: &str,
        query: &[(String, String)],
    ) -> Result<T> {
        let body = self.get_bytes(url, query).await?;
        serde_json::from_slice(&body).map_err(|e| Error::decode(url, e))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn url_joins_with_one_slash() {
        let c = Client::builder()
            .base_url("http://h:1/")
            .build()
            .expect("client");
        assert_eq!(c.url("/api/menu/"), "http://h:1/api/menu/");
        assert_eq!(c.url("x"), "http://h:1/x");
    }

    #[test]
    fn default_user_agent_names_the_crate() {
        assert!(ClientBuilder::default()
            .user_agent
            .starts_with("gridapi-spp/"));
    }
}

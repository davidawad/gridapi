//! Subscription-key handling: a user-provided key or the public key that the
//! Data Miner 2 web app ships.

use std::fmt;

use serde::Deserialize;

use crate::error::{snippet, Error};

/// Environment variable read by [`KeyMode::from_env`].
pub const API_KEY_ENV: &str = "PJM_API_KEY";

/// Where the public Data Miner 2 web app publishes its (rotating) key.
pub const DEFAULT_SETTINGS_URL: &str = "https://dataminer2.pjm.com/config/settings.json";

/// A PJM API-management subscription key. `Debug` never prints the value.
#[derive(Clone, PartialEq, Eq)]
pub struct ApiKey(String);

impl ApiKey {
    /// Wrap a key, trimming whitespace.
    ///
    /// # Errors
    /// [`Error::InvalidKey`] when the key is empty or not printable ASCII (it
    /// is sent as an HTTP header value).
    pub fn new(key: impl AsRef<str>) -> Result<Self, Error> {
        let key = key.as_ref().trim();
        if key.is_empty() {
            return Err(Error::InvalidKey("key is empty"));
        }
        if !key.bytes().all(|b| b.is_ascii_graphic()) {
            return Err(Error::InvalidKey("key must be printable ASCII"));
        }
        Ok(Self(key.to_owned()))
    }

    /// The raw key, for building the request header.
    pub(crate) fn expose(&self) -> &str {
        &self.0
    }
}

impl fmt::Debug for ApiKey {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("ApiKey(<redacted>)")
    }
}

/// Which key the client sends.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
#[non_exhaustive]
pub enum KeyMode {
    /// `PJM_API_KEY` when set and non-empty, otherwise the public web-app key.
    #[default]
    Auto,
    /// A key you provisioned at <https://apiportal.pjm.com/>.
    User(ApiKey),
    /// The key the Data Miner 2 web app ships, fetched at runtime from
    /// `settings.json` and refetched when PJM answers 401 (it rotates).
    PublicWebApp,
}

impl KeyMode {
    /// Resolve [`KeyMode::Auto`] against the process environment.
    ///
    /// # Errors
    /// [`Error::InvalidKey`] when `PJM_API_KEY` is set to an unusable value.
    pub fn from_env() -> Result<Self, Error> {
        Self::resolve_with(std::env::var(API_KEY_ENV).ok().as_deref())
    }

    /// Pure core of [`KeyMode::from_env`]: `var` is the value of `PJM_API_KEY`.
    pub(crate) fn resolve_with(var: Option<&str>) -> Result<Self, Error> {
        match var.map(str::trim).filter(|v| !v.is_empty()) {
            Some(key) => ApiKey::new(key).map(Self::User),
            None => Ok(Self::PublicWebApp),
        }
    }
}

/// The slice of `settings.json` this crate reads.
#[derive(Deserialize)]
struct Settings {
    #[serde(rename = "subscriptionKey")]
    subscription_key: String,
}

/// Parse a `settings.json` body into the public subscription key.
pub(crate) fn parse_settings(body: &[u8]) -> Result<ApiKey, Error> {
    let text = String::from_utf8_lossy(body);
    let text = text.trim_start_matches('\u{feff}');
    let settings: Settings = serde_json::from_str(text).map_err(|e| {
        Error::PublicKey(format!(
            "settings.json has no subscriptionKey ({e}); body starts: {}",
            snippet(body)
        ))
    })?;
    ApiKey::new(&settings.subscription_key)
        .map_err(|e| Error::PublicKey(format!("settings.json subscriptionKey unusable: {e}")))
}

/// Fetch the public web-app subscription key from `settings_url`
/// (normally [`DEFAULT_SETTINGS_URL`]).
///
/// # Errors
/// [`Error::PublicKey`] on any network, status or parse failure.
pub async fn fetch_public_key(settings_url: &str) -> Result<ApiKey, Error> {
    fetch_public_key_with(&reqwest::Client::new(), settings_url).await
}

pub(crate) async fn fetch_public_key_with(
    http: &reqwest::Client,
    settings_url: &str,
) -> Result<ApiKey, Error> {
    let resp = http
        .get(settings_url)
        .send()
        .await
        .map_err(|e| Error::PublicKey(format!("GET {settings_url}: {e}")))?;
    let status = resp.status();
    let body = resp
        .bytes()
        .await
        .map_err(|e| Error::PublicKey(format!("reading {settings_url}: {e}")))?;
    if !status.is_success() {
        return Err(Error::PublicKey(format!(
            "GET {settings_url} returned HTTP {}",
            status.as_u16()
        )));
    }
    parse_settings(&body)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn debug_redacts() {
        let key = ApiKey::new("abc123").unwrap();
        assert_eq!(format!("{key:?}"), "ApiKey(<redacted>)");
    }

    #[test]
    fn key_validation() {
        assert!(ApiKey::new("  ").is_err());
        assert!(ApiKey::new("has space").is_err());
        assert!(ApiKey::new("naïve").is_err());
        assert_eq!(ApiKey::new(" k1 ").unwrap().expose(), "k1");
    }

    #[test]
    fn env_resolution() {
        assert_eq!(KeyMode::resolve_with(None).unwrap(), KeyMode::PublicWebApp);
        assert_eq!(
            KeyMode::resolve_with(Some("  ")).unwrap(),
            KeyMode::PublicWebApp
        );
        assert_eq!(
            KeyMode::resolve_with(Some("k")).unwrap(),
            KeyMode::User(ApiKey::new("k").unwrap())
        );
        assert!(KeyMode::resolve_with(Some("a b")).is_err());
    }

    #[test]
    fn settings_fixture_parses() {
        let body = include_bytes!("../tests/fixtures/settings.json");
        assert_eq!(parse_settings(body).unwrap().expose(), "FIXTURE_PUBLIC_KEY");
    }

    #[test]
    fn settings_with_bom_parses() {
        let body = "\u{feff}{\"subscriptionKey\": \"k\"}";
        assert_eq!(parse_settings(body.as_bytes()).unwrap().expose(), "k");
    }

    #[test]
    fn settings_without_key_is_public_key_error() {
        assert!(matches!(
            parse_settings(b"{\"tool\": \"Data Miner 2\"}"),
            Err(Error::PublicKey(_))
        ));
    }
}

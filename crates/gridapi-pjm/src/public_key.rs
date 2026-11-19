//! The public Data Miner 2 web-app key (feature `pjm-public-web-key`, off by
//! default).
//!
//! PJM's own web app ships a subscription key in
//! `https://dataminer2.pjm.com/config/settings.json`. Reading it is not
//! forbidden by any single clause, but it is a key issued to PJM's web app,
//! not to you; see [`KeyMode::PublicWebApp`](crate::KeyMode::PublicWebApp) for
//! the PJM terms quoted. Register your own key at <https://apiportal.pjm.com/>.

use serde::Deserialize;

use crate::auth::ApiKey;
use crate::error::{snippet, Error};

/// Where the public Data Miner 2 web app publishes its (rotating) key.
pub const DEFAULT_SETTINGS_URL: &str = "https://dataminer2.pjm.com/config/settings.json";

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

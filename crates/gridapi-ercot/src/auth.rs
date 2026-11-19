//! Azure B2C resource-owner-password token flow with in-memory caching and refresh.

use std::time::{Duration, Instant};

use serde::Deserialize;
use tokio::sync::Mutex;

use crate::config::{Config, Credentials};
use crate::error::{Error, Result};
use crate::http::Http;

/// Tokens are refreshed this long before they expire.
const EXPIRY_SKEW: Duration = Duration::from_secs(120);

/// Lifetime assumed when the token endpoint omits `expires_in` (ERCOT issues one hour).
const FALLBACK_LIFETIME: Duration = Duration::from_secs(3600);

/// A cached bearer token plus what is needed to renew it.
#[derive(Debug, Clone)]
pub(crate) struct Token {
    bearer: String,
    refresh_token: Option<String>,
    expires_at: Instant,
}

impl Token {
    fn is_fresh(&self, now: Instant) -> bool {
        self.expires_at > now + EXPIRY_SKEW
    }
}

#[derive(Debug, Deserialize)]
struct TokenResponse {
    id_token: Option<String>,
    access_token: Option<String>,
    refresh_token: Option<String>,
    #[serde(default, deserialize_with = "seconds_from_str_or_num")]
    expires_in: Option<u64>,
}

/// B2C returns `expires_in` as a JSON string ("3600"); accept numbers too.
fn seconds_from_str_or_num<'de, D>(de: D) -> Result<Option<u64>, D::Error>
where
    D: serde::Deserializer<'de>,
{
    #[derive(Deserialize)]
    #[serde(untagged)]
    enum Secs {
        Num(u64),
        Text(String),
    }
    match Option::<Secs>::deserialize(de)? {
        None => Ok(None),
        Some(Secs::Num(n)) => Ok(Some(n)),
        Some(Secs::Text(s)) => s.trim().parse().map(Some).map_err(serde::de::Error::custom),
    }
}

/// Converts a token endpoint body into a [`Token`] relative to `now`.
///
/// The API wants the `id_token`; the `access_token` is a fallback for tenants that
/// only return that one.
fn token_from_body(body: &[u8], now: Instant) -> Result<Token> {
    let parsed: TokenResponse =
        serde_json::from_slice(body).map_err(|e| Error::decode("token response", e))?;
    let bearer = parsed
        .id_token
        .or(parsed.access_token)
        .ok_or_else(|| Error::decode("token response", "no id_token or access_token"))?;
    let lifetime = parsed
        .expires_in
        .map_or(FALLBACK_LIFETIME, Duration::from_secs);
    Ok(Token {
        bearer,
        refresh_token: parsed.refresh_token,
        expires_at: now + lifetime,
    })
}

/// Caches one bearer token and renews it before expiry.
///
/// Renewal is single-flight: concurrent callers wait for the one in-progress
/// refresh instead of each hitting the token endpoint.
#[derive(Debug)]
pub(crate) struct TokenProvider {
    credentials: Credentials,
    token_url: String,
    client_id: String,
    cached: Mutex<Option<Token>>,
}

impl TokenProvider {
    pub(crate) fn new(credentials: Credentials, config: &Config) -> Self {
        Self {
            credentials,
            token_url: config.token_url.clone(),
            client_id: config.client_id.clone(),
            cached: Mutex::new(None),
        }
    }

    pub(crate) fn subscription_key(&self) -> &str {
        &self.credentials.subscription_key
    }

    /// Returns a bearer token that is valid for at least the expiry skew.
    pub(crate) async fn bearer(&self, http: &Http) -> Result<String> {
        let mut slot = self.cached.lock().await;
        if let Some(token) = slot.as_ref().filter(|t| t.is_fresh(Instant::now())) {
            return Ok(token.bearer.clone());
        }
        let stale_refresh = slot.as_ref().and_then(|t| t.refresh_token.clone());
        let fresh = self.renew(http, stale_refresh.as_deref()).await?;
        let bearer = fresh.bearer.clone();
        *slot = Some(fresh);
        Ok(bearer)
    }

    /// Drops the cached token so the next call logs in again (after an HTTP 401).
    pub(crate) async fn invalidate(&self) {
        *self.cached.lock().await = None;
    }

    /// Tries the refresh token first, then falls back to the password grant.
    async fn renew(&self, http: &Http, refresh_token: Option<&str>) -> Result<Token> {
        if let Some(rt) = refresh_token {
            match self.request_token(http, &self.refresh_form(rt)).await {
                Ok(token) => return Ok(token),
                // A rejected refresh token is routine (expired); log in again.
                Err(Error::Auth { .. }) => {}
                Err(other) => return Err(other),
            }
        }
        self.request_token(http, &self.password_form()).await
    }

    fn scope(&self) -> String {
        format!("openid {} offline_access", self.client_id)
    }

    fn password_form(&self) -> Vec<(&'static str, String)> {
        vec![
            ("grant_type", "password".to_owned()),
            ("username", self.credentials.username.clone()),
            ("password", self.credentials.password.clone()),
            ("scope", self.scope()),
            ("client_id", self.client_id.clone()),
            ("response_type", "id_token".to_owned()),
        ]
    }

    fn refresh_form(&self, refresh_token: &str) -> Vec<(&'static str, String)> {
        vec![
            ("grant_type", "refresh_token".to_owned()),
            ("refresh_token", refresh_token.to_owned()),
            ("scope", self.scope()),
            ("client_id", self.client_id.clone()),
            ("response_type", "id_token".to_owned()),
        ]
    }

    async fn request_token(&self, http: &Http, form: &[(&'static str, String)]) -> Result<Token> {
        let raw = http
            .execute(|c| c.post(&self.token_url).form(form))
            .await
            .map_err(|e| match e {
                // 4xx from the token endpoint means the grant was refused.
                Error::Api {
                    status, message, ..
                } if (400..500).contains(&status) && status != 429 => {
                    Error::Auth { status, message }
                }
                other => other,
            })?;
        token_from_body(&raw.body, Instant::now())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn at(secs: u64) -> Instant {
        Instant::now() + Duration::from_secs(secs)
    }

    #[test]
    fn parses_string_expiry_like_b2c() {
        let body = br#"{"id_token":"abc","access_token":"zzz","token_type":"Bearer","expires_in":"3600","refresh_token":"r"}"#;
        let now = Instant::now();
        let t = token_from_body(body, now).expect("valid");
        assert_eq!(t.bearer, "abc", "id_token preferred over access_token");
        assert_eq!(t.refresh_token.as_deref(), Some("r"));
        assert_eq!(t.expires_at, now + Duration::from_secs(3600));
    }

    #[test]
    fn parses_numeric_expiry_and_access_token_fallback() {
        let t = token_from_body(
            br#"{"access_token":"only","expires_in":60}"#,
            Instant::now(),
        )
        .expect("valid");
        assert_eq!(t.bearer, "only");
    }

    #[test]
    fn missing_token_is_a_decode_error() {
        let err = token_from_body(br#"{"expires_in":"60"}"#, Instant::now()).expect_err("no token");
        assert!(matches!(err, Error::Decode { .. }), "{err}");
    }

    #[test]
    fn freshness_respects_skew() {
        let mk = |secs| Token {
            bearer: "b".into(),
            refresh_token: None,
            expires_at: at(secs),
        };
        assert!(mk(3600).is_fresh(Instant::now()));
        assert!(!mk(30).is_fresh(Instant::now()), "inside the skew window");
        assert!(!mk(0).is_fresh(Instant::now()));
    }
}

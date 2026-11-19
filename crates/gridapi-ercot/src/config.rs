//! Credentials, retry policy and client configuration.

use std::time::Duration;

use crate::error::{Error, Result};

/// Production base URL of the ERCOT Public API report service.
pub const DEFAULT_BASE_URL: &str = "https://api.ercot.com/api/public-reports";

/// Production Azure B2C token endpoint (resource-owner password flow).
pub const DEFAULT_TOKEN_URL: &str =
    "https://ercotb2c.b2clogin.com/ercotb2c.onmicrosoft.com/B2C_1_PUBAPI-ROPC-FLOW/oauth2/v2.0/token";

/// OAuth client id of the ERCOT Public API application (public, published by ERCOT).
pub const DEFAULT_CLIENT_ID: &str = "fec253ea-0d06-4272-a5e6-b478baeecd70";

/// Unofficial ERCOT dashboard feed carrying the live generation fuel mix.
pub const DEFAULT_FUEL_MIX_URL: &str =
    "https://www.ercot.com/api/1/services/read/dashboards/fuel-mix.json";

/// Browser-like User-Agent; the CDN in front of ERCOT challenges bare HTTP clients.
pub const DEFAULT_USER_AGENT: &str = "Mozilla/5.0 (Macintosh; Intel Mac OS X 10_15_7) \
AppleWebKit/620.92 (KHTML, like Gecko) Chrome/124.0 Safari/620.92 gridapi-ercot";

/// Environment variable holding the ERCOT API Explorer account e-mail.
pub const ENV_USERNAME: &str = "ERCOT_USERNAME";
/// Environment variable holding the ERCOT API Explorer account password.
pub const ENV_PASSWORD: &str = "ERCOT_PASSWORD";
/// Environment variable holding the API subscription key.
pub const ENV_SUBSCRIPTION_KEY: &str = "ERCOT_SUBSCRIPTION_KEY";

/// Login and subscription key for the ERCOT Public API.
///
/// Create both at <https://apiexplorer.ercot.com>. `Debug` redacts every field.
#[derive(Clone, PartialEq, Eq)]
pub struct Credentials {
    pub(crate) username: String,
    pub(crate) password: String,
    pub(crate) subscription_key: String,
}

impl Credentials {
    /// Builds credentials from explicit values.
    #[must_use]
    pub fn new(
        username: impl Into<String>,
        password: impl Into<String>,
        subscription_key: impl Into<String>,
    ) -> Self {
        Self {
            username: username.into(),
            password: password.into(),
            subscription_key: subscription_key.into(),
        }
    }

    /// Reads `ERCOT_USERNAME`, `ERCOT_PASSWORD` and `ERCOT_SUBSCRIPTION_KEY`.
    ///
    /// # Errors
    ///
    /// [`Error::Config`] naming every variable that is unset or empty.
    pub fn from_env() -> Result<Self> {
        Self::from_lookup(|name| std::env::var(name).ok())
    }

    /// Like [`Credentials::from_env`] with an injectable variable lookup.
    ///
    /// # Errors
    ///
    /// [`Error::Config`] naming every variable that is unset or empty.
    pub fn from_lookup(lookup: impl Fn(&str) -> Option<String>) -> Result<Self> {
        let get = |name: &str| lookup(name).filter(|v| !v.trim().is_empty());
        let (username, password, key) = (
            get(ENV_USERNAME),
            get(ENV_PASSWORD),
            get(ENV_SUBSCRIPTION_KEY),
        );
        match (username, password, key) {
            (Some(u), Some(p), Some(k)) => Ok(Self::new(u, p, k)),
            (u, p, k) => {
                let missing: Vec<&str> = [
                    (ENV_USERNAME, u.is_none()),
                    (ENV_PASSWORD, p.is_none()),
                    (ENV_SUBSCRIPTION_KEY, k.is_none()),
                ]
                .into_iter()
                .filter_map(|(name, absent)| absent.then_some(name))
                .collect();
                Err(Error::Config(format!(
                    "missing environment variable(s): {}",
                    missing.join(", ")
                )))
            }
        }
    }
}

impl std::fmt::Debug for Credentials {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Credentials")
            .field("username", &"<redacted>")
            .field("password", &"<redacted>")
            .field("subscription_key", &"<redacted>")
            .finish()
    }
}

/// Retry behaviour for 429, 5xx, transport failures and bot-check pages.
///
/// Delays use full jitter: attempt `n` sleeps a random duration in
/// `0..=min(max_delay, base_delay * 2^(n-1))`, or the server's `Retry-After`
/// (plus up to 25% jitter) when one is sent.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RetryPolicy {
    /// Total attempts including the first (1 disables retries).
    pub max_attempts: u32,
    /// Backoff ceiling for the first retry.
    pub base_delay: Duration,
    /// Upper bound on any single sleep.
    pub max_delay: Duration,
}

impl Default for RetryPolicy {
    fn default() -> Self {
        Self {
            max_attempts: 5,
            base_delay: Duration::from_millis(500),
            max_delay: Duration::from_secs(30),
        }
    }
}

impl RetryPolicy {
    /// A policy that never retries.
    #[must_use]
    pub fn none() -> Self {
        Self {
            max_attempts: 1,
            ..Self::default()
        }
    }

    /// Sleep before retry number `attempt` (1-based: the delay after the first failure is 1).
    ///
    /// `unit` is a uniform sample in `[0, 1)`; passing it in keeps this pure and testable.
    #[must_use]
    pub fn delay(&self, attempt: u32, retry_after: Option<Duration>, unit: f64) -> Duration {
        let unit = unit.clamp(0.0, 1.0);
        match retry_after {
            Some(server) => {
                let jitter = server.mul_f64(0.20 * unit);
                (server + jitter).min(self.max_delay.max(server))
            }
            None => {
                let exp = attempt.saturating_sub(1).min(20);
                let ceiling = self
                    .base_delay
                    .saturating_mul(1u32 << exp)
                    .min(self.max_delay);
                ceiling.mul_f64(unit)
            }
        }
    }
}

/// Resolved client configuration. Build one through [`crate::ClientBuilder`].
#[derive(Debug, Clone)]
pub(crate) struct Config {
    pub base_url: String,
    pub token_url: String,
    pub fuel_mix_url: String,
    pub client_id: String,
    pub user_agent: String,
    pub timeout: Duration,
    pub retry: RetryPolicy,
}

impl Default for Config {
    fn default() -> Self {
        Self {
            base_url: DEFAULT_BASE_URL.to_owned(),
            token_url: DEFAULT_TOKEN_URL.to_owned(),
            fuel_mix_url: DEFAULT_FUEL_MIX_URL.to_owned(),
            client_id: DEFAULT_CLIENT_ID.to_owned(),
            user_agent: DEFAULT_USER_AGENT.to_owned(),
            timeout: Duration::from_secs(120),
            retry: RetryPolicy::default(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn debug_never_leaks_secrets() {
        let c = Credentials::new("me@example.com", "hunter2", "key-123");
        let shown = format!("{c:?}");
        for secret in ["me@example.com", "hunter2", "key-123"] {
            assert!(!shown.contains(secret), "leaked {secret} in {shown}");
        }
    }

    #[test]
    fn from_lookup_reports_every_missing_variable() {
        let err = Credentials::from_lookup(|n| (n == ENV_USERNAME).then(|| "u".into()))
            .expect_err("two variables are absent");
        let msg = err.to_string();
        assert!(
            msg.contains(ENV_PASSWORD) && msg.contains(ENV_SUBSCRIPTION_KEY),
            "{msg}"
        );
        assert!(!msg.contains(ENV_USERNAME), "{msg}");
    }

    #[test]
    fn from_lookup_rejects_blank_values() {
        let err = Credentials::from_lookup(|_| Some("   ".into())).expect_err("blank");
        assert!(matches!(err, Error::Config(_)));
    }

    #[test]
    fn from_lookup_accepts_all_three() {
        let c = Credentials::from_lookup(|n| Some(format!("v-{n}"))).expect("complete");
        assert_eq!(c.username, "v-ERCOT_USERNAME");
        assert_eq!(c.subscription_key, "v-ERCOT_SUBSCRIPTION_KEY");
    }

    #[test]
    fn backoff_is_bounded_and_grows() {
        let p = RetryPolicy {
            max_attempts: 8,
            base_delay: Duration::from_millis(100),
            max_delay: Duration::from_secs(2),
        };
        let top = |attempt| p.delay(attempt, None, 0.999_999);
        assert!(top(1) <= Duration::from_millis(100));
        assert!(top(2) <= Duration::from_millis(200));
        assert!(top(3) > top(1));
        assert!(top(30) <= Duration::from_secs(2), "capped at max_delay");
        assert_eq!(
            p.delay(4, None, 0.0),
            Duration::ZERO,
            "full jitter reaches zero"
        );
    }

    #[test]
    fn retry_after_wins_over_backoff_and_adds_bounded_jitter() {
        let p = RetryPolicy::default();
        let server = Duration::from_secs(10);
        let d = p.delay(1, Some(server), 1.0);
        assert!(d >= server && d <= server + server / 4, "{d:?}");
    }
}

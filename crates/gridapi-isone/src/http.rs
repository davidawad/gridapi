//! HTTP transport: one GET with retry and jittered exponential backoff.

// Basic-auth credentials serve only the Web Services client (feature `isone-webservices`).
#![cfg_attr(not(feature = "isone-webservices"), allow(dead_code))]

use std::{
    collections::hash_map::RandomState,
    fmt,
    hash::{BuildHasher, Hasher},
    time::Duration,
};

use bytes::Bytes;
use reqwest::header::{ACCEPT, RETRY_AFTER};

use crate::error::Error;

/// Longest `Retry-After` the client will honour; anything larger is clamped.
const MAX_RETRY_AFTER: Duration = Duration::from_secs(120);

/// Name of the environment variable holding the Web Services username.
pub const USERNAME_VARIABLE: &str = "ISONE_USERNAME";
/// Name of the environment variable holding the Web Services password.
pub const PASSWORD_VARIABLE: &str = "ISONE_PASSWORD";

/// HTTP basic credentials for the Web Services API.
///
/// `Debug` never prints the password.
#[derive(Clone, PartialEq, Eq)]
pub struct Credentials {
    username: String,
    password: String,
}

impl Credentials {
    /// Credentials from explicit values.
    #[must_use]
    pub fn new(username: impl Into<String>, password: impl Into<String>) -> Self {
        Self {
            username: username.into(),
            password: password.into(),
        }
    }

    /// Read `ISONE_USERNAME` and `ISONE_PASSWORD` from the process environment.
    pub fn from_env() -> Result<Self, Error> {
        Self::from_lookup(|name| std::env::var(name).ok())
    }

    /// Like [`Credentials::from_env`] but reading variables through `lookup`, so
    /// callers with their own secret store do not have to touch the environment.
    pub fn from_lookup(lookup: impl Fn(&str) -> Option<String>) -> Result<Self, Error> {
        let get = |variable: &str| {
            lookup(variable)
                .filter(|v| !v.trim().is_empty())
                .ok_or_else(|| Error::MissingCredentials {
                    variable: variable.to_owned(),
                })
        };
        Ok(Self::new(get(USERNAME_VARIABLE)?, get(PASSWORD_VARIABLE)?))
    }

    /// The account name.
    #[must_use]
    pub fn username(&self) -> &str {
        &self.username
    }
}

impl fmt::Debug for Credentials {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Credentials")
            .field("username", &self.username)
            .field("password", &"<redacted>")
            .finish()
    }
}

/// How failed requests are retried.
///
/// Retries apply to transport failures, HTTP 429 and HTTP 5xx. The wait before
/// retry `n` (counting from zero) is "equal jitter": half of
/// `min(max_delay, base_delay * 2^n)` plus a random amount up to the other half,
/// so concurrent clients do not retry in lock step. A `Retry-After` header raises
/// the wait to at least the server's hint.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RetryPolicy {
    /// Retries after the first attempt. Zero disables retrying.
    pub max_retries: u32,
    /// Backoff before the first retry.
    pub base_delay: Duration,
    /// Ceiling on the exponential backoff.
    pub max_delay: Duration,
}

impl RetryPolicy {
    /// A policy that never retries.
    pub const NONE: Self = Self {
        max_retries: 0,
        base_delay: Duration::ZERO,
        max_delay: Duration::ZERO,
    };

    /// Wait before retry number `attempt` (zero-based). `unit` is a random number
    /// in `[0, 1)`; `retry_after` is the server's hint.
    #[must_use]
    pub fn delay(&self, attempt: u32, unit: f64, retry_after: Option<Duration>) -> Duration {
        let exponent = attempt.min(20);
        let capped = self
            .base_delay
            .saturating_mul(1_u32 << exponent)
            .min(self.max_delay);
        let half = capped / 2;
        let jittered = half + half.mul_f64(unit.clamp(0.0, 1.0));
        retry_after.map_or(jittered, |hint| jittered.max(hint.min(MAX_RETRY_AFTER)))
    }
}

impl Default for RetryPolicy {
    fn default() -> Self {
        Self {
            max_retries: 4,
            base_delay: Duration::from_millis(500),
            max_delay: Duration::from_secs(20),
        }
    }
}

/// A random number in `[0, 1)` from the standard library's per-process random
/// hasher keys, so the crate needs no RNG dependency.
fn unit_random() -> f64 {
    let bits = RandomState::new().build_hasher().finish() >> 11;
    // 53 random bits scaled into [0, 1); the cast is exact because bits < 2^53.
    #[allow(clippy::cast_precision_loss)]
    let numerator = bits as f64;
    numerator / 9_007_199_254_740_992.0
}

/// What a GET asks for beyond the URL.
#[derive(Debug, Clone, Copy, Default)]
pub(crate) struct Call<'a> {
    /// Basic-auth credentials, for the Web Services API.
    pub(crate) credentials: Option<&'a Credentials>,
    /// Ask for JSON via the `Accept` header.
    pub(crate) json: bool,
}

/// The retrying GET used by both clients.
#[derive(Debug, Clone)]
pub(crate) struct Transport {
    client: reqwest::Client,
    retry: RetryPolicy,
}

impl Transport {
    pub(crate) fn new(client: reqwest::Client, retry: RetryPolicy) -> Self {
        Self { client, retry }
    }

    /// GET `url`, retrying transient failures. HTTP 404 becomes [`Error::NotFound`]
    /// and HTTP 401 becomes [`Error::Unauthorized`].
    pub(crate) async fn get(&self, url: &str, call: Call<'_>) -> Result<Bytes, Error> {
        let mut attempt = 0;
        loop {
            match self.attempt(url, call).await {
                Err(e) if e.is_retryable() && attempt < self.retry.max_retries => {
                    let wait = self.retry.delay(attempt, unit_random(), e.retry_after());
                    tracing::warn!(
                        url,
                        attempt = attempt + 1,
                        max_retries = self.retry.max_retries,
                        wait_ms = u64::try_from(wait.as_millis()).unwrap_or(u64::MAX),
                        error = %e,
                        "retrying after transient failure"
                    );
                    tokio::time::sleep(wait).await;
                    attempt += 1;
                }
                outcome => return outcome,
            }
        }
    }

    async fn attempt(&self, url: &str, call: Call<'_>) -> Result<Bytes, Error> {
        let transport = |source: reqwest::Error| Error::Transport {
            url: url.to_owned(),
            source: Box::new(source),
        };
        let mut request = self.client.get(url);
        if call.json {
            request = request.header(ACCEPT, "application/json");
        }
        if let Some(c) = call.credentials {
            request = request.basic_auth(&c.username, Some(&c.password));
        }
        let response = request.send().await.map_err(transport)?;
        let status = response.status();
        tracing::debug!(url, status = status.as_u16(), "response");
        if status.is_success() {
            return response.bytes().await.map_err(transport);
        }
        match status.as_u16() {
            404 => Err(Error::NotFound {
                url: url.to_owned(),
            }),
            401 => Err(Error::Unauthorized {
                url: url.to_owned(),
            }),
            code => {
                let retry_after = response
                    .headers()
                    .get(RETRY_AFTER)
                    .and_then(|v| v.to_str().ok())
                    .and_then(|v| v.trim().parse::<u64>().ok())
                    .map(Duration::from_secs);
                Err(Error::Status {
                    url: url.to_owned(),
                    status: code,
                    retry_after,
                })
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const POLICY: RetryPolicy = RetryPolicy {
        max_retries: 4,
        base_delay: Duration::from_millis(100),
        max_delay: Duration::from_secs(1),
    };

    #[test]
    fn delay_doubles_then_caps() {
        let lows: Vec<_> = (0..6).map(|n| POLICY.delay(n, 0.0, None)).collect();
        assert_eq!(
            lows,
            [50, 100, 200, 400, 500, 500].map(Duration::from_millis)
        );
    }

    #[test]
    fn delay_jitter_spans_the_upper_half() {
        assert_eq!(POLICY.delay(1, 1.0, None), Duration::from_millis(200));
        assert_eq!(POLICY.delay(1, 0.5, None), Duration::from_millis(150));
    }

    #[test]
    fn retry_after_raises_the_floor_but_is_clamped() {
        let hint = Some(Duration::from_secs(5));
        assert_eq!(POLICY.delay(0, 0.0, hint), Duration::from_secs(5));
        let huge = Some(Duration::from_secs(10_000));
        assert_eq!(POLICY.delay(0, 0.0, huge), MAX_RETRY_AFTER);
    }

    #[test]
    fn unit_random_stays_in_range_and_varies() {
        let samples: Vec<f64> = (0..64).map(|_| unit_random()).collect();
        assert!(samples.iter().all(|u| (0.0..1.0).contains(u)));
        assert!(
            samples
                .windows(2)
                .any(|w| (w[0] - w[1]).abs() > f64::EPSILON),
            "64 draws should not all be equal"
        );
    }

    #[test]
    fn credentials_never_print_the_password() {
        let c = Credentials::new("alice", "hunter2");
        let shown = format!("{c:?}");
        assert!(shown.contains("alice"));
        assert!(!shown.contains("hunter2"), "leaked: {shown}");
    }

    #[test]
    fn credentials_load_from_a_lookup() {
        let env = |name: &str| match name {
            "ISONE_USERNAME" => Some("alice".to_owned()),
            "ISONE_PASSWORD" => Some("s3cret".to_owned()),
            _ => None,
        };
        let c = Credentials::from_lookup(env).expect("both set");
        assert_eq!(c.username(), "alice");
    }

    #[test]
    fn missing_or_blank_credentials_name_the_variable() {
        let only_user = |name: &str| (name == "ISONE_USERNAME").then(|| "alice".to_owned());
        let err = Credentials::from_lookup(only_user).expect_err("no password");
        assert!(
            matches!(&err, Error::MissingCredentials { variable } if variable == "ISONE_PASSWORD"),
            "got {err:?}"
        );
        let blank = |_: &str| Some("   ".to_owned());
        let err = Credentials::from_lookup(blank).expect_err("blank username");
        assert!(
            matches!(&err, Error::MissingCredentials { variable } if variable == "ISONE_USERNAME"),
            "got {err:?}"
        );
    }
}

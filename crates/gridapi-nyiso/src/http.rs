//! HTTP transport: one GET with retry and jittered exponential backoff.

use std::{
    collections::hash_map::RandomState,
    hash::{BuildHasher, Hasher},
    time::Duration,
};

use bytes::Bytes;
use reqwest::header::RETRY_AFTER;

use crate::error::Error;

/// Longest `Retry-After` the client will honour; anything larger is clamped.
const MAX_RETRY_AFTER: Duration = Duration::from_secs(120);

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

/// The retrying GET used by every dataset.
#[derive(Debug, Clone)]
pub(crate) struct Transport {
    client: reqwest::Client,
    retry: RetryPolicy,
}

impl Transport {
    pub(crate) fn new(client: reqwest::Client, retry: RetryPolicy) -> Self {
        Self { client, retry }
    }

    /// GET `url`, retrying transient failures. HTTP 404 becomes [`Error::NotFound`].
    pub(crate) async fn get(&self, url: &str) -> Result<Bytes, Error> {
        let mut attempt = 0;
        loop {
            match self.attempt(url).await {
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

    async fn attempt(&self, url: &str) -> Result<Bytes, Error> {
        let transport = |source: reqwest::Error| Error::Transport {
            url: url.to_owned(),
            source: Box::new(source),
        };
        let response = self.client.get(url).send().await.map_err(transport)?;
        let status = response.status();
        tracing::debug!(url, status = status.as_u16(), "response");
        if status.is_success() {
            return response.bytes().await.map_err(transport);
        }
        if status.as_u16() == 404 {
            return Err(Error::NotFound {
                url: url.to_owned(),
            });
        }
        let retry_after = response
            .headers()
            .get(RETRY_AFTER)
            .and_then(|v| v.to_str().ok())
            .and_then(|v| v.trim().parse::<u64>().ok())
            .map(Duration::from_secs);
        Err(Error::Status {
            url: url.to_owned(),
            status: status.as_u16(),
            retry_after,
        })
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
        // unit = 0 gives the lower bound: half of the capped backoff.
        let lows: Vec<_> = (0..6).map(|n| POLICY.delay(n, 0.0, None)).collect();
        assert_eq!(
            lows,
            [50, 100, 200, 400, 500, 500].map(Duration::from_millis)
        );
    }

    #[test]
    fn delay_jitter_spans_the_upper_half() {
        let high = POLICY.delay(1, 1.0, None);
        assert_eq!(high, Duration::from_millis(200), "unit=1 gives the cap");
        let mid = POLICY.delay(1, 0.5, None);
        assert_eq!(mid, Duration::from_millis(150));
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
    fn none_policy_has_no_wait() {
        assert_eq!(RetryPolicy::NONE.delay(3, 0.9, None), Duration::ZERO);
    }
}

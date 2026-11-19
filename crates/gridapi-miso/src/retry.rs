//! Retry policy: exponential backoff with full jitter.

use std::time::Duration;

/// How failed requests are retried.
///
/// Retries apply to HTTP 429, HTTP 5xx and connection or timeout errors. The
/// delay before retry `n` is drawn uniformly from `0..=min(max_delay, base_delay * 2^n)`
/// (full jitter), and is raised to the server's `Retry-After` when that is longer.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RetryPolicy {
    /// Total attempts including the first. `1` disables retrying.
    pub max_attempts: u32,
    /// The backoff ceiling before the first retry.
    pub base_delay: Duration,
    /// The longest a single backoff may grow to.
    pub max_delay: Duration,
}

impl Default for RetryPolicy {
    fn default() -> Self {
        Self {
            max_attempts: 4,
            base_delay: Duration::from_millis(500),
            max_delay: Duration::from_secs(20),
        }
    }
}

impl RetryPolicy {
    /// A policy that never retries.
    pub const fn none() -> Self {
        Self {
            max_attempts: 1,
            base_delay: Duration::ZERO,
            max_delay: Duration::ZERO,
        }
    }

    /// The jitter ceiling for the retry that follows failed attempt `attempt` (0-based).
    pub(crate) fn ceiling(&self, attempt: u32) -> Duration {
        let factor = 1u32.checked_shl(attempt).unwrap_or(u32::MAX);
        self.base_delay
            .checked_mul(factor)
            .unwrap_or(self.max_delay)
            .min(self.max_delay)
    }

    /// The delay to sleep before the retry that follows failed attempt `attempt`.
    pub(crate) fn delay(&self, attempt: u32, retry_after: Option<Duration>) -> Duration {
        let ceiling = self.ceiling(attempt);
        let jittered = ceiling.mul_f64(fastrand::f64());
        retry_after.map_or(jittered, |ra| jittered.max(ra.min(self.max_delay)))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ceiling_doubles_then_caps() {
        let p = RetryPolicy {
            max_attempts: 8,
            base_delay: Duration::from_millis(100),
            max_delay: Duration::from_millis(700),
        };
        let got: Vec<u128> = (0..5).map(|a| p.ceiling(a).as_millis()).collect();
        assert_eq!(got, vec![100, 200, 400, 700, 700]);
    }

    #[test]
    fn huge_attempt_counts_do_not_overflow() {
        let p = RetryPolicy::default();
        assert_eq!(p.ceiling(500), p.max_delay);
    }

    #[test]
    fn delay_stays_within_ceiling_and_honours_retry_after() {
        let p = RetryPolicy::default();
        for attempt in 0..6 {
            let d = p.delay(attempt, None);
            assert!(d <= p.ceiling(attempt), "attempt {attempt}: {d:?}");
        }
        let waited = p.delay(0, Some(Duration::from_secs(3)));
        assert!(waited >= Duration::from_secs(3), "{waited:?}");
        let capped = p.delay(0, Some(Duration::from_secs(3600)));
        assert!(capped <= p.max_delay);
    }
}

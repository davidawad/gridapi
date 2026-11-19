//! Retry policy: exponential backoff with full jitter on `429`, `5xx` and
//! transient transport failures. Nothing is shared between callers; every
//! request computes its own delays.

use std::time::Duration;

/// How hard to try before giving up on a request.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RetryPolicy {
    /// Total attempts, including the first. `1` disables retrying.
    pub max_attempts: u32,
    /// Backoff ceiling for the first retry; doubles on each further retry.
    pub base_delay: Duration,
    /// Upper bound for any single wait (also caps a server `Retry-After`).
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

    /// Full-jitter delay before retry number `retry` (0 is the first retry).
    ///
    /// `unit` is a uniform sample in `[0, 1)`; passing it in keeps this pure
    /// and testable. The result is `unit * min(max_delay, base_delay * 2^retry)`.
    pub fn backoff(&self, retry: u32, unit: f64) -> Duration {
        let factor = 2u32.saturating_pow(retry.min(16));
        let ceiling = self.base_delay.saturating_mul(factor).min(self.max_delay);
        ceiling.mul_f64(unit.clamp(0.0, 1.0))
    }

    /// Delay to use given an optional server `Retry-After`: the larger of the
    /// jittered backoff and the server hint, never beyond `max_delay`.
    pub fn delay(&self, retry: u32, unit: f64, retry_after: Option<Duration>) -> Duration {
        let jittered = self.backoff(retry, unit);
        retry_after
            .map_or(jittered, |hint| hint.max(jittered))
            .min(self.max_delay)
    }
}

/// Whether an HTTP status is worth retrying.
pub fn is_retryable_status(status: u16) -> bool {
    status == 429 || status == 408 || (500..=599).contains(&status)
}

/// Parse a `Retry-After` header given in delta-seconds. HTTP-date values are
/// ignored (AESO and IESO use seconds when they send the header at all).
pub fn parse_retry_after(value: &str) -> Option<Duration> {
    value.trim().parse::<u64>().ok().map(Duration::from_secs)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn backoff_doubles_until_the_cap_and_scales_with_the_jitter_sample() {
        let p = RetryPolicy {
            max_attempts: 5,
            base_delay: Duration::from_millis(100),
            max_delay: Duration::from_millis(700),
        };
        assert_eq!(p.backoff(0, 1.0), Duration::from_millis(100));
        assert_eq!(p.backoff(1, 1.0), Duration::from_millis(200));
        assert_eq!(p.backoff(2, 1.0), Duration::from_millis(400));
        assert_eq!(p.backoff(3, 1.0), Duration::from_millis(700), "capped");
        assert_eq!(
            p.backoff(30, 1.0),
            Duration::from_millis(700),
            "no overflow"
        );
        assert_eq!(p.backoff(2, 0.5), Duration::from_millis(200));
        assert_eq!(p.backoff(2, 0.0), Duration::ZERO);
    }

    #[test]
    fn backoff_clamps_a_bad_jitter_sample() {
        let p = RetryPolicy::default();
        assert_eq!(p.backoff(0, 7.0), p.backoff(0, 1.0));
        assert_eq!(p.backoff(0, -1.0), Duration::ZERO);
    }

    #[test]
    fn retry_after_wins_when_larger_but_is_capped() {
        let p = RetryPolicy {
            max_attempts: 3,
            base_delay: Duration::from_millis(100),
            max_delay: Duration::from_secs(2),
        };
        assert_eq!(
            p.delay(0, 0.0, Some(Duration::from_secs(1))),
            Duration::from_secs(1)
        );
        assert_eq!(
            p.delay(0, 0.0, Some(Duration::from_secs(60))),
            Duration::from_secs(2)
        );
        assert_eq!(p.delay(0, 1.0, None), Duration::from_millis(100));
    }

    #[test]
    fn only_throttling_and_server_errors_retry() {
        for s in [408, 429, 500, 502, 503, 504] {
            assert!(is_retryable_status(s), "{s}");
        }
        for s in [200, 301, 400, 401, 403, 404] {
            assert!(!is_retryable_status(s), "{s}");
        }
    }

    #[test]
    fn retry_after_parses_seconds_only() {
        assert_eq!(parse_retry_after(" 7 "), Some(Duration::from_secs(7)));
        assert_eq!(parse_retry_after("Wed, 21 Oct 2026 07:28:00 GMT"), None);
    }
}

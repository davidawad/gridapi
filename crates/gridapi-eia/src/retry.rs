//! Retry policy: exponential backoff with jitter on 429 and 5xx.

use std::time::Duration;

/// How hard to retry transient failures (HTTP 429, 5xx, transport errors).
///
/// Delays use "equal jitter": half of the exponential cap is fixed and half
/// is random, so concurrent callers never retry in lockstep and never get a
/// zero wait. A server `Retry-After` header, when present, is honoured up to
/// [`RetryPolicy::max_delay`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RetryPolicy {
    /// Retries after the first attempt (0 disables retrying).
    pub max_retries: u32,
    /// Exponential base: attempt `n` waits up to `base_delay * 2^n`.
    pub base_delay: Duration,
    /// Upper bound for any single wait.
    pub max_delay: Duration,
}

impl Default for RetryPolicy {
    fn default() -> Self {
        Self {
            max_retries: 4,
            base_delay: Duration::from_millis(500),
            max_delay: Duration::from_secs(30),
        }
    }
}

impl RetryPolicy {
    /// A policy that never retries.
    #[must_use]
    pub const fn none() -> Self {
        Self {
            max_retries: 0,
            base_delay: Duration::ZERO,
            max_delay: Duration::ZERO,
        }
    }

    /// Wait before retry number `retry` (0-based), given a jitter draw in
    /// `[0, 1)` and an optional server-requested delay. Pure, so it is unit
    /// tested without sleeping.
    #[must_use]
    pub fn delay(&self, retry: u32, jitter: f64, retry_after: Option<Duration>) -> Duration {
        let cap = self
            .base_delay
            .saturating_mul(2u32.saturating_pow(retry.min(20)))
            .min(self.max_delay);
        let jitter = jitter.clamp(0.0, 1.0);
        let computed = cap / 2 + cap.div_f64(2.0).mul_f64(jitter);
        retry_after.map_or(computed, |ra| ra.min(self.max_delay).max(computed.min(ra)))
    }
}

/// 429 and every 5xx are transient.
#[must_use]
pub fn is_retryable_status(status: u16) -> bool {
    status == 429 || (500..600).contains(&status)
}

/// Parse a `Retry-After` header holding delta-seconds (the form EIA's gateway
/// uses). HTTP-date values are ignored and fall back to computed backoff.
#[must_use]
pub fn parse_retry_after(value: &str) -> Option<Duration> {
    value.trim().parse::<u64>().ok().map(Duration::from_secs)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn p() -> RetryPolicy {
        RetryPolicy {
            max_retries: 5,
            base_delay: Duration::from_millis(100),
            max_delay: Duration::from_secs(2),
        }
    }

    #[test]
    fn delay_grows_exponentially_with_jitter_bounds() {
        let lo = |n| p().delay(n, 0.0, None);
        let hi = |n| p().delay(n, 1.0, None);
        assert_eq!(lo(0), Duration::from_millis(50));
        assert_eq!(hi(0), Duration::from_millis(100));
        assert_eq!(lo(1), Duration::from_millis(100));
        assert_eq!(hi(1), Duration::from_millis(200));
        assert_eq!(hi(2), Duration::from_millis(400));
    }

    #[test]
    fn delay_is_capped_and_never_zero() {
        assert_eq!(p().delay(30, 1.0, None), Duration::from_secs(2));
        assert!(p().delay(0, 0.0, None) > Duration::ZERO);
        assert_eq!(p().delay(100, 0.5, None), Duration::from_millis(1500));
    }

    #[test]
    fn out_of_range_jitter_is_clamped() {
        assert_eq!(p().delay(0, 7.0, None), p().delay(0, 1.0, None));
        assert_eq!(p().delay(0, -3.0, None), p().delay(0, 0.0, None));
    }

    #[test]
    fn retry_after_is_honoured_and_capped() {
        let one = Some(Duration::from_secs(1));
        assert_eq!(p().delay(0, 0.0, one), Duration::from_secs(1));
        let huge = Some(Duration::from_secs(3600));
        assert_eq!(p().delay(0, 0.0, huge), Duration::from_secs(2));
    }

    #[test]
    fn status_classification() {
        for s in [429, 500, 502, 503, 504] {
            assert!(is_retryable_status(s), "{s}");
        }
        for s in [200, 301, 400, 401, 403, 404] {
            assert!(!is_retryable_status(s), "{s}");
        }
    }

    #[test]
    fn retry_after_parsing() {
        assert_eq!(parse_retry_after(" 7 "), Some(Duration::from_secs(7)));
        assert_eq!(parse_retry_after("Wed, 21 Oct 2026 07:28:00 GMT"), None);
    }
}

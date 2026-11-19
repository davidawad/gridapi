//! Retry policy: exponential backoff with jitter, never a queue.

use std::time::Duration;

use rand::Rng;

/// How many times to try a request and how long to wait between tries.
///
/// The delay before retry `n` (starting at 0) is drawn uniformly from the upper
/// half of `min(max_delay, base_delay * 2^n)`, so concurrent clients that fail
/// together do not retry together.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RetryPolicy {
    /// Total attempts including the first; `1` disables retrying.
    pub max_attempts: u32,
    /// Backoff ceiling for the first retry.
    pub base_delay: Duration,
    /// Upper bound for any single backoff.
    pub max_delay: Duration,
}

impl Default for RetryPolicy {
    fn default() -> Self {
        Self {
            max_attempts: 5,
            base_delay: Duration::from_secs(2),
            max_delay: Duration::from_secs(60),
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

    /// The backoff ceiling before jitter for retry number `retry` (0-based).
    #[must_use]
    pub fn ceiling(&self, retry: u32) -> Duration {
        let factor = 1u32.checked_shl(retry.min(20)).unwrap_or(u32::MAX);
        self.base_delay
            .checked_mul(factor)
            .map_or(self.max_delay, |d| d.min(self.max_delay))
    }

    /// The backoff for retry number `retry`, jittered by `unit` in `[0, 1)`.
    #[must_use]
    pub fn delay_with(&self, retry: u32, unit: f64) -> Duration {
        let ceiling = self.ceiling(retry);
        ceiling.mul_f64(0.5 + 0.5 * unit.clamp(0.0, 1.0))
    }

    /// The jittered backoff for retry number `retry`, using thread-local randomness.
    #[must_use]
    pub fn delay(&self, retry: u32) -> Duration {
        self.delay_with(retry, rand::thread_rng().gen::<f64>())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ceiling_doubles_then_caps() {
        let p = RetryPolicy::default();
        assert_eq!(p.ceiling(0), Duration::from_secs(2));
        assert_eq!(p.ceiling(1), Duration::from_secs(4));
        assert_eq!(p.ceiling(4), Duration::from_secs(32));
        assert_eq!(p.ceiling(5), Duration::from_secs(60));
        assert_eq!(p.ceiling(1000), Duration::from_secs(60));
    }

    #[test]
    fn jitter_stays_in_upper_half() {
        let p = RetryPolicy::default();
        assert_eq!(p.delay_with(1, 0.0), Duration::from_secs(2));
        assert_eq!(p.delay_with(1, 1.0), Duration::from_secs(4));
        let sampled = p.delay(2);
        assert!(sampled >= Duration::from_secs(4) && sampled <= Duration::from_secs(8));
    }

    #[test]
    fn none_means_single_attempt() {
        assert_eq!(RetryPolicy::none().max_attempts, 1);
    }
}

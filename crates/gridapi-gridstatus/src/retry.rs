//! Retry policy: exponential backoff with jitter, honouring `Retry-After`.

use std::time::{Duration, SystemTime, UNIX_EPOCH};

/// How the client retries 429, 500, 502, 503, 504 and transient transport errors.
///
/// Delay before retry `n` (0-based) is `min(max_delay, base_delay * 2^n)`,
/// scaled by a random factor in `[0.5, 1.0]` ("equal jitter") so concurrent
/// clients do not retry in lockstep. A server `Retry-After` raises the delay
/// to at least that value; if it exceeds `max_delay` the client gives up
/// instead of sleeping that long.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RetryPolicy {
    /// Retries after the first attempt (0 disables retrying).
    pub max_retries: u32,
    /// Delay before the first retry, before jitter.
    pub base_delay: Duration,
    /// Cap on any single delay, and on how long `Retry-After` may ask us to wait.
    pub max_delay: Duration,
}

impl RetryPolicy {
    /// 3 retries, 500 ms base, 30 s cap.
    pub const DEFAULT: Self = Self {
        max_retries: 3,
        base_delay: Duration::from_millis(500),
        max_delay: Duration::from_secs(30),
    };

    /// Never retry.
    pub const NONE: Self = Self {
        max_retries: 0,
        base_delay: Duration::ZERO,
        max_delay: Duration::ZERO,
    };

    /// Delay before retry number `retry` (0-based), given a jitter draw in `[0, 1]`
    /// and the server's `Retry-After`, or `None` when the server asked for longer
    /// than `max_delay` and the caller should give up.
    pub fn delay(
        &self,
        retry: u32,
        jitter: f64,
        retry_after: Option<Duration>,
    ) -> Option<Duration> {
        if retry_after.is_some_and(|ra| ra > self.max_delay) {
            return None;
        }
        let exp = self
            .base_delay
            .saturating_mul(2_u32.saturating_pow(retry.min(16)))
            .min(self.max_delay);
        let factor = 0.5 + 0.5 * jitter.clamp(0.0, 1.0);
        let backoff = exp.mul_f64(factor);
        Some(retry_after.map_or(backoff, |ra| ra.max(backoff)))
    }
}

impl Default for RetryPolicy {
    fn default() -> Self {
        Self::DEFAULT
    }
}

/// A jitter draw in `[0, 1)` from the clock and the attempt number (splitmix64).
/// No global RNG state; quality only needs to decorrelate retrying clients.
pub(crate) fn jitter(seed: u32) -> f64 {
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |d| u64::try_from(d.as_nanos()).unwrap_or(0));
    let mut z = nanos.wrapping_add(u64::from(seed).wrapping_mul(0x9E37_79B9_7F4A_7C15));
    z = (z ^ (z >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
    z = (z ^ (z >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
    z ^= z >> 31;
    // Top 53 bits to a double in [0, 1).
    #[expect(clippy::cast_precision_loss, reason = "53-bit value fits f64 exactly")]
    let unit = (z >> 11) as f64 / (1_u64 << 53) as f64;
    unit
}

#[cfg(test)]
mod tests {
    use super::*;

    const P: RetryPolicy = RetryPolicy::DEFAULT;

    #[test]
    fn backoff_doubles_and_caps() {
        // jitter 1.0 gives the full delay.
        assert_eq!(P.delay(0, 1.0, None), Some(Duration::from_millis(500)));
        assert_eq!(P.delay(1, 1.0, None), Some(Duration::from_secs(1)));
        assert_eq!(P.delay(2, 1.0, None), Some(Duration::from_secs(2)));
        assert_eq!(P.delay(20, 1.0, None), Some(Duration::from_secs(30)));
    }

    #[test]
    fn jitter_scales_between_half_and_full() {
        assert_eq!(P.delay(0, 0.0, None), Some(Duration::from_millis(250)));
        assert_eq!(P.delay(0, 0.5, None), Some(Duration::from_millis(375)));
    }

    #[test]
    fn retry_after_is_a_floor() {
        let d = P.delay(0, 1.0, Some(Duration::from_secs(5)));
        assert_eq!(d, Some(Duration::from_secs(5)));
        // Backoff larger than the hint wins.
        let d = P.delay(3, 1.0, Some(Duration::from_millis(100)));
        assert_eq!(d, Some(Duration::from_secs(4)));
    }

    #[test]
    fn gives_up_when_retry_after_exceeds_cap() {
        assert_eq!(P.delay(0, 1.0, Some(Duration::from_secs(3600))), None);
    }

    #[test]
    fn jitter_draws_stay_in_unit_interval_and_vary() {
        let draws: Vec<f64> = (0..64).map(jitter).collect();
        assert!(draws.iter().all(|d| (0.0..1.0).contains(d)));
        let first = draws[0];
        assert!(draws.iter().any(|d| (d - first).abs() > 1e-9));
    }
}

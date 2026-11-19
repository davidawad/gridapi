//! Client-side request pacing and retry back-off.
//!
//! PJM's Data Miner 2 terms cap non-members at 6 data connections per minute
//! and members at 600
//! (<https://www.pjm.com/markets-and-operations/etools/data-miner-2>). The
//! limiter is a strict sliding window owned by one [`Client`](crate::Client)
//! (and its clones). There is no process-global state; independent clients do
//! not coordinate.

use std::collections::VecDeque;
use std::sync::Mutex;
use std::time::{Duration, Instant};

use crate::error::Error;

/// A request budget: at most `max_requests` in any `per` window.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RateLimit {
    max_requests: u32,
    per: Duration,
}

impl RateLimit {
    /// PJM non-member limit: 6 requests per minute.
    pub const NON_MEMBER: Self = Self {
        max_requests: 6,
        per: Duration::from_secs(60),
    };

    /// PJM member limit: 600 requests per minute.
    pub const MEMBER: Self = Self {
        max_requests: 600,
        per: Duration::from_secs(60),
    };

    /// A custom budget.
    ///
    /// # Errors
    /// [`Error::InvalidQuery`] when `max_requests` or `per` is zero.
    pub fn new(max_requests: u32, per: Duration) -> Result<Self, Error> {
        if max_requests == 0 || per.is_zero() {
            return Err(Error::InvalidQuery(
                "rate limit needs max_requests > 0 and a non-zero window".into(),
            ));
        }
        Ok(Self { max_requests, per })
    }

    /// Requests allowed per window.
    #[must_use]
    pub fn max_requests(&self) -> u32 {
        self.max_requests
    }

    /// The window length.
    #[must_use]
    pub fn per(&self) -> Duration {
        self.per
    }
}

/// Sliding-window pacer. `reserve` hands out the earliest permitted slot for a
/// request; the caller sleeps outside any lock. The mutex guards a few
/// timestamps for nanoseconds and is never held across an `.await`.
#[derive(Debug)]
pub(crate) struct Throttle {
    limit: Option<RateLimit>,
    slots: Mutex<VecDeque<Instant>>,
}

impl Throttle {
    pub(crate) fn new(limit: Option<RateLimit>) -> Self {
        Self {
            limit,
            slots: Mutex::new(VecDeque::new()),
        }
    }

    /// Wait until the request budget allows one more request.
    pub(crate) async fn acquire(&self) {
        let wait = self.reserve(Instant::now());
        if !wait.is_zero() {
            tracing::debug!(
                wait_ms = wait.as_millis(),
                "gridapi-pjm: throttling request"
            );
            tokio::time::sleep(wait).await;
        }
    }

    /// Reserve a slot at or after `now`; returns how long to wait for it.
    pub(crate) fn reserve(&self, now: Instant) -> Duration {
        let Some(limit) = self.limit else {
            return Duration::ZERO;
        };
        let mut slots = self
            .slots
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        // Slots older than one window no longer constrain anything.
        while slots.front().is_some_and(|&s| s + limit.per <= now) {
            slots.pop_front();
        }
        let slot = if slots.len() >= limit.max_requests as usize {
            // The oldest of the last `max_requests` slots sets the earliest
            // time the window has room again.
            slots
                .front()
                .map_or(now, |&oldest| (oldest + limit.per).max(now))
        } else {
            now
        };
        slots.push_back(slot);
        while slots.len() > limit.max_requests as usize {
            slots.pop_front();
        }
        slot.saturating_duration_since(now)
    }
}

/// Retry policy for 429, 5xx and transient transport failures.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RetryPolicy {
    /// Retries after the first attempt (0 disables retrying).
    pub max_retries: u32,
    /// Back-off ceiling for the first retry; doubles each attempt.
    pub base_delay: Duration,
    /// Upper bound on any single back-off.
    pub max_delay: Duration,
}

impl Default for RetryPolicy {
    fn default() -> Self {
        Self {
            max_retries: 3,
            base_delay: Duration::from_secs(1),
            max_delay: Duration::from_secs(30),
        }
    }
}

/// Longest `Retry-After` the client will honour, whatever the server asks.
pub(crate) const MAX_RETRY_AFTER: Duration = Duration::from_secs(120);

/// Full-jitter exponential back-off: a uniform draw in
/// `[0, min(max_delay, base_delay * 2^attempt))`, never shorter than the
/// server's `Retry-After` (capped at [`MAX_RETRY_AFTER`]).
///
/// `jitter` is a uniform sample in `[0, 1)`; `attempt` counts from 0.
pub(crate) fn backoff_delay(
    policy: &RetryPolicy,
    attempt: u32,
    jitter: f64,
    retry_after: Option<Duration>,
) -> Duration {
    let ceiling = policy
        .base_delay
        .saturating_mul(2u32.saturating_pow(attempt.min(16)))
        .min(policy.max_delay);
    let jittered = ceiling.mul_f64(jitter.clamp(0.0, 1.0));
    retry_after.map_or(jittered, |ra| jittered.max(ra.min(MAX_RETRY_AFTER)))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn t0() -> Instant {
        Instant::now()
    }

    #[test]
    fn allows_a_burst_up_to_the_limit_then_waits_for_the_window() {
        let throttle = Throttle::new(Some(RateLimit::NON_MEMBER));
        let start = t0();
        for _ in 0..6 {
            assert_eq!(throttle.reserve(start), Duration::ZERO);
        }
        // The 7th request must wait for the first slot to age out.
        assert_eq!(throttle.reserve(start), Duration::from_secs(60));
        // The 8th is spaced after the 2nd slot, which was also at `start`.
        assert_eq!(throttle.reserve(start), Duration::from_secs(60));
    }

    #[test]
    fn slots_age_out_after_a_window() {
        let throttle = Throttle::new(Some(RateLimit::NON_MEMBER));
        let start = t0();
        for _ in 0..6 {
            throttle.reserve(start);
        }
        let later = start + Duration::from_secs(61);
        assert_eq!(throttle.reserve(later), Duration::ZERO);
    }

    #[test]
    fn never_more_than_limit_in_any_window() {
        let limit = RateLimit::new(3, Duration::from_secs(10)).unwrap();
        let throttle = Throttle::new(Some(limit));
        let start = t0();
        // 12 requests arriving at once get slots; no window of 10 s holds >3.
        let slots: Vec<Duration> = (0..12).map(|_| throttle.reserve(start)).collect();
        for (i, s) in slots.iter().enumerate() {
            let in_window = slots
                .iter()
                .filter(|o| **o >= *s && **o < *s + Duration::from_secs(10))
                .count();
            assert!(
                in_window <= 3,
                "window starting at slot {i} holds {in_window}"
            );
        }
    }

    #[test]
    fn unlimited_never_waits() {
        let throttle = Throttle::new(None);
        let start = t0();
        assert!((0..1000).all(|_| throttle.reserve(start).is_zero()));
    }

    #[test]
    fn clients_do_not_share_state() {
        let a = Throttle::new(Some(RateLimit::NON_MEMBER));
        let b = Throttle::new(Some(RateLimit::NON_MEMBER));
        let start = t0();
        for _ in 0..6 {
            a.reserve(start);
        }
        assert_eq!(b.reserve(start), Duration::ZERO);
    }

    #[test]
    fn rate_limit_rejects_zero() {
        assert!(RateLimit::new(0, Duration::from_secs(1)).is_err());
        assert!(RateLimit::new(1, Duration::ZERO).is_err());
        assert_eq!(RateLimit::MEMBER.max_requests(), 600);
        assert_eq!(RateLimit::MEMBER.per(), Duration::from_secs(60));
    }

    #[test]
    fn backoff_is_bounded_by_the_doubling_ceiling() {
        let p = RetryPolicy::default();
        assert_eq!(backoff_delay(&p, 0, 0.999_999, None).as_secs(), 0);
        assert_eq!(backoff_delay(&p, 0, 1.0, None), Duration::from_secs(1));
        assert_eq!(backoff_delay(&p, 2, 1.0, None), Duration::from_secs(4));
        assert_eq!(backoff_delay(&p, 10, 1.0, None), p.max_delay);
        assert_eq!(backoff_delay(&p, 2, 0.0, None), Duration::ZERO);
        assert_eq!(backoff_delay(&p, 2, 0.5, None), Duration::from_secs(2));
    }

    #[test]
    fn backoff_honours_retry_after_but_caps_it() {
        let p = RetryPolicy::default();
        let ra = Some(Duration::from_secs(20));
        assert_eq!(backoff_delay(&p, 0, 0.5, ra), Duration::from_secs(20));
        let huge = Some(Duration::from_secs(86_400));
        assert_eq!(backoff_delay(&p, 0, 0.5, huge), MAX_RETRY_AFTER);
    }

    #[test]
    fn backoff_survives_extreme_attempts() {
        let p = RetryPolicy::default();
        assert_eq!(backoff_delay(&p, u32::MAX, 1.0, None), p.max_delay);
    }
}

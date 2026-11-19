//! Lock-free per-instance request spacing.
//!
//! Each caller atomically reserves the next free send slot with a
//! compare-and-swap, then sleeps until its slot. No mutex, no queue, no
//! shared state beyond one `AtomicU64` owned by the client instance (the
//! Grid Status free tier allows 1 request per second).

use std::sync::atomic::{AtomicU64, Ordering};
use std::time::Duration;

use tokio::time::Instant;

/// Spaces calls at least `interval` apart. Cheap to share behind an `Arc`.
#[derive(Debug)]
pub(crate) struct Throttle {
    origin: Instant,
    interval_nanos: u64,
    /// Nanoseconds since `origin` at which the next call may start.
    next_slot: AtomicU64,
}

impl Throttle {
    pub(crate) fn new(interval: Duration) -> Self {
        Self {
            origin: Instant::now(),
            interval_nanos: u64::try_from(interval.as_nanos()).unwrap_or(u64::MAX),
            next_slot: AtomicU64::new(0),
        }
    }

    /// Reserve the next slot and return how long the caller must wait for it.
    fn reserve(&self, now_nanos: u64) -> u64 {
        let mut current = self.next_slot.load(Ordering::Acquire);
        loop {
            let slot = current.max(now_nanos);
            let next = slot.saturating_add(self.interval_nanos);
            match self.next_slot.compare_exchange_weak(
                current,
                next,
                Ordering::AcqRel,
                Ordering::Acquire,
            ) {
                Ok(_) => return slot - now_nanos,
                Err(actual) => current = actual,
            }
        }
    }

    /// Record that a request just finished: the next one may not start until `interval`
    /// after this moment. The server's window is measured around the response, not the send:
    /// a call that took 1.7 s was followed 0.0 s after its response by one that drew
    /// `429 Too Many Requests. Limit: 1 per 1 second` (live, 2026-11-18).
    pub(crate) fn finish(&self) {
        let now = u64::try_from(self.origin.elapsed().as_nanos()).unwrap_or(u64::MAX);
        self.complete(now);
    }

    fn complete(&self, now_nanos: u64) {
        self.next_slot.fetch_max(
            now_nanos.saturating_add(self.interval_nanos),
            Ordering::AcqRel,
        );
    }

    /// Wait until this caller's slot arrives.
    pub(crate) async fn wait(&self) {
        let now = u64::try_from(self.origin.elapsed().as_nanos()).unwrap_or(u64::MAX);
        let delay = self.reserve(now);
        if delay > 0 {
            tokio::time::sleep(Duration::from_nanos(delay)).await;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn first_call_is_free_then_spaced_by_interval() {
        let t = Throttle::new(Duration::from_secs(1));
        assert_eq!(t.reserve(0), 0);
        assert_eq!(t.reserve(0), 1_000_000_000);
        assert_eq!(t.reserve(0), 2_000_000_000);
    }

    /// Regression: a slow response must push the next send back by a full
    /// interval from its completion.
    #[test]
    fn a_slow_response_delays_the_next_slot_from_its_completion() {
        let t = Throttle::new(Duration::from_secs(1));
        assert_eq!(t.reserve(0), 0);
        t.complete(1_700_000_000);
        assert_eq!(t.reserve(1_700_000_000), 1_000_000_000);
    }

    #[test]
    fn idle_time_refills_the_slot() {
        let t = Throttle::new(Duration::from_secs(1));
        assert_eq!(t.reserve(0), 0);
        // 5s later the previous reservation is long past: no wait.
        assert_eq!(t.reserve(5_000_000_000), 0);
        assert_eq!(t.reserve(5_000_000_000), 1_000_000_000);
    }

    #[tokio::test]
    async fn concurrent_callers_get_distinct_slots() {
        let t = std::sync::Arc::new(Throttle::new(Duration::from_millis(40)));
        let start = Instant::now();
        let handles: Vec<_> = (0..4)
            .map(|_| {
                let t = std::sync::Arc::clone(&t);
                tokio::spawn(async move { t.wait().await })
            })
            .collect();
        for h in handles {
            h.await.expect("task joins");
        }
        // 4 callers at 40ms spacing: the last waits ~120ms.
        assert!(start.elapsed() >= Duration::from_millis(110));
    }
}

//! Per-client request pacing (OASIS allows about one request per five seconds).

use std::sync::atomic::{AtomicU64, Ordering};
use std::time::Duration;

use tokio::time::Instant;

/// Hands out request slots at least `interval` apart.
///
/// The state is one atomic offset, so callers never wait on a lock: each caller
/// reserves the next free slot with a compare-and-swap, then sleeps until it.
/// One pacer belongs to one [`Client`](crate::Client); separate clients do not
/// share it.
#[derive(Debug)]
pub struct Pacer {
    origin: Instant,
    interval: Duration,
    /// Nanoseconds after `origin` at which the next slot opens.
    next_slot: AtomicU64,
}

fn nanos(duration: Duration) -> u64 {
    u64::try_from(duration.as_nanos()).unwrap_or(u64::MAX)
}

impl Pacer {
    /// A pacer whose first slot is open immediately.
    #[must_use]
    pub fn new(interval: Duration) -> Self {
        Self {
            origin: Instant::now(),
            interval,
            next_slot: AtomicU64::new(0),
        }
    }

    /// The configured gap between slots.
    #[must_use]
    pub fn interval(&self) -> Duration {
        self.interval
    }

    /// Claims the next slot and returns the instant it opens.
    fn reserve(&self) -> Instant {
        let now = nanos(self.origin.elapsed());
        let step = nanos(self.interval);
        let mut current = self.next_slot.load(Ordering::Acquire);
        loop {
            let slot = current.max(now);
            match self.next_slot.compare_exchange_weak(
                current,
                slot.saturating_add(step),
                Ordering::AcqRel,
                Ordering::Acquire,
            ) {
                Ok(_) => return self.origin + Duration::from_nanos(slot),
                Err(seen) => current = seen,
            }
        }
    }

    /// Sleeps until this caller's slot opens.
    pub async fn wait(&self) {
        tokio::time::sleep_until(self.reserve()).await;
    }

    /// Pushes the next slot out to at least `delay` from now.
    ///
    /// Used after a 429: the server said to back off, so every later request
    /// from this client honours it.
    pub fn defer(&self, delay: Duration) {
        let floor = nanos(self.origin.elapsed()).saturating_add(nanos(delay));
        self.next_slot.fetch_max(floor, Ordering::AcqRel);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test(start_paused = true)]
    async fn slots_are_spaced_by_the_interval() {
        let pacer = Pacer::new(Duration::from_secs(5));
        let begin = Instant::now();
        pacer.wait().await;
        assert!(
            begin.elapsed() < Duration::from_millis(1),
            "first slot is free"
        );
        pacer.wait().await;
        assert_eq!(begin.elapsed(), Duration::from_secs(5));
        pacer.wait().await;
        assert_eq!(begin.elapsed(), Duration::from_secs(10));
    }

    #[tokio::test(start_paused = true)]
    async fn concurrent_callers_get_distinct_slots() {
        let pacer = std::sync::Arc::new(Pacer::new(Duration::from_secs(5)));
        let begin = Instant::now();
        let tasks: Vec<_> = (0..3)
            .map(|_| {
                let pacer = std::sync::Arc::clone(&pacer);
                tokio::spawn(async move {
                    pacer.wait().await;
                    begin.elapsed()
                })
            })
            .collect();
        let mut done = Vec::new();
        for task in tasks {
            done.push(task.await.unwrap_or_default());
        }
        done.sort();
        assert_eq!(
            done,
            [
                Duration::ZERO,
                Duration::from_secs(5),
                Duration::from_secs(10)
            ]
        );
    }

    #[tokio::test(start_paused = true)]
    async fn defer_extends_the_wait() {
        let pacer = Pacer::new(Duration::from_secs(1));
        let begin = Instant::now();
        pacer.defer(Duration::from_secs(7));
        pacer.wait().await;
        assert_eq!(begin.elapsed(), Duration::from_secs(7));
    }
}

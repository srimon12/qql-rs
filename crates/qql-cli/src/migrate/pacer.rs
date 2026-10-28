//! Cross-worker pace limiter for upsert batch dispatch.
//!
//! `--batch-delay-ms` only sleeps between ingest windows and `--workers`
//! dispatches that many concurrent upserts, so a window can still burst. The
//! pacer hands out one dispatch slot per `1 / max_rps` across every worker.

use std::sync::{Arc, Mutex};
use std::time::Duration;

use tokio::time::Instant;

/// Shared next-slot limiter. Burst is one: the first dispatch runs immediately.
///
/// One pacer covers every worker in this process. Two concurrent `qql migrate`
/// runs do not share a budget.
#[derive(Debug)]
pub(crate) struct Pacer {
    interval: Duration,
    next: Mutex<Instant>,
}

impl Pacer {
    /// `max_rps == 0` means unlimited (no pacer).
    pub(crate) fn new(max_rps: u64) -> Option<Arc<Self>> {
        if max_rps == 0 {
            return None;
        }
        Some(Arc::new(Self {
            interval: Duration::from_secs_f64(1.0 / max_rps as f64),
            next: Mutex::new(Instant::now()),
        }))
    }

    /// Reserve the next slot, then sleep for it without holding the lock.
    ///
    /// An idle pacer must not bank credit: the first slot after a quiet period
    /// is immediate, and the next one is a full interval later. A waiter
    /// cancelled mid-sleep has already reserved its slot, so the next caller
    /// still waits for the interval.
    pub(crate) async fn acquire(&self) {
        let wait = {
            let mut next = self
                .next
                .lock()
                .expect("pacer mutex is never held across an await");
            let now = Instant::now();
            if *next < now {
                *next = now;
            }
            let wait = next.saturating_duration_since(now);
            *next += self.interval;
            wait
        };
        tokio::time::sleep(wait).await;
    }
}

#[cfg(test)]
mod tests {
    use std::time::Duration;

    use super::Pacer;

    #[tokio::test(start_paused = true)]
    async fn zero_is_unlimited() {
        assert!(Pacer::new(0).is_none());
        assert!(Pacer::new(1).is_some());
    }

    #[tokio::test(start_paused = true)]
    async fn three_acquires_at_ten_rps_take_two_hundred_ms() {
        let pacer = Pacer::new(10).expect("pacing enabled");
        let start = tokio::time::Instant::now();
        for _ in 0..3 {
            pacer.acquire().await;
        }
        assert!(
            start.elapsed() >= Duration::from_millis(200),
            "three slots at 10 rps must span at least two intervals: {:?}",
            start.elapsed()
        );
    }

    #[tokio::test(start_paused = true)]
    async fn idle_time_does_not_bank_a_burst() {
        let pacer = Pacer::new(1).expect("pacing enabled");
        pacer.acquire().await;
        tokio::time::advance(Duration::from_secs(10)).await;

        let start = tokio::time::Instant::now();
        pacer.acquire().await;
        pacer.acquire().await;
        assert!(
            start.elapsed() >= Duration::from_secs(1),
            "two slots after idle time must still be one interval apart: {:?}",
            start.elapsed()
        );
    }
}

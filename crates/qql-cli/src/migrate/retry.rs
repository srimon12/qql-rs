//! Jittered exponential backoff and a consecutive-failure circuit.
//!
//! Transient ingest errors (transport, HTTP 408/425/429/5xx) are retried
//! with full jitter. Permanent backend errors (auth, not-found) fail
//! immediately. After [`CIRCUIT_THRESHOLD`] exhausted retry sequences the
//! circuit opens so concurrent workers stop piling onto a rate-limited
//! cluster.

use std::error::Error;
use std::future::Future;
use std::sync::atomic::{AtomicBool, AtomicU32, Ordering};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use qql_core::error::{ErrorKind, QqlError};

/// Attempts per call (first try + retries).
pub(crate) const MAX_ATTEMPTS: u32 = 6;
/// Exhausted retry sequences before the ingest circuit opens.
pub(crate) const CIRCUIT_THRESHOLD: u32 = 3;
const BASE_DELAY: Duration = Duration::from_millis(100);
const MAX_DELAY: Duration = Duration::from_secs(10);

/// Shareable across concurrent upsert workers.
#[derive(Debug)]
pub(crate) struct Circuit {
    consecutive_exhausted: AtomicU32,
    open: AtomicBool,
}

impl Default for Circuit {
    fn default() -> Self {
        Self {
            consecutive_exhausted: AtomicU32::new(0),
            open: AtomicBool::new(false),
        }
    }
}

impl Circuit {
    pub(crate) fn is_open(&self) -> bool {
        self.open.load(Ordering::Acquire)
    }

    pub(crate) fn on_success(&self) {
        self.consecutive_exhausted.store(0, Ordering::Release);
        self.open.store(false, Ordering::Release);
    }

    pub(crate) fn record_exhausted(&self) {
        let n = self.consecutive_exhausted.fetch_add(1, Ordering::AcqRel) + 1;
        if n >= CIRCUIT_THRESHOLD {
            self.open.store(true, Ordering::Release);
        }
    }
}

/// Transport failures and retryable HTTP statuses.
pub(crate) fn is_retryable(err: &QqlError) -> bool {
    if err.kind == ErrorKind::Transport {
        return true;
    }
    if let Some(status) = err.field("status_code").and_then(|s| s.parse::<u16>().ok()) {
        return matches!(status, 408 | 425 | 429 | 500 | 502 | 503 | 504);
    }
    false
}

fn is_retryable_error(err: &(dyn Error + 'static)) -> bool {
    if let Some(qql) = err.downcast_ref::<QqlError>() {
        return is_retryable(qql);
    }
    let lower = err.to_string().to_ascii_lowercase();
    lower.contains("429")
        || lower.contains("too many requests")
        || lower.contains("service unavailable")
        || lower.contains("timed out")
        || lower.contains("timeout")
        || lower.contains("connection reset")
        || lower.contains("connection refused")
        || lower.contains("temporarily unavailable")
}

/// Full jitter on exponential backoff: `U(0, min(cap, base*2^attempt))`.
pub(crate) fn backoff(attempt: u32) -> Duration {
    let exp = BASE_DELAY.saturating_mul(1u32 << attempt.min(6));
    let cap = exp.min(MAX_DELAY).as_millis() as u64;
    Duration::from_millis(jitter(cap.saturating_add(1)))
}

fn jitter(cap: u64) -> u64 {
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.subsec_nanos() as u64)
        .unwrap_or(0);
    nanos % cap.max(1)
}

/// Retry `op` on transient errors. Non-retryable failures propagate
/// immediately. An open circuit fails fast without calling `op`.
pub(crate) async fn retry<T, F, Fut>(circuit: &Circuit, mut op: F) -> Result<T, Box<dyn Error>>
where
    F: FnMut() -> Fut,
    Fut: Future<Output = Result<T, Box<dyn Error>>>,
{
    if circuit.is_open() {
        return Err(
            "migrate circuit open: too many consecutive transient failures (HTTP 429/5xx or transport)"
                .into(),
        );
    }
    let mut last: Option<Box<dyn Error>> = None;
    for attempt in 0..MAX_ATTEMPTS {
        match op().await {
            Ok(value) => {
                circuit.on_success();
                return Ok(value);
            }
            Err(err) if is_retryable_error(err.as_ref()) => {
                last = Some(err);
                if attempt + 1 < MAX_ATTEMPTS {
                    tokio::time::sleep(backoff(attempt)).await;
                    if circuit.is_open() {
                        return Err(
                            "migrate circuit open: too many consecutive transient failures (HTTP 429/5xx or transport)"
                                .into(),
                        );
                    }
                }
            }
            Err(err) => return Err(err),
        }
    }
    circuit.record_exhausted();
    Err(last.expect("retry loop records at least one error"))
}

#[cfg(test)]
mod tests {
    use super::{CIRCUIT_THRESHOLD, Circuit, is_retryable};
    use qql_core::error::QqlError;

    #[test]
    fn transport_and_429_are_retryable_auth_is_not() {
        let transport = QqlError::transport("QQL-TRANSPORT", "connection reset", None);
        assert!(is_retryable(&transport));
        let rate =
            QqlError::backend("QQL-BACKEND-HTTP", "too many requests", None).with_status(429);
        assert!(is_retryable(&rate));
        let auth = QqlError::backend("QQL-BACKEND-AUTH", "forbidden", None).with_status(403);
        assert!(!is_retryable(&auth));
        let missing = QqlError::backend(
            "QQL-BACKEND-COLLECTION-NOT-FOUND",
            "no such collection",
            None,
        )
        .with_status(404);
        assert!(!is_retryable(&missing));
    }

    #[test]
    fn circuit_opens_after_threshold_and_resets_on_success() {
        let circuit = Circuit::default();
        for _ in 0..CIRCUIT_THRESHOLD - 1 {
            circuit.record_exhausted();
            assert!(!circuit.is_open());
        }
        circuit.record_exhausted();
        assert!(circuit.is_open());
        circuit.on_success();
        assert!(!circuit.is_open());
    }
}

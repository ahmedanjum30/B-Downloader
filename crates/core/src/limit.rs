use std::sync::Mutex;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{Duration, Instant};

/// How far ahead of the limit a burst may run after a quiet moment, so a
/// connection that waited doesn't start with a backlog of allowance.
const BURST: Duration = Duration::from_millis(100);

/// Caps how fast all downloads together receive data. Connections report
/// what they received and wait their turn, so the limit is shared fairly
/// between them; the server slows down because we read more slowly.
#[derive(Debug, Default)]
pub(crate) struct Limiter {
    /// Bytes per second; 0 is no limit.
    rate: AtomicU64,
    /// When the data received so far is paid for at the current rate.
    paid_until: Mutex<Option<Instant>>,
}

impl Limiter {
    pub fn set(&self, bytes_per_second: Option<u64>) {
        self.rate.store(bytes_per_second.unwrap_or(0), Ordering::Relaxed);
    }

    pub fn get(&self) -> Option<u64> {
        Some(self.rate.load(Ordering::Relaxed)).filter(|&r| r > 0)
    }

    /// Accounts for `bytes` just received and returns how long to wait before reading more.
    pub fn delay(&self, bytes: u64) -> Duration {
        let Some(rate) = self.get() else { return Duration::ZERO };
        let now = Instant::now();
        let mut paid_until = self.paid_until.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
        let earliest = now.checked_sub(BURST).unwrap_or(now);
        let start = paid_until.map_or(earliest, |t| t.max(earliest));
        let until = start + Duration::from_secs_f64(bytes as f64 / rate as f64);
        *paid_until = Some(until);
        until.saturating_duration_since(now)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn no_limit_never_waits() {
        let limiter = Limiter::default();
        assert_eq!(limiter.delay(u64::MAX), Duration::ZERO);
    }

    #[test]
    fn waits_add_up_across_connections() {
        let limiter = Limiter::default();
        limiter.set(Some(1_000_000));
        // Three connections receive 500 KB each at once: 1.5 s of data at 1 MB/s.
        let waits: Vec<_> = (0..3).map(|_| limiter.delay(500_000)).collect();
        let last = waits[2].as_secs_f64();
        assert!((1.3..=1.5).contains(&last), "{waits:?}");
        assert!(waits[0] < waits[1] && waits[1] < waits[2]);
    }

    #[test]
    fn turning_it_off_stops_waiting() {
        let limiter = Limiter::default();
        limiter.set(Some(1000));
        assert!(limiter.delay(10_000) > Duration::ZERO);
        limiter.set(None);
        assert_eq!(limiter.delay(10_000), Duration::ZERO);
    }
}

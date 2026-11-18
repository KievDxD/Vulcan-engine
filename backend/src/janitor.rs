use crate::limiter::RateLimiterEngine;
use crate::telemetry::{now_epoch_ms, TelemetryStore};
use parking_lot::Mutex;
use serde::{Deserialize, Serialize};
use std::sync::Arc;
use std::time::Duration;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct JanitorStats {
    pub keys_evicted_total: u64,
    pub last_sweep_evicted: usize,
    pub sweeps_completed: u64,
    pub active_buckets: usize,
    pub uptime_secs: u64,
    pub last_sweep_timestamp_ms: u64,
    pub estimated_memory_kb: usize,
}

pub struct JanitorController {
    inner: Mutex<JanitorState>,
}

struct JanitorState {
    keys_evicted_total: u64,
    last_sweep_evicted: usize,
    sweeps_completed: u64,
    last_sweep_timestamp_ms: u64,
}

impl Default for JanitorController {
    fn default() -> Self {
        Self::new()
    }
}

impl JanitorController {
    pub fn new() -> Self {
        Self {
            inner: Mutex::new(JanitorState {
                keys_evicted_total: 0,
                last_sweep_evicted: 0,
                sweeps_completed: 0,
                last_sweep_timestamp_ms: now_epoch_ms(),
            }),
        }
    }

    /// Evicts expired DashMap key buckets, compacts stale telemetry windows, and updates memory stats.
    pub fn run_sweep(
        &self,
        limiter: &RateLimiterEngine,
        telemetry: &TelemetryStore,
        max_idle: Duration,
    ) -> JanitorStats {
        let evicted_buckets = limiter.evict_idle_buckets(max_idle);
        let compacted_windows = telemetry.compact_stale_windows();
        let total_evicted_this_pass = evicted_buckets + compacted_windows;
        let counters = limiter.get_counters();

        let mut guard = self.inner.lock();
        guard.keys_evicted_total += evicted_buckets as u64;
        guard.last_sweep_evicted = total_evicted_this_pass;
        guard.sweeps_completed += 1;
        guard.last_sweep_timestamp_ms = now_epoch_ms();

        Self::build_stats(&guard, counters.active_buckets, telemetry.uptime_secs(), telemetry.recent_events_len())
    }

    pub fn stats(&self, limiter: &RateLimiterEngine, telemetry: &TelemetryStore) -> JanitorStats {
        let guard = self.inner.lock();
        let counters = limiter.get_counters();
        Self::build_stats(
            &guard,
            counters.active_buckets,
            telemetry.uptime_secs(),
            telemetry.recent_events_len(),
        )
    }

    fn build_stats(
        guard: &JanitorState,
        active_buckets: usize,
        uptime_secs: u64,
        buffered_events: usize,
    ) -> JanitorStats {
        // Approximate memory footprint for active buckets (~192 B each) + ring events (~256 B each)
        let estimated_bytes = (active_buckets * 192) + (buffered_events * 256) + 128 * 1024;
        JanitorStats {
            keys_evicted_total: guard.keys_evicted_total,
            last_sweep_evicted: guard.last_sweep_evicted,
            sweeps_completed: guard.sweeps_completed,
            active_buckets,
            uptime_secs,
            last_sweep_timestamp_ms: guard.last_sweep_timestamp_ms,
            estimated_memory_kb: (estimated_bytes / 1024).max(128),
        }
    }
}

pub fn spawn_janitor_loop(
    limiter: Arc<RateLimiterEngine>,
    telemetry: Arc<TelemetryStore>,
    janitor: Arc<JanitorController>,
    sweep_interval: Duration,
    max_idle: Duration,
) {
    tokio::spawn(async move {
        let mut ticker = tokio::time::interval(sweep_interval);
        loop {
            ticker.tick().await;
            janitor.run_sweep(&limiter, &telemetry, max_idle);
        }
    });
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;
    use std::time::{SystemTime, UNIX_EPOCH};

    fn temp_path(tag: &str) -> PathBuf {
        let nanos = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        std::env::temp_dir().join(format!("vulcan_janitor_{}_{}.json", tag, nanos))
    }

    #[test]
    fn janitor_sweep_records_evictions_and_increments_pass_counter() {
        let limiter = RateLimiterEngine::new();
        let path = temp_path("sweep");
        let telemetry = TelemetryStore::new(&path);
        let janitor = JanitorController::new();

        limiter.evaluate("api-prod", "ephemeral-k1", 1, None);
        limiter.evaluate("api-prod", "ephemeral-k2", 1, None);

        // Zero max_idle evicts immediately
        let stats = janitor.run_sweep(&limiter, &telemetry, Duration::from_secs(0));
        assert_eq!(stats.sweeps_completed, 1);
        assert_eq!(stats.keys_evicted_total, 2);
        assert_eq!(stats.active_buckets, 0);
        assert!(stats.estimated_memory_kb >= 128);
        let _ = std::fs::remove_file(path);
    }
}

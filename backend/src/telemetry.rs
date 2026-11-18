use crate::janitor::JanitorStats;
use crate::limiter::{
    Algorithm, Decision, LimiterCounters, OverloadGuardConfig, PolicyMode, PriorityTier,
    RateLimitRule,
};
use crate::ring::ClusterTopologySnapshot;
use parking_lot::Mutex;
use serde::{Deserialize, Serialize};
use std::collections::{HashMap, VecDeque};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{Instant, SystemTime, UNIX_EPOCH};
use tokio::sync::broadcast;

pub const EVENT_RING_CAPACITY: usize = 2000;
pub const LATENCY_WINDOW_CAPACITY: usize = 1000;
pub const SERIES_HISTORY_CAPACITY: usize = 60;
pub const ANOMALY_HISTORY_CAPACITY: usize = 50;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DecisionEvent {
    pub id: u64,
    pub timestamp_ms: u64,
    pub namespace: String,
    pub client_key: String,
    pub node_id: String,
    pub node_hash: String,
    pub algorithm: Algorithm,
    pub mode: PolicyMode,
    pub priority: PriorityTier,
    pub load_shed_active: bool,
    pub decision: Decision,
    pub remaining: u64,
    pub limit: u64,
    pub eval_time_us: f64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LatencySummary {
    pub p50_us: f64,
    pub p95_us: f64,
    pub p99_us: f64,
    pub max_us: f64,
    pub mean_us: f64,
    pub jitter_us: f64,
    pub sample_count: usize,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ThroughputPoint {
    pub timestamp_ms: u64,
    pub allowed_rps: f64,
    pub throttled_rps: f64,
    pub shadow_rps: f64,
    pub shed_rps: f64,
    pub total_rps: f64,
    pub mean_eval_us: f64,
    pub anomaly_detected: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AnomalyAlert {
    pub id: u64,
    pub timestamp_ms: u64,
    pub client_key: String,
    pub namespace: String,
    pub node_id: String,
    pub observed_hits: u64,
    pub baseline_mean: f64,
    pub z_score: f64,
    pub severity: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PersistedState {
    pub version: String,
    pub saved_at_ms: u64,
    pub rules: Vec<RateLimitRule>,
    pub total_persisted_events: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TelemetrySnapshot {
    pub timestamp_ms: u64,
    pub uptime_secs: u64,
    pub counters: LimiterCounters,
    pub current_rps: f64,
    pub rejection_rate_pct: f64,
    pub latency: LatencySummary,
    pub series: Vec<ThroughputPoint>,
    pub recent_events: Vec<DecisionEvent>,
    pub anomalies: Vec<AnomalyAlert>,
    pub topology: ClusterTopologySnapshot,
    pub overload_guard: OverloadGuardConfig,
    pub janitor: JanitorStats,
}

#[derive(Debug)]
struct KeyWindowTracker {
    namespace: String,
    node_id: String,
    hits: u64,
}

struct TelemetryInner {
    events: VecDeque<DecisionEvent>,
    latencies_us: VecDeque<f64>,
    series: VecDeque<ThroughputPoint>,
    anomalies: VecDeque<AnomalyAlert>,
    window_keys: HashMap<String, KeyWindowTracker>,
    last_alert_ms_by_key: HashMap<String, u64>,
    tick_allowed: u64,
    tick_throttled: u64,
    tick_shadow: u64,
    tick_shed: u64,
    tick_latencies: Vec<f64>,
    last_tick_at: Instant,
    next_event_id: u64,
    next_anomaly_id: u64,
}

pub struct TelemetryStore {
    inner: Mutex<TelemetryInner>,
    started_at: Instant,
    snapshot_path: PathBuf,
    total_recorded: AtomicU64,
    stream_tx: broadcast::Sender<TelemetrySnapshot>,
}

pub fn now_epoch_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_millis() as u64)
        .unwrap_or(0)
}

fn percentile_sorted(sorted: &[f64], q: f64) -> f64 {
    if sorted.is_empty() {
        return 0.0;
    }
    if sorted.len() == 1 {
        return sorted[0];
    }
    let pos = q.clamp(0.0, 1.0) * (sorted.len() - 1) as f64;
    let lo = pos.floor() as usize;
    let hi = pos.ceil() as usize;
    if lo == hi {
        sorted[lo]
    } else {
        let w = pos - lo as f64;
        sorted[lo] * (1.0 - w) + sorted[hi] * w
    }
}

fn round_2(val: f64) -> f64 {
    (val * 100.0).round() / 100.0
}

impl TelemetryStore {
    pub fn new(snapshot_path: impl AsRef<Path>) -> Self {
        let (stream_tx, _) = broadcast::channel(64);
        Self {
            inner: Mutex::new(TelemetryInner {
                events: VecDeque::with_capacity(EVENT_RING_CAPACITY),
                latencies_us: VecDeque::with_capacity(LATENCY_WINDOW_CAPACITY),
                series: VecDeque::with_capacity(SERIES_HISTORY_CAPACITY),
                anomalies: VecDeque::with_capacity(ANOMALY_HISTORY_CAPACITY),
                window_keys: HashMap::new(),
                last_alert_ms_by_key: HashMap::new(),
                tick_allowed: 0,
                tick_throttled: 0,
                tick_shadow: 0,
                tick_shed: 0,
                tick_latencies: Vec::with_capacity(256),
                last_tick_at: Instant::now(),
                next_event_id: 1,
                next_anomaly_id: 1,
            }),
            started_at: Instant::now(),
            snapshot_path: snapshot_path.as_ref().to_path_buf(),
            total_recorded: AtomicU64::new(0),
            stream_tx,
        }
    }

    pub fn subscribe(&self) -> broadcast::Receiver<TelemetrySnapshot> {
        self.stream_tx.subscribe()
    }

    pub fn uptime_secs(&self) -> u64 {
        self.started_at.elapsed().as_secs()
    }

    pub fn total_recorded(&self) -> u64 {
        self.total_recorded.load(Ordering::Relaxed)
    }

    #[allow(clippy::too_many_arguments)]
    pub fn record_event(
        &self,
        namespace: &str,
        client_key: &str,
        node_id: &str,
        node_hash: u64,
        algorithm: Algorithm,
        mode: PolicyMode,
        priority: PriorityTier,
        load_shed_active: bool,
        decision: Decision,
        remaining: u64,
        limit: u64,
        eval_time_us: f64,
    ) -> DecisionEvent {
        let ts = now_epoch_ms();
        let mut guard = self.inner.lock();
        let id = guard.next_event_id;
        guard.next_event_id += 1;

        let event = DecisionEvent {
            id,
            timestamp_ms: ts,
            namespace: namespace.to_string(),
            client_key: client_key.to_string(),
            node_id: node_id.to_string(),
            node_hash: format!("{:016x}", node_hash),
            algorithm,
            mode,
            priority,
            load_shed_active,
            decision,
            remaining,
            limit,
            eval_time_us: round_2(eval_time_us),
        };

        if guard.events.len() >= EVENT_RING_CAPACITY {
            guard.events.pop_front();
        }
        guard.events.push_back(event.clone());

        if guard.latencies_us.len() >= LATENCY_WINDOW_CAPACITY {
            guard.latencies_us.pop_front();
        }
        guard.latencies_us.push_back(eval_time_us);
        guard.tick_latencies.push(eval_time_us);

        match decision {
            Decision::Allowed => guard.tick_allowed += 1,
            Decision::Throttled => guard.tick_throttled += 1,
            Decision::ShadowFlagged => guard.tick_shadow += 1,
            Decision::ShedOverload => guard.tick_shed += 1,
        }

        let tracker = guard
            .window_keys
            .entry(client_key.to_string())
            .or_insert_with(|| KeyWindowTracker {
                namespace: namespace.to_string(),
                node_id: node_id.to_string(),
                hits: 0,
            });
        tracker.hits += 1;
        tracker.namespace = namespace.to_string();
        tracker.node_id = node_id.to_string();

        drop(guard);
        self.total_recorded.fetch_add(1, Ordering::Relaxed);
        event
    }

    /// Runs Z-Score (> 3.0 sigma) anomaly detection across active key frequencies.
    pub fn evaluate_zscore_anomalies(&self, threshold: f64) -> Vec<AnomalyAlert> {
        let mut guard = self.inner.lock();
        Self::detect_anomalies_locked(&mut guard, threshold, now_epoch_ms())
    }

    /// Compacts stale anomaly alert cooldown markers and sparse key frequency trackers.
    pub fn compact_stale_windows(&self) -> usize {
        let now_ms = now_epoch_ms();
        let mut guard = self.inner.lock();
        let before_alerts = guard.last_alert_ms_by_key.len();
        guard
            .last_alert_ms_by_key
            .retain(|_, ts| now_ms.saturating_sub(*ts) < 60_000);
        let removed_alerts = before_alerts.saturating_sub(guard.last_alert_ms_by_key.len());

        let before_keys = guard.window_keys.len();
        if before_keys > 250 {
            guard.window_keys.retain(|_, v| {
                v.hits /= 2;
                v.hits > 0
            });
        }
        let removed_keys = before_keys.saturating_sub(guard.window_keys.len());
        removed_alerts + removed_keys
    }

    fn detect_anomalies_locked(
        guard: &mut TelemetryInner,
        threshold: f64,
        now_ms: u64,
    ) -> Vec<AnomalyAlert> {
        if guard.window_keys.len() < 11 {
            return Vec::new();
        }

        let counts: Vec<f64> = guard.window_keys.values().map(|t| t.hits as f64).collect();
        let n = counts.len() as f64;
        let mean = counts.iter().sum::<f64>() / n;
        let variance = counts.iter().map(|v| (v - mean).powi(2)).sum::<f64>() / n;
        let std_dev = variance.sqrt();

        if std_dev <= f64::EPSILON {
            return Vec::new();
        }

        let mut new_alerts = Vec::new();
        let entries: Vec<(String, String, String, u64)> = guard
            .window_keys
            .iter()
            .map(|(k, v)| (k.clone(), v.namespace.clone(), v.node_id.clone(), v.hits))
            .collect();

        for (key, namespace, node_id, hits) in entries {
            if hits < 10 {
                continue;
            }
            let z = (hits as f64 - mean) / std_dev;
            if z > threshold {
                let recently_alerted = guard
                    .last_alert_ms_by_key
                    .get(&key)
                    .map(|&prev| now_ms.saturating_sub(prev) < 4000)
                    .unwrap_or(false);

                if recently_alerted {
                    continue;
                }

                let id = guard.next_anomaly_id;
                guard.next_anomaly_id += 1;
                let severity = if z >= 3.5 { "CRITICAL" } else { "WARNING" };

                let alert = AnomalyAlert {
                    id,
                    timestamp_ms: now_ms,
                    client_key: key.clone(),
                    namespace,
                    node_id,
                    observed_hits: hits,
                    baseline_mean: round_2(mean),
                    z_score: round_2(z),
                    severity: severity.to_string(),
                };

                guard.last_alert_ms_by_key.insert(key, now_ms);
                if guard.anomalies.len() >= ANOMALY_HISTORY_CAPACITY {
                    guard.anomalies.pop_front();
                }
                guard.anomalies.push_back(alert.clone());
                new_alerts.push(alert);
            }
        }

        new_alerts
    }

    /// Advances the 500ms time-series tick, evaluates Z-Score anomalies, and returns a snapshot.
    pub fn advance_tick_and_snapshot(
        &self,
        counters: LimiterCounters,
        topology: ClusterTopologySnapshot,
        overload_guard: OverloadGuardConfig,
        janitor: JanitorStats,
    ) -> TelemetrySnapshot {
        let now_ms = now_epoch_ms();
        let mut guard = self.inner.lock();

        let elapsed_secs = guard.last_tick_at.elapsed().as_secs_f64().max(0.25);
        guard.last_tick_at = Instant::now();

        let detected = Self::detect_anomalies_locked(&mut guard, 3.0, now_ms);

        let allowed_rps = round_2(guard.tick_allowed as f64 / elapsed_secs);
        let throttled_rps = round_2(guard.tick_throttled as f64 / elapsed_secs);
        let shadow_rps = round_2(guard.tick_shadow as f64 / elapsed_secs);
        let shed_rps = round_2(guard.tick_shed as f64 / elapsed_secs);
        let total_rps = round_2(allowed_rps + throttled_rps + shadow_rps + shed_rps);

        let mean_eval_us = if guard.tick_latencies.is_empty() {
            if guard.latencies_us.is_empty() {
                0.0
            } else {
                round_2(
                    guard.latencies_us.iter().sum::<f64>() / guard.latencies_us.len() as f64,
                )
            }
        } else {
            round_2(guard.tick_latencies.iter().sum::<f64>() / guard.tick_latencies.len() as f64)
        };

        let point = ThroughputPoint {
            timestamp_ms: now_ms,
            allowed_rps,
            throttled_rps,
            shadow_rps,
            shed_rps,
            total_rps,
            mean_eval_us,
            anomaly_detected: !detected.is_empty(),
        };

        if guard.series.len() >= SERIES_HISTORY_CAPACITY {
            guard.series.pop_front();
        }
        guard.series.push_back(point);

        guard.tick_allowed = 0;
        guard.tick_throttled = 0;
        guard.tick_shadow = 0;
        guard.tick_shed = 0;
        guard.tick_latencies.clear();

        if guard.window_keys.len() > 200 || now_ms % 6000 < 600 {
            guard.window_keys.retain(|_, v| {
                v.hits /= 2;
                v.hits > 0
            });
        }

        let snapshot = Self::build_snapshot_locked(
            &guard,
            self.started_at.elapsed().as_secs(),
            now_ms,
            counters,
            topology,
            overload_guard,
            janitor,
        );
        drop(guard);

        let _ = self.stream_tx.send(snapshot.clone());
        snapshot
    }

    pub fn current_snapshot(
        &self,
        counters: LimiterCounters,
        topology: ClusterTopologySnapshot,
        overload_guard: OverloadGuardConfig,
        janitor: JanitorStats,
    ) -> TelemetrySnapshot {
        let guard = self.inner.lock();
        Self::build_snapshot_locked(
            &guard,
            self.started_at.elapsed().as_secs(),
            now_epoch_ms(),
            counters,
            topology,
            overload_guard,
            janitor,
        )
    }

    fn build_snapshot_locked(
        guard: &TelemetryInner,
        uptime_secs: u64,
        timestamp_ms: u64,
        counters: LimiterCounters,
        topology: ClusterTopologySnapshot,
        overload_guard: OverloadGuardConfig,
        janitor: JanitorStats,
    ) -> TelemetrySnapshot {
        let latency = Self::compute_latency_summary(&guard.latencies_us);
        let current_rps = guard.series.back().map(|p| p.total_rps).unwrap_or(0.0);
        let rejected = counters.throttled + counters.shed_overload;
        let rejection_rate_pct = if counters.total_evaluations > 0 {
            round_2((rejected as f64 / counters.total_evaluations as f64) * 100.0)
        } else {
            0.0
        };

        let recent_events: Vec<DecisionEvent> = guard
            .events
            .iter()
            .rev()
            .take(120)
            .cloned()
            .collect();

        let anomalies: Vec<AnomalyAlert> = guard.anomalies.iter().rev().take(20).cloned().collect();
        let series: Vec<ThroughputPoint> = guard.series.iter().cloned().collect();

        TelemetrySnapshot {
            timestamp_ms,
            uptime_secs,
            counters,
            current_rps,
            rejection_rate_pct,
            latency,
            series,
            recent_events,
            anomalies,
            topology,
            overload_guard,
            janitor,
        }
    }

    pub fn compute_latency_summary(samples: &VecDeque<f64>) -> LatencySummary {
        if samples.is_empty() {
            return LatencySummary {
                p50_us: 0.0,
                p95_us: 0.0,
                p99_us: 0.0,
                max_us: 0.0,
                mean_us: 0.0,
                jitter_us: 0.0,
                sample_count: 0,
            };
        }

        let raw: Vec<f64> = samples.iter().copied().collect();
        let n = raw.len();
        let mean_us = raw.iter().sum::<f64>() / n as f64;
        let jitter_us = if n > 1 {
            raw.windows(2).map(|w| (w[1] - w[0]).abs()).sum::<f64>() / (n - 1) as f64
        } else {
            0.0
        };

        let mut sorted = raw;
        sorted.sort_by(|a, b| a.total_cmp(b));

        LatencySummary {
            p50_us: round_2(percentile_sorted(&sorted, 0.50)),
            p95_us: round_2(percentile_sorted(&sorted, 0.95)),
            p99_us: round_2(percentile_sorted(&sorted, 0.99)),
            max_us: round_2(*sorted.last().unwrap_or(&0.0)),
            mean_us: round_2(mean_us),
            jitter_us: round_2(jitter_us),
            sample_count: n,
        }
    }

    pub fn recent_events_len(&self) -> usize {
        self.inner.lock().events.len()
    }

    /// Atomically persists dynamic rules & telemetry counters to disk via temp-file + rename.
    pub fn persist_snapshot(&self, rules: Vec<RateLimitRule>) -> Result<(), String> {
        if let Some(parent) = self.snapshot_path.parent() {
            if !parent.as_os_str().is_empty() {
                std::fs::create_dir_all(parent)
                    .map_err(|e| format!("failed to create snapshot dir: {}", e))?;
            }
        }

        let payload = PersistedState {
            version: "2.0.0".to_string(),
            saved_at_ms: now_epoch_ms(),
            rules,
            total_persisted_events: self.total_recorded(),
        };

        let json = serde_json::to_string_pretty(&payload)
            .map_err(|e| format!("failed to serialize snapshot: {}", e))?;

        let tmp_path = self.snapshot_path.with_extension("json.tmp");
        std::fs::write(&tmp_path, json)
            .map_err(|e| format!("failed to write temp snapshot: {}", e))?;
        std::fs::rename(&tmp_path, &self.snapshot_path)
            .map_err(|e| format!("failed to rename snapshot atomically: {}", e))?;

        Ok(())
    }

    pub fn load_persisted_snapshot(&self) -> Option<PersistedState> {
        let data = std::fs::read_to_string(&self.snapshot_path).ok()?;
        serde_json::from_str::<PersistedState>(&data).ok()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::{SystemTime, UNIX_EPOCH};

    fn temp_snapshot_path(name: &str) -> PathBuf {
        let nanos = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        std::env::temp_dir().join(format!("vulcan_test_{}_{}.json", name, nanos))
    }

    #[test]
    fn ring_buffer_bounds_events_to_2000() {
        let store = TelemetryStore::new(temp_snapshot_path("ring"));
        for i in 0..2350u64 {
            store.record_event(
                "api-prod",
                &format!("key-{}", i % 10),
                "edge-iad-01",
                0xdeadbeef,
                Algorithm::TokenBucket,
                PolicyMode::Enforce,
                PriorityTier::Standard,
                false,
                Decision::Allowed,
                50,
                60,
                3.5 + (i % 7) as f64,
            );
        }
        assert_eq!(store.recent_events_len(), EVENT_RING_CAPACITY);
        assert_eq!(store.total_recorded(), 2350);
    }

    #[test]
    fn zscore_detector_flags_traffic_exceeding_three_sigma() {
        let store = TelemetryStore::new(temp_snapshot_path("zscore"));
        for k in 0..16 {
            let hits = 2 + (k % 3);
            for _ in 0..hits {
                store.record_event(
                    "api-prod",
                    &format!("normal-ip-{}", k),
                    "edge-iad-01",
                    1234,
                    Algorithm::TokenBucket,
                    PolicyMode::Enforce,
                    PriorityTier::Standard,
                    false,
                    Decision::Allowed,
                    50,
                    60,
                    2.5,
                );
            }
        }
        for _ in 0..120 {
            store.record_event(
                "api-prod",
                "attacker-99.210.44.12",
                "edge-fra-04",
                9999,
                Algorithm::TokenBucket,
                PolicyMode::Enforce,
                PriorityTier::Standard,
                false,
                Decision::Throttled,
                0,
                60,
                4.2,
            );
        }

        let alerts = store.evaluate_zscore_anomalies(3.0);
        assert_eq!(alerts.len(), 1);
        assert_eq!(alerts[0].client_key, "attacker-99.210.44.12");
        assert!(alerts[0].z_score > 3.0);
        assert_eq!(alerts[0].severity, "CRITICAL");
    }

    #[test]
    fn atomic_snapshot_persists_and_reloads_rules() {
        let path = temp_snapshot_path("persist");
        let store = TelemetryStore::new(&path);
        let rules = vec![RateLimitRule {
            namespace: "persisted-api".to_string(),
            algorithm: Algorithm::SlidingWindow,
            capacity: 42,
            refill_per_sec: 12.5,
            mode: PolicyMode::Shadow,
            description: "persisted".to_string(),
        }];

        store.persist_snapshot(rules.clone()).expect("persist ok");
        let reloaded = store
            .load_persisted_snapshot()
            .expect("snapshot loads cleanly");
        assert_eq!(reloaded.rules.len(), 1);
        assert_eq!(reloaded.rules[0].namespace, "persisted-api");
        assert_eq!(reloaded.rules[0].capacity, 42);
        let _ = std::fs::remove_file(path);
    }
}

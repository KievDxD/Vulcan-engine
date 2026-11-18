use dashmap::DashMap;
use parking_lot::{Mutex, RwLock};
use serde::{Deserialize, Serialize};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant};

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum Algorithm {
    TokenBucket,
    SlidingWindow,
}

impl Algorithm {
    pub fn as_str(&self) -> &'static str {
        match self {
            Algorithm::TokenBucket => "token_bucket",
            Algorithm::SlidingWindow => "sliding_window",
        }
    }
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum PolicyMode {
    Enforce,
    Shadow,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, Default)]
#[serde(rename_all = "snake_case")]
pub enum PriorityTier {
    Critical,
    #[default]
    Standard,
    BestEffort,
}

impl PriorityTier {
    pub fn as_str(&self) -> &'static str {
        match self {
            PriorityTier::Critical => "critical",
            PriorityTier::Standard => "standard",
            PriorityTier::BestEffort => "best_effort",
        }
    }
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum Decision {
    Allowed,
    Throttled,
    ShadowFlagged,
    ShedOverload,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OverloadGuardConfig {
    pub enabled: bool,
    pub max_rps_threshold: f64,
    pub shed_best_effort: bool,
    pub shedding_active: bool,
}

impl Default for OverloadGuardConfig {
    fn default() -> Self {
        Self {
            enabled: false,
            max_rps_threshold: 450.0,
            shed_best_effort: true,
            shedding_active: false,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RateLimitRule {
    pub namespace: String,
    pub algorithm: Algorithm,
    pub capacity: u64,
    pub refill_per_sec: f64,
    pub mode: PolicyMode,
    #[serde(default)]
    pub description: String,
}

#[derive(Debug)]
struct KeyState {
    // Token Bucket fields
    tokens: f64,
    last_refill: Instant,
    // Sliding Window Counter fields (1s sliding window)
    window_start: Instant,
    prev_count: u64,
    curr_count: u64,
    // Janitor LRU eviction timestamp
    last_accessed: Instant,
}

impl KeyState {
    fn new(capacity: u64, now: Instant) -> Self {
        Self {
            tokens: capacity as f64,
            last_refill: now,
            window_start: now,
            prev_count: 0,
            curr_count: 0,
            last_accessed: now,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EvaluationOutcome {
    pub decision: Decision,
    pub priority: PriorityTier,
    pub load_shed_active: bool,
    pub algorithm: Algorithm,
    pub mode: PolicyMode,
    pub limit: u64,
    pub remaining: u64,
    pub reset_secs: u64,
    pub retry_after_secs: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LimiterCounters {
    pub total_evaluations: u64,
    pub allowed: u64,
    pub throttled: u64,
    pub shadow_flagged: u64,
    pub shed_overload: u64,
    pub keys_evicted: u64,
    pub active_buckets: usize,
}

pub struct RateLimiterEngine {
    rules: DashMap<String, RateLimitRule>,
    buckets: DashMap<String, Arc<Mutex<KeyState>>>,
    overload_guard: RwLock<OverloadGuardConfig>,
    total_evaluations: AtomicU64,
    allowed_count: AtomicU64,
    throttled_count: AtomicU64,
    shadow_count: AtomicU64,
    shed_overload_count: AtomicU64,
    keys_evicted_count: AtomicU64,
    window_duration: Duration,
}

impl Default for RateLimiterEngine {
    fn default() -> Self {
        Self::new()
    }
}

impl RateLimiterEngine {
    pub fn new() -> Self {
        let engine = Self {
            rules: DashMap::new(),
            buckets: DashMap::new(),
            overload_guard: RwLock::new(OverloadGuardConfig::default()),
            total_evaluations: AtomicU64::new(0),
            allowed_count: AtomicU64::new(0),
            throttled_count: AtomicU64::new(0),
            shadow_count: AtomicU64::new(0),
            shed_overload_count: AtomicU64::new(0),
            keys_evicted_count: AtomicU64::new(0),
            window_duration: Duration::from_secs(1),
        };
        engine.seed_default_rules();
        engine
    }

    pub fn with_window_duration(mut self, duration: Duration) -> Self {
        self.window_duration = duration;
        self
    }

    pub fn seed_default_rules(&self) {
        let defaults = [
            RateLimitRule {
                namespace: "api-prod".to_string(),
                algorithm: Algorithm::TokenBucket,
                capacity: 60,
                refill_per_sec: 20.0,
                mode: PolicyMode::Enforce,
                description: "Core production REST & GraphQL ingress gateway".to_string(),
            },
            RateLimitRule {
                namespace: "ai-inference".to_string(),
                algorithm: Algorithm::TokenBucket,
                capacity: 20,
                refill_per_sec: 6.0,
                mode: PolicyMode::Enforce,
                description: "GPU inference token quota & burst protection".to_string(),
            },
            RateLimitRule {
                namespace: "public-edge".to_string(),
                algorithm: Algorithm::SlidingWindow,
                capacity: 90,
                refill_per_sec: 45.0,
                mode: PolicyMode::Shadow,
                description: "Shadow-mode policy auditing edge CDN requests".to_string(),
            },
            RateLimitRule {
                namespace: "checkout-core".to_string(),
                algorithm: Algorithm::SlidingWindow,
                capacity: 35,
                refill_per_sec: 15.0,
                mode: PolicyMode::Enforce,
                description: "Strict payment and checkout anti-abuse sliding window".to_string(),
            },
        ];

        for rule in defaults {
            self.rules.insert(rule.namespace.clone(), rule);
        }
    }

    pub fn get_overload_guard(&self) -> OverloadGuardConfig {
        self.overload_guard.read().clone()
    }

    pub fn configure_overload_guard(
        &self,
        enabled: bool,
        max_rps_threshold: f64,
        shed_best_effort: bool,
        shedding_active: Option<bool>,
    ) -> OverloadGuardConfig {
        let mut guard = self.overload_guard.write();
        guard.enabled = enabled;
        guard.max_rps_threshold = max_rps_threshold.max(1.0);
        guard.shed_best_effort = shed_best_effort;
        guard.shedding_active = if enabled {
            shedding_active.unwrap_or(true)
        } else {
            false
        };
        guard.clone()
    }

    /// Updates overload shedding state automatically when armed with an RPS threshold.
    pub fn sync_overload_with_rps(&self, current_rps: f64) -> OverloadGuardConfig {
        let mut guard = self.overload_guard.write();
        if guard.enabled && current_rps >= guard.max_rps_threshold {
            guard.shedding_active = true;
        }
        guard.clone()
    }

    pub fn list_rules(&self) -> Vec<RateLimitRule> {
        let mut list: Vec<RateLimitRule> = self.rules.iter().map(|r| r.value().clone()).collect();
        list.sort_by(|a, b| a.namespace.cmp(&b.namespace));
        list
    }

    pub fn upsert_rule(&self, rule: RateLimitRule) -> RateLimitRule {
        let clean = RateLimitRule {
            namespace: rule.namespace.trim().to_string(),
            algorithm: rule.algorithm,
            capacity: rule.capacity.max(1),
            refill_per_sec: rule.refill_per_sec.max(0.1),
            mode: rule.mode,
            description: rule.description,
        };
        self.rules.insert(clean.namespace.clone(), clean.clone());
        let prefix = format!("{}:", clean.namespace);
        self.buckets.retain(|k, _| !k.starts_with(&prefix));
        clean
    }

    pub fn delete_rule(&self, namespace: &str) -> Option<RateLimitRule> {
        let removed = self.rules.remove(namespace).map(|(_, rule)| rule);
        if removed.is_some() {
            let prefix = format!("{}:", namespace);
            self.buckets.retain(|k, _| !k.starts_with(&prefix));
        }
        removed
    }

    pub fn load_rules(&self, rules: Vec<RateLimitRule>) {
        if rules.is_empty() {
            return;
        }
        for rule in rules {
            self.rules.insert(rule.namespace.clone(), rule);
        }
    }

    pub fn get_counters(&self) -> LimiterCounters {
        LimiterCounters {
            total_evaluations: self.total_evaluations.load(Ordering::Relaxed),
            allowed: self.allowed_count.load(Ordering::Relaxed),
            throttled: self.throttled_count.load(Ordering::Relaxed),
            shadow_flagged: self.shadow_count.load(Ordering::Relaxed),
            shed_overload: self.shed_overload_count.load(Ordering::Relaxed),
            keys_evicted: self.keys_evicted_count.load(Ordering::Relaxed),
            active_buckets: self.buckets.len(),
        }
    }

    /// Evicts inactive key buckets whose `last_accessed` exceeds `max_idle`.
    pub fn evict_idle_buckets(&self, max_idle: Duration) -> usize {
        self.evict_idle_buckets_at(max_idle, Instant::now())
    }

    pub fn evict_idle_buckets_at(&self, max_idle: Duration, now: Instant) -> usize {
        let mut evicted = 0usize;
        self.buckets.retain(|_, state_arc| {
            let expired = if let Some(guard) = state_arc.try_lock() {
                now.saturating_duration_since(guard.last_accessed) >= max_idle
            } else {
                false
            };
            if expired {
                evicted += 1;
                false
            } else {
                true
            }
        });
        if evicted > 0 {
            self.keys_evicted_count
                .fetch_add(evicted as u64, Ordering::Relaxed);
        }
        evicted
    }

    pub fn evaluate(
        &self,
        namespace: &str,
        key: &str,
        requested_tokens: u64,
        algorithm_override: Option<Algorithm>,
    ) -> EvaluationOutcome {
        self.evaluate_with_priority_at(
            namespace,
            key,
            requested_tokens,
            algorithm_override,
            PriorityTier::Standard,
            Instant::now(),
        )
    }

    pub fn evaluate_with_priority(
        &self,
        namespace: &str,
        key: &str,
        requested_tokens: u64,
        algorithm_override: Option<Algorithm>,
        priority: PriorityTier,
    ) -> EvaluationOutcome {
        self.evaluate_with_priority_at(
            namespace,
            key,
            requested_tokens,
            algorithm_override,
            priority,
            Instant::now(),
        )
    }

    pub fn evaluate_at(
        &self,
        namespace: &str,
        key: &str,
        requested_tokens: u64,
        algorithm_override: Option<Algorithm>,
        now: Instant,
    ) -> EvaluationOutcome {
        self.evaluate_with_priority_at(
            namespace,
            key,
            requested_tokens,
            algorithm_override,
            PriorityTier::Standard,
            now,
        )
    }

    /// Evaluates a request with priority-aware adaptive load-shedding and per-key granular locking.
    pub fn evaluate_with_priority_at(
        &self,
        namespace: &str,
        key: &str,
        requested_tokens: u64,
        algorithm_override: Option<Algorithm>,
        priority: PriorityTier,
        now: Instant,
    ) -> EvaluationOutcome {
        let base_rule = self
            .rules
            .get(namespace)
            .map(|r| r.value().clone())
            .unwrap_or_else(|| RateLimitRule {
                namespace: namespace.to_string(),
                algorithm: Algorithm::TokenBucket,
                capacity: 100,
                refill_per_sec: 40.0,
                mode: PolicyMode::Enforce,
                description: "Dynamic fallback rule".to_string(),
            });

        let algorithm = algorithm_override.unwrap_or(base_rule.algorithm);
        let guard = self.get_overload_guard();
        let shedding_active = guard.enabled && guard.shedding_active;

        // Adaptive Load-Shedding: shed best_effort immediately with HTTP 503 before touching DashMap buckets
        if shedding_active && priority == PriorityTier::BestEffort && guard.shed_best_effort {
            self.total_evaluations.fetch_add(1, Ordering::Relaxed);
            self.shed_overload_count.fetch_add(1, Ordering::Relaxed);
            return EvaluationOutcome {
                decision: Decision::ShedOverload,
                priority,
                load_shed_active: true,
                algorithm,
                mode: base_rule.mode,
                limit: base_rule.capacity,
                remaining: 0,
                reset_secs: 5,
                retry_after_secs: 5,
            };
        }

        // Priority queuing capacity shaping under overload
        let mut effective_rule = base_rule.clone();
        if shedding_active {
            match priority {
                PriorityTier::Critical => {
                    effective_rule.capacity = base_rule.capacity.saturating_mul(2).max(1);
                }
                PriorityTier::Standard => {
                    effective_rule.capacity = (base_rule.capacity / 2).max(1);
                }
                PriorityTier::BestEffort => {}
            }
        }

        let tokens_req = requested_tokens.max(1);
        let bucket_key = format!("{}:{}", namespace, key);

        let bucket_arc = if let Some(existing) = self.buckets.get(&bucket_key) {
            Arc::clone(existing.value())
        } else {
            self.buckets
                .entry(bucket_key)
                .or_insert_with(|| {
                    Arc::new(Mutex::new(KeyState::new(effective_rule.capacity, now)))
                })
                .value()
                .clone()
        };

        let (permitted, remaining, reset_secs, retry_after_secs) = {
            let mut state = bucket_arc.lock();
            state.last_accessed = now;
            if shedding_active && priority == PriorityTier::Critical {
                // Guarantee critical traffic preserves minimum operational headroom during overload
                state.tokens = state.tokens.max(tokens_req as f64);
            }
            match algorithm {
                Algorithm::TokenBucket => {
                    Self::eval_token_bucket(&mut state, &effective_rule, tokens_req, now)
                }
                Algorithm::SlidingWindow => Self::eval_sliding_window(
                    &mut state,
                    &effective_rule,
                    tokens_req,
                    now,
                    self.window_duration,
                ),
            }
        };

        self.total_evaluations.fetch_add(1, Ordering::Relaxed);

        let decision = if permitted {
            self.allowed_count.fetch_add(1, Ordering::Relaxed);
            Decision::Allowed
        } else {
            match effective_rule.mode {
                PolicyMode::Enforce => {
                    self.throttled_count.fetch_add(1, Ordering::Relaxed);
                    Decision::Throttled
                }
                PolicyMode::Shadow => {
                    self.shadow_count.fetch_add(1, Ordering::Relaxed);
                    Decision::ShadowFlagged
                }
            }
        };

        EvaluationOutcome {
            decision,
            priority,
            load_shed_active: shedding_active,
            algorithm,
            mode: effective_rule.mode,
            limit: effective_rule.capacity,
            remaining,
            reset_secs,
            retry_after_secs,
        }
    }

    fn eval_token_bucket(
        state: &mut KeyState,
        rule: &RateLimitRule,
        requested: u64,
        now: Instant,
    ) -> (bool, u64, u64, u64) {
        if now > state.last_refill {
            let elapsed = now.duration_since(state.last_refill).as_secs_f64();
            let replenished = elapsed * rule.refill_per_sec;
            state.tokens = (state.tokens + replenished).min(rule.capacity as f64);
            state.last_refill = now;
        } else {
            state.tokens = state.tokens.min(rule.capacity as f64);
        }

        let req = requested as f64;
        let rate = rule.refill_per_sec.max(0.001);

        if state.tokens >= req {
            state.tokens -= req;
            let remaining = state.tokens.floor().max(0.0) as u64;
            let reset_secs = ((rule.capacity as f64 - state.tokens).max(0.0) / rate).ceil() as u64;
            (true, remaining, reset_secs, 0)
        } else {
            let remaining = state.tokens.floor().max(0.0) as u64;
            let deficit = req - state.tokens;
            let retry_after = (deficit / rate).ceil().max(1.0) as u64;
            let reset_secs = ((rule.capacity as f64 - state.tokens).max(0.0) / rate).ceil() as u64;
            (false, remaining, reset_secs, retry_after)
        }
    }

    fn eval_sliding_window(
        state: &mut KeyState,
        rule: &RateLimitRule,
        requested: u64,
        now: Instant,
        window_duration: Duration,
    ) -> (bool, u64, u64, u64) {
        let win_secs = window_duration.as_secs_f64().max(0.001);

        if now > state.window_start {
            let elapsed = now.duration_since(state.window_start);
            if elapsed >= window_duration * 2 {
                state.prev_count = 0;
                state.curr_count = 0;
                state.window_start = now;
            } else if elapsed >= window_duration {
                state.prev_count = state.curr_count;
                state.curr_count = 0;
                state.window_start += window_duration;
            }
        }

        let elapsed_in_window = if now > state.window_start {
            now.duration_since(state.window_start).as_secs_f64()
        } else {
            0.0
        };
        let weight_prev = (1.0 - (elapsed_in_window / win_secs)).clamp(0.0, 1.0);
        let weighted_current = (state.prev_count as f64 * weight_prev) + state.curr_count as f64;
        let cap = rule.capacity as f64;
        let reset_secs = (win_secs - elapsed_in_window).ceil().max(1.0) as u64;

        if weighted_current + requested as f64 <= cap {
            state.curr_count += requested;
            let remaining = (cap - (weighted_current + requested as f64))
                .floor()
                .max(0.0) as u64;
            (true, remaining, reset_secs, 0)
        } else {
            let remaining = (cap - weighted_current).floor().max(0.0) as u64;
            let retry_after = reset_secs.max(1);
            (false, remaining, reset_secs, retry_after)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::thread;

    #[test]
    fn token_bucket_burst_and_continuous_refill() {
        let limiter = RateLimiterEngine::new();
        limiter.upsert_rule(RateLimitRule {
            namespace: "tb-test".to_string(),
            algorithm: Algorithm::TokenBucket,
            capacity: 5,
            refill_per_sec: 2.0,
            mode: PolicyMode::Enforce,
            description: "test".to_string(),
        });

        let t0 = Instant::now();
        for i in 0..5 {
            let out = limiter.evaluate_at("tb-test", "client-a", 1, None, t0);
            assert_eq!(out.decision, Decision::Allowed);
            assert_eq!(out.remaining, 4 - i);
        }

        let throttled = limiter.evaluate_at("tb-test", "client-a", 1, None, t0);
        assert_eq!(throttled.decision, Decision::Throttled);
        assert_eq!(throttled.remaining, 0);
        assert!(throttled.retry_after_secs >= 1);

        let t1 = t0 + Duration::from_millis(1500);
        for _ in 0..3 {
            let out = limiter.evaluate_at("tb-test", "client-a", 1, None, t1);
            assert_eq!(out.decision, Decision::Allowed);
        }
        let throttled_again = limiter.evaluate_at("tb-test", "client-a", 1, None, t1);
        assert_eq!(throttled_again.decision, Decision::Throttled);
    }

    #[test]
    fn sliding_window_prevents_boundary_burst_spikes() {
        let limiter = RateLimiterEngine::new().with_window_duration(Duration::from_secs(1));
        limiter.upsert_rule(RateLimitRule {
            namespace: "sw-test".to_string(),
            algorithm: Algorithm::SlidingWindow,
            capacity: 10,
            refill_per_sec: 10.0,
            mode: PolicyMode::Enforce,
            description: "sliding window boundary test".to_string(),
        });

        let t0 = Instant::now();
        let init = limiter.evaluate_at("sw-test", "edge-user", 1, None, t0);
        assert_eq!(init.decision, Decision::Allowed);

        let t_near_end = t0 + Duration::from_millis(800);
        for _ in 0..9 {
            let out = limiter.evaluate_at("sw-test", "edge-user", 1, None, t_near_end);
            assert_eq!(out.decision, Decision::Allowed);
        }

        let t_next = t0 + Duration::from_millis(1150);
        let first_in_next = limiter.evaluate_at("sw-test", "edge-user", 1, None, t_next);
        assert_eq!(first_in_next.decision, Decision::Allowed);
        let second_in_next = limiter.evaluate_at("sw-test", "edge-user", 1, None, t_next);
        assert_eq!(second_in_next.decision, Decision::Throttled);
    }

    #[test]
    fn shadow_mode_audits_without_blocking() {
        let limiter = RateLimiterEngine::new();
        let t0 = Instant::now();
        for _ in 0..90 {
            let out = limiter.evaluate_at("public-edge", "shadow-ip", 1, None, t0);
            assert_eq!(out.decision, Decision::Allowed);
        }
        let flagged = limiter.evaluate_at("public-edge", "shadow-ip", 1, None, t0);
        assert_eq!(flagged.decision, Decision::ShadowFlagged);
        assert_eq!(flagged.mode, PolicyMode::Shadow);
    }

    #[test]
    fn concurrent_lock_free_evaluations_remain_exact() {
        let limiter = Arc::new(RateLimiterEngine::new());
        limiter.upsert_rule(RateLimitRule {
            namespace: "conc-test".to_string(),
            algorithm: Algorithm::TokenBucket,
            capacity: 500,
            refill_per_sec: 0.001,
            mode: PolicyMode::Enforce,
            description: "concurrency accuracy".to_string(),
        });

        let mut handles = Vec::new();
        for _ in 0..16 {
            let lim = Arc::clone(&limiter);
            handles.push(thread::spawn(move || {
                let mut allowed = 0u64;
                let mut throttled = 0u64;
                for _ in 0..60 {
                    let res = lim.evaluate("conc-test", "shared-hot-key", 1, None);
                    match res.decision {
                        Decision::Allowed => allowed += 1,
                        Decision::Throttled => throttled += 1,
                        Decision::ShadowFlagged | Decision::ShedOverload => {}
                    }
                }
                (allowed, throttled)
            }));
        }

        let mut total_allowed = 0;
        let mut total_throttled = 0;
        for h in handles {
            let (a, t) = h.join().unwrap();
            total_allowed += a;
            total_throttled += t;
        }

        assert_eq!(total_allowed + total_throttled, 960);
        assert!(
            total_allowed >= 500 && total_allowed <= 502,
            "expected ~500 allowed under contention, got {}",
            total_allowed
        );
    }

    #[test]
    fn rules_crud_and_bucket_invalidation() {
        let limiter = RateLimiterEngine::new();
        assert!(limiter.list_rules().len() >= 4);

        limiter.upsert_rule(RateLimitRule {
            namespace: "custom-ns".to_string(),
            algorithm: Algorithm::TokenBucket,
            capacity: 2,
            refill_per_sec: 1.0,
            mode: PolicyMode::Enforce,
            description: "custom".to_string(),
        });

        assert_eq!(
            limiter.evaluate("custom-ns", "k1", 2, None).decision,
            Decision::Allowed
        );
        assert_eq!(
            limiter.evaluate("custom-ns", "k1", 1, None).decision,
            Decision::Throttled
        );

        let removed = limiter.delete_rule("custom-ns");
        assert!(removed.is_some());
        assert!(limiter.delete_rule("custom-ns").is_none());
    }

    #[test]
    fn overload_guard_sheds_best_effort_and_preserves_critical() {
        let limiter = RateLimiterEngine::new();
        limiter.configure_overload_guard(true, 100.0, true, Some(true));

        let best = limiter.evaluate_with_priority(
            "api-prod",
            "batch-crawler",
            1,
            None,
            PriorityTier::BestEffort,
        );
        assert_eq!(best.decision, Decision::ShedOverload);
        assert!(best.load_shed_active);
        assert_eq!(best.remaining, 0);

        let critical = limiter.evaluate_with_priority(
            "api-prod",
            "payment-auth",
            1,
            None,
            PriorityTier::Critical,
        );
        assert_eq!(critical.decision, Decision::Allowed);
        assert!(critical.load_shed_active);
        assert_eq!(critical.limit, 120); // 2x headroom over base 60
        assert_eq!(limiter.get_counters().shed_overload, 1);
    }

    #[test]
    fn overload_guard_degrades_standard_priority_capacity() {
        let limiter = RateLimiterEngine::new();
        limiter.upsert_rule(RateLimitRule {
            namespace: "std-degrade".to_string(),
            algorithm: Algorithm::TokenBucket,
            capacity: 10,
            refill_per_sec: 1.0,
            mode: PolicyMode::Enforce,
            description: "degradation test".to_string(),
        });
        limiter.configure_overload_guard(true, 50.0, true, Some(true));

        let t0 = Instant::now();
        // Degraded capacity for Standard is 10 / 2 = 5
        for _ in 0..5 {
            let out = limiter.evaluate_with_priority_at(
                "std-degrade",
                "std-client",
                1,
                None,
                PriorityTier::Standard,
                t0,
            );
            assert_eq!(out.decision, Decision::Allowed);
            assert_eq!(out.limit, 5);
        }
        let sixth = limiter.evaluate_with_priority_at(
            "std-degrade",
            "std-client",
            1,
            None,
            PriorityTier::Standard,
            t0,
        );
        assert_eq!(sixth.decision, Decision::Throttled);
    }

    #[test]
    fn janitor_evicts_expired_buckets_and_tracks_stats() {
        let limiter = RateLimiterEngine::new();
        let t0 = Instant::now();
        limiter.evaluate_at("api-prod", "stale-key-1", 1, None, t0);
        limiter.evaluate_at("api-prod", "stale-key-2", 1, None, t0);
        let t_fresh = t0 + Duration::from_secs(25);
        limiter.evaluate_at("api-prod", "fresh-key", 1, None, t_fresh);

        assert_eq!(limiter.get_counters().active_buckets, 3);
        let evicted = limiter.evict_idle_buckets_at(Duration::from_secs(15), t_fresh + Duration::from_secs(1));
        assert_eq!(evicted, 2);
        assert_eq!(limiter.get_counters().active_buckets, 1);
        assert_eq!(limiter.get_counters().keys_evicted, 2);
    }
}

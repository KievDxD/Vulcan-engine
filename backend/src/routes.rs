use crate::janitor::{spawn_janitor_loop, JanitorController};
use crate::limiter::{
    Algorithm, Decision, OverloadGuardConfig, PolicyMode, PriorityTier, RateLimitRule,
    RateLimiterEngine,
};
use crate::ring::ConsistentHashRing;
use crate::telemetry::{TelemetrySnapshot, TelemetryStore};
use axum::extract::{Path, State};
use axum::http::{header, HeaderMap, HeaderValue, StatusCode};
use axum::response::sse::{Event, KeepAlive, Sse};
use axum::response::{IntoResponse, Response};
use axum::routing::{delete, get, post};
use axum::{Json, Router};
use futures::stream::{self, Stream, StreamExt};
use serde::{Deserialize, Serialize};
use std::convert::Infallible;
use std::path::PathBuf;
use std::sync::Arc;
use std::time::{Duration, Instant};
use tokio_stream::wrappers::BroadcastStream;
use tower_http::cors::{Any, CorsLayer};
use tower_http::services::{ServeDir, ServeFile};

pub struct AppState {
    pub limiter: Arc<RateLimiterEngine>,
    pub ring: Arc<ConsistentHashRing>,
    pub telemetry: Arc<TelemetryStore>,
    pub janitor: Arc<JanitorController>,
}

impl AppState {
    pub fn new(snapshot_path: impl Into<PathBuf>) -> Arc<Self> {
        let limiter = Arc::new(RateLimiterEngine::new());
        let ring = Arc::new(ConsistentHashRing::new());
        let telemetry = Arc::new(TelemetryStore::new(snapshot_path.into()));
        let janitor = Arc::new(JanitorController::new());

        if let Some(persisted) = telemetry.load_persisted_snapshot() {
            limiter.load_rules(persisted.rules);
        }

        Arc::new(Self {
            limiter,
            ring,
            telemetry,
            janitor,
        })
    }

    pub fn snapshot(&self) -> TelemetrySnapshot {
        self.telemetry.current_snapshot(
            self.limiter.get_counters(),
            self.ring.get_topology(),
            self.limiter.get_overload_guard(),
            self.janitor.stats(&self.limiter, &self.telemetry),
        )
    }
}

#[derive(Debug, Deserialize)]
pub struct EvaluateRequest {
    pub namespace: String,
    pub key: String,
    pub tokens: Option<u64>,
    pub algorithm: Option<Algorithm>,
    #[serde(default)]
    pub priority: PriorityTier,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct EvaluateResponse {
    pub decision: Decision,
    pub http_status: u16,
    pub namespace: String,
    pub key: String,
    pub node_id: String,
    pub node_hash: String,
    pub algorithm: Algorithm,
    pub mode: PolicyMode,
    pub priority: PriorityTier,
    pub load_shed_active: bool,
    pub limit: u64,
    pub remaining: u64,
    pub reset_secs: u64,
    pub retry_after_secs: u64,
    pub eval_time_us: f64,
}

#[derive(Debug, Deserialize)]
pub struct BatchIngestRequest {
    pub events: Vec<EvaluateRequest>,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct BatchIngestResponse {
    pub total: usize,
    pub allowed: usize,
    pub throttled: usize,
    pub shadow_flagged: usize,
    pub shed_overload: usize,
    pub total_time_us: f64,
    pub avg_eval_us: f64,
    pub anomalies_detected: usize,
}

#[derive(Debug, Deserialize)]
pub struct ChaosBurstRequest {
    pub mode: Option<String>,
    pub count: Option<usize>,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct ChaosBurstResponse {
    pub mode: String,
    pub dispatched: usize,
    pub allowed: usize,
    pub throttled: usize,
    pub shadow_flagged: usize,
    pub shed_overload: usize,
    pub mean_eval_us: f64,
    pub new_anomalies: usize,
}

#[derive(Debug, Deserialize)]
pub struct ChaosOverloadRequest {
    pub count: Option<usize>,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct ChaosOverloadResponse {
    pub state: String,
    pub dispatched: usize,
    pub allowed_critical: usize,
    pub allowed_standard: usize,
    pub throttled_standard: usize,
    pub shed_best_effort: usize,
    pub mean_eval_us: f64,
}

#[derive(Debug, Deserialize)]
pub struct OverloadGuardRequest {
    pub enabled: bool,
    pub max_rps_threshold: Option<f64>,
    pub shed_best_effort: Option<bool>,
    pub shedding_active: Option<bool>,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct HealthResponse {
    pub status: String,
    pub version: String,
    pub runtime: String,
    pub uptime_secs: u64,
    pub total_evaluations: u64,
    pub active_rules: usize,
    pub online_nodes: usize,
    pub total_nodes: usize,
    pub buffer_events: usize,
    pub active_buckets: usize,
    pub load_shedding_active: bool,
}

#[derive(Debug, Deserialize)]
pub struct UpsertRuleRequest {
    pub namespace: String,
    pub algorithm: Algorithm,
    pub capacity: u64,
    pub refill_per_sec: f64,
    pub mode: PolicyMode,
    #[serde(default)]
    pub description: String,
}

pub fn build_router(state: Arc<AppState>, static_dir: Option<PathBuf>) -> Router {
    let cors = CorsLayer::new()
        .allow_origin(Any)
        .allow_methods(Any)
        .allow_headers(Any);

    let api_router = Router::new()
        .route("/health", get(health_handler))
        .route("/metrics", get(prometheus_metrics_handler))
        .route("/v1/evaluate", post(evaluate_handler))
        .route("/v1/ingest/batch", post(batch_ingest_handler))
        .route("/api/stream", get(sse_stream_handler))
        .route("/api/telemetry", get(telemetry_snapshot_handler))
        .route("/api/rules", get(list_rules_handler).post(upsert_rule_handler))
        .route("/api/rules/:namespace", delete(delete_rule_handler))
        .route("/api/cluster/topology", get(cluster_topology_handler))
        .route(
            "/api/cluster/nodes/:node_id/toggle",
            post(toggle_cluster_node_handler),
        )
        .route(
            "/api/system/overload-guard",
            get(get_overload_guard_handler).post(configure_overload_guard_handler),
        )
        .route("/api/chaos/burst", post(chaos_burst_handler))
        .route("/api/chaos/overload", post(chaos_overload_handler))
        .with_state(state)
        .layer(cors);

    if let Some(dir) = static_dir {
        if dir.exists() {
            let index_file = dir.join("index.html");
            let serve_dir = ServeDir::new(dir).not_found_service(ServeFile::new(index_file));
            return api_router.fallback_service(serve_dir);
        }
    }

    api_router
}

pub fn spawn_telemetry_ticker(state: Arc<AppState>) {
    tokio::spawn(async move {
        let mut interval = tokio::time::interval(Duration::from_millis(500));
        loop {
            interval.tick().await;
            let counters = state.limiter.get_counters();
            let topology = state.ring.get_topology();
            let overload_guard = state.limiter.get_overload_guard();
            let janitor = state.janitor.stats(&state.limiter, &state.telemetry);
            let snapshot = state.telemetry.advance_tick_and_snapshot(
                counters,
                topology,
                overload_guard,
                janitor,
            );
            state.limiter.sync_overload_with_rps(snapshot.current_rps);
        }
    });
}

pub fn spawn_janitor_task(state: Arc<AppState>) {
    spawn_janitor_loop(
        Arc::clone(&state.limiter),
        Arc::clone(&state.telemetry),
        Arc::clone(&state.janitor),
        Duration::from_secs(5),
        Duration::from_secs(120),
    );
}

fn evaluate_single(state: &AppState, req: &EvaluateRequest) -> EvaluateResponse {
    let start = Instant::now();
    let (node_id, node_hash) = state.ring.route_key(&req.key);
    let tokens = req.tokens.unwrap_or(1).max(1);
    let outcome = state.limiter.evaluate_with_priority(
        &req.namespace,
        &req.key,
        tokens,
        req.algorithm,
        req.priority,
    );
    let eval_time_us = (start.elapsed().as_secs_f64() * 1_000_000.0).max(0.1);
    let rounded_us = (eval_time_us * 100.0).round() / 100.0;

    state.telemetry.record_event(
        &req.namespace,
        &req.key,
        &node_id,
        node_hash,
        outcome.algorithm,
        outcome.mode,
        outcome.priority,
        outcome.load_shed_active,
        outcome.decision,
        outcome.remaining,
        outcome.limit,
        rounded_us,
    );

    let http_status = match outcome.decision {
        Decision::ShedOverload => 503,
        Decision::Throttled => 429,
        Decision::Allowed | Decision::ShadowFlagged => 200,
    };

    EvaluateResponse {
        decision: outcome.decision,
        http_status,
        namespace: req.namespace.clone(),
        key: req.key.clone(),
        node_id,
        node_hash: format!("{:016x}", node_hash),
        algorithm: outcome.algorithm,
        mode: outcome.mode,
        priority: outcome.priority,
        load_shed_active: outcome.load_shed_active,
        limit: outcome.limit,
        remaining: outcome.remaining,
        reset_secs: outcome.reset_secs,
        retry_after_secs: outcome.retry_after_secs,
        eval_time_us: rounded_us,
    }
}

async fn health_handler(State(state): State<Arc<AppState>>) -> Json<HealthResponse> {
    let counters = state.limiter.get_counters();
    let topology = state.ring.get_topology();
    let guard = state.limiter.get_overload_guard();
    Json(HealthResponse {
        status: "ok".to_string(),
        version: env!("CARGO_PKG_VERSION").to_string(),
        runtime: "rust-tokio-axum".to_string(),
        uptime_secs: state.telemetry.uptime_secs(),
        total_evaluations: counters.total_evaluations,
        active_rules: state.limiter.list_rules().len(),
        online_nodes: topology.online_nodes,
        total_nodes: topology.total_nodes,
        buffer_events: state.telemetry.recent_events_len(),
        active_buckets: counters.active_buckets,
        load_shedding_active: guard.enabled && guard.shedding_active,
    })
}

async fn prometheus_metrics_handler(State(state): State<Arc<AppState>>) -> Response {
    let snapshot = state.snapshot();
    let c = &snapshot.counters;
    let l = &snapshot.latency;
    let t = &snapshot.topology;
    let j = &snapshot.janitor;
    let shed_active = if snapshot.overload_guard.enabled && snapshot.overload_guard.shedding_active
    {
        1
    } else {
        0
    };

    let body = format!(
        "# HELP vulcan_requests_total Total rate-limit evaluations partitioned by decision.\n\
         # TYPE vulcan_requests_total counter\n\
         vulcan_requests_total{{decision=\"allowed\"}} {}\n\
         vulcan_requests_total{{decision=\"throttled\"}} {}\n\
         vulcan_requests_total{{decision=\"shadow_flagged\"}} {}\n\
         vulcan_requests_total{{decision=\"shed_overload\"}} {}\n\
         # HELP vulcan_eval_duration_microseconds Engine evaluation latency quantiles in microseconds.\n\
         # TYPE vulcan_eval_duration_microseconds summary\n\
         vulcan_eval_duration_microseconds{{quantile=\"0.5\"}} {:.2}\n\
         vulcan_eval_duration_microseconds{{quantile=\"0.95\"}} {:.2}\n\
         vulcan_eval_duration_microseconds{{quantile=\"0.99\"}} {:.2}\n\
         vulcan_eval_duration_microseconds_max {:.2}\n\
         vulcan_eval_duration_microseconds_mean {:.2}\n\
         # HELP vulcan_ring_active_nodes Number of online edge nodes in the Consistent Hashing ring.\n\
         # TYPE vulcan_ring_active_nodes gauge\n\
         vulcan_ring_active_nodes {}\n\
         # HELP vulcan_ring_total_vnodes Active virtual replicas across the Consistent Hashing ring.\n\
         # TYPE vulcan_ring_total_vnodes gauge\n\
         vulcan_ring_total_vnodes {}\n\
         # HELP vulcan_active_buckets Active per-key state buckets currently held in DashMap.\n\
         # TYPE vulcan_active_buckets gauge\n\
         vulcan_active_buckets {}\n\
         # HELP vulcan_janitor_keys_evicted_total Expired buckets reclaimed by the Tokio background janitor.\n\
         # TYPE vulcan_janitor_keys_evicted_total counter\n\
         vulcan_janitor_keys_evicted_total {}\n\
         # HELP vulcan_overload_shedding_active Whether Adaptive Load-Shedding protection is currently active (1=active, 0=nominal).\n\
         # TYPE vulcan_overload_shedding_active gauge\n\
         vulcan_overload_shedding_active {}\n",
        c.allowed,
        c.throttled,
        c.shadow_flagged,
        c.shed_overload,
        l.p50_us,
        l.p95_us,
        l.p99_us,
        l.max_us,
        l.mean_us,
        t.online_nodes,
        t.total_vnodes,
        c.active_buckets,
        j.keys_evicted_total,
        shed_active
    );

    let mut headers = HeaderMap::new();
    headers.insert(
        header::CONTENT_TYPE,
        HeaderValue::from_static("text/plain; version=0.0.4; charset=utf-8"),
    );
    (StatusCode::OK, headers, body).into_response()
}

async fn evaluate_handler(
    State(state): State<Arc<AppState>>,
    Json(req): Json<EvaluateRequest>,
) -> Response {
    let res = evaluate_single(&state, &req);

    let mut headers = HeaderMap::new();
    headers.insert(
        "X-RateLimit-Limit",
        HeaderValue::from_str(&res.limit.to_string()).unwrap_or_else(|_| HeaderValue::from_static("0")),
    );
    headers.insert(
        "X-RateLimit-Remaining",
        HeaderValue::from_str(&res.remaining.to_string())
            .unwrap_or_else(|_| HeaderValue::from_static("0")),
    );
    headers.insert(
        "X-RateLimit-Reset",
        HeaderValue::from_str(&res.reset_secs.to_string())
            .unwrap_or_else(|_| HeaderValue::from_static("1")),
    );
    headers.insert(
        "X-Vulcan-Node",
        HeaderValue::from_str(&res.node_id)
            .unwrap_or_else(|_| HeaderValue::from_static("unassigned")),
    );
    headers.insert(
        "X-Vulcan-Priority",
        HeaderValue::from_static(res.priority.as_str()),
    );
    headers.insert(
        "X-Vulcan-Load-Shed",
        HeaderValue::from_static(
            if res.load_shed_active || res.decision == Decision::ShedOverload {
                "active"
            } else {
                "nominal"
            },
        ),
    );

    if matches!(
        res.decision,
        Decision::Throttled | Decision::ShadowFlagged | Decision::ShedOverload
    ) {
        headers.insert(
            header::RETRY_AFTER,
            HeaderValue::from_str(&res.retry_after_secs.max(1).to_string())
                .unwrap_or_else(|_| HeaderValue::from_static("1")),
        );
    }

    let status = match res.decision {
        Decision::ShedOverload => StatusCode::SERVICE_UNAVAILABLE,
        Decision::Throttled => StatusCode::TOO_MANY_REQUESTS,
        Decision::Allowed | Decision::ShadowFlagged => StatusCode::OK,
    };

    (status, headers, Json(res)).into_response()
}

async fn batch_ingest_handler(
    State(state): State<Arc<AppState>>,
    Json(req): Json<BatchIngestRequest>,
) -> Json<BatchIngestResponse> {
    let start = Instant::now();
    let total = req.events.len();
    let mut allowed = 0usize;
    let mut throttled = 0usize;
    let mut shadow_flagged = 0usize;
    let mut shed_overload = 0usize;
    let mut eval_sum = 0.0f64;

    for item in &req.events {
        let res = evaluate_single(&state, item);
        eval_sum += res.eval_time_us;
        match res.decision {
            Decision::Allowed => allowed += 1,
            Decision::Throttled => throttled += 1,
            Decision::ShadowFlagged => shadow_flagged += 1,
            Decision::ShedOverload => shed_overload += 1,
        }
    }

    let anomalies = state.telemetry.evaluate_zscore_anomalies(3.0);
    let total_time_us = ((start.elapsed().as_secs_f64() * 1_000_000.0) * 100.0).round() / 100.0;
    let avg_eval_us = if total > 0 {
        ((eval_sum / total as f64) * 100.0).round() / 100.0
    } else {
        0.0
    };

    Json(BatchIngestResponse {
        total,
        allowed,
        throttled,
        shadow_flagged,
        shed_overload,
        total_time_us,
        avg_eval_us,
        anomalies_detected: anomalies.len(),
    })
}

async fn telemetry_snapshot_handler(
    State(state): State<Arc<AppState>>,
) -> Json<TelemetrySnapshot> {
    Json(state.snapshot())
}

async fn sse_stream_handler(
    State(state): State<Arc<AppState>>,
) -> Sse<impl Stream<Item = Result<Event, Infallible>>> {
    let initial = state.snapshot();
    let initial_json = serde_json::to_string(&initial).unwrap_or_else(|_| "{}".to_string());
    let initial_stream = stream::once(async move {
        Ok::<Event, Infallible>(Event::default().event("telemetry").data(initial_json))
    });

    let rx = state.telemetry.subscribe();
    let live_stream = BroadcastStream::new(rx).filter_map(|msg| async move {
        match msg {
            Ok(snapshot) => {
                let json = serde_json::to_string(&snapshot).ok()?;
                Some(Ok::<Event, Infallible>(
                    Event::default().event("telemetry").data(json),
                ))
            }
            Err(_) => None,
        }
    });

    Sse::new(initial_stream.chain(live_stream)).keep_alive(
        KeepAlive::new()
            .interval(Duration::from_secs(10))
            .text("keep-alive"),
    )
}

async fn list_rules_handler(State(state): State<Arc<AppState>>) -> Json<Vec<RateLimitRule>> {
    Json(state.limiter.list_rules())
}

async fn upsert_rule_handler(
    State(state): State<Arc<AppState>>,
    Json(payload): Json<UpsertRuleRequest>,
) -> (StatusCode, Json<RateLimitRule>) {
    let rule = state.limiter.upsert_rule(RateLimitRule {
        namespace: payload.namespace,
        algorithm: payload.algorithm,
        capacity: payload.capacity,
        refill_per_sec: payload.refill_per_sec,
        mode: payload.mode,
        description: payload.description,
    });
    let _ = state.telemetry.persist_snapshot(state.limiter.list_rules());
    (StatusCode::OK, Json(rule))
}

async fn delete_rule_handler(
    State(state): State<Arc<AppState>>,
    Path(namespace): Path<String>,
) -> Response {
    if let Some(removed) = state.limiter.delete_rule(&namespace) {
        let _ = state.telemetry.persist_snapshot(state.limiter.list_rules());
        (StatusCode::OK, Json(removed)).into_response()
    } else {
        (
            StatusCode::NOT_FOUND,
            Json(serde_json::json!({ "error": format!("Rule '{}' not found", namespace) })),
        )
            .into_response()
    }
}

async fn cluster_topology_handler(
    State(state): State<Arc<AppState>>,
) -> Json<crate::ring::ClusterTopologySnapshot> {
    Json(state.ring.get_topology())
}

async fn toggle_cluster_node_handler(
    State(state): State<Arc<AppState>>,
    Path(node_id): Path<String>,
) -> Response {
    match state.ring.toggle_node(&node_id) {
        Ok(res) => (StatusCode::OK, Json(res)).into_response(),
        Err(msg) => (
            StatusCode::BAD_REQUEST,
            Json(serde_json::json!({ "error": msg })),
        )
            .into_response(),
    }
}

async fn get_overload_guard_handler(
    State(state): State<Arc<AppState>>,
) -> Json<OverloadGuardConfig> {
    Json(state.limiter.get_overload_guard())
}

async fn configure_overload_guard_handler(
    State(state): State<Arc<AppState>>,
    Json(req): Json<OverloadGuardRequest>,
) -> Json<OverloadGuardConfig> {
    let current = state.limiter.get_overload_guard();
    let updated = state.limiter.configure_overload_guard(
        req.enabled,
        req.max_rps_threshold.unwrap_or(current.max_rps_threshold),
        req.shed_best_effort.unwrap_or(current.shed_best_effort),
        req.shedding_active,
    );
    state.telemetry.advance_tick_and_snapshot(
        state.limiter.get_counters(),
        state.ring.get_topology(),
        updated.clone(),
        state.janitor.stats(&state.limiter, &state.telemetry),
    );
    Json(updated)
}

async fn chaos_burst_handler(
    State(state): State<Arc<AppState>>,
    Json(req): Json<ChaosBurstRequest>,
) -> Json<ChaosBurstResponse> {
    let mode = req
        .mode
        .unwrap_or_else(|| "normal".to_string())
        .to_lowercase();
    let count = req.count.unwrap_or(if mode == "ddos" { 160 } else { 110 }).min(1500);

    let baseline_keys = [
        "18.204.11.42",
        "34.198.22.10",
        "52.87.145.9",
        "104.16.84.20",
        "172.64.33.19",
        "45.134.8.71",
        "190.24.118.5",
        "200.89.14.33",
        "181.49.77.12",
        "88.198.42.90",
        "91.108.56.14",
        "138.197.12.7",
        "159.89.202.30",
        "64.233.160.1",
        "142.250.72.14",
        "198.51.100.84",
        "203.0.113.19",
        "13.58.241.6",
    ];
    let namespaces = ["api-prod", "ai-inference", "public-edge", "checkout-core"];

    let mut tasks = Vec::with_capacity(count);

    for i in 0..count {
        let st = Arc::clone(&state);
        let is_ddos = mode == "ddos";
        let req_item = if is_ddos && i >= 24 {
            EvaluateRequest {
                namespace: if i % 3 == 0 {
                    "checkout-core".to_string()
                } else {
                    "api-prod".to_string()
                },
                key: "attacker-bot-99.210.44.12".to_string(),
                tokens: Some(1),
                algorithm: None,
                priority: PriorityTier::Standard,
            }
        } else {
            let ns = namespaces[i % namespaces.len()].to_string();
            let key = format!("edge-{}", baseline_keys[i % baseline_keys.len()]);
            EvaluateRequest {
                namespace: ns,
                key,
                tokens: Some(1),
                algorithm: None,
                priority: if i % 5 == 0 {
                    PriorityTier::Critical
                } else if i % 4 == 0 {
                    PriorityTier::BestEffort
                } else {
                    PriorityTier::Standard
                },
            }
        };

        tasks.push(tokio::spawn(async move { evaluate_single(&st, &req_item) }));
    }

    let results = futures::future::join_all(tasks).await;
    let mut allowed = 0usize;
    let mut throttled = 0usize;
    let mut shadow_flagged = 0usize;
    let mut shed_overload = 0usize;
    let mut sum_us = 0.0f64;

    for r in results.into_iter().flatten() {
        sum_us += r.eval_time_us;
        match r.decision {
            Decision::Allowed => allowed += 1,
            Decision::Throttled => throttled += 1,
            Decision::ShadowFlagged => shadow_flagged += 1,
            Decision::ShedOverload => shed_overload += 1,
        }
    }

    let anomalies = state.telemetry.evaluate_zscore_anomalies(3.0);
    let mean_eval_us = if count > 0 {
        ((sum_us / count as f64) * 100.0).round() / 100.0
    } else {
        0.0
    };

    state.telemetry.advance_tick_and_snapshot(
        state.limiter.get_counters(),
        state.ring.get_topology(),
        state.limiter.get_overload_guard(),
        state.janitor.stats(&state.limiter, &state.telemetry),
    );

    Json(ChaosBurstResponse {
        mode,
        dispatched: count,
        allowed,
        throttled,
        shadow_flagged,
        shed_overload,
        mean_eval_us,
        new_anomalies: anomalies.len(),
    })
}

async fn chaos_overload_handler(
    State(state): State<Arc<AppState>>,
    req: Option<Json<ChaosOverloadRequest>>,
) -> Json<ChaosOverloadResponse> {
    let count = req.and_then(|r| r.count).unwrap_or(150).min(1500);
    let current = state.limiter.get_overload_guard();
    let active_guard = state.limiter.configure_overload_guard(
        true,
        current.max_rps_threshold,
        true,
        Some(true),
    );

    let mut tasks = Vec::with_capacity(count);
    for i in 0..count {
        let st = Arc::clone(&state);
        let (priority, ns, key) = match i % 3 {
            0 => (
                PriorityTier::Critical,
                "checkout-core".to_string(),
                format!("crit-pay-gateway-{}", i % 12),
            ),
            1 => (
                PriorityTier::Standard,
                "ai-inference".to_string(),
                format!("std-api-tenant-{}", i % 3),
            ),
            _ => (
                PriorityTier::BestEffort,
                "api-prod".to_string(),
                format!("crawler-batch-{}", i % 25),
            ),
        };

        let eval_req = EvaluateRequest {
            namespace: ns,
            key,
            tokens: Some(1),
            algorithm: None,
            priority,
        };
        tasks.push(tokio::spawn(async move { evaluate_single(&st, &eval_req) }));
    }

    let results = futures::future::join_all(tasks).await;
    let mut allowed_critical = 0usize;
    let mut allowed_standard = 0usize;
    let mut throttled_standard = 0usize;
    let mut shed_best_effort = 0usize;
    let mut sum_us = 0.0f64;

    for r in results.into_iter().flatten() {
        sum_us += r.eval_time_us;
        match (r.priority, r.decision) {
            (PriorityTier::Critical, Decision::Allowed) => allowed_critical += 1,
            (PriorityTier::Standard, Decision::Allowed) => allowed_standard += 1,
            (PriorityTier::Standard, Decision::Throttled) => throttled_standard += 1,
            (PriorityTier::BestEffort, Decision::ShedOverload) => shed_best_effort += 1,
            _ => {}
        }
    }

    let mean_eval_us = if count > 0 {
        ((sum_us / count as f64) * 100.0).round() / 100.0
    } else {
        0.0
    };

    state.telemetry.advance_tick_and_snapshot(
        state.limiter.get_counters(),
        state.ring.get_topology(),
        active_guard,
        state.janitor.stats(&state.limiter, &state.telemetry),
    );

    Json(ChaosOverloadResponse {
        state: "OVERLOAD_SHEDDING".to_string(),
        dispatched: count,
        allowed_critical,
        allowed_standard,
        throttled_standard,
        shed_best_effort,
        mean_eval_us,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::body::Body;
    use axum::http::Request;
    use http_body_util::BodyExt;
    use std::time::{SystemTime, UNIX_EPOCH};
    use tower::ServiceExt;

    fn test_app() -> (Arc<AppState>, Router, PathBuf) {
        let nanos = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let path = std::env::temp_dir().join(format!("vulcan_routes_test_{}.json", nanos));
        let state = AppState::new(&path);
        let router = build_router(Arc::clone(&state), None);
        (state, router, path)
    }

    async fn read_json<T: serde::de::DeserializeOwned>(resp: Response) -> T {
        let bytes = resp.into_body().collect().await.unwrap().to_bytes();
        serde_json::from_slice(&bytes).unwrap()
    }

    #[tokio::test]
    async fn health_endpoint_returns_runtime_status() {
        let (_state, app, path) = test_app();
        let resp = app
            .oneshot(Request::builder().uri("/health").body(Body::empty()).unwrap())
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::OK);
        let body: HealthResponse = read_json(resp).await;
        assert_eq!(body.status, "ok");
        assert_eq!(body.online_nodes, 4);
        assert!(body.active_rules >= 4);
        assert!(!body.load_shedding_active);
        let _ = std::fs::remove_file(path);
    }

    #[tokio::test]
    async fn evaluate_returns_ratelimit_headers_and_429_when_exhausted() {
        let (_state, app, path) = test_app();

        for _ in 0..20 {
            let req = Request::builder()
                .method("POST")
                .uri("/v1/evaluate")
                .header("content-type", "application/json")
                .body(Body::from(
                    r#"{"namespace":"ai-inference","key":"client-gpu-01","tokens":1}"#,
                ))
                .unwrap();
            let resp = app.clone().oneshot(req).await.unwrap();
            assert_eq!(resp.status(), StatusCode::OK);
            assert!(resp.headers().contains_key("X-RateLimit-Limit"));
            assert!(resp.headers().contains_key("X-RateLimit-Remaining"));
            assert!(resp.headers().contains_key("X-RateLimit-Reset"));
            assert!(resp.headers().contains_key("X-Vulcan-Node"));
            assert_eq!(
                resp.headers().get("X-Vulcan-Load-Shed").unwrap(),
                "nominal"
            );
        }

        let req = Request::builder()
            .method("POST")
            .uri("/v1/evaluate")
            .header("content-type", "application/json")
            .body(Body::from(
                r#"{"namespace":"ai-inference","key":"client-gpu-01","tokens":1}"#,
            ))
            .unwrap();
        let resp = app.oneshot(req).await.unwrap();
        assert_eq!(resp.status(), StatusCode::TOO_MANY_REQUESTS);
        assert!(resp.headers().contains_key(header::RETRY_AFTER));
        let body: EvaluateResponse = read_json(resp).await;
        assert_eq!(body.decision, Decision::Throttled);
        assert_eq!(body.http_status, 429);
        assert!(body.eval_time_us > 0.0);
        let _ = std::fs::remove_file(path);
    }

    #[tokio::test]
    async fn batch_ingest_and_rules_crud_endpoints_work() {
        let (_state, app, path) = test_app();

        let upsert = Request::builder()
            .method("POST")
            .uri("/api/rules")
            .header("content-type", "application/json")
            .body(Body::from(
                r#"{"namespace":"stress-ns","algorithm":"sliding_window","capacity":5,"refill_per_sec":5.0,"mode":"enforce","description":"batch test"}"#,
            ))
            .unwrap();
        let resp = app.clone().oneshot(upsert).await.unwrap();
        assert_eq!(resp.status(), StatusCode::OK);

        let batch = Request::builder()
            .method("POST")
            .uri("/v1/ingest/batch")
            .header("content-type", "application/json")
            .body(Body::from(
                r#"{"events":[{"namespace":"stress-ns","key":"b1"},{"namespace":"stress-ns","key":"b1"},{"namespace":"stress-ns","key":"b1"},{"namespace":"stress-ns","key":"b1"},{"namespace":"stress-ns","key":"b1"},{"namespace":"stress-ns","key":"b1"},{"namespace":"stress-ns","key":"b1"}]}"#,
            ))
            .unwrap();
        let batch_resp = app.clone().oneshot(batch).await.unwrap();
        assert_eq!(batch_resp.status(), StatusCode::OK);
        let parsed: BatchIngestResponse = read_json(batch_resp).await;
        assert_eq!(parsed.total, 7);
        assert_eq!(parsed.allowed, 5);
        assert_eq!(parsed.throttled, 2);

        let del = Request::builder()
            .method("DELETE")
            .uri("/api/rules/stress-ns")
            .body(Body::empty())
            .unwrap();
        let del_resp = app.oneshot(del).await.unwrap();
        assert_eq!(del_resp.status(), StatusCode::OK);
        let _ = std::fs::remove_file(path);
    }

    #[tokio::test]
    async fn cluster_topology_and_chaos_ddos_endpoints() {
        let (_state, app, path) = test_app();

        let toggle = Request::builder()
            .method("POST")
            .uri("/api/cluster/nodes/edge-fra-04/toggle")
            .body(Body::empty())
            .unwrap();
        let toggle_resp = app.clone().oneshot(toggle).await.unwrap();
        assert_eq!(toggle_resp.status(), StatusCode::OK);

        let chaos = Request::builder()
            .method("POST")
            .uri("/api/chaos/burst")
            .header("content-type", "application/json")
            .body(Body::from(r#"{"mode":"ddos","count":150}"#))
            .unwrap();
        let chaos_resp = app.clone().oneshot(chaos).await.unwrap();
        assert_eq!(chaos_resp.status(), StatusCode::OK);
        let chaos_body: ChaosBurstResponse = read_json(chaos_resp).await;
        assert_eq!(chaos_body.dispatched, 150);
        assert!(chaos_body.throttled > 0);
        assert!(chaos_body.new_anomalies >= 1);

        let sse_req = Request::builder()
            .uri("/api/stream")
            .body(Body::empty())
            .unwrap();
        let sse_resp = app.oneshot(sse_req).await.unwrap();
        assert_eq!(sse_resp.status(), StatusCode::OK);
        let ct = sse_resp
            .headers()
            .get(header::CONTENT_TYPE)
            .unwrap()
            .to_str()
            .unwrap();
        assert!(ct.contains("text/event-stream"));

        let _ = std::fs::remove_file(path);
    }

    #[tokio::test]
    async fn overload_guard_and_evaluate_return_503_for_best_effort_and_200_for_critical() {
        let (_state, app, path) = test_app();

        let enable_guard = Request::builder()
            .method("POST")
            .uri("/api/system/overload-guard")
            .header("content-type", "application/json")
            .body(Body::from(
                r#"{"enabled":true,"max_rps_threshold":120.0,"shed_best_effort":true}"#,
            ))
            .unwrap();
        let guard_resp = app.clone().oneshot(enable_guard).await.unwrap();
        assert_eq!(guard_resp.status(), StatusCode::OK);
        let guard_cfg: OverloadGuardConfig = read_json(guard_resp).await;
        assert!(guard_cfg.enabled && guard_cfg.shedding_active);

        // Best-effort request must receive HTTP 503 + X-Vulcan-Load-Shed: active
        let best_req = Request::builder()
            .method("POST")
            .uri("/v1/evaluate")
            .header("content-type", "application/json")
            .body(Body::from(
                r#"{"namespace":"api-prod","key":"bg-crawler-01","tokens":1,"priority":"best_effort"}"#,
            ))
            .unwrap();
        let best_resp = app.clone().oneshot(best_req).await.unwrap();
        assert_eq!(best_resp.status(), StatusCode::SERVICE_UNAVAILABLE);
        assert_eq!(
            best_resp.headers().get("X-Vulcan-Load-Shed").unwrap(),
            "active"
        );
        assert_eq!(
            best_resp.headers().get("X-Vulcan-Priority").unwrap(),
            "best_effort"
        );
        let best_body: EvaluateResponse = read_json(best_resp).await;
        assert_eq!(best_body.decision, Decision::ShedOverload);
        assert_eq!(best_body.http_status, 503);

        // Critical request must still pass with HTTP 200
        let crit_req = Request::builder()
            .method("POST")
            .uri("/v1/evaluate")
            .header("content-type", "application/json")
            .body(Body::from(
                r#"{"namespace":"api-prod","key":"auth-core-01","tokens":1,"priority":"critical"}"#,
            ))
            .unwrap();
        let crit_resp = app.oneshot(crit_req).await.unwrap();
        assert_eq!(crit_resp.status(), StatusCode::OK);
        assert_eq!(
            crit_resp.headers().get("X-Vulcan-Priority").unwrap(),
            "critical"
        );
        let crit_body: EvaluateResponse = read_json(crit_resp).await;
        assert_eq!(crit_body.decision, Decision::Allowed);

        let _ = std::fs::remove_file(path);
    }

    #[tokio::test]
    async fn chaos_overload_endpoint_triggers_shedding_across_priority_tiers() {
        let (_state, app, path) = test_app();

        let overload_req = Request::builder()
            .method("POST")
            .uri("/api/chaos/overload")
            .header("content-type", "application/json")
            .body(Body::from(r#"{"count":90}"#))
            .unwrap();
        let resp = app.oneshot(overload_req).await.unwrap();
        assert_eq!(resp.status(), StatusCode::OK);
        let body: ChaosOverloadResponse = read_json(resp).await;
        assert_eq!(body.state, "OVERLOAD_SHEDDING");
        assert_eq!(body.dispatched, 90);
        assert_eq!(body.allowed_critical, 30);
        assert_eq!(body.shed_best_effort, 30);
        assert!(body.throttled_standard > 0);
        let _ = std::fs::remove_file(path);
    }

    #[tokio::test]
    async fn prometheus_metrics_endpoint_emits_openmetrics_format() {
        let (_state, app, path) = test_app();

        // Dispatch a single evaluation first
        let eval = Request::builder()
            .method("POST")
            .uri("/v1/evaluate")
            .header("content-type", "application/json")
            .body(Body::from(
                r#"{"namespace":"api-prod","key":"prom-probe","tokens":1}"#,
            ))
            .unwrap();
        let _ = app.clone().oneshot(eval).await.unwrap();

        let metrics_req = Request::builder()
            .uri("/metrics")
            .body(Body::empty())
            .unwrap();
        let resp = app.oneshot(metrics_req).await.unwrap();
        assert_eq!(resp.status(), StatusCode::OK);
        let ct = resp
            .headers()
            .get(header::CONTENT_TYPE)
            .unwrap()
            .to_str()
            .unwrap();
        assert!(ct.contains("text/plain; version=0.0.4"));

        let bytes = resp.into_body().collect().await.unwrap().to_bytes();
        let text = String::from_utf8(bytes.to_vec()).unwrap();
        assert!(text.contains("vulcan_requests_total{decision=\"allowed\"} 1"));
        assert!(text.contains("vulcan_eval_duration_microseconds{quantile=\"0.99\"}"));
        assert!(text.contains("vulcan_ring_active_nodes 4"));
        assert!(text.contains("vulcan_active_buckets 1"));
        assert!(text.contains("vulcan_janitor_keys_evicted_total 0"));

        let _ = std::fs::remove_file(path);
    }
}

use axum::body::Body;
use axum::http::{Request, StatusCode};
use http_body_util::BodyExt;
use std::path::PathBuf;
use std::sync::Arc;
use std::time::{Duration, SystemTime, UNIX_EPOCH};
use tower::ServiceExt;
use vulcan_engine::limiter::Decision;
use vulcan_engine::ring::fnv1a_64;
use vulcan_engine::routes::{build_router, AppState, EvaluateResponse};

fn temp_state_path(tag: &str) -> PathBuf {
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    std::env::temp_dir().join(format!("vulcan_it_{}_{}.json", tag, nanos))
}

#[tokio::test]
async fn shadow_mode_endpoint_returns_200_with_shadow_flagged_decision() {
    let path = temp_state_path("shadow");
    let state = AppState::new(&path);
    let app = build_router(state, None);

    for _ in 0..90 {
        let req = Request::builder()
            .method("POST")
            .uri("/v1/evaluate")
            .header("content-type", "application/json")
            .body(Body::from(
                r#"{"namespace":"public-edge","key":"shadow-tester-1","tokens":1}"#,
            ))
            .unwrap();
        let resp = app.clone().oneshot(req).await.unwrap();
        assert_eq!(resp.status(), StatusCode::OK);
    }

    let req = Request::builder()
        .method("POST")
        .uri("/v1/evaluate")
        .header("content-type", "application/json")
        .body(Body::from(
            r#"{"namespace":"public-edge","key":"shadow-tester-1","tokens":1}"#,
        ))
        .unwrap();
    let resp = app.oneshot(req).await.unwrap();
    assert_eq!(resp.status(), StatusCode::OK);
    let bytes = resp.into_body().collect().await.unwrap().to_bytes();
    let body: EvaluateResponse = serde_json::from_slice(&bytes).unwrap();
    assert_eq!(body.decision, Decision::ShadowFlagged);
    let _ = std::fs::remove_file(path);
}

#[tokio::test]
async fn ring_topology_arc_shares_sum_to_100_percent() {
    let path = temp_state_path("ring_sum");
    let state = AppState::new(&path);
    let topo = state.ring.get_topology();

    let sum_pct: f64 = topo.nodes.iter().map(|n| n.key_share_pct).sum();
    assert!(
        (sum_pct - 100.0).abs() < 0.25,
        "expected ring shares to sum ~100%, got {}",
        sum_pct
    );
    assert_ne!(fnv1a_64(b"key-alpha"), fnv1a_64(b"key-beta"));
    let _ = std::fs::remove_file(path);
}

#[tokio::test]
async fn concurrent_http_evaluations_are_handled_without_errors() {
    let path = temp_state_path("http_conc");
    let state = AppState::new(&path);
    let app = Arc::new(build_router(state.clone(), None));

    let mut handles = Vec::new();
    for i in 0..40 {
        let router = (*app).clone();
        handles.push(tokio::spawn(async move {
            let payload = format!(
                r#"{{"namespace":"checkout-core","key":"buyer-{}","tokens":1}}"#,
                i % 5
            );
            let req = Request::builder()
                .method("POST")
                .uri("/v1/evaluate")
                .header("content-type", "application/json")
                .body(Body::from(payload))
                .unwrap();
            let resp = router.oneshot(req).await.unwrap();
            resp.status()
        }));
    }

    for h in handles {
        let status = h.await.unwrap();
        assert_eq!(status, StatusCode::OK);
    }

    let snapshot = state.snapshot();
    assert_eq!(snapshot.counters.total_evaluations, 40);
    assert_eq!(snapshot.counters.allowed, 40);
    let _ = std::fs::remove_file(path);
}

#[tokio::test]
async fn janitor_sweeps_buckets_and_updates_metrics_and_snapshot() {
    let path = temp_state_path("janitor_it");
    let state = AppState::new(&path);
    let app = build_router(state.clone(), None);

    for i in 0..3 {
        let payload = format!(r#"{{"namespace":"api-prod","key":"ephemeral-{}"}}"#, i);
        let req = Request::builder()
            .method("POST")
            .uri("/v1/evaluate")
            .header("content-type", "application/json")
            .body(Body::from(payload))
            .unwrap();
        let resp = app.clone().oneshot(req).await.unwrap();
        assert_eq!(resp.status(), StatusCode::OK);
    }

    let stats = state
        .janitor
        .run_sweep(&state.limiter, &state.telemetry, Duration::from_secs(0));
    assert_eq!(stats.keys_evicted_total, 3);
    assert_eq!(stats.active_buckets, 0);

    let metrics_resp = app
        .oneshot(Request::builder().uri("/metrics").body(Body::empty()).unwrap())
        .await
        .unwrap();
    let bytes = metrics_resp.into_body().collect().await.unwrap().to_bytes();
    let text = String::from_utf8(bytes.to_vec()).unwrap();
    assert!(text.contains("vulcan_janitor_keys_evicted_total 3"));
    assert!(text.contains("vulcan_active_buckets 0"));
    let _ = std::fs::remove_file(path);
}

#[tokio::test]
async fn priority_headers_and_load_shed_headers_are_returned_accurately() {
    let path = temp_state_path("prio_hdr");
    let state = AppState::new(&path);
    let app = build_router(state.clone(), None);

    // Nominal state check
    let nominal_req = Request::builder()
        .method("POST")
        .uri("/v1/evaluate")
        .header("content-type", "application/json")
        .body(Body::from(
            r#"{"namespace":"api-prod","key":"std-user","priority":"standard"}"#,
        ))
        .unwrap();
    let nominal_resp = app.clone().oneshot(nominal_req).await.unwrap();
    assert_eq!(nominal_resp.status(), StatusCode::OK);
    assert_eq!(
        nominal_resp.headers().get("X-Vulcan-Load-Shed").unwrap(),
        "nominal"
    );
    assert_eq!(
        nominal_resp.headers().get("X-Vulcan-Priority").unwrap(),
        "standard"
    );

    // Enable overload shedding and verify 503 + active header
    state
        .limiter
        .configure_overload_guard(true, 100.0, true, Some(true));

    let shed_req = Request::builder()
        .method("POST")
        .uri("/v1/evaluate")
        .header("content-type", "application/json")
        .body(Body::from(
            r#"{"namespace":"api-prod","key":"low-prio-job","priority":"best_effort"}"#,
        ))
        .unwrap();
    let shed_resp = app.oneshot(shed_req).await.unwrap();
    assert_eq!(shed_resp.status(), StatusCode::SERVICE_UNAVAILABLE);
    assert_eq!(
        shed_resp.headers().get("X-Vulcan-Load-Shed").unwrap(),
        "active"
    );
    assert!(shed_resp.headers().contains_key("Retry-After"));
    let _ = std::fs::remove_file(path);
}

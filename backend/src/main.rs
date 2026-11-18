use std::net::SocketAddr;
use std::path::PathBuf;
use vulcan_engine::routes::{
    build_router, spawn_janitor_task, spawn_telemetry_ticker, AppState,
};

#[tokio::main]
async fn main() {
    let port: u16 = std::env::var("PORT")
        .ok()
        .and_then(|p| p.parse().ok())
        .unwrap_or(8080);

    let data_path = std::env::var("VULCAN_SNAPSHOT_PATH")
        .map(PathBuf::from)
        .unwrap_or_else(|_| PathBuf::from("data/vulcan_snapshot.json"));

    let static_dir = std::env::var("VULCAN_STATIC_DIR")
        .map(PathBuf::from)
        .unwrap_or_else(|_| {
            let local = PathBuf::from("../frontend/dist");
            if local.exists() {
                local
            } else {
                PathBuf::from("frontend/dist")
            }
        });

    let state = AppState::new(data_path);
    spawn_telemetry_ticker(state.clone());
    spawn_janitor_task(state.clone());

    let app = build_router(state, Some(static_dir.clone()));
    let addr = SocketAddr::from(([0, 0, 0, 0], port));

    println!(
        "VULCAN // ENGINE v{} listening on http://{} (static: {})",
        env!("CARGO_PKG_VERSION"),
        addr,
        static_dir.display()
    );

    let listener = tokio::net::TcpListener::bind(addr)
        .await
        .expect("failed to bind TCP listener");

    axum::serve(listener, app)
        .await
        .expect("HTTP server exited unexpectedly");
}

use axum::{routing::get, Json, Router};
use serde_json::json;
use std::net::SocketAddr;

/// Starts a lightweight background HTTP server for health checks.
/// Render Web Services ping this endpoint to verify container health.
pub async fn start_health_server(port: u16) {
    let app = Router::new()
        .route("/", get(|| async { Json(json!({ "status": "ok", "app": "binbot" })) }))
        .route("/healthz", get(|| async { Json(json!({ "status": "ok", "app": "binbot" })) }));

    let addr = SocketAddr::from(([0, 0, 0, 0], port));

    match tokio::net::TcpListener::bind(addr).await {
        Ok(listener) => {
            tokio::spawn(async move {
                if let Err(e) = axum::serve(listener, app).await {
                    tracing::error!("Health server terminated: {e}");
                }
            });
            tracing::info!("✓ Web health service running on port {port}");
        }
        Err(e) => {
            tracing::warn!("Health server could not bind to port {port}: {e}");
        }
    }
}

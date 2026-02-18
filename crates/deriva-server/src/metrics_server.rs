use crate::metrics::encode_metrics;
use crate::state::ServerState;
use axum::extract::State;
use axum::http::StatusCode;
use axum::response::IntoResponse;
use axum::routing::get;
use axum::Router;
use std::net::SocketAddr;
use std::sync::Arc;

async fn metrics_handler() -> String {
    encode_metrics()
}

async fn health_handler(State(state): State<Arc<ServerState>>) -> impl IntoResponse {
    let cache_entries = state.cache.entry_count().await;
    let dag_recipes = state.dag.len();
    let uptime = state.start_time.elapsed().as_secs();

    (
        StatusCode::OK,
        axum::Json(serde_json::json!({
            "status": "healthy",
            "cache_entries": cache_entries,
            "dag_recipes": dag_recipes,
            "uptime_seconds": uptime,
        })),
    )
}

pub async fn start_metrics_server(port: u16, state: Arc<ServerState>) {
    let app = Router::new()
        .route("/metrics", get(metrics_handler))
        .route("/health", get(health_handler))
        .with_state(state);
    let addr = SocketAddr::from(([0, 0, 0, 0], port));
    tracing::info!("Metrics server listening on {}", addr);
    let listener = tokio::net::TcpListener::bind(addr).await.unwrap();
    axum::serve(listener, app).await.unwrap();
}

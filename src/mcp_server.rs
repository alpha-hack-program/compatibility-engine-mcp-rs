use rmcp::transport::{StreamableHttpServerConfig, streamable_http_server::{
    StreamableHttpService, session::local::LocalSessionManager,
}};
use tracing_subscriber::{
    layer::SubscriberExt,
    util::SubscriberInitExt,
    {self},
};
mod common;
use common::{compatibility_engine::CompatibilityEngine, metrics};
use axum::{response::IntoResponse, http::StatusCode};

use std::time::Duration;

const BIND_ADDRESS: &str = "127.0.0.1:8001";
const SHUTDOWN_TIMEOUT: Duration = Duration::from_secs(5);

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    tracing_subscriber::registry()
        .with(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| "debug".to_string().into()),
        )
        .with(tracing_subscriber::fmt::layer())
        .init();

    // Use environment variable or the static value
    let bind_address = std::env::var("BIND_ADDRESS").unwrap_or_else(|_| BIND_ADDRESS.to_string());
    tracing::info!("Starting streamable-http Compatibility Engine MCP server on {}", bind_address);

    let service = StreamableHttpService::new(
        || Ok(CompatibilityEngine::new()),
        LocalSessionManager::default().into(),
        StreamableHttpServerConfig {
            sse_retry: None,
            ..Default::default()
        },
    );

    let router = axum::Router::new()
        .nest_service("/mcp", service)
        .route("/metrics", axum::routing::get(metrics_handler))
        .route("/health", axum::routing::get(health_handler));
    
    let tcp_listener = tokio::net::TcpListener::bind(bind_address).await?;

    tracing::info!("Server started. Press Ctrl+C to stop.");

    axum::serve(tcp_listener, router)
        .with_graceful_shutdown(async {
            tokio::signal::ctrl_c().await.ok();
            tracing::info!("Shutdown signal received, stopping server...");

            // Force exit after timeout if graceful shutdown hangs
            tokio::spawn(async {
                tokio::time::sleep(SHUTDOWN_TIMEOUT).await;
                tracing::warn!("Force exit after {:?} timeout", SHUTDOWN_TIMEOUT);
                std::process::exit(0);
            });
        })
        .await?;

    tracing::info!("Server stopped");

    Ok(())
}

/// Handler for the /metrics endpoint
async fn metrics_handler() -> impl IntoResponse {
    let output = metrics::METRICS.gather();
    (StatusCode::OK, output)
}

/// Handler for the /health endpoint
async fn health_handler() -> impl IntoResponse {
    let output = "OK";
    (StatusCode::OK, output)
}
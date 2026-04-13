use rmcp::transport::{StreamableHttpServerConfig, streamable_http_server::{
    StreamableHttpService, session::local::LocalSessionManager,
}};
use tracing_subscriber::{
    layer::SubscriberExt,
    util::SubscriberInitExt,
    {self},
};
mod common;
use common::{compatibility_engine::CompatibilityEngine, telemetry::Telemetry};
use axum::{response::IntoResponse, http::StatusCode};
use opentelemetry::global;

use std::time::Duration;

const BIND_ADDRESS: &str = "127.0.0.1:8001";
const SHUTDOWN_TIMEOUT: Duration = Duration::from_secs(5);

/// Streamable HTTP config: rmcp defaults `allowed_hosts` to loopback only (DNS rebinding
/// protection). Behind OpenShift Routes / Ingress, `Host` is the public hostname — add it via
/// `MCP_ALLOWED_HOSTS` (comma-separated), or set `MCP_DISABLE_HOST_CHECK=true` only if you accept
/// that risk (e.g. strictly internal ClusterIP with no hostile DNS).
fn streamable_http_config() -> StreamableHttpServerConfig {
    let disable_check = std::env::var("MCP_DISABLE_HOST_CHECK")
        .map(|v| {
            let v = v.trim().to_ascii_lowercase();
            matches!(v.as_str(), "1" | "true" | "yes" | "on")
        })
        .unwrap_or(false);

    if disable_check {
        return StreamableHttpServerConfig::default()
            .disable_allowed_hosts()
            .with_sse_retry(None);
    }

    let mut cfg = StreamableHttpServerConfig::default().with_sse_retry(None);
    if let Ok(extra) = std::env::var("MCP_ALLOWED_HOSTS") {
        let extra_hosts: Vec<String> = extra
            .split(',')
            .map(|s| s.trim().to_string())
            .filter(|s| !s.is_empty())
            .collect();
        if !extra_hosts.is_empty() {
            let merged: Vec<String> = cfg
                .allowed_hosts
                .iter()
                .cloned()
                .chain(extra_hosts)
                .collect();
            cfg = cfg.with_allowed_hosts(merged);
        }
    }
    cfg
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let telemetry = Telemetry::install("compatibility-engine-mcp-server")?;

    tracing_subscriber::registry()
        .with(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| "debug".to_string().into()),
        )
        .with(
            tracing_opentelemetry::layer()
                .with_tracer(global::tracer("compatibility-engine")),
        )
        .with(tracing_subscriber::fmt::layer())
        .init();

    // Use environment variable or the static value
    let bind_address = std::env::var("BIND_ADDRESS").unwrap_or_else(|_| BIND_ADDRESS.to_string());
    tracing::info!("Starting streamable-http Compatibility Engine MCP server on {}", bind_address);

    let service = StreamableHttpService::new(
        || Ok(CompatibilityEngine::new()),
        LocalSessionManager::default().into(),
        streamable_http_config(),
    );

    let router = axum::Router::new()
        .nest_service("/mcp", service)
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
    telemetry.shutdown();

    Ok(())
}

/// Handler for the /health endpoint
async fn health_handler() -> impl IntoResponse {
    let output = "OK";
    (StatusCode::OK, output)
}

use anyhow::Result;

use rmcp::{ServiceExt, transport::stdio};
use tracing_subscriber::{self, EnvFilter};
use tracing_subscriber::{layer::SubscriberExt, util::SubscriberInitExt};

mod common;
use common::{compatibility_engine::CompatibilityEngine, telemetry::Telemetry};
use opentelemetry::global;

#[tokio::main]
async fn main() -> Result<()> {
    let telemetry = Telemetry::install("compatibility-engine-mcp-server-stdio")?;

    tracing_subscriber::registry()
        .with(EnvFilter::from_default_env().add_directive(tracing::Level::DEBUG.into()))
        .with(
            tracing_opentelemetry::layer()
                .with_tracer(global::tracer("compatibility-engine")),
        )
        .with(
            tracing_subscriber::fmt::layer()
                .with_writer(std::io::stderr)
                .with_ansi(false),
        )
        .init();

    tracing::info!("Starting Compatibility Engine MCP server using stdio transport");

    // Create an instance of our compatibility-engine router
    let service = CompatibilityEngine::new().serve(stdio()).await.inspect_err(|e| {
        tracing::error!("serving error: {:?}", e);
    })?;

    service.waiting().await?;
    telemetry.shutdown();
    Ok(())
}

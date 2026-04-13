//! OpenTelemetry bootstrap: OTLP traces and metrics to a collector.
//!
//! Configuration follows the standard OTLP environment variables. In the usual case you only
//! need:
//! - [`OTEL_EXPORTER_OTLP_ENDPOINT`](https://opentelemetry.io/docs/specs/otel/protocol/exporter/)
//! - [`OTEL_EXPORTER_OTLP_PROTOCOL`](https://opentelemetry.io/docs/specs/otel/protocol/exporter/)
//!   (`grpc`, `http/protobuf`, or `http/json`).
//!
//! Per-signal variables override the generic ones when set, for example
//! `OTEL_EXPORTER_OTLP_TRACES_PROTOCOL` / `OTEL_EXPORTER_OTLP_METRICS_PROTOCOL` and the matching
//! `*_ENDPOINT` variables.
//!
//! If **all** protocol variables are unset, this crate defaults to **gRPC** (backward compatible).
//! Some Kubernetes OTLP gRPC frontends are trace-only; if metrics export fails with
//! `unknown service …MetricsService`, use `OTEL_EXPORTER_OTLP_PROTOCOL=http/protobuf` and an HTTP
//! OTLP base URL (often port `4318`).
//!
//! **`OTEL_SERVICE_NAME`** (standard) sets `service.name` on the resource when set in the
//! environment. If unset, the fallback name passed to [`Telemetry::install`] is used (so local
//! runs still get a stable service name). This matches typical Kubernetes / OpenShift deployments
//! that set `OTEL_EXPORTER_OTLP_*` and `OTEL_SERVICE_NAME` like other language SDKs.
//!
//! Variables such as `OTEL_TRACES_EXPORTER` / `OTEL_METRICS_EXPORTER` are used by some SDKs
//! (e.g. Python auto-instrumentation) to choose exporters; this binary always exports via OTLP
//! when telemetry is installed and does not read those toggles.

use anyhow::Context;
use opentelemetry::{global, KeyValue};
use opentelemetry_otlp::{Protocol, WithExportConfig};
use opentelemetry_sdk::metrics::SdkMeterProvider;
use opentelemetry_sdk::trace::SdkTracerProvider;
use opentelemetry_sdk::Resource;

use super::metrics;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum OtlpTransport {
    Grpc,
    HttpProtobuf,
    HttpJson,
}

/// Resolves transport for traces (`metrics == false`) or metrics (`metrics == true`).
///
/// Precedence: `OTEL_EXPORTER_OTLP_{TRACES|METRICS}_PROTOCOL`, then
/// `OTEL_EXPORTER_OTLP_PROTOCOL`, then gRPC.
fn resolved_transport(metrics: bool) -> OtlpTransport {
    let specific = if metrics {
        std::env::var("OTEL_EXPORTER_OTLP_METRICS_PROTOCOL")
    } else {
        std::env::var("OTEL_EXPORTER_OTLP_TRACES_PROTOCOL")
    };
    let raw = specific
        .or_else(|_| std::env::var("OTEL_EXPORTER_OTLP_PROTOCOL"))
        .unwrap_or_default();
    let raw = raw.trim().to_ascii_lowercase();
    match raw.as_str() {
        "http/protobuf" => OtlpTransport::HttpProtobuf,
        "http/json" => OtlpTransport::HttpJson,
        "grpc" | "" => OtlpTransport::Grpc,
        other => {
            tracing::warn!(
                protocol = other,
                metrics,
                "unknown OTLP protocol env value; using grpc. Expected grpc, http/protobuf, or http/json"
            );
            OtlpTransport::Grpc
        }
    }
}

fn build_span_exporter() -> anyhow::Result<opentelemetry_otlp::SpanExporter> {
    match resolved_transport(false) {
        OtlpTransport::Grpc => opentelemetry_otlp::SpanExporter::builder()
            .with_tonic()
            .build()
            .context("failed to build OTLP span exporter (gRPC)"),
        OtlpTransport::HttpProtobuf => opentelemetry_otlp::SpanExporter::builder()
            .with_http()
            .with_protocol(Protocol::HttpBinary)
            .build()
            .context("failed to build OTLP span exporter (HTTP/protobuf)"),
        OtlpTransport::HttpJson => opentelemetry_otlp::SpanExporter::builder()
            .with_http()
            .with_protocol(Protocol::HttpJson)
            .build()
            .context("failed to build OTLP span exporter (HTTP/json)"),
    }
}

fn build_metric_exporter() -> anyhow::Result<opentelemetry_otlp::MetricExporter> {
    match resolved_transport(true) {
        OtlpTransport::Grpc => opentelemetry_otlp::MetricExporter::builder()
            .with_tonic()
            .build()
            .context("failed to build OTLP metric exporter (gRPC)"),
        OtlpTransport::HttpProtobuf => opentelemetry_otlp::MetricExporter::builder()
            .with_http()
            .with_protocol(Protocol::HttpBinary)
            .build()
            .context("failed to build OTLP metric exporter (HTTP/protobuf)"),
        OtlpTransport::HttpJson => opentelemetry_otlp::MetricExporter::builder()
            .with_http()
            .with_protocol(Protocol::HttpJson)
            .build()
            .context("failed to build OTLP metric exporter (HTTP/json)"),
    }
}

/// Owns SDK providers so we can shut them down cleanly (flush pending export).
pub struct Telemetry {
    tracer_provider: SdkTracerProvider,
    meter_provider: SdkMeterProvider,
}

impl Telemetry {
    /// Install global tracer and meter providers, OTLP exporters, and engine metrics instruments.
    pub fn install(service_name: impl Into<std::borrow::Cow<'static, str>>) -> anyhow::Result<Self> {
        let fallback_service_name = service_name.into();
        // `Resource::builder()` runs detectors: `OTEL_SERVICE_NAME` and `OTEL_RESOURCE_ATTRIBUTES`
        // already set `service.name` when present. Only merge a fallback when `OTEL_SERVICE_NAME`
        // is unset so deployments can use the same env pattern as Python/Java OTel apps.
        let resource_builder = Resource::builder().with_attribute(KeyValue::new(
            "service.version",
            env!("CARGO_PKG_VERSION"),
        ));
        let resource = if std::env::var("OTEL_SERVICE_NAME")
            .map(|s| !s.trim().is_empty())
            .unwrap_or(false)
        {
            resource_builder.build()
        } else {
            resource_builder
                .with_service_name(fallback_service_name)
                .build()
        };

        let span_exporter = build_span_exporter()?;

        let tracer_provider = SdkTracerProvider::builder()
            .with_batch_exporter(span_exporter)
            .with_resource(resource.clone())
            .build();

        let metric_exporter = build_metric_exporter()?;

        let meter_provider = SdkMeterProvider::builder()
            .with_periodic_exporter(metric_exporter)
            .with_resource(resource)
            .build();

        let _ = global::set_tracer_provider(tracer_provider.clone());
        let _ = global::set_meter_provider(meter_provider.clone());

        metrics::init();

        Ok(Self {
            tracer_provider,
            meter_provider,
        })
    }

    pub fn shutdown(self) {
        if let Err(e) = self.meter_provider.shutdown() {
            tracing::warn!(error = ?e, "OpenTelemetry meter provider shutdown");
        }
        if let Err(e) = self.tracer_provider.shutdown() {
            tracing::warn!(error = ?e, "OpenTelemetry tracer provider shutdown");
        }
    }
}

//! Engine metrics exported via the global OpenTelemetry [`MeterProvider`] (OTLP).
//!
//! Call [`init`] once after [`opentelemetry::global::set_meter_provider`]. If the process never
//! calls [`init`], recording functions are no-ops so unit tests can run without telemetry setup.

use std::sync::OnceLock;
use std::time::Instant;

use opentelemetry::global;
use opentelemetry::metrics::{Counter, Histogram, UpDownCounter};

struct EngineInstruments {
    requests_total: Counter<u64>,
    errors_total: Counter<u64>,
    request_duration_seconds: Histogram<f64>,
    active_requests: UpDownCounter<i64>,
}

static INSTRUMENTS: OnceLock<EngineInstruments> = OnceLock::new();

/// Registers instruments against the current global meter provider.
///
/// Must run exactly once, after the meter provider is installed.
pub fn init() {
    let meter = global::meter("compatibility_engine");
    let instruments = EngineInstruments {
        requests_total: meter
            .u64_counter("compatibility.engine.requests")
            .with_description("Total number of compatibility engine calculation requests")
            .build(),
        errors_total: meter
            .u64_counter("compatibility.engine.errors")
            .with_description("Total number of errors in compatibility engine calculations")
            .build(),
        request_duration_seconds: meter
            .f64_histogram("compatibility.engine.request.duration.seconds")
            .with_unit("s")
            .with_description(
                "Duration of compatibility engine calculation requests in seconds",
            )
            .build(),
        active_requests: meter
            .i64_up_down_counter("compatibility.engine.active_requests")
            .with_description("Number of active compatibility engine calculation requests")
            .build(),
    };
    if INSTRUMENTS.set(instruments).is_err() {
        tracing::warn!("compatibility engine metrics already initialized; ignoring duplicate init");
    }
}

fn instruments() -> Option<&'static EngineInstruments> {
    INSTRUMENTS.get()
}

/// Timer that records request duration and active request count when dropped.
pub struct RequestTimer {
    start: Option<Instant>,
}

impl RequestTimer {
    pub fn new() -> Self {
        if let Some(i) = instruments() {
            i.active_requests.add(1, &[]);
            Self {
                start: Some(Instant::now()),
            }
        } else {
            Self { start: None }
        }
    }
}

impl Drop for RequestTimer {
    fn drop(&mut self) {
        let Some(i) = instruments() else {
            return;
        };
        if let Some(start) = self.start.take() {
            i.request_duration_seconds
                .record(start.elapsed().as_secs_f64(), &[]);
            i.active_requests.add(-1, &[]);
        }
    }
}

pub fn increment_requests() {
    if let Some(i) = instruments() {
        i.requests_total.add(1, &[]);
    }
}

pub fn increment_errors() {
    if let Some(i) = instruments() {
        i.errors_total.add(1, &[]);
    }
}

//! Tracing + OpenTelemetry initialization. Console tracing is always installed;
//! OTLP export is layered on when `OTEL_EXPORTER_OTLP_ENDPOINT` is set.

use std::{
    net::IpAddr,
    sync::{LazyLock, OnceLock},
    time::Duration,
};

use axum::http::Uri;
use opentelemetry::{
    KeyValue, global,
    metrics::{Counter, Histogram},
    trace::TracerProvider as _,
};
use opentelemetry_otlp::WithExportConfig as _;
use opentelemetry_sdk::metrics::{PeriodicReader, SdkMeterProvider};
use tracing_subscriber::{EnvFilter, layer::SubscriberExt, util::SubscriberInitExt};

const EXPORT_TIMEOUT: Duration = Duration::from_secs(5);

static METER_PROVIDER: OnceLock<SdkMeterProvider> = OnceLock::new();
static TRACER_PROVIDER: OnceLock<opentelemetry_sdk::trace::SdkTracerProvider> = OnceLock::new();
static TOOL_METRICS: LazyLock<ToolMetrics> = LazyLock::new(ToolMetrics::new);

struct ToolMetrics {
    calls: Counter<u64>,
    errors: Counter<u64>,
    duration_ms: Histogram<f64>,
}

impl ToolMetrics {
    fn new() -> Self {
        let meter = global::meter("act-mcp-server");
        Self {
            calls: meter
                .u64_counter("mcp.tool.invocations")
                .with_description("Bounded MCP tool invocations")
                .build(),
            errors: meter
                .u64_counter("mcp.tool.errors")
                .with_description("Bounded MCP tool errors")
                .build(),
            duration_ms: meter
                .f64_histogram("mcp.tool.duration_ms")
                .with_description("Bounded MCP tool latency in milliseconds")
                .build(),
        }
    }
}

/// Record bounded, low-cardinality tool telemetry without arguments or results.
pub fn record_tool_call(tool_class: &'static str, outcome: &'static str, elapsed: Duration) {
    let attributes = [
        KeyValue::new("mcp.tool.class", tool_class),
        KeyValue::new("mcp.tool.outcome", outcome),
    ];
    TOOL_METRICS.calls.add(1, &attributes);
    if outcome == "error" {
        TOOL_METRICS.errors.add(1, &attributes);
    }
    TOOL_METRICS
        .duration_ms
        .record(elapsed.as_secs_f64() * 1_000.0, &attributes);
}

pub fn init(service_name: &str, filter: EnvFilter) -> anyhow::Result<()> {
    let registry = tracing_subscriber::registry().with(filter).with(
        tracing_subscriber::fmt::layer()
            .json()
            .flatten_event(true)
            .with_ansi(false)
            .with_writer(std::io::stderr),
    );

    match std::env::var("OTEL_EXPORTER_OTLP_ENDPOINT") {
        Ok(endpoint) if !endpoint.is_empty() => {
            let endpoint = sanitize_otlp_endpoint(&endpoint)?;
            let span_exporter = opentelemetry_otlp::SpanExporter::builder()
                .with_tonic()
                .with_endpoint(endpoint.clone())
                .with_timeout(EXPORT_TIMEOUT)
                .build()?;
            let metric_exporter = opentelemetry_otlp::MetricExporter::builder()
                .with_tonic()
                .with_endpoint(endpoint)
                .with_timeout(EXPORT_TIMEOUT)
                .build()?;
            let resource = opentelemetry_sdk::Resource::builder()
                .with_service_name(service_name.to_string())
                .build();
            let tracer_provider = opentelemetry_sdk::trace::SdkTracerProvider::builder()
                .with_resource(resource.clone())
                .with_batch_exporter(span_exporter)
                .build();
            let reader = PeriodicReader::builder(metric_exporter).build();
            let meter_provider = SdkMeterProvider::builder()
                .with_resource(resource)
                .with_reader(reader)
                .build();
            global::set_meter_provider(meter_provider.clone());
            let tracer = tracer_provider.tracer("act-mcp-server");
            METER_PROVIDER.set(meter_provider).map_err(|_| {
                anyhow::anyhow!("OpenTelemetry metrics provider was already initialized")
            })?;
            TRACER_PROVIDER.set(tracer_provider).map_err(|_| {
                anyhow::anyhow!("OpenTelemetry trace provider was already initialized")
            })?;
            registry
                .with(tracing_opentelemetry::layer().with_tracer(tracer))
                .init();
            tracing::info!("OTLP trace and metric export enabled");
        }
        _ => {
            registry.init();
            tracing::info!("OTEL_EXPORTER_OTLP_ENDPOINT not set; console tracing only");
        }
    }

    Ok(())
}

pub fn shutdown() {
    if let Some(provider) = METER_PROVIDER.get()
        && let Err(error) = provider.shutdown()
    {
        tracing::warn!(%error, "OpenTelemetry metric shutdown failed");
    }
    if let Some(provider) = TRACER_PROVIDER.get()
        && let Err(error) = provider.shutdown()
    {
        tracing::warn!(%error, "OpenTelemetry trace shutdown failed");
    }
}

fn sanitize_otlp_endpoint(raw: &str) -> anyhow::Result<String> {
    let uri = raw
        .parse::<Uri>()
        .map_err(|_| anyhow::anyhow!("OTEL_EXPORTER_OTLP_ENDPOINT is malformed"))?;
    let scheme = uri
        .scheme_str()
        .ok_or_else(|| anyhow::anyhow!("OTEL_EXPORTER_OTLP_ENDPOINT requires a scheme"))?;
    if !matches!(scheme, "http" | "https") {
        anyhow::bail!("OTEL_EXPORTER_OTLP_ENDPOINT must use http or https");
    }
    let authority = uri
        .authority()
        .ok_or_else(|| anyhow::anyhow!("OTEL_EXPORTER_OTLP_ENDPOINT requires an authority"))?;
    if authority.as_str().contains('@') {
        anyhow::bail!("OTEL_EXPORTER_OTLP_ENDPOINT must not contain credentials");
    }
    if uri
        .path_and_query()
        .and_then(|value| value.query())
        .is_some()
    {
        anyhow::bail!("OTEL_EXPORTER_OTLP_ENDPOINT must not contain a query");
    }
    let host = uri
        .host()
        .ok_or_else(|| anyhow::anyhow!("OTEL_EXPORTER_OTLP_ENDPOINT requires a host"))?;
    if !safe_otlp_host(host) {
        anyhow::bail!("OTEL_EXPORTER_OTLP_ENDPOINT host is unsafe");
    }
    Ok(raw.to_owned())
}

fn safe_otlp_host(host: &str) -> bool {
    let host = host.trim_matches(['[', ']']).trim_end_matches('.');
    if [
        "metadata",
        "metadata.google.internal",
        "metadata.azure.internal",
        "instance-data.ec2.internal",
    ]
    .iter()
    .any(|blocked| host.eq_ignore_ascii_case(blocked))
    {
        return false;
    }
    match host.parse::<IpAddr>() {
        Ok(IpAddr::V4(address)) => !address.is_unspecified() && !address.is_link_local(),
        Ok(IpAddr::V6(address)) => !address.is_unspecified() && !address.is_unicast_link_local(),
        Err(_) => true,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn accepts_local_and_remote_otlp_collectors() {
        assert!(sanitize_otlp_endpoint("http://127.0.0.1:4317").is_ok());
        assert!(sanitize_otlp_endpoint("https://otel.example.test:4317/v1").is_ok());
    }

    #[test]
    fn rejects_unsafe_otlp_endpoints() {
        for endpoint in [
            "http://169.254.169.254:4317",
            "http://[fe80::1]:4317",
            "http://0.0.0.0:4317",
            "http://[::]:4317",
            "http://metadata.google.internal:4317",
            "https://user:secret@otel.example.test:4317",
            "https://otel.example.test:4317?token=secret",
            "file:///tmp/collector",
        ] {
            assert!(
                sanitize_otlp_endpoint(endpoint).is_err(),
                "accepted unsafe endpoint {endpoint}"
            );
        }
    }
}

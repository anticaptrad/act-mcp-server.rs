//! Tracing + OpenTelemetry initialization. Console tracing is always installed;
//! OTLP export is layered on when `OTEL_EXPORTER_OTLP_ENDPOINT` is set.

use opentelemetry::KeyValue;
use opentelemetry_otlp::WithExportConfig;
use opentelemetry_sdk::Resource;
use tracing_subscriber::{EnvFilter, layer::SubscriberExt, util::SubscriberInitExt};

pub fn init(service_name: &str, filter: EnvFilter) -> anyhow::Result<()> {
    let registry = tracing_subscriber::registry()
        .with(filter)
        .with(tracing_subscriber::fmt::layer().with_writer(std::io::stderr));

    match std::env::var("OTEL_EXPORTER_OTLP_ENDPOINT") {
        Ok(endpoint) if !endpoint.is_empty() => {
            let tracer = opentelemetry_otlp::new_pipeline()
                .tracing()
                .with_exporter(
                    opentelemetry_otlp::new_exporter()
                        .tonic()
                        .with_endpoint(endpoint),
                )
                .with_trace_config(
                    opentelemetry_sdk::trace::config().with_resource(Resource::new(vec![
                        KeyValue::new("service.name", service_name.to_string()),
                    ])),
                )
                .install_batch(opentelemetry_sdk::runtime::Tokio)?;
            registry
                .with(tracing_opentelemetry::layer().with_tracer(tracer))
                .init();
            tracing::info!("OTLP trace export enabled");
        }
        _ => {
            registry.init();
            tracing::info!("OTEL_EXPORTER_OTLP_ENDPOINT not set; console tracing only");
        }
    }

    Ok(())
}

pub fn shutdown() {
    opentelemetry::global::shutdown_tracer_provider();
}

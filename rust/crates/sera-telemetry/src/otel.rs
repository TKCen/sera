//! OpenTelemetry triad initialisation helpers.
//!
//! The `otlp-exporter` feature enables gRPC export via `opentelemetry-otlp`.
//! Without the feature the public API is present but is a no-op stub so
//! dependents can always call `init_otel` unconditionally.

use thiserror::Error;

/// Errors produced during OTel initialisation.
#[derive(Debug, Error)]
pub enum OtelInitError {
    #[error("invalid endpoint URL: {reason}")]
    InvalidEndpoint { reason: String },
    #[error("tracer provider setup failed: {reason}")]
    ProviderSetup { reason: String },
}

/// Initialise the OpenTelemetry triad (traces, metrics, logs).
///
/// When compiled without the `otlp-exporter` feature this is a no-op that
/// always returns `Ok(())`.  Enable the feature to get real OTLP/gRPC export.
#[cfg(not(feature = "otlp-exporter"))]
pub fn init_otel(
    _service_name: &str,
    _endpoint: &str,
) -> Result<(), OtelInitError> {
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn init_otel_stub_is_ok_for_any_input() {
        // Without otlp-exporter feature the function is a pure no-op.
        assert!(init_otel("sera-test", "http://localhost:4317").is_ok());
    }

    #[test]
    fn init_otel_stub_ok_empty_strings() {
        // Even empty inputs are accepted by the no-op stub.
        assert!(init_otel("", "").is_ok());
    }

    #[test]
    fn otel_init_error_display_invalid_endpoint() {
        let e = OtelInitError::InvalidEndpoint {
            reason: "empty".to_string(),
        };
        assert!(e.to_string().contains("invalid endpoint URL"));
    }

    #[test]
    fn otel_init_error_display_provider_setup() {
        let e = OtelInitError::ProviderSetup {
            reason: "tls failure".to_string(),
        };
        assert!(e.to_string().contains("tracer provider setup failed"));
    }
}

/// Initialise the OpenTelemetry triad with OTLP/gRPC export.
#[cfg(feature = "otlp-exporter")]
pub fn init_otel(
    service_name: &str,
    endpoint: &str,
) -> Result<(), OtelInitError> {
    use opentelemetry::trace::TracerProvider as _;
    use opentelemetry_otlp::WithExportConfig;
    use tracing_subscriber::prelude::*;

    if endpoint.is_empty() {
        return Err(OtelInitError::InvalidEndpoint {
            reason: "endpoint must not be empty".to_string(),
        });
    }

    let exporter = opentelemetry_otlp::new_exporter()
        .tonic()
        .with_endpoint(endpoint);

    let tracer_provider = opentelemetry_otlp::new_pipeline()
        .tracing()
        .with_exporter(exporter)
        .with_trace_config(
            opentelemetry_sdk::trace::Config::default().with_resource(
                opentelemetry_sdk::Resource::new(vec![
                    opentelemetry::KeyValue::new("service.name", service_name.to_string()),
                ]),
            ),
        )
        .install_batch(opentelemetry_sdk::runtime::Tokio)
        .map_err(|e| OtelInitError::ProviderSetup {
            reason: e.to_string(),
        })?;

    let telemetry_layer =
        tracing_opentelemetry::layer().with_tracer(tracer_provider.tracer(service_name.to_string()));

    tracing_subscriber::registry()
        .with(telemetry_layer)
        .try_init()
        .map_err(|e| OtelInitError::ProviderSetup {
            reason: e.to_string(),
        })?;

    Ok(())
}

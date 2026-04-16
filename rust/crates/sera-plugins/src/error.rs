//! Plugin-specific error types.

use sera_errors::{SeraError, SeraErrorCode};
use thiserror::Error;

/// Errors produced by the plugin subsystem.
#[derive(Debug, Error)]
pub enum PluginError {
    #[error("plugin registration failed: {reason}")]
    RegistrationFailed { reason: String },

    #[error("health check failed for plugin '{name}': {reason}")]
    HealthCheckFailed { name: String, reason: String },

    #[error("plugin not found: {name}")]
    PluginNotFound { name: String },

    #[error("plugin '{name}' is unhealthy")]
    PluginUnhealthy { name: String },

    #[error("manifest invalid: {reason}")]
    ManifestInvalid { reason: String },

    #[error("circuit breaker open for plugin '{name}'")]
    CircuitOpen { name: String },

    #[error("unauthorized: {reason}")]
    Unauthorized { reason: String },

    #[error("connection failed to '{endpoint}': {reason}")]
    ConnectionFailed { endpoint: String, reason: String },
}

impl From<PluginError> for SeraError {
    fn from(err: PluginError) -> Self {
        let code = match &err {
            PluginError::RegistrationFailed { .. } => SeraErrorCode::InvalidInput,
            PluginError::HealthCheckFailed { .. } => SeraErrorCode::Unavailable,
            PluginError::PluginNotFound { .. } => SeraErrorCode::NotFound,
            PluginError::PluginUnhealthy { .. } => SeraErrorCode::Unavailable,
            PluginError::ManifestInvalid { .. } => SeraErrorCode::InvalidInput,
            PluginError::CircuitOpen { .. } => SeraErrorCode::Unavailable,
            PluginError::Unauthorized { .. } => SeraErrorCode::Unauthorized,
            PluginError::ConnectionFailed { .. } => SeraErrorCode::Unavailable,
        };
        SeraError::new(code, err.to_string())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn plugin_not_found_to_sera_error() {
        let err = PluginError::PluginNotFound {
            name: "my-plugin".into(),
        };
        let sera: SeraError = err.into();
        assert_eq!(sera.code, SeraErrorCode::NotFound);
        assert!(sera.message.contains("my-plugin"));
    }

    #[test]
    fn registration_failed_to_sera_error() {
        let err = PluginError::RegistrationFailed {
            reason: "duplicate name".into(),
        };
        let sera: SeraError = err.into();
        assert_eq!(sera.code, SeraErrorCode::InvalidInput);
    }

    #[test]
    fn circuit_open_to_sera_error() {
        let err = PluginError::CircuitOpen {
            name: "failing-plugin".into(),
        };
        let sera: SeraError = err.into();
        assert_eq!(sera.code, SeraErrorCode::Unavailable);
    }

    #[test]
    fn unauthorized_to_sera_error() {
        let err = PluginError::Unauthorized {
            reason: "missing mTLS cert".into(),
        };
        let sera: SeraError = err.into();
        assert_eq!(sera.code, SeraErrorCode::Unauthorized);
    }

    #[test]
    fn connection_failed_to_sera_error() {
        let err = PluginError::ConnectionFailed {
            endpoint: "localhost:9090".into(),
            reason: "refused".into(),
        };
        let sera: SeraError = err.into();
        assert_eq!(sera.code, SeraErrorCode::Unavailable);
    }

    #[test]
    fn error_display_includes_context() {
        let err = PluginError::HealthCheckFailed {
            name: "p".into(),
            reason: "timeout".into(),
        };
        let msg = err.to_string();
        assert!(msg.contains("p"));
        assert!(msg.contains("timeout"));
    }
}

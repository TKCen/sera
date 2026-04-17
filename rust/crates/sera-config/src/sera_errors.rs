//! `From` impls bridging sera-config error types into [`SeraError`].

use sera_errors::{SeraError, SeraErrorCode};

use crate::config_store::ConfigStoreError;
use crate::manifest_loader::ManifestLoadError;
use crate::watchers::FileWatcherError;

impl From<ConfigStoreError> for SeraError {
    fn from(err: ConfigStoreError) -> Self {
        let code = match &err {
            ConfigStoreError::NotFound(_) => SeraErrorCode::NotFound,
            ConfigStoreError::Serialise(_) => SeraErrorCode::Serialization,
            ConfigStoreError::Backend(_) => SeraErrorCode::Internal,
        };
        SeraError::with_source(code, err.to_string(), err)
    }
}

impl From<ManifestLoadError> for SeraError {
    fn from(err: ManifestLoadError) -> Self {
        let code = match &err {
            ManifestLoadError::IoError { .. } => SeraErrorCode::Internal,
            ManifestLoadError::ParseError { .. } => SeraErrorCode::Serialization,
            ManifestLoadError::ValidationError { .. } => SeraErrorCode::InvalidInput,
            ManifestLoadError::UnsupportedKind { .. } => SeraErrorCode::InvalidInput,
        };
        SeraError::with_source(code, err.to_string(), err)
    }
}

impl From<FileWatcherError> for SeraError {
    fn from(err: FileWatcherError) -> Self {
        SeraError::with_source(SeraErrorCode::Internal, err.to_string(), err)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn config_store_not_found_maps_to_not_found() {
        let e: SeraError = ConfigStoreError::NotFound("mykey".into()).into();
        assert_eq!(e.code, SeraErrorCode::NotFound);
        assert!(e.message.contains("mykey"));
    }

    #[test]
    fn config_store_serialise_maps_to_serialization() {
        let e: SeraError = ConfigStoreError::Serialise("bad json".into()).into();
        assert_eq!(e.code, SeraErrorCode::Serialization);
    }

    #[test]
    fn config_store_backend_maps_to_internal() {
        let e: SeraError = ConfigStoreError::Backend("conn refused".into()).into();
        assert_eq!(e.code, SeraErrorCode::Internal);
    }

    #[test]
    fn manifest_load_io_error_maps_to_internal() {
        let e: SeraError = ManifestLoadError::IoError {
            path: "/etc/sera.yaml".into(),
            source: std::io::Error::new(std::io::ErrorKind::NotFound, "no such file"),
        }
        .into();
        assert_eq!(e.code, SeraErrorCode::Internal);
    }

    #[test]
    fn manifest_load_parse_error_maps_to_serialization() {
        let raw = "key: : bad";
        let parse_err = serde_yaml::from_str::<serde_yaml::Value>(raw).unwrap_err();
        let e: SeraError = ManifestLoadError::ParseError {
            document_index: 0,
            source: parse_err,
        }
        .into();
        assert_eq!(e.code, SeraErrorCode::Serialization);
    }

    #[test]
    fn manifest_load_unsupported_kind_maps_to_invalid_input() {
        let e: SeraError = ManifestLoadError::UnsupportedKind {
            kind: "Widget".into(),
            document_index: 1,
        }
        .into();
        assert_eq!(e.code, SeraErrorCode::InvalidInput);
        assert!(e.message.contains("Widget"));
    }
}

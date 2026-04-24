//! `From` impls bridging sera-tools error types into [`SeraError`].
//!
//! Covers:
//! - [`SsrfError`] — SSRF validation errors
//! - [`BashAstError`] — bash command safety errors
//! - [`SandboxError`] — sandbox provider errors

use sera_errors::{SeraError, SeraErrorCode};

use crate::bash_ast::BashAstError;
use crate::sandbox::SandboxError;
use crate::ssrf::SsrfError;

impl From<SsrfError> for SeraError {
    fn from(err: SsrfError) -> Self {
        let code = match &err {
            SsrfError::Loopback => SeraErrorCode::Forbidden,
            SsrfError::LinkLocal => SeraErrorCode::Forbidden,
            SsrfError::CloudMetadata => SeraErrorCode::Forbidden,
            SsrfError::NotAllowed { .. } => SeraErrorCode::Forbidden,
            SsrfError::PrivateRange => SeraErrorCode::Forbidden,
            SsrfError::ParseError { .. } => SeraErrorCode::InvalidInput,
        };
        SeraError::with_source(code, err.to_string(), err)
    }
}

impl From<BashAstError> for SeraError {
    fn from(err: BashAstError) -> Self {
        SeraError::with_source(SeraErrorCode::Forbidden, err.to_string(), err)
    }
}

impl From<SandboxError> for SeraError {
    fn from(err: SandboxError) -> Self {
        let code = match &err {
            SandboxError::NotFound => SeraErrorCode::NotFound,
            SandboxError::NotImplemented => SeraErrorCode::NotImplemented,
            SandboxError::PolicyViolation { .. } => SeraErrorCode::Forbidden,
            SandboxError::InvalidSourceMount { .. } => SeraErrorCode::InvalidInput,
            SandboxError::CreateFailed { .. } => SeraErrorCode::Internal,
            SandboxError::ExecFailed { .. } => SeraErrorCode::Internal,
            SandboxError::DestroyFailed { .. } => SeraErrorCode::Internal,
        };
        SeraError::with_source(code, err.to_string(), err)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // --- SsrfError ---

    #[test]
    fn ssrf_loopback_maps_to_forbidden() {
        let e: SeraError = SsrfError::Loopback.into();
        assert_eq!(e.code, SeraErrorCode::Forbidden);
        assert!(e.message.contains("loopback"));
    }

    #[test]
    fn ssrf_link_local_maps_to_forbidden() {
        let e: SeraError = SsrfError::LinkLocal.into();
        assert_eq!(e.code, SeraErrorCode::Forbidden);
    }

    #[test]
    fn ssrf_cloud_metadata_maps_to_forbidden() {
        let e: SeraError = SsrfError::CloudMetadata.into();
        assert_eq!(e.code, SeraErrorCode::Forbidden);
    }

    #[test]
    fn ssrf_not_allowed_maps_to_forbidden() {
        let e: SeraError = SsrfError::NotAllowed { reason: "hostname".into() }.into();
        assert_eq!(e.code, SeraErrorCode::Forbidden);
        assert!(e.message.contains("hostname"));
    }

    #[test]
    fn ssrf_private_range_maps_to_forbidden() {
        let e: SeraError = SsrfError::PrivateRange.into();
        assert_eq!(e.code, SeraErrorCode::Forbidden);
    }

    #[test]
    fn ssrf_parse_error_maps_to_invalid_input() {
        let e: SeraError = SsrfError::ParseError { reason: "bad octet".into() }.into();
        assert_eq!(e.code, SeraErrorCode::InvalidInput);
    }

    // --- BashAstError ---

    #[test]
    fn bash_ast_backtick_maps_to_forbidden() {
        let e: SeraError = BashAstError::BacktickSubstitution.into();
        assert_eq!(e.code, SeraErrorCode::Forbidden);
        assert!(e.message.contains("backtick"));
    }

    #[test]
    fn bash_ast_process_substitution_maps_to_forbidden() {
        let e: SeraError = BashAstError::ProcessSubstitution.into();
        assert_eq!(e.code, SeraErrorCode::Forbidden);
    }

    #[test]
    fn bash_ast_metachar_injection_maps_to_forbidden() {
        let e: SeraError = BashAstError::MetacharInjection.into();
        assert_eq!(e.code, SeraErrorCode::Forbidden);
    }

    #[test]
    fn bash_ast_fd_process_substitution_maps_to_forbidden() {
        let e: SeraError = BashAstError::FdProcessSubstitution.into();
        assert_eq!(e.code, SeraErrorCode::Forbidden);
    }

    // --- SandboxError ---

    #[test]
    fn sandbox_not_found_maps_to_not_found() {
        let e: SeraError = SandboxError::NotFound.into();
        assert_eq!(e.code, SeraErrorCode::NotFound);
    }

    #[test]
    fn sandbox_not_implemented_maps_to_not_implemented() {
        let e: SeraError = SandboxError::NotImplemented.into();
        assert_eq!(e.code, SeraErrorCode::NotImplemented);
    }

    #[test]
    fn sandbox_policy_violation_maps_to_forbidden() {
        let e: SeraError = SandboxError::PolicyViolation { reason: "net denied".into() }.into();
        assert_eq!(e.code, SeraErrorCode::Forbidden);
        assert!(e.message.contains("net denied"));
    }

    #[test]
    fn sandbox_invalid_source_mount_maps_to_invalid_input() {
        let e: SeraError =
            SandboxError::InvalidSourceMount { reason: "bad prefix".into() }.into();
        assert_eq!(e.code, SeraErrorCode::InvalidInput);
    }

    #[test]
    fn sandbox_create_failed_maps_to_internal() {
        let e: SeraError = SandboxError::CreateFailed { reason: "OOM".into() }.into();
        assert_eq!(e.code, SeraErrorCode::Internal);
    }

    #[test]
    fn sandbox_exec_failed_maps_to_internal() {
        let e: SeraError = SandboxError::ExecFailed { reason: "exit 1".into() }.into();
        assert_eq!(e.code, SeraErrorCode::Internal);
    }
}

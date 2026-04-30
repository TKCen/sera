//! Loader for `AgentTemplate` YAML files (`templates/*.yaml`).
//!
//! Wraps `serde_yaml::from_str::<AgentTemplate>` and runs the egress
//! invariant check from
//! `sera_types::manifest::AgentTemplate::validate_and_normalize_egress`:
//! sandboxed templates get an implicit `InferenceLocal` baseline, and
//! sandboxed templates that explicitly drop `InferenceLocal` from
//! `egress.allowed` (without `mode = Disabled`) are rejected.
//!
//! Exists for `sera-eq0m` / `sera-xgb0`: the canonical entry point for
//! callers who want their YAML parse to obey the egress contract without
//! having to remember the validation step.

use std::path::Path;

use sera_types::manifest::{AgentTemplate, EgressManifestError};

/// Errors raised while loading an `AgentTemplate` YAML file.
#[derive(Debug, thiserror::Error)]
pub enum AgentTemplateLoadError {
    #[error("failed to read agent template file '{path}': {source}")]
    IoError {
        path: String,
        #[source]
        source: std::io::Error,
    },
    #[error("YAML parse error in '{path}': {source}")]
    ParseError {
        path: String,
        #[source]
        source: serde_yaml::Error,
    },
    #[error("egress validation failed: {source}")]
    EgressError {
        #[from]
        source: EgressManifestError,
    },
}

/// Parse a YAML string as a single `AgentTemplate` and apply the
/// sandboxed-egress invariant. Sandboxed templates without an `egress`
/// block come back with `allowed = [InferenceLocal]` and `mode = Strict`.
pub fn parse_agent_template(yaml: &str) -> Result<AgentTemplate, AgentTemplateLoadError> {
    let mut template: AgentTemplate =
        serde_yaml::from_str(yaml).map_err(|source| AgentTemplateLoadError::ParseError {
            path: "<inline>".to_string(),
            source,
        })?;
    template.validate_and_normalize_egress()?;
    Ok(template)
}

/// Read an `AgentTemplate` YAML file from disk and apply the
/// sandboxed-egress invariant.
pub fn load_agent_template_file(path: &Path) -> Result<AgentTemplate, AgentTemplateLoadError> {
    let content =
        std::fs::read_to_string(path).map_err(|source| AgentTemplateLoadError::IoError {
            path: path.display().to_string(),
            source,
        })?;
    let mut template: AgentTemplate = serde_yaml::from_str(&content).map_err(|source| {
        AgentTemplateLoadError::ParseError {
            path: path.display().to_string(),
            source,
        }
    })?;
    template.validate_and_normalize_egress()?;
    Ok(template)
}

#[cfg(test)]
mod tests {
    use super::*;
    use sera_types::sandbox::{EgressEndpoint, EgressMode};

    const NON_SANDBOXED: &str = r#"
apiVersion: sera/v1
kind: AgentTemplate
metadata:
  name: plain
spec:
  model:
    provider: openai
    model: gpt-4o
"#;

    const SANDBOXED_NO_EGRESS: &str = r#"
apiVersion: sera/v1
kind: AgentTemplate
metadata:
  name: sandboxed-default
spec:
  sandboxBoundary: tier-2
  sandbox:
    image: sera-byoh:latest
"#;

    const SANDBOXED_DROPS_INFERENCE_LOCAL: &str = r#"
apiVersion: sera/v1
kind: AgentTemplate
metadata:
  name: sandboxed-bad
spec:
  sandboxBoundary: tier-2
  egress:
    mode: strict
    allowed:
      - type: domain
        name: api.github.com
"#;

    const SANDBOXED_DISABLED: &str = r#"
apiVersion: sera/v1
kind: AgentTemplate
metadata:
  name: sandboxed-disabled
spec:
  sandboxBoundary: tier-3
  egress:
    mode: disabled
    allowed: []
"#;

    const SANDBOXED_AUDIT_ONLY_OK: &str = r#"
apiVersion: sera/v1
kind: AgentTemplate
metadata:
  name: sandboxed-audit
spec:
  sandboxBoundary: tier-2
  egress:
    mode: audit_only
    allowed:
      - type: inference_local
      - type: domain
        name: api.openai.com
"#;

    #[test]
    fn parse_non_sandboxed_template_leaves_egress_none() {
        let template = parse_agent_template(NON_SANDBOXED).unwrap();
        assert!(template.spec.egress.is_none());
    }

    #[test]
    fn parse_sandboxed_template_inserts_inference_local_baseline() {
        let template = parse_agent_template(SANDBOXED_NO_EGRESS).unwrap();
        let egress = template.spec.egress.expect("baseline must be inserted");
        assert_eq!(egress.mode, EgressMode::Strict);
        assert_eq!(egress.allowed.len(), 1);
        assert!(matches!(egress.allowed[0], EgressEndpoint::InferenceLocal));
    }

    #[test]
    fn parse_sandboxed_template_rejects_dropped_inference_local() {
        let err = parse_agent_template(SANDBOXED_DROPS_INFERENCE_LOCAL).unwrap_err();
        assert!(
            matches!(err, AgentTemplateLoadError::EgressError { .. }),
            "expected EgressError, got: {err:?}",
        );
        let msg = err.to_string();
        assert!(
            msg.contains("InferenceLocal"),
            "error must mention the missing endpoint; got: {msg}",
        );
    }

    #[test]
    fn parse_sandboxed_template_with_disabled_mode_skips_baseline_check() {
        let template = parse_agent_template(SANDBOXED_DISABLED).unwrap();
        let egress = template.spec.egress.unwrap();
        assert_eq!(egress.mode, EgressMode::Disabled);
        assert!(egress.allowed.is_empty());
    }

    #[test]
    fn parse_sandboxed_template_audit_only_with_inference_local_passes() {
        let template = parse_agent_template(SANDBOXED_AUDIT_ONLY_OK).unwrap();
        let egress = template.spec.egress.unwrap();
        assert_eq!(egress.mode, EgressMode::AuditOnly);
        assert!(egress.allows_inference_local());
        assert_eq!(egress.allowed.len(), 2);
    }

    #[test]
    fn load_agent_template_file_round_trips_disk() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("template.yaml");
        std::fs::write(&path, SANDBOXED_NO_EGRESS).unwrap();
        let template = load_agent_template_file(&path).unwrap();
        assert!(template.spec.egress.unwrap().allows_inference_local());
    }

    #[test]
    fn load_agent_template_file_missing_returns_io_error() {
        let err = load_agent_template_file(Path::new("/nonexistent/template.yaml")).unwrap_err();
        assert!(matches!(err, AgentTemplateLoadError::IoError { .. }));
    }

    #[test]
    fn parse_agent_template_invalid_yaml_returns_parse_error() {
        let err = parse_agent_template("this is: [invalid").unwrap_err();
        assert!(matches!(err, AgentTemplateLoadError::ParseError { .. }));
    }
}

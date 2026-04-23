//! HookChain YAML manifest parsing and validation.
//!
//! Implements SPEC-hooks §4 (Hook Configuration) + SPEC-config HookChain kind.
//!
//! Manifest shape (canonical):
//!
//! ```yaml
//! apiVersion: sera.dev/v1
//! kind: HookChain
//! metadata:
//!   name: pre-route-default
//! spec:
//!   hook_point: pre_route
//!   timeout_ms: 5000     # optional; defaults to 5000
//!   fail_open: false     # optional; defaults to false
//!   hooks:
//!     - hook: content-filter
//!       config:
//!         blocked_patterns: ["spam"]
//!     - hook: rate-limiter
//!       enabled: true        # optional; defaults to true
//!       config:
//!         requests_per_minute: 60
//! ```
//!
//! Mirrors the `sera-plugins::manifest::PluginManifest` pattern so operators
//! see the same `apiVersion` / `kind` / `metadata` / `spec` envelope across
//! resource kinds. Parses into [`sera_types::hook::HookChain`] via
//! [`HookChainManifest::into_chain`].

use std::collections::HashSet;

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use sera_types::hook::{HookChain, HookInstance, HookPoint};

use crate::error::ManifestError;

/// Top-level structure of a HookChain manifest file.
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct HookChainManifest {
    /// Must be `"sera.dev/v1"`.
    #[serde(rename = "apiVersion")]
    pub api_version: String,
    /// Must be `"HookChain"`.
    pub kind: String,
    pub metadata: HookChainManifestMetadata,
    pub spec: HookChainManifestSpec,
}

/// Metadata block for a HookChain manifest.
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct HookChainManifestMetadata {
    /// Unique chain name. Matches `^[a-z][a-z0-9-]*$`.
    pub name: String,
    #[serde(default, skip_serializing_if = "std::collections::HashMap::is_empty")]
    pub labels: std::collections::HashMap<String, String>,
    #[serde(default, skip_serializing_if = "std::collections::HashMap::is_empty")]
    pub annotations: std::collections::HashMap<String, String>,
}

/// Spec block for a HookChain manifest.
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct HookChainManifestSpec {
    /// The hook point this chain fires at. Serde snake_case (e.g. `pre_route`).
    ///
    /// `HookPoint` lives in `sera-types` which does not depend on `schemars`;
    /// for schema generation we describe the field as a string (the on-wire
    /// form) and let serde validate it against the enum at parse time.
    #[schemars(with = "String", description = "HookPoint in snake_case")]
    pub hook_point: HookPoint,
    /// Ordered hook instances.
    #[serde(default)]
    pub hooks: Vec<HookInstanceManifest>,
    /// Chain-wide wall-clock budget in milliseconds. Defaults to 5000.
    #[serde(default = "default_chain_timeout_ms")]
    pub timeout_ms: u64,
    /// If true, hook errors are logged and skipped. Defaults to false (fail-closed).
    #[serde(default)]
    pub fail_open: bool,
}

fn default_chain_timeout_ms() -> u64 {
    5000
}

/// A hook entry within a chain manifest.
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct HookInstanceManifest {
    /// Registered hook name (key in `HookRegistry`).
    pub hook: String,
    /// Per-instance configuration passed to `Hook::init`.
    #[serde(default)]
    pub config: serde_json::Value,
    /// Toggle without removing from the chain.
    #[serde(default = "default_true")]
    pub enabled: bool,
}

fn default_true() -> bool {
    true
}

impl HookChainManifest {
    /// Parse a YAML string into a manifest and validate it.
    pub fn from_yaml(yaml: &str) -> Result<Self, ManifestError> {
        let manifest: Self =
            serde_yaml::from_str(yaml).map_err(|e| ManifestError::Parse(e.to_string()))?;
        manifest.validate()?;
        Ok(manifest)
    }

    /// Validate structural invariants (static, does not require a registry).
    ///
    /// Errors on:
    /// - Wrong `apiVersion` / `kind`.
    /// - Empty chain name, or name not matching `^[a-z][a-z0-9-]*$`.
    /// - Duplicate `hook` references within the chain.
    /// - `timeout_ms == 0`.
    pub fn validate(&self) -> Result<(), ManifestError> {
        if self.api_version != "sera.dev/v1" {
            return Err(ManifestError::Invalid(format!(
                "apiVersion must be 'sera.dev/v1', got '{}'",
                self.api_version
            )));
        }
        if self.kind != "HookChain" {
            return Err(ManifestError::Invalid(format!(
                "kind must be 'HookChain', got '{}'",
                self.kind
            )));
        }
        if !is_valid_chain_name(&self.metadata.name) {
            return Err(ManifestError::Invalid(format!(
                "metadata.name '{}' is invalid — must match ^[a-z][a-z0-9-]*$",
                self.metadata.name
            )));
        }
        if self.spec.timeout_ms == 0 {
            return Err(ManifestError::Invalid("spec.timeout_ms must be > 0".into()));
        }

        let mut seen: HashSet<&str> = HashSet::new();
        for inst in &self.spec.hooks {
            if inst.hook.is_empty() {
                return Err(ManifestError::Invalid(
                    "spec.hooks[].hook must not be empty".into(),
                ));
            }
            if !seen.insert(inst.hook.as_str()) {
                return Err(ManifestError::Invalid(format!(
                    "duplicate hook id '{}' in chain '{}'",
                    inst.hook, self.metadata.name
                )));
            }
        }
        Ok(())
    }

    /// Convert a validated manifest into a runtime [`HookChain`].
    pub fn into_chain(self) -> Result<HookChain, ManifestError> {
        self.validate()?;
        let hooks = self
            .spec
            .hooks
            .into_iter()
            .map(|i| HookInstance {
                hook_ref: i.hook,
                config: i.config,
                enabled: i.enabled,
            })
            .collect();
        Ok(HookChain {
            name: self.metadata.name,
            point: self.spec.hook_point,
            hooks,
            timeout_ms: self.spec.timeout_ms,
            fail_open: self.spec.fail_open,
        })
    }
}

/// Return true iff `name` matches `^[a-z][a-z0-9-]*$`.
fn is_valid_chain_name(name: &str) -> bool {
    let mut chars = name.chars();
    match chars.next() {
        Some(c) if c.is_ascii_lowercase() => {}
        _ => return false,
    }
    chars.all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '-')
}

/// Generate the JSON Schema for `HookChainManifest`.
///
/// Useful for IDE completion, CI validation, and documentation.
pub fn json_schema() -> serde_json::Value {
    serde_json::to_value(schemars::schema_for!(HookChainManifest))
        .expect("schemars output is always JSON-serialisable")
}

#[cfg(test)]
mod tests {
    use super::*;

    const MINIMAL_YAML: &str = r#"
apiVersion: sera.dev/v1
kind: HookChain
metadata:
  name: pre-route-default
spec:
  hook_point: pre_route
  hooks:
    - hook: content-filter
      config:
        blocked_patterns: ["spam"]
    - hook: rate-limiter
      config:
        requests_per_minute: 60
"#;

    #[test]
    fn parses_minimal_manifest() {
        let m = HookChainManifest::from_yaml(MINIMAL_YAML).unwrap();
        assert_eq!(m.metadata.name, "pre-route-default");
        assert_eq!(m.spec.hook_point, HookPoint::PreRoute);
        assert_eq!(m.spec.hooks.len(), 2);
        assert_eq!(m.spec.timeout_ms, 5000); // default
        assert!(!m.spec.fail_open);
    }

    #[test]
    fn defaults_enabled_true() {
        let m = HookChainManifest::from_yaml(MINIMAL_YAML).unwrap();
        assert!(m.spec.hooks[0].enabled);
        assert!(m.spec.hooks[1].enabled);
    }

    #[test]
    fn into_chain_preserves_shape() {
        let m = HookChainManifest::from_yaml(MINIMAL_YAML).unwrap();
        let chain = m.into_chain().unwrap();
        assert_eq!(chain.name, "pre-route-default");
        assert_eq!(chain.point, HookPoint::PreRoute);
        assert_eq!(chain.hooks[0].hook_ref, "content-filter");
        assert_eq!(chain.hooks[1].hook_ref, "rate-limiter");
    }

    #[test]
    fn rejects_unknown_api_version() {
        let yaml = MINIMAL_YAML.replace("sera.dev/v1", "sera.dev/v2");
        let err = HookChainManifest::from_yaml(&yaml).unwrap_err();
        assert!(matches!(err, ManifestError::Invalid(_)));
    }

    #[test]
    fn rejects_wrong_kind() {
        let yaml = MINIMAL_YAML.replace("kind: HookChain", "kind: Plugin");
        let err = HookChainManifest::from_yaml(&yaml).unwrap_err();
        assert!(matches!(err, ManifestError::Invalid(_)));
    }

    #[test]
    fn rejects_unknown_hook_point() {
        let yaml = MINIMAL_YAML.replace("pre_route", "not_a_real_point");
        let err = HookChainManifest::from_yaml(&yaml).unwrap_err();
        // serde_yaml fails at parse time with an unknown enum variant.
        assert!(matches!(err, ManifestError::Parse(_)));
    }

    #[test]
    fn rejects_duplicate_hook_ids() {
        let yaml = r#"
apiVersion: sera.dev/v1
kind: HookChain
metadata:
  name: dup-chain
spec:
  hook_point: pre_tool
  hooks:
    - hook: secret-injector
      config: {}
    - hook: secret-injector
      config: {}
"#;
        let err = HookChainManifest::from_yaml(yaml).unwrap_err();
        match err {
            ManifestError::Invalid(msg) => assert!(
                msg.contains("duplicate hook id"),
                "unexpected message: {msg}"
            ),
            other => panic!("expected Invalid, got {other:?}"),
        }
    }

    #[test]
    fn rejects_invalid_chain_name() {
        let yaml = MINIMAL_YAML.replace("pre-route-default", "BadName");
        let err = HookChainManifest::from_yaml(&yaml).unwrap_err();
        assert!(matches!(err, ManifestError::Invalid(_)));
    }

    #[test]
    fn rejects_zero_timeout() {
        let yaml = r#"
apiVersion: sera.dev/v1
kind: HookChain
metadata:
  name: zero-timeout
spec:
  hook_point: pre_route
  timeout_ms: 0
  hooks: []
"#;
        let err = HookChainManifest::from_yaml(yaml).unwrap_err();
        assert!(matches!(err, ManifestError::Invalid(_)));
    }

    #[test]
    fn all_hook_points_parse() {
        // Regression guard: every variant in HookPoint::ALL must be parsable
        // from its serde name in a manifest.
        for point in HookPoint::ALL {
            let name = serde_json::to_string(point).unwrap();
            let name = name.trim_matches('"');
            // Chain metadata.name must be hyphen-separated; substitute the
            // underscores from the hook-point name so the manifest is valid.
            let chain_name = format!("chain-{}", name.replace('_', "-"));
            let yaml = format!(
                r#"
apiVersion: sera.dev/v1
kind: HookChain
metadata:
  name: {chain_name}
spec:
  hook_point: {name}
  hooks: []
"#,
                chain_name = chain_name,
                name = name
            );
            let m = HookChainManifest::from_yaml(&yaml)
                .unwrap_or_else(|e| panic!("point {name} failed to parse: {e:?}"));
            assert_eq!(&m.spec.hook_point, point);
        }
    }

    #[test]
    fn json_schema_has_required_top_level_fields() {
        let schema = json_schema();
        // The schema contract is that top-level required fields cover the
        // apiVersion/kind/metadata/spec envelope — this guards the manifest
        // shape against accidental removals.
        let required = schema
            .get("required")
            .and_then(|v| v.as_array())
            .expect("root schema has required array");
        let names: Vec<&str> = required.iter().filter_map(|v| v.as_str()).collect();
        assert!(names.contains(&"apiVersion"));
        assert!(names.contains(&"kind"));
        assert!(names.contains(&"metadata"));
        assert!(names.contains(&"spec"));
    }
}

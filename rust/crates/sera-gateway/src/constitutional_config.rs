//! Constitutional rule config-file loader.
//!
//! Reads a YAML rule list at startup and seeds a [`ConstitutionalRegistry`].
//!
//! # File format
//!
//! ```yaml
//! - id: forbid-unreviewed-code-changes
//!   description: "Code-scoped changes must carry ReviewedByHuman scope"
//!   enforcement_point: pre_approval
//!   scopes: [code_evolution]
//!   blast_radii: [gateway_core, runtime_crate]
//!   required_scopes: [gateway_core]
//! ```
//!
//! # Env-var
//!
//! `SERA_CONSTITUTIONAL_RULES_PATH` — path to the YAML file.
//! Default: `/etc/sera/constitutional_rules.yaml`.
//!
//! * **File absent** → log INFO, return empty (backward-compat).
//! * **File present but invalid** → log ERROR, return `Err` (fail-fast).

use serde::Deserialize;
use sha2::{Digest, Sha256};

use sera_meta::constitutional::{ConstitutionalRegistry, ConstitutionalRule};
use sera_meta::ChangeArtifactScope;
use sera_types::evolution::{BlastRadius, ConstitutionalEnforcementPoint};
use sera_types::evolution::ConstitutionalRule as ConstitutionalRuleBase;

/// Default path consulted when `SERA_CONSTITUTIONAL_RULES_PATH` is not set.
pub const DEFAULT_RULES_PATH: &str = "/etc/sera/constitutional_rules.yaml";

/// Wire-format for one rule entry in the YAML file.
#[derive(Debug, Deserialize)]
pub struct RuleEntry {
    pub id: String,
    pub description: String,
    pub enforcement_point: ConstitutionalEnforcementPoint,
    #[serde(default)]
    pub scopes: Vec<ChangeArtifactScope>,
    #[serde(default)]
    pub blast_radii: Vec<BlastRadius>,
    #[serde(default)]
    pub required_scopes: Vec<BlastRadius>,
}

impl RuleEntry {
    /// Convert into a [`ConstitutionalRule`].
    ///
    /// The `content_hash` is derived from `id || description` so that the
    /// hash changes whenever either field changes, giving a stable but
    /// deterministic fingerprint without requiring operators to supply raw
    /// bytes in the config file.
    fn into_rule(self) -> ConstitutionalRule {
        let mut hasher = Sha256::new();
        hasher.update(self.id.as_bytes());
        hasher.update(b"|");
        hasher.update(self.description.as_bytes());
        let digest = hasher.finalize();
        let mut content_hash = [0u8; 32];
        content_hash.copy_from_slice(&digest[..32]);

        ConstitutionalRule::new(
            ConstitutionalRuleBase {
                id: self.id,
                description: self.description,
                enforcement_point: self.enforcement_point,
                content_hash,
            },
            self.scopes,
            self.blast_radii,
            self.required_scopes,
        )
    }
}

/// Parse a YAML string into a list of [`RuleEntry`] values.
///
/// Returns `Err` on any parse failure so the caller can fail-fast.
pub fn parse_rules(yaml: &str) -> Result<Vec<RuleEntry>, serde_yaml::Error> {
    serde_yaml::from_str::<Vec<RuleEntry>>(yaml)
}

/// Seed `registry` from the file at `path`.
///
/// * Missing file → returns `Ok(0)` (registry untouched).
/// * Parse error → returns `Err`.
pub async fn seed_registry_from_file(
    registry: &ConstitutionalRegistry,
    path: &str,
) -> Result<usize, Box<dyn std::error::Error + Send + Sync>> {
    let yaml = match tokio::fs::read_to_string(path).await {
        Ok(content) => content,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
            tracing::info!(
                path,
                "Constitutional rules file not found — starting with empty registry"
            );
            return Ok(0);
        }
        Err(e) => return Err(Box::new(e)),
    };

    let entries =
        parse_rules(&yaml).map_err(|e| -> Box<dyn std::error::Error + Send + Sync> {
            Box::new(e)
        })?;

    let count = entries.len();
    for entry in entries {
        registry.register(entry.into_rule()).await;
    }

    tracing::info!(path, count, "Constitutional rules loaded from file");
    Ok(count)
}

/// Read the rules-file path from env, falling back to [`DEFAULT_RULES_PATH`],
/// then seed `registry`. On parse error, logs the error and returns `Err` so
/// the caller can exit 1.
pub async fn seed_registry_from_env(
    registry: &ConstitutionalRegistry,
) -> Result<usize, Box<dyn std::error::Error + Send + Sync>> {
    let path = std::env::var("SERA_CONSTITUTIONAL_RULES_PATH")
        .unwrap_or_else(|_| DEFAULT_RULES_PATH.to_string());
    seed_registry_from_file(registry, &path).await
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::NamedTempFile;
    use std::io::Write as _;

    // ---- parse_rules -------------------------------------------------------

    #[test]
    fn parse_two_valid_rules() {
        let yaml = r#"
- id: rule-one
  description: "First rule"
  enforcement_point: pre_approval
  scopes: [code_evolution]
  blast_radii: [gateway_core, runtime_crate]
  required_scopes: [gateway_core]
- id: rule-two
  description: "Second rule"
  enforcement_point: pre_proposal
  scopes: [agent_improvement]
  blast_radii: [agent_memory]
  required_scopes: []
"#;
        let entries = parse_rules(yaml).expect("should parse");
        assert_eq!(entries.len(), 2);
        assert_eq!(entries[0].id, "rule-one");
        assert_eq!(entries[1].id, "rule-two");
    }

    #[test]
    fn parse_empty_file_returns_empty_vec() {
        let entries = parse_rules("[]").expect("empty array should parse");
        assert!(entries.is_empty());

        // A truly empty string also deserialises as null → empty vec via
        // serde_yaml treating null as an empty sequence is NOT the case;
        // operators should supply [] for an empty file. Verify the explicit
        // empty-array form only.
    }

    #[test]
    fn parse_invalid_blast_radius_returns_error() {
        let yaml = r#"
- id: bad-rule
  description: "Has unknown blast radius"
  enforcement_point: pre_approval
  blast_radii: [not_a_real_radius]
"#;
        let result = parse_rules(yaml);
        assert!(
            result.is_err(),
            "unknown blast_radius variant should produce a parse error"
        );
    }

    #[test]
    fn parse_invalid_enforcement_point_returns_error() {
        let yaml = r#"
- id: bad-ep
  description: "Bad enforcement point"
  enforcement_point: during_lunch
"#;
        let result = parse_rules(yaml);
        assert!(result.is_err(), "unknown enforcement_point should error");
    }

    // ---- seed_registry_from_file -------------------------------------------

    #[tokio::test]
    async fn seed_from_valid_file_registers_rules() {
        let yaml = r#"
- id: r1
  description: "Rule one"
  enforcement_point: pre_approval
  scopes: [code_evolution]
  blast_radii: [gateway_core]
  required_scopes: []
- id: r2
  description: "Rule two"
  enforcement_point: pre_proposal
  scopes: [agent_improvement]
  blast_radii: [agent_memory]
  required_scopes: [agent_memory]
"#;
        let mut tmp = NamedTempFile::new().unwrap();
        tmp.write_all(yaml.as_bytes()).unwrap();

        let registry = ConstitutionalRegistry::new();
        let count = seed_registry_from_file(&registry, tmp.path().to_str().unwrap())
            .await
            .expect("seeding should succeed");

        assert_eq!(count, 2);
        assert_eq!(registry.all_rules().await.len(), 2);
        assert!(registry.get("r1").await.is_some());
        assert!(registry.get("r2").await.is_some());
    }

    #[tokio::test]
    async fn seed_from_empty_yaml_array_leaves_registry_empty() {
        let mut tmp = NamedTempFile::new().unwrap();
        tmp.write_all(b"[]").unwrap();

        let registry = ConstitutionalRegistry::new();
        let count = seed_registry_from_file(&registry, tmp.path().to_str().unwrap())
            .await
            .expect("empty array should succeed");

        assert_eq!(count, 0);
        assert!(registry.all_rules().await.is_empty());
    }

    #[tokio::test]
    async fn seed_missing_file_returns_zero_no_crash() {
        let registry = ConstitutionalRegistry::new();
        let result = seed_registry_from_file(&registry, "/nonexistent/path/rules.yaml").await;
        assert!(result.is_ok(), "missing file should not error");
        assert_eq!(result.unwrap(), 0);
        assert!(registry.all_rules().await.is_empty());
    }

    #[tokio::test]
    async fn seed_from_file_with_invalid_yaml_returns_error() {
        let mut tmp = NamedTempFile::new().unwrap();
        tmp.write_all(b"- id: bad\n  blast_radii: [not_real]\n  enforcement_point: pre_approval\n").unwrap();

        let registry = ConstitutionalRegistry::new();
        let result = seed_registry_from_file(&registry, tmp.path().to_str().unwrap()).await;
        assert!(result.is_err(), "invalid blast_radius should propagate as error");
    }

    // ---- content_hash is deterministic -------------------------------------

    #[test]
    fn content_hash_is_deterministic() {
        let entry1 = RuleEntry {
            id: "my-rule".to_string(),
            description: "some desc".to_string(),
            enforcement_point: ConstitutionalEnforcementPoint::PreApproval,
            scopes: vec![],
            blast_radii: vec![],
            required_scopes: vec![],
        };
        let entry2 = RuleEntry {
            id: "my-rule".to_string(),
            description: "some desc".to_string(),
            enforcement_point: ConstitutionalEnforcementPoint::PreApproval,
            scopes: vec![],
            blast_radii: vec![],
            required_scopes: vec![],
        };
        let r1 = entry1.into_rule();
        let r2 = entry2.into_rule();
        assert_eq!(r1.base.content_hash, r2.base.content_hash);
    }
}

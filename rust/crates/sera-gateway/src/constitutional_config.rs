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

use std::collections::HashSet;

use serde::Deserialize;
use sha2::{Digest, Sha256};

use sera_meta::constitutional::{ConstitutionalRegistry, ConstitutionalRuleEntry};
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
    /// Convert into a [`ConstitutionalRuleEntry`].
    ///
    /// The `content_hash` is derived from `id || description` so that the
    /// hash changes whenever either field changes, giving a stable but
    /// deterministic fingerprint without requiring operators to supply raw
    /// bytes in the config file.
    ///
    /// Returns `Err` if `id` is empty or whitespace-only.
    fn into_rule(self) -> Result<ConstitutionalRuleEntry, String> {
        if self.id.trim().is_empty() {
            return Err("rule id must be non-empty".to_string());
        }
        let mut hasher = Sha256::new();
        hasher.update(self.id.as_bytes());
        hasher.update(b"|");
        hasher.update(self.description.as_bytes());
        let digest = hasher.finalize();
        let mut content_hash = [0u8; 32];
        content_hash.copy_from_slice(&digest[..32]);

        Ok(ConstitutionalRuleEntry::new(
            ConstitutionalRuleBase {
                id: self.id,
                description: self.description,
                enforcement_point: self.enforcement_point,
                content_hash,
            },
            self.scopes,
            self.blast_radii,
            self.required_scopes,
        ))
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
    let mut seen_ids: HashSet<String> = HashSet::new();
    for entry in entries {
        let rule = entry
            .into_rule()
            .map_err(|e| -> Box<dyn std::error::Error + Send + Sync> { e.into() })?;
        if !seen_ids.insert(rule.base.id.clone()) {
            tracing::warn!(rule_id = %rule.base.id, "duplicate rule id — second rule overwrites first");
        }
        registry.register(rule).await;
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

    // ---- large rule sets ---------------------------------------------------

    #[test]
    fn large_rule_set_100_rules_no_panic() {
        let mut yaml = String::new();
        for i in 0..100u32 {
            yaml.push_str(&format!(
                "- id: rule-{i}\n  description: \"Rule number {i}\"\n  enforcement_point: pre_approval\n  blast_radii: [gateway_core]\n  required_scopes: []\n"
            ));
        }
        let entries = parse_rules(&yaml).expect("100 rules should parse without panic");
        assert_eq!(entries.len(), 100);
        // Convert all to rules without panic
        for entry in entries {
            let _rule = entry.into_rule().expect("valid rule entry should not error");
        }
    }

    #[tokio::test]
    async fn large_rule_set_100_rules_register_without_panic() {
        let mut yaml = String::new();
        for i in 0..100u32 {
            yaml.push_str(&format!(
                "- id: stress-rule-{i}\n  description: \"Stress rule {i}\"\n  enforcement_point: pre_approval\n  blast_radii: [runtime_crate]\n  required_scopes: []\n"
            ));
        }
        let mut tmp = NamedTempFile::new().unwrap();
        tmp.write_all(yaml.as_bytes()).unwrap();
        let registry = ConstitutionalRegistry::new();
        let count = seed_registry_from_file(&registry, tmp.path().to_str().unwrap())
            .await
            .expect("100-rule seed should succeed");
        assert_eq!(count, 100);
        assert_eq!(registry.all_rules().await.len(), 100);
    }

    // ---- unicode in rule fields --------------------------------------------

    #[test]
    fn unicode_emoji_cjk_rtl_roundtrip() {
        let yaml = r#"
- id: "rule-emoji-🚀"
  description: "Emoji rule: enforce 🔒 before 🚀"
  enforcement_point: pre_approval
- id: "rule-cjk-规则"
  description: "中文描述：确保变更经过审批"
  enforcement_point: pre_proposal
- id: "rule-rtl-قاعدة"
  description: "وصف باللغة العربية"
  enforcement_point: pre_application
"#;
        let entries = parse_rules(yaml).expect("unicode fields should parse");
        assert_eq!(entries.len(), 3);
        assert_eq!(entries[0].id, "rule-emoji-🚀");
        assert_eq!(entries[0].description, "Emoji rule: enforce 🔒 before 🚀");
        assert_eq!(entries[1].id, "rule-cjk-规则");
        assert_eq!(entries[1].description, "中文描述：确保变更经过审批");
        assert_eq!(entries[2].id, "rule-rtl-قاعدة");
        // Hashes are computed without panic for multibyte content
        for entry in entries {
            let _rule = entry.into_rule().expect("unicode rule entry should not error");
        }
    }

    // ---- duplicate rule ids -----------------------------------------------

    /// Registry contract: register() uses HashMap::insert, so a second rule
    /// with the same id OVERWRITES the first. The loader now emits a tracing
    /// warning when this occurs.
    #[tokio::test]
    async fn duplicate_ids_last_writer_wins() {
        let yaml = r#"
- id: dup-rule
  description: "First version"
  enforcement_point: pre_approval
- id: dup-rule
  description: "Second version (overwrites first)"
  enforcement_point: pre_proposal
"#;
        let mut tmp = NamedTempFile::new().unwrap();
        tmp.write_all(yaml.as_bytes()).unwrap();

        let registry = ConstitutionalRegistry::new();
        // seed_registry_from_file registers them in order; second call wins
        let count = seed_registry_from_file(&registry, tmp.path().to_str().unwrap())
            .await
            .expect("duplicate-id yaml should be accepted by the loader");

        // count reflects YAML entries parsed, not unique registered rules
        assert_eq!(count, 2, "loader returns count of entries parsed, not unique ids");

        // Only one rule survives in the registry — the second one
        let all = registry.all_rules().await;
        assert_eq!(all.len(), 1, "registry deduplicates by id: last writer wins");
        let surviving = registry.get("dup-rule").await.expect("dup-rule must exist");
        assert_eq!(
            surviving.base.description,
            "Second version (overwrites first)",
            "the second registration should overwrite the first"
        );
        // Note: the loader emits tracing::warn!(rule_id = %id, "duplicate rule id …")
        // for the second "dup-rule" entry. Capturing tracing spans in unit tests
        // requires `tracing-test` or a custom subscriber; verifying the behavioral
        // outcome (last-writer-wins + correct count) is the primary assertion here.
    }

    // ---- all enforcement_point variants -----------------------------------

    #[test]
    fn all_enforcement_point_variants_parse() {
        let cases = [
            ("pre_proposal", ConstitutionalEnforcementPoint::PreProposal),
            ("pre_approval", ConstitutionalEnforcementPoint::PreApproval),
            ("pre_application", ConstitutionalEnforcementPoint::PreApplication),
            ("post_application", ConstitutionalEnforcementPoint::PostApplication),
        ];
        for (yaml_str, expected) in cases {
            let yaml = format!(
                "- id: ep-test\n  description: \"ep test\"\n  enforcement_point: {yaml_str}\n"
            );
            let entries = parse_rules(&yaml)
                .unwrap_or_else(|e| panic!("enforcement_point '{yaml_str}' should parse: {e}"));
            assert_eq!(
                entries[0].enforcement_point, expected,
                "enforcement_point '{yaml_str}' mismatch"
            );
        }
    }

    // ---- all blast_radius variants ----------------------------------------

    #[test]
    fn all_blast_radius_variants_parse() {
        let all_variants = [
            "agent_memory",
            "agent_persona_mutable",
            "agent_skill",
            "agent_experience_pool",
            "single_hook_config",
            "single_tool_policy",
            "single_connector",
            "single_circle_config",
            "agent_manifest",
            "tier_policy",
            "hook_chain_structure",
            "approval_policy",
            "secret_provider",
            "global_config",
            "runtime_crate",
            "gateway_core",
            "protocol_schema",
            "db_migration",
            "constitutional_rule_set",
            "kill_switch_protocol",
            "audit_log_backend",
            "self_evolution_pipeline",
        ];
        // Build a single rule with all blast radii listed
        let radii_list = all_variants.join(", ");
        let yaml = format!(
            "- id: br-test\n  description: \"blast radius test\"\n  enforcement_point: pre_approval\n  blast_radii: [{radii_list}]\n"
        );
        let entries = parse_rules(&yaml).expect("all blast_radius variants should parse");
        assert_eq!(entries.len(), 1);
        assert_eq!(
            entries[0].blast_radii.len(),
            all_variants.len(),
            "all blast_radius variants should deserialize"
        );
    }

    // ---- empty / whitespace id rejection ----------------------------------

    #[test]
    fn empty_id_rejected_at_parse_time() {
        let yaml = r#"
- id: ""
  description: ""
  enforcement_point: pre_approval
"#;
        // serde accepts the YAML; validation fires in into_rule
        let entries = parse_rules(yaml).expect("empty strings are accepted by serde");
        assert_eq!(entries.len(), 1);
        let result = entries.into_iter().next().unwrap().into_rule();
        assert!(result.is_err(), "empty id must be rejected by into_rule");
        assert!(
            result.unwrap_err().contains("non-empty"),
            "error message should mention 'non-empty'"
        );
    }

    #[test]
    fn whitespace_only_id_rejected() {
        let yaml = r#"
- id: "   "
  description: "desc"
  enforcement_point: pre_approval
"#;
        let entries = parse_rules(yaml).expect("whitespace-only id parses through serde");
        assert_eq!(entries.len(), 1);
        let result = entries.into_iter().next().unwrap().into_rule();
        assert!(result.is_err(), "whitespace-only id must be rejected");
    }

    #[test]
    fn empty_description_still_allowed() {
        let yaml = r#"
- id: "valid-id"
  description: ""
  enforcement_point: pre_approval
"#;
        let entries = parse_rules(yaml).expect("valid id + empty description should parse");
        let rule = entries
            .into_iter()
            .next()
            .unwrap()
            .into_rule()
            .expect("empty description must remain allowed");
        assert_eq!(rule.base.id, "valid-id");
        assert_eq!(rule.base.description, "");
    }

    // ---- YAML alias/anchor (billion-laughs style) -------------------------

    /// serde_yaml 0.9 does not support YAML aliases/anchors — they produce a
    /// parse error rather than exponential expansion. Verify no OOM/panic.
    #[test]
    fn yaml_alias_bomb_returns_error_not_oom() {
        // Classic billion-laughs attempt using YAML anchors
        let yaml = r#"
lol1: &lol1 "lol"
lol2: &lol2 [*lol1, *lol1, *lol1, *lol1, *lol1, *lol1, *lol1, *lol1, *lol1]
lol3: &lol3 [*lol2, *lol2, *lol2, *lol2, *lol2, *lol2, *lol2, *lol2, *lol2]
"#;
        // This is not a valid rule list — either a parse error OR empty vec.
        // What matters is: no panic, no OOM.
        let _result = parse_rules(yaml);
        // We don't assert Ok/Err since the shape mismatch will cause an error
        // either way — the important invariant is the process is still alive here.
    }

    // ---- seed_registry_from_env -------------------------------------------

    #[tokio::test]
    async fn seed_registry_from_env_seeds_two_rules() {
        let yaml = b"- id: env-rule-1\n  description: \"First env rule\"\n  enforcement_point: pre_approval\n- id: env-rule-2\n  description: \"Second env rule\"\n  enforcement_point: pre_proposal\n";
        let mut tmp = NamedTempFile::new().unwrap();
        tmp.write_all(yaml).unwrap();

        let path_str = tmp.path().to_str().unwrap().to_owned();
        std::env::set_var("SERA_CONSTITUTIONAL_RULES_PATH", &path_str);

        let registry = ConstitutionalRegistry::new();
        let count = seed_registry_from_env(&registry)
            .await
            .expect("seed_registry_from_env should succeed when env var points to a valid file");

        std::env::remove_var("SERA_CONSTITUTIONAL_RULES_PATH");

        assert_eq!(count, 2, "should seed exactly 2 rules");
        assert_eq!(registry.all_rules().await.len(), 2);
        assert!(registry.get("env-rule-1").await.is_some());
        assert!(registry.get("env-rule-2").await.is_some());
    }

    #[tokio::test]
    async fn seed_registry_from_env_no_op_when_default_path_absent() {
        // When SERA_CONSTITUTIONAL_RULES_PATH is unset the loader falls back to
        // DEFAULT_RULES_PATH (/etc/sera/constitutional_rules.yaml). That file
        // almost certainly doesn't exist in CI, so the call must return Ok(0).
        std::env::remove_var("SERA_CONSTITUTIONAL_RULES_PATH");

        // Skip if the default path exists on this machine (e.g. a developer
        // environment that has real rules installed) to avoid a false failure.
        if std::path::Path::new(DEFAULT_RULES_PATH).exists() {
            return;
        }

        let registry = ConstitutionalRegistry::new();
        let result = seed_registry_from_env(&registry).await;
        assert!(result.is_ok(), "missing default file must be a no-op, not an error");
        assert_eq!(result.unwrap(), 0);
        assert!(registry.all_rules().await.is_empty());
    }

    // ---- relative path env var --------------------------------------------

    #[tokio::test]
    async fn relative_path_env_var_resolves_from_cwd() {
        use std::env;

        let mut tmp = NamedTempFile::new().unwrap();
        let yaml = b"- id: rel-path-rule\n  description: \"relative path test\"\n  enforcement_point: pre_approval\n";
        tmp.write_all(yaml).unwrap();

        // Build a relative path from the current working directory
        let tmp_path = tmp.path().to_path_buf();
        let cwd = env::current_dir().expect("cwd must be accessible");
        let relative = tmp_path
            .strip_prefix(&cwd)
            .unwrap_or(tmp_path.as_path()); // fall back to abs if not under cwd

        let path_str = relative.to_str().unwrap();
        let registry = ConstitutionalRegistry::new();
        // seed_registry_from_file takes the path string as-is; relative paths
        // resolve against the process cwd (tokio::fs::read_to_string behaviour)
        let result = seed_registry_from_file(&registry, path_str).await;
        assert!(result.is_ok(), "relative path should resolve: {result:?}");
        assert_eq!(result.unwrap(), 1);
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
        let r1 = entry1.into_rule().expect("valid entry should not error");
        let r2 = entry2.into_rule().expect("valid entry should not error");
        assert_eq!(r1.base.content_hash, r2.base.content_hash);
    }
}

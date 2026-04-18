//! Contract tests — verify that golden YAML files in `contracts/manifests/`
//! parse correctly into Rust types.
//!
//! These tests act as a canary: if someone changes the manifest schema in
//! `sera-types` or edits a golden file, at least one test here will fail,
//! making the breakage explicit.
//!
//! The golden files live at `rust/contracts/manifests/` (relative to the
//! workspace root). Tests resolve the path at runtime via `CARGO_MANIFEST_DIR`,
//! which points to this crate's directory (`crates/sera-testing`).

#[cfg(test)]
mod tests {
    use sera_types::manifest::AgentTemplate;
    use std::path::PathBuf;

    /// Returns the path to `rust/contracts/manifests/`.
    fn contracts_dir() -> PathBuf {
        // CARGO_MANIFEST_DIR = .../rust/crates/sera-testing
        let manifest_dir = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
        manifest_dir
            .join("..") // crates/
            .join("..") // rust/
            .join("contracts")
            .join("manifests")
    }

    fn parse_template(filename: &str) -> AgentTemplate {
        let path = contracts_dir().join(filename);
        let yaml = std::fs::read_to_string(&path)
            .unwrap_or_else(|e| panic!("failed to read {}: {e}", path.display()));
        serde_yaml::from_str(&yaml)
            .unwrap_or_else(|e| panic!("failed to parse {filename}: {e}"))
    }

    // ── example-minimal ──────────────────────────────────────────────────────

    #[test]
    fn contract_example_minimal_parses() {
        let t = parse_template("example-minimal.template.yaml");
        assert_eq!(t.api_version, "sera/v1");
        assert_eq!(t.kind, "AgentTemplate");
        assert_eq!(t.metadata.name, "example-minimal");
    }

    #[test]
    fn contract_example_minimal_sandbox_boundary() {
        let t = parse_template("example-minimal.template.yaml");
        assert_eq!(t.spec.sandbox_boundary.as_deref(), Some("tier-3"));
    }

    #[test]
    fn contract_example_minimal_lifecycle_mode() {
        use sera_types::LifecycleMode;
        let t = parse_template("example-minimal.template.yaml");
        let mode = t.spec.lifecycle.as_ref().unwrap().mode.clone();
        assert_eq!(mode, LifecycleMode::Ephemeral);
    }

    #[test]
    fn contract_example_minimal_model_provider() {
        let t = parse_template("example-minimal.template.yaml");
        let model = t.spec.model.as_ref().unwrap();
        assert_eq!(model.provider.as_deref(), Some("openai"));
        assert_eq!(model.model_name.as_deref(), Some("gpt-4o"));
    }

    // ── example-full ─────────────────────────────────────────────────────────

    #[test]
    fn contract_example_full_parses() {
        let t = parse_template("example-full.template.yaml");
        assert_eq!(t.metadata.name, "example-full");
        assert_eq!(t.kind, "AgentTemplate");
    }

    #[test]
    fn contract_example_full_skills() {
        let t = parse_template("example-full.template.yaml");
        let skills = t.spec.skills.as_ref().unwrap();
        assert!(skills.contains(&"shell-exec".to_string()));
        assert!(skills.contains(&"file-manager".to_string()));
    }

    #[test]
    fn contract_example_full_tools_allowed_and_denied() {
        let t = parse_template("example-full.template.yaml");
        let tools = t.spec.tools.as_ref().unwrap();
        let allowed = tools.allowed.as_ref().unwrap();
        let denied = tools.denied.as_ref().unwrap();
        assert!(allowed.contains(&"bash".to_string()));
        assert!(allowed.contains(&"read_file".to_string()));
        assert!(denied.contains(&"rm".to_string()));
    }

    #[test]
    fn contract_example_full_lifecycle_persistent() {
        use sera_types::LifecycleMode;
        let t = parse_template("example-full.template.yaml");
        let mode = t.spec.lifecycle.as_ref().unwrap().mode.clone();
        assert_eq!(mode, LifecycleMode::Persistent);
    }

    #[test]
    fn contract_example_full_policy_ref() {
        let t = parse_template("example-full.template.yaml");
        assert_eq!(t.spec.policy_ref.as_deref(), Some("default-restricted"));
    }

    // ── byoh-rust-example ─────────────────────────────────────────────────────

    #[test]
    fn contract_byoh_rust_parses() {
        let t = parse_template("byoh-rust-example.template.yaml");
        assert_eq!(t.metadata.name, "byoh-rust-example");
    }

    #[test]
    fn contract_byoh_rust_sandbox_image() {
        let t = parse_template("byoh-rust-example.template.yaml");
        let sandbox = t.spec.sandbox.as_ref().unwrap();
        assert_eq!(
            sandbox.image.as_deref(),
            Some("sera-byoh-rust-agent:latest")
        );
    }

    #[test]
    fn contract_byoh_rust_not_builtin() {
        let t = parse_template("byoh-rust-example.template.yaml");
        assert!(!t.metadata.builtin);
    }

    #[test]
    fn contract_byoh_rust_identity_role() {
        let t = parse_template("byoh-rust-example.template.yaml");
        let role = t
            .spec
            .identity
            .as_ref()
            .and_then(|i| i.role.as_deref());
        assert_eq!(role, Some("Example BYOH Rust agent"));
    }

    // ── round-trip: serialize back to YAML and re-parse ───────────────────────

    #[test]
    fn contract_example_minimal_roundtrip() {
        let original = parse_template("example-minimal.template.yaml");
        let serialized = serde_yaml::to_string(&original).unwrap();
        let reparsed: AgentTemplate = serde_yaml::from_str(&serialized).unwrap();
        assert_eq!(reparsed.metadata.name, original.metadata.name);
        assert_eq!(reparsed.spec.sandbox_boundary, original.spec.sandbox_boundary);
    }

    #[test]
    fn contract_example_full_roundtrip() {
        let original = parse_template("example-full.template.yaml");
        let serialized = serde_yaml::to_string(&original).unwrap();
        let reparsed: AgentTemplate = serde_yaml::from_str(&serialized).unwrap();
        assert_eq!(reparsed.metadata.name, original.metadata.name);
        assert_eq!(
            reparsed.spec.skills.as_ref().map(|s| s.len()),
            original.spec.skills.as_ref().map(|s| s.len()),
        );
    }
}

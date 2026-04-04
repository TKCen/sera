//! Golden manifest tests — validate that sera-domain can parse all YAML
//! files in contracts/manifests/ without error.

use sera_domain::manifest::AgentTemplate;
use sera_domain::policy::SandboxBoundary;
use std::fs;
use std::path::Path;

fn contracts_dir() -> &'static Path {
    // Tests run from the crate root (rust/crates/sera-domain/),
    // so contracts/ is at ../../contracts/
    Path::new("../../contracts/manifests")
}

#[test]
fn parse_all_agent_templates() {
    let dir = contracts_dir();
    assert!(dir.exists(), "contracts/manifests/ directory not found at {dir:?}");

    let mut count = 0;
    for entry in fs::read_dir(dir).unwrap() {
        let entry = entry.unwrap();
        let path = entry.path();
        if !path.extension().is_some_and(|e| e == "yaml") {
            continue;
        }

        let filename = path.file_name().unwrap().to_string_lossy().to_string();

        // Skip non-template files (sandbox boundaries, etc.)
        if !filename.contains("template") {
            continue;
        }

        let contents = fs::read_to_string(&path)
            .unwrap_or_else(|e| panic!("Failed to read {filename}: {e}"));

        let template: AgentTemplate = serde_yaml::from_str(&contents)
            .unwrap_or_else(|e| panic!("Failed to parse {filename}: {e}"));

        assert_eq!(template.api_version, "sera/v1", "{filename}: wrong apiVersion");
        assert_eq!(template.kind, "AgentTemplate", "{filename}: wrong kind");
        assert!(!template.metadata.name.is_empty(), "{filename}: empty name");

        count += 1;
    }

    assert!(count >= 3, "Expected at least 3 template files, found {count}");
}

#[test]
fn parse_all_sandbox_boundaries() {
    let dir = contracts_dir();

    let mut count = 0;
    for entry in fs::read_dir(dir).unwrap() {
        let entry = entry.unwrap();
        let path = entry.path();
        if !path.extension().is_some_and(|e| e == "yaml") {
            continue;
        }

        let filename = path.file_name().unwrap().to_string_lossy().to_string();

        // Only tier files
        if !filename.starts_with("tier-") {
            continue;
        }

        let contents = fs::read_to_string(&path)
            .unwrap_or_else(|e| panic!("Failed to read {filename}: {e}"));

        let boundary: SandboxBoundary = serde_yaml::from_str(&contents)
            .unwrap_or_else(|e| panic!("Failed to parse {filename}: {e}"));

        assert_eq!(boundary.api_version, "sera/v1", "{filename}: wrong apiVersion");
        assert_eq!(boundary.kind, "SandboxBoundary", "{filename}: wrong kind");
        assert!(!boundary.metadata.name.is_empty(), "{filename}: empty name");

        count += 1;
    }

    assert!(count >= 3, "Expected at least 3 sandbox boundary files, found {count}");
}

#[test]
fn template_roundtrip_yaml() {
    let dir = contracts_dir();
    let minimal_path = dir.join("example-minimal.template.yaml");
    let contents = fs::read_to_string(&minimal_path).unwrap();

    // Deserialize
    let template: AgentTemplate = serde_yaml::from_str(&contents).unwrap();

    // Serialize back to YAML
    let reserialized = serde_yaml::to_string(&template).unwrap();

    // Deserialize again
    let reparsed: AgentTemplate = serde_yaml::from_str(&reserialized).unwrap();

    // Compare key fields (structural equality)
    assert_eq!(template.metadata.name, reparsed.metadata.name);
    assert_eq!(template.api_version, reparsed.api_version);
    assert_eq!(
        template.spec.sandbox_boundary,
        reparsed.spec.sandbox_boundary
    );
    assert_eq!(
        template.spec.lifecycle.as_ref().map(|l| l.mode),
        reparsed.spec.lifecycle.as_ref().map(|l| l.mode)
    );
}

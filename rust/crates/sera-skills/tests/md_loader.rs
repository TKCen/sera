//! Integration tests for the SKILL.md loader.
//!
//! Exercises the fixture files under `tests/fixtures/` end-to-end: disk read,
//! frontmatter parse, field propagation, default-tier behaviour, and
//! warn-but-load semantics for unknown keys.

use std::path::PathBuf;

use sera_skills::md_loader::{load_skill_md, DEFAULT_TIER};

fn fixture_path(name: &str) -> PathBuf {
    let mut p = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    p.push("tests");
    p.push("fixtures");
    p.push(name);
    p
}

#[tokio::test]
async fn lookup_invoice_fixture_loads() {
    let skill = load_skill_md(&fixture_path("lookup-invoice.md"))
        .await
        .unwrap();
    assert_eq!(skill.name, "lookup-invoice");
    assert!(skill.description.contains("finance API"));
    assert_eq!(
        skill.inputs.get("invoice_id").map(String::as_str),
        Some("string")
    );
    assert_eq!(skill.tier, 1);
    assert!(skill.body.contains("# Behaviour"));
}

#[tokio::test]
async fn summarise_thread_fixture_tier_two() {
    let skill = load_skill_md(&fixture_path("summarise-thread.md"))
        .await
        .unwrap();
    assert_eq!(skill.name, "summarise-thread");
    assert_eq!(skill.tier, 2);
    assert_eq!(skill.inputs.len(), 2);
}

#[tokio::test]
async fn no_tier_fixture_defaults() {
    let skill = load_skill_md(&fixture_path("no-tier.md")).await.unwrap();
    assert_eq!(skill.tier, DEFAULT_TIER);
    assert!(skill.inputs.is_empty());
}

#[tokio::test]
async fn unknown_keys_fixture_still_loads() {
    let skill = load_skill_md(&fixture_path("unknown-keys.md"))
        .await
        .unwrap();
    assert_eq!(skill.name, "unknown-keys");
    assert_eq!(skill.tier, 1);
    // Unknown keys should not appear anywhere on the Skill struct.
    assert!(skill.body.contains("Body preserved"));
}

#[tokio::test]
async fn source_path_propagated() {
    let path = fixture_path("lookup-invoice.md");
    let skill = load_skill_md(&path).await.unwrap();
    assert_eq!(skill.source_path.as_deref(), Some(path.as_path()));
}

#[tokio::test]
async fn missing_fixture_errors() {
    let path = fixture_path("does-not-exist.md");
    let err = load_skill_md(&path).await.unwrap_err();
    // IO not-found error, not a format error.
    let msg = err.to_string();
    assert!(msg.contains("IO") || msg.contains("os error") || msg.contains("No such"));
}

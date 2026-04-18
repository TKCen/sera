//! Integration smoke tests — spawn a real `rust-analyzer` process and exercise
//! the phase 1 / phase 2 LSP tools against a tiny fixture crate.
//!
//! Gated behind `--features integration` and `#[ignore]` so CI does not
//! attempt to run these unless explicitly requested.

#![cfg(feature = "integration")]

use std::io::Write;

use sera_tools::lsp::{
    FindReferencingSymbolsInput, FindReferencingSymbolsTool, FindSymbolInput, FindSymbolTool,
    GetSymbolsOverviewInput, GetSymbolsOverviewTool, LspServerConfig, LspServerRegistry,
    LspToolsState,
};

/// Build a temp crate whose `src/lib.rs` is the supplied `content`.
fn make_fixture(content: &str) -> tempfile::TempDir {
    let dir = tempfile::tempdir().expect("tempdir");
    let src = dir.path().join("src");
    std::fs::create_dir_all(&src).expect("mkdir src");

    let mut cargo = std::fs::File::create(dir.path().join("Cargo.toml")).unwrap();
    cargo
        .write_all(
            br#"[package]
name = "lsp-fixture"
version = "0.0.1"
edition = "2021"

[lib]
path = "src/lib.rs"
"#,
        )
        .unwrap();
    cargo.sync_all().unwrap();

    let mut lib = std::fs::File::create(src.join("lib.rs")).unwrap();
    lib.write_all(content.as_bytes()).unwrap();
    lib.sync_all().unwrap();
    dir
}

fn rust_state() -> LspToolsState {
    let mut reg = LspServerRegistry::new();
    reg.register(LspServerConfig::default_rust());
    LspToolsState::new(reg)
}

#[tokio::test]
#[ignore]
async fn rust_analyzer_get_symbols_overview_smoke() {
    if which::which("rust-analyzer").is_err() {
        eprintln!("rust-analyzer not on PATH — skipping integration test");
        return;
    }

    let dir = make_fixture("pub struct Foo;\npub fn bar() {}\n");
    let state = rust_state();

    let overview = GetSymbolsOverviewTool::new()
        .invoke(
            GetSymbolsOverviewInput {
                path: "src/lib.rs".into(),
                depth: 0,
            },
            &state,
            dir.path(),
        )
        .await
        .expect("invoke must succeed");

    assert_eq!(overview.language, "rust");
    let names: Vec<&str> = overview.symbols.iter().map(|s| s.name.as_str()).collect();
    assert!(
        names.iter().any(|n| *n == "Foo"),
        "expected symbol Foo in {names:?}"
    );
    assert!(
        names.iter().any(|n| *n == "bar"),
        "expected symbol bar in {names:?}"
    );

    if let Some(sup) = state.supervisors.read().await.get("rust").cloned() {
        let _ = sup.shutdown().await;
    }
}

#[tokio::test]
#[ignore]
async fn rust_analyzer_find_symbol_smoke() {
    if which::which("rust-analyzer").is_err() {
        eprintln!("rust-analyzer not on PATH — skipping integration test");
        return;
    }

    let dir = make_fixture("pub struct Foo;\npub fn bar() {}\n");
    let state = rust_state();

    let result = FindSymbolTool::new()
        .invoke(
            FindSymbolInput {
                name_path_pattern: "bar".into(),
                relative_path: String::new(),
                depth: 0,
                include_body: false,
                include_kinds: Vec::new(),
                max_matches: 0,
            },
            &state,
            dir.path(),
        )
        .await
        .expect("find_symbol invoke");

    assert!(
        !result.matches.is_empty(),
        "expected at least one match for `bar` in {:?}",
        result
    );
    assert!(
        result
            .matches
            .iter()
            .any(|m| m.name_path.ends_with("bar")),
        "expected a match ending in `bar`, got: {:?}",
        result.matches
    );

    if let Some(sup) = state.supervisors.read().await.get("rust").cloned() {
        let _ = sup.shutdown().await;
    }
}

#[tokio::test]
#[ignore]
async fn rust_analyzer_find_references_smoke() {
    if which::which("rust-analyzer").is_err() {
        eprintln!("rust-analyzer not on PATH — skipping integration test");
        return;
    }

    // `baz` calls `bar` — we should find the reference on the `bar()` call
    // site inside baz.
    let content = "pub fn bar() {}\npub fn baz() { bar(); }\n";
    let dir = make_fixture(content);
    let state = rust_state();

    let result = FindReferencingSymbolsTool::new()
        .invoke(
            FindReferencingSymbolsInput {
                name_path: "bar".into(),
                relative_path: "src/lib.rs".into(),
                include_kinds: Vec::new(),
            },
            &state,
            dir.path(),
        )
        .await
        .expect("find_referencing_symbols invoke");

    assert!(
        !result.references.is_empty(),
        "expected at least one caller of `bar` in {:?}",
        result
    );
    for r in &result.references {
        assert!(!r.snippet.is_empty(), "snippet must not be empty");
    }

    if let Some(sup) = state.supervisors.read().await.get("rust").cloned() {
        let _ = sup.shutdown().await;
    }
}

//! Integration smoke test — spawns a real `rust-analyzer` process and fetches
//! document symbols for a tiny fixture file.
//!
//! Gated behind `--features integration` and `#[ignore]` so CI does not
//! attempt to run it unless explicitly requested.

#![cfg(feature = "integration")]

use std::io::Write;

use sera_tools::lsp::{
    GetSymbolsOverviewInput, GetSymbolsOverviewTool, LspServerConfig, LspServerRegistry,
    LspToolsState,
};

#[tokio::test]
#[ignore]
async fn rust_analyzer_get_symbols_overview_smoke() {
    // Skip silently if rust-analyzer isn't installed.
    if which::which("rust-analyzer").is_err() {
        eprintln!("rust-analyzer not on PATH — skipping integration test");
        return;
    }

    // Build a temp crate with a couple of symbols rust-analyzer can see.
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
    lib.write_all(b"pub struct Foo;\npub fn bar() {}\n").unwrap();
    lib.sync_all().unwrap();

    // Registry pointing at the locally-installed rust-analyzer.
    let mut reg = LspServerRegistry::new();
    reg.register(LspServerConfig::default_rust());
    let state = LspToolsState::new(reg);

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

    // Shut down the spawned supervisor to avoid leaking the child process.
    if let Some(sup) = state.supervisors.read().await.get("rust").cloned() {
        let _ = sup.shutdown().await;
    }
}

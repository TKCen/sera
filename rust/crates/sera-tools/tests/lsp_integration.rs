//! Integration smoke test — spawns a real `rust-analyzer` process and fetches
//! document symbols for a tiny fixture file.
//!
//! Gated behind `--features integration` and `#[ignore]` so CI does not
//! attempt to run it unless explicitly requested.

#![cfg(feature = "integration")]

use std::path::PathBuf;
use std::sync::Arc;

use sera_tools::lsp::{
    client::default_initialize_params, LspClient, LspProcessSupervisor, LspServerConfig,
};

#[tokio::test]
#[ignore]
async fn rust_analyzer_document_symbol_smoke() {
    // Skip silently if rust-analyzer isn't installed.
    if which::which("rust-analyzer").is_err() {
        eprintln!("rust-analyzer not on PATH — skipping integration test");
        return;
    }

    let config = LspServerConfig::default_rust();
    let mut supervisor = LspProcessSupervisor::new(&config)
        .await
        .expect("spawn rust-analyzer");

    let project_root = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    let _init = supervisor
        .initialize(&project_root)
        .await
        .expect("initialize rust-analyzer");

    let client: Arc<LspClient<_, _>> = supervisor.client();
    let fixture = project_root.join("src/lib.rs");
    let uri_str = format!("file://{}", fixture.to_string_lossy());
    let uri: lsp_types::Uri = uri_str.parse().expect("fixture path -> uri");
    let symbols = client
        .document_symbol(uri)
        .await
        .expect("documentSymbol must succeed");

    assert!(
        !symbols.is_empty(),
        "rust-analyzer should return at least one symbol for sera-tools/src/lib.rs"
    );

    let _ = supervisor.shutdown().await;
}

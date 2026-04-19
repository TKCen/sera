pub mod corrections;
pub mod registry;
pub mod sandbox;
pub mod sera_errors;
pub mod ssrf;
pub mod binary_identity;
pub mod bash_ast;
pub mod inference_local;
pub mod kill_switch;
pub mod knowledge_ingest;
pub mod lsp;

// Phase-1 re-exports for the LSP code-introspection layer.
pub use lsp::{GetSymbolsOverviewTool, LspToolsState};

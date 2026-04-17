//! LSP-backed tool implementations. Phase 1 ships `get_symbols_overview`.

use serde::{Deserialize, Serialize};

pub mod get_symbols_overview;
pub use get_symbols_overview::{GetSymbolsOverviewInput, GetSymbolsOverviewTool};

/// Byte-offset range — stable across editors and CRLF-safe
/// (see `docs/plan/LSP-TOOLS-DESIGN.md` §3.1).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ByteRange {
    pub start: u32,
    pub end: u32,
}

/// One entry in the symbol overview.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SymbolEntry {
    pub name: String,
    /// `lsp_types::SymbolKind` — maps 1:1 to the LSP integer enum. Kept
    /// strongly typed for serde at the tool boundary.
    pub kind: lsp_types::SymbolKind,
    pub range: ByteRange,
    #[serde(default)]
    pub children: Vec<SymbolEntry>,
}

/// Result of a `get_symbols_overview` call.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SymbolsOverview {
    pub path: String,
    pub language: String,
    pub symbols: Vec<SymbolEntry>,
}

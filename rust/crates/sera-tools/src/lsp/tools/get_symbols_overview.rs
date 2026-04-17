//! `get_symbols_overview` — top-level symbols of a file or directory.
//!
//! Phase 1 scope — Rust-only via rust-analyzer. See
//! `docs/plan/LSP-TOOLS-DESIGN.md` §3.1.

use serde::{Deserialize, Serialize};

use crate::lsp::error::{LspError, ToolError};
use crate::registry::Tool;

use super::SymbolsOverview;

/// Input schema for `get_symbols_overview`.
///
/// The shape matches `docs/plan/LSP-TOOLS-DESIGN.md` §3.1 exactly.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GetSymbolsOverviewInput {
    /// Relative path to a file or directory within the project root.
    pub path: String,
    /// How deep into child symbols to descend. 0 = top-level only.
    #[serde(default)]
    pub depth: u8,
}

/// Tool implementation — Phase 1 provides the `name`/`description` surface
/// required by today's `Tool` trait; the LSP-backed `invoke` method is
/// exposed as an inherent method until the full `Tool` trait (per
/// `SPEC-tools.md` §3.1) is wired up by a follow-up bead.
pub struct GetSymbolsOverviewTool;

impl GetSymbolsOverviewTool {
    pub const NAME: &'static str = "get_symbols_overview";
    pub const DESCRIPTION: &'static str =
        "Return top-level symbols in a file (names, kinds, byte ranges) \
         without reading file bodies. Backed by an LSP server.";

    pub fn new() -> Self {
        Self
    }
}

impl Default for GetSymbolsOverviewTool {
    fn default() -> Self {
        Self::new()
    }
}

impl Tool for GetSymbolsOverviewTool {
    fn name(&self) -> &str {
        Self::NAME
    }

    fn description(&self) -> &str {
        Self::DESCRIPTION
    }
}

impl GetSymbolsOverviewTool {
    /// Placeholder invocation surface. Phase 1 returns `Unsupported` because
    /// wiring supervisors + cache end-to-end is scoped to the integration
    /// test behind the `integration` feature flag. The real invocation path
    /// lands in `sera-lsp-phase1b` (follow-up bead).
    pub async fn invoke(
        &self,
        _input: GetSymbolsOverviewInput,
        _state: &crate::lsp::state::LspToolsState,
    ) -> Result<SymbolsOverview, ToolError> {
        Err(LspError::Unsupported {
            language: "get_symbols_overview invoke (phase 1b — wiring bead)".into(),
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tool_trait_surface_is_stable() {
        let t = GetSymbolsOverviewTool::new();
        assert_eq!(t.name(), "get_symbols_overview");
        assert!(!t.description().is_empty());
    }

    #[test]
    fn input_roundtrips_through_serde() {
        let json = serde_json::json!({
            "path": "src/lib.rs",
            "depth": 1
        });
        let input: GetSymbolsOverviewInput = serde_json::from_value(json).unwrap();
        assert_eq!(input.path, "src/lib.rs");
        assert_eq!(input.depth, 1);
    }

    #[test]
    fn input_depth_defaults_to_zero() {
        let json = serde_json::json!({"path": "src/lib.rs"});
        let input: GetSymbolsOverviewInput = serde_json::from_value(json).unwrap();
        assert_eq!(input.depth, 0);
    }
}

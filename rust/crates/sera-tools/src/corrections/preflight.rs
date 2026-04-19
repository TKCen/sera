//! Preflight trait consulted before a tool executes.

use super::catalog::CorrectionCatalog;
use super::types::ToolCorrection;

/// Preflight contract. Implementors consult a catalog (or any other source of
/// anti-pattern rules) and either permit the call (`Ok(())`) or veto it with
/// a [`ToolCorrection`].
pub trait ToolPreflight: Send + Sync {
    /// Check a prospective tool invocation.
    ///
    /// `invocation_text` is the tool-specific string to match against — for
    /// bash tools it is the command string, for generic tools it is the
    /// JSON-serialized args. Keeping this textual (rather than structured)
    /// lets a single rule engine serve every tool.
    fn check_invocation(
        &self,
        tool_name: &str,
        invocation_text: &str,
    ) -> Result<(), ToolCorrection>;
}

/// Default implementation backed by a [`CorrectionCatalog`]. Cheap to clone.
#[derive(Clone)]
pub struct DefaultPreflight {
    catalog: CorrectionCatalog,
}

impl DefaultPreflight {
    pub fn new(catalog: CorrectionCatalog) -> Self {
        Self { catalog }
    }

    pub fn catalog(&self) -> &CorrectionCatalog {
        &self.catalog
    }
}

impl ToolPreflight for DefaultPreflight {
    fn check_invocation(
        &self,
        tool_name: &str,
        invocation_text: &str,
    ) -> Result<(), ToolCorrection> {
        match self.catalog.check(tool_name, invocation_text) {
            Some(correction) => Err(correction),
            None => Ok(()),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::corrections::types::{CorrectionFile, CorrectionRule};
    use tempfile::TempDir;

    fn seed_tool(dir: &std::path::Path, tool: &str, rule: CorrectionRule) {
        let d = dir.join(tool).join("active");
        std::fs::create_dir_all(&d).unwrap();
        let file = CorrectionFile { rules: vec![rule] };
        std::fs::write(
            d.join("corrections.yaml"),
            serde_yaml::to_string(&file).unwrap(),
        )
        .unwrap();
    }

    #[test]
    fn default_preflight_passes_unmatched_calls() {
        let dir = TempDir::new().unwrap();
        seed_tool(
            dir.path(),
            "bash",
            CorrectionRule::new("r", r"rm\s+-rf\s+/$", "don't", "seed"),
        );
        let cat = CorrectionCatalog::load(dir.path()).unwrap();
        let pf = DefaultPreflight::new(cat);
        assert!(pf.check_invocation("bash", "ls -la").is_ok());
    }

    #[test]
    fn default_preflight_blocks_matched_calls() {
        let dir = TempDir::new().unwrap();
        seed_tool(
            dir.path(),
            "bash",
            CorrectionRule::new("sleep-chain", r"sleep\s+\d+\s*&&", "use until", "seed"),
        );
        let cat = CorrectionCatalog::load(dir.path()).unwrap();
        let pf = DefaultPreflight::new(cat);
        let err = pf
            .check_invocation("bash", "sleep 30 && gh pr checks")
            .unwrap_err();
        assert!(err.is_blocked());
    }
}

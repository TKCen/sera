//! Bridges the sera-tools [`CorrectionCatalog`] into the runtime's tool hook
//! chain.
//!
//! When a [`ToolCorrection::Blocked`] fires, the hook aborts the dispatch —
//! the [`RegistryDispatcher`] then surfaces the correction text as a
//! [`ToolError::AbortedByHook`] with the rendered message, which the
//! runtime turn loop feeds back to the model as a tool result.
//!
//! [`CorrectionCatalog`]: sera_tools::corrections::CorrectionCatalog
//! [`ToolCorrection::Blocked`]: sera_tools::corrections::ToolCorrection
//! [`RegistryDispatcher`]: crate::tools::dispatcher::RegistryDispatcher
//! [`ToolError::AbortedByHook`]: crate::turn::ToolError::AbortedByHook

use async_trait::async_trait;
use sera_tools::corrections::{DefaultPreflight, ToolPreflight};

use crate::tool_hooks::{ToolCallCtx, ToolHook, ToolHookOutcome};

/// Tool hook that preflights every invocation through a correction catalog.
///
/// The catalog is keyed by tool name — `shell-exec` calls look up rules under
/// `bash/`, `http-request` under `http/`, etc. The mapping keeps the catalog
/// directory layout aligned with the skill's conceptual tool classes rather
/// than the runtime's exact tool names.
pub struct CorrectionHook {
    preflight: DefaultPreflight,
}

impl CorrectionHook {
    pub fn new(preflight: DefaultPreflight) -> Self {
        Self { preflight }
    }

    /// Map a runtime tool name onto the catalog key for rule lookup.
    ///
    /// Runtime tool names (`shell-exec`) differ from the skill's tool classes
    /// (`bash`). The mapping lives here so the catalog stays portable across
    /// tool-name refactors.
    fn catalog_key(tool_name: &str) -> &str {
        match tool_name {
            "shell-exec" => "bash",
            "http-request" | "web-fetch" => "http",
            "file-write" | "file-edit" => "file",
            other => other,
        }
    }

    /// Build the invocation string the catalog matches against.
    ///
    /// - For bash-shaped tools, match on the raw command so regexes like
    ///   `sleep\s+\d+\s*&&` behave intuitively.
    /// - Everything else gets its JSON args serialized; patterns can match
    ///   any field the model passed in.
    fn invocation_text(tool_name: &str, args: &serde_json::Value) -> String {
        if tool_name == "shell-exec"
            && let Some(cmd) = args.get("command").and_then(|v| v.as_str())
        {
            return cmd.to_string();
        }
        args.to_string()
    }
}

#[async_trait]
impl ToolHook for CorrectionHook {
    fn id(&self) -> &str {
        "correction-catalog"
    }

    async fn pre(&self, ctx: &ToolCallCtx<'_>) -> ToolHookOutcome {
        let key = Self::catalog_key(&ctx.input.name);
        let text = Self::invocation_text(&ctx.input.name, &ctx.input.arguments);
        match self.preflight.check_invocation(key, &text) {
            Ok(()) => ToolHookOutcome::Continue,
            Err(correction) if correction.is_blocked() => {
                ToolHookOutcome::Abort(correction.render())
            }
            Err(warning) => {
                tracing::warn!(
                    tool = %ctx.input.name,
                    warning = %warning.render(),
                    "correction warning (non-fatal)"
                );
                ToolHookOutcome::Continue
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use sera_tools::corrections::{
        CorrectionCatalog, CorrectionFile, CorrectionRule, CorrectionSeverity, DefaultPreflight,
        MatchKind,
    };
    use sera_types::tool::{ToolContext, ToolInput};
    use tempfile::TempDir;

    fn make_input(name: &str, args: serde_json::Value) -> ToolInput {
        ToolInput {
            name: name.to_string(),
            arguments: args,
            call_id: "call-1".to_string(),
        }
    }

    #[tokio::test]
    async fn blocked_rule_aborts_with_correction_text() {
        let dir = TempDir::new().unwrap();
        let d = dir.path().join("bash").join("active");
        std::fs::create_dir_all(&d).unwrap();
        std::fs::write(
            d.join("corrections.yaml"),
            r#"rules:
  - id: sleep-chain
    pattern: "sleep\\s+\\d+\\s*&&"
    matches: regex
    severity: block
    antipattern: "sleep N && cmd"
    correction: "Use until-loop"
    added_by: test
"#,
        )
        .unwrap();

        let cat = CorrectionCatalog::load(dir.path()).unwrap();
        let hook = CorrectionHook::new(DefaultPreflight::new(cat));

        let input = make_input(
            "shell-exec",
            serde_json::json!({"command": "sleep 30 && gh pr checks"}),
        );
        let ctx = ToolContext::default();
        let call_ctx = ToolCallCtx::new(&input, &ctx);
        match hook.pre(&call_ctx).await {
            ToolHookOutcome::Abort(reason) => {
                assert!(reason.contains("until-loop"));
                assert!(reason.contains("Blocked"));
            }
            other => panic!("expected Abort, got {other:?}"),
        }
    }

    #[tokio::test]
    async fn warn_rule_continues_without_aborting() {
        let dir = TempDir::new().unwrap();
        let mut rule = CorrectionRule::new(
            "plain-http",
            "http://",
            "prefer https",
            "seed",
        );
        rule.matches = MatchKind::Substring;
        rule.severity = CorrectionSeverity::Warn;
        let d = dir.path().join("http").join("active");
        std::fs::create_dir_all(&d).unwrap();
        std::fs::write(
            d.join("corrections.yaml"),
            serde_yaml::to_string(&CorrectionFile { rules: vec![rule] }).unwrap(),
        )
        .unwrap();

        let cat = CorrectionCatalog::load(dir.path()).unwrap();
        let hook = CorrectionHook::new(DefaultPreflight::new(cat));

        let input = make_input(
            "http-request",
            serde_json::json!({"url": "http://example.com"}),
        );
        let ctx = ToolContext::default();
        let call_ctx = ToolCallCtx::new(&input, &ctx);
        assert!(matches!(
            hook.pre(&call_ctx).await,
            ToolHookOutcome::Continue
        ));
    }

    #[tokio::test]
    async fn unmapped_tool_is_not_blocked() {
        // No catalog entry for `echo-tool` → hook returns Continue.
        let dir = TempDir::new().unwrap();
        let cat = CorrectionCatalog::load(dir.path()).unwrap();
        let hook = CorrectionHook::new(DefaultPreflight::new(cat));
        let input = make_input("echo-tool", serde_json::json!({"msg": "hi"}));
        let ctx = ToolContext::default();
        let call_ctx = ToolCallCtx::new(&input, &ctx);
        assert!(matches!(
            hook.pre(&call_ctx).await,
            ToolHookOutcome::Continue
        ));
    }

}

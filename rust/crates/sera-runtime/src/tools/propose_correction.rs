//! Meta-tool: the agent proposes a new correction rule.
//!
//! Writes the rule to `<root>/<tool>/proposed/<id>.yaml`. It is NOT enforced
//! until an admin promotes it — see [`sera_tools::corrections::CorrectionCatalog::approve`].
//! A future skill can auto-promote rules that fire N times without triggering
//! any complaint.

use std::collections::HashMap;
use std::sync::Arc;

use async_trait::async_trait;
use sera_tools::corrections::{
    CorrectionCatalog, CorrectionRule, CorrectionSeverity, MatchKind,
};
use sera_types::tool::{
    ExecutionTarget, FunctionParameters, ParameterSchema, RiskLevel, Tool, ToolContext, ToolError,
    ToolInput, ToolMetadata, ToolOutput, ToolSchema,
};

pub struct ProposeCorrection {
    catalog: Arc<CorrectionCatalog>,
}

impl ProposeCorrection {
    pub fn new(catalog: Arc<CorrectionCatalog>) -> Self {
        Self { catalog }
    }
}

#[async_trait]
impl Tool for ProposeCorrection {
    fn metadata(&self) -> ToolMetadata {
        ToolMetadata {
            name: "propose-correction".to_string(),
            description:
                "Propose a new tool-layer correction rule that blocks or warns on a specific \
                 invocation pattern. The proposed rule is written to disk for admin review — it \
                 does not take effect until approved."
                    .to_string(),
            version: "1.0.0".to_string(),
            author: None,
            risk_level: RiskLevel::Write,
            execution_target: ExecutionTarget::InProcess,
            tags: vec!["meta".to_string(), "correction".to_string()],
        }
    }

    fn schema(&self) -> ToolSchema {
        let mut properties: HashMap<String, ParameterSchema> = HashMap::new();
        properties.insert(
            "tool_name".to_string(),
            ParameterSchema {
                schema_type: "string".to_string(),
                description: Some(
                    "Catalog key the rule applies to (e.g. 'bash', 'http', 'file')."
                        .to_string(),
                ),
                enum_values: None,
                default: None,
            },
        );
        properties.insert(
            "id".to_string(),
            ParameterSchema {
                schema_type: "string".to_string(),
                description: Some(
                    "Stable kebab-case identifier for the rule (e.g. 'sleep-chain-polling').".to_string(),
                ),
                enum_values: None,
                default: None,
            },
        );
        properties.insert(
            "pattern".to_string(),
            ParameterSchema {
                schema_type: "string".to_string(),
                description: Some(
                    "Pattern to match. Interpreted per `match_kind` (regex by default)."
                        .to_string(),
                ),
                enum_values: None,
                default: None,
            },
        );
        properties.insert(
            "correction".to_string(),
            ParameterSchema {
                schema_type: "string".to_string(),
                description: Some(
                    "The corrective text surfaced to the model when the rule fires. \
                     Name the better alternative explicitly."
                        .to_string(),
                ),
                enum_values: None,
                default: None,
            },
        );
        properties.insert(
            "reason".to_string(),
            ParameterSchema {
                schema_type: "string".to_string(),
                description: Some(
                    "Why this rule belongs in the catalog (for the reviewer).".to_string(),
                ),
                enum_values: None,
                default: None,
            },
        );
        properties.insert(
            "match_kind".to_string(),
            ParameterSchema {
                schema_type: "string".to_string(),
                description: Some(
                    "How `pattern` is interpreted: 'regex', 'substring', or 'exact'. \
                     Defaults to 'regex'."
                        .to_string(),
                ),
                enum_values: Some(vec![
                    "regex".to_string(),
                    "substring".to_string(),
                    "exact".to_string(),
                ]),
                default: None,
            },
        );
        properties.insert(
            "severity".to_string(),
            ParameterSchema {
                schema_type: "string".to_string(),
                description: Some(
                    "'block' cancels the call, 'warn' annotates without blocking. \
                     Defaults to 'block'."
                        .to_string(),
                ),
                enum_values: Some(vec!["block".to_string(), "warn".to_string()]),
                default: None,
            },
        );
        properties.insert(
            "antipattern".to_string(),
            ParameterSchema {
                schema_type: "string".to_string(),
                description: Some(
                    "Short human-readable name of the anti-pattern. Optional; falls back to id."
                        .to_string(),
                ),
                enum_values: None,
                default: None,
            },
        );

        ToolSchema {
            parameters: FunctionParameters {
                schema_type: "object".to_string(),
                properties,
                required: vec![
                    "tool_name".to_string(),
                    "id".to_string(),
                    "pattern".to_string(),
                    "correction".to_string(),
                    "reason".to_string(),
                ],
            },
        }
    }

    async fn execute(
        &self,
        input: ToolInput,
        ctx: ToolContext,
    ) -> Result<ToolOutput, ToolError> {
        let args = &input.arguments;
        let tool_name = required_str(args, "tool_name")?;
        let id = required_str(args, "id")?;
        let pattern = required_str(args, "pattern")?;
        let correction = required_str(args, "correction")?;
        let reason = required_str(args, "reason")?;
        if reason.trim().is_empty() {
            return Err(ToolError::InvalidInput(
                "`reason` must explain why this rule belongs in the catalog".to_string(),
            ));
        }

        let match_kind = match args.get("match_kind").and_then(|v| v.as_str()) {
            Some("substring") => MatchKind::Substring,
            Some("exact") => MatchKind::Exact,
            Some("regex") | None => MatchKind::Regex,
            Some(other) => {
                return Err(ToolError::InvalidInput(format!(
                    "unknown match_kind '{other}'"
                )));
            }
        };
        let severity = match args.get("severity").and_then(|v| v.as_str()) {
            Some("warn") => CorrectionSeverity::Warn,
            Some("block") | None => CorrectionSeverity::Block,
            Some(other) => {
                return Err(ToolError::InvalidInput(format!(
                    "unknown severity '{other}'"
                )));
            }
        };

        // Sanity-compile a regex pattern so we refuse obviously broken rules
        // before they ever reach review.
        if matches!(match_kind, MatchKind::Regex)
            && let Err(e) = regex::Regex::new(pattern) {
                return Err(ToolError::InvalidInput(format!(
                    "invalid regex pattern: {e}"
                )));
            }

        let antipattern = args
            .get("antipattern")
            .and_then(|v| v.as_str())
            .unwrap_or(id)
            .to_string();

        let mut rule = CorrectionRule::new(id, pattern, correction, principal_label(&ctx));
        rule.antipattern = antipattern;
        rule.matches = match_kind;
        rule.severity = severity;

        let path = self
            .catalog
            .propose(tool_name, rule)
            .map_err(|e| ToolError::ExecutionFailed(format!("propose: {e}")))?;

        Ok(ToolOutput::success(format!(
            "Proposed correction rule '{id}' for tool '{tool_name}'.\n\
             Reason: {reason}\n\
             Written to: {}\n\
             This rule is NOT yet enforced — an admin must approve it.",
            path.display()
        )))
    }
}

fn required_str<'a>(args: &'a serde_json::Value, name: &str) -> Result<&'a str, ToolError> {
    args.get(name)
        .and_then(|v| v.as_str())
        .filter(|s| !s.is_empty())
        .ok_or_else(|| ToolError::InvalidInput(format!("missing required field '{name}'")))
}

fn principal_label(ctx: &ToolContext) -> String {
    format!("agent:{}", ctx.principal.id.0)
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    fn make_ctx() -> ToolContext {
        ToolContext::default()
    }

    #[tokio::test]
    async fn valid_proposal_is_written_to_proposed_dir() {
        let dir = TempDir::new().unwrap();
        let cat = Arc::new(CorrectionCatalog::load(dir.path()).unwrap());
        let tool = ProposeCorrection::new(cat.clone());

        let input = ToolInput {
            name: "propose-correction".to_string(),
            arguments: serde_json::json!({
                "tool_name": "bash",
                "id": "my-rule",
                "pattern": r"echo\s+secret",
                "correction": "never echo secrets",
                "reason": "secrets in logs",
            }),
            call_id: "c1".to_string(),
        };
        let out = tool.execute(input, make_ctx()).await.unwrap();
        assert!(!out.is_error);
        assert!(out.content.contains("Proposed"));
        // Proposed rules are not enforced.
        assert!(cat.check("bash", "echo secret").is_none());
        // But the file landed in proposed/.
        let proposed = dir.path().join("bash").join("proposed").join("my-rule.yaml");
        assert!(proposed.exists());
    }

    #[tokio::test]
    async fn invalid_regex_is_rejected_before_write() {
        let dir = TempDir::new().unwrap();
        let cat = Arc::new(CorrectionCatalog::load(dir.path()).unwrap());
        let tool = ProposeCorrection::new(cat.clone());

        let input = ToolInput {
            name: "propose-correction".to_string(),
            arguments: serde_json::json!({
                "tool_name": "bash",
                "id": "bad-re",
                "pattern": "[unterminated",
                "correction": "fix",
                "reason": "...",
            }),
            call_id: "c2".to_string(),
        };
        let err = tool.execute(input, make_ctx()).await.unwrap_err();
        assert!(matches!(err, ToolError::InvalidInput(_)));
        assert!(
            !dir.path().join("bash").join("proposed").join("bad-re.yaml").exists(),
            "no file should be written when pattern is invalid"
        );
    }

    #[tokio::test]
    async fn missing_reason_is_rejected() {
        let dir = TempDir::new().unwrap();
        let cat = Arc::new(CorrectionCatalog::load(dir.path()).unwrap());
        let tool = ProposeCorrection::new(cat);

        let input = ToolInput {
            name: "propose-correction".to_string(),
            arguments: serde_json::json!({
                "tool_name": "bash",
                "id": "r",
                "pattern": "foo",
                "correction": "bar",
            }),
            call_id: "c3".to_string(),
        };
        let err = tool.execute(input, make_ctx()).await.unwrap_err();
        assert!(matches!(err, ToolError::InvalidInput(_)));
    }

    #[tokio::test]
    async fn unknown_severity_is_rejected() {
        let dir = TempDir::new().unwrap();
        let cat = Arc::new(CorrectionCatalog::load(dir.path()).unwrap());
        let tool = ProposeCorrection::new(cat);

        let input = ToolInput {
            name: "propose-correction".to_string(),
            arguments: serde_json::json!({
                "tool_name": "bash",
                "id": "r",
                "pattern": "foo",
                "correction": "bar",
                "reason": "why",
                "severity": "delete-production",
            }),
            call_id: "c4".to_string(),
        };
        let err = tool.execute(input, make_ctx()).await.unwrap_err();
        assert!(matches!(err, ToolError::InvalidInput(_)));
    }
}

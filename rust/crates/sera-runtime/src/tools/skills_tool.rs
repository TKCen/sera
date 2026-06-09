//! Hermes-parity resident skill tools (SPEC-tools §4.2).
//!
//! Two read-only tools that expose the local skills directory to the agent:
//! - [`SkillsList`] — enumerate available skills (`skills_list`)
//! - [`SkillView`]  — view a single skill's body and companion files (`skill_view`)
//!
//! Both tools share a `skills_dir: PathBuf` injected at construction time via
//! [`crate::tools::TraitToolRegistry::with_skills`].

use std::collections::HashMap;
use std::path::PathBuf;

use async_trait::async_trait;
use sera_skills::discovery::discover_skills;
use sera_types::tool::{
    ExecutionTarget, FunctionParameters, ParameterSchema, RiskLevel, Tool, ToolContext, ToolError,
    ToolInput, ToolMetadata, ToolOutput, ToolSchema, ToolScope,
};

// ── Shared discovery ─────────────────────────────────────────────────────────

/// Subdirectories under a Hermes-layout skill directory that may contain
/// companion files surfaced by [`SkillView`].
const COMPANION_SUBDIRS: &[&str] = &["references", "scripts", "templates", "assets"];

// ── SkillsList ────────────────────────────────────────────────────────────────

/// Lists all skills available in the configured skills directory.
///
/// Hermes-parity resident tool — SPEC-tools §4.2. Read-only: scans the local
/// filesystem only; no network calls.
pub struct SkillsList {
    pub(crate) skills_dir: PathBuf,
}

#[async_trait]
impl Tool for SkillsList {
    fn metadata(&self) -> ToolMetadata {
        ToolMetadata {
            name: "skills_list".to_string(),
            description: "List all available skills in the agent's skills directory".to_string(),
            version: "1.0.0".to_string(),
            author: None,
            risk_level: RiskLevel::Read,
            execution_target: ExecutionTarget::InProcess,
            tags: vec!["skills".to_string(), "discovery".to_string()],
            scope: ToolScope::Read,
        }
    }

    fn schema(&self) -> ToolSchema {
        ToolSchema {
            parameters: FunctionParameters {
                schema_type: "object".to_string(),
                properties: HashMap::new(),
                required: vec![],
            },
        }
    }

    async fn execute(
        &self,
        _input: ToolInput,
        _ctx: ToolContext,
    ) -> Result<ToolOutput, ToolError> {
        let skills = discover_skills(&self.skills_dir).await;

        if skills.is_empty() {
            return Ok(ToolOutput::success(
                "No skills available.".to_string(),
            ));
        }

        let lines: Vec<String> = skills
            .iter()
            .map(|(skill, _)| format!("{} — {}", skill.name, skill.description))
            .collect();

        Ok(ToolOutput::success(lines.join("\n")))
    }
}

// ── SkillView ─────────────────────────────────────────────────────────────────

/// View the full definition of a single skill by name.
///
/// Returns the skill's header (name, description, optional version), its
/// markdown body, and — for Hermes-layout skills in their own subdirectory —
/// a `Companion files:` section listing files under `references/`, `scripts/`,
/// `templates/`, and `assets/`.
///
/// Hermes-parity resident tool — SPEC-tools §4.2.
pub struct SkillView {
    pub(crate) skills_dir: PathBuf,
}

#[async_trait]
impl Tool for SkillView {
    fn metadata(&self) -> ToolMetadata {
        ToolMetadata {
            name: "skill_view".to_string(),
            description: "View the full definition and companion files of a skill by name"
                .to_string(),
            version: "1.0.0".to_string(),
            author: None,
            risk_level: RiskLevel::Read,
            execution_target: ExecutionTarget::InProcess,
            tags: vec!["skills".to_string(), "discovery".to_string()],
            scope: ToolScope::Read,
        }
    }

    fn schema(&self) -> ToolSchema {
        let mut properties = HashMap::new();
        properties.insert(
            "name".to_string(),
            ParameterSchema {
                schema_type: "string".to_string(),
                description: Some("Name of the skill to view".to_string()),
                enum_values: None,
                default: None,
            },
        );
        ToolSchema {
            parameters: FunctionParameters {
                schema_type: "object".to_string(),
                properties,
                required: vec!["name".to_string()],
            },
        }
    }

    async fn execute(
        &self,
        input: ToolInput,
        _ctx: ToolContext,
    ) -> Result<ToolOutput, ToolError> {
        let requested_name = input
            .arguments
            .get("name")
            .and_then(|v| v.as_str())
            .ok_or_else(|| ToolError::InvalidInput("Missing 'name' parameter".to_string()))?;

        let skills = discover_skills(&self.skills_dir).await;

        // Single scan: build the available-name list up front, then find by
        // name. Avoids re-scanning the directory on the not-found path.
        let available = skills
            .iter()
            .map(|(s, _)| s.name.clone())
            .collect::<Vec<_>>()
            .join(", ");

        // Find the skill by name.
        let (skill, skill_path) = match skills.into_iter().find(|(s, _)| s.name == requested_name) {
            Some(found) => found,
            None => {
                let msg = if available.is_empty() {
                    format!("Skill '{requested_name}' not found. No skills are available.")
                } else {
                    format!(
                        "Skill '{requested_name}' not found. Available: {available}"
                    )
                };
                return Err(ToolError::InvalidInput(msg));
            }
        };

        // Build header.
        let mut output = format!("# {}\n", skill.name);
        output.push_str(&format!("Description: {}\n", skill.description));
        if let Some(ref ver) = skill.version {
            output.push_str(&format!("Version: {ver}\n"));
        }
        output.push('\n');
        output.push_str(&skill.body);

        // Companion files section — only for Hermes layout (skill file is SKILL.md
        // inside its own subdirectory, not a flat *.md in skills_dir).
        let skill_dir = skill_path.parent();
        if let Some(skill_dir) = skill_dir
            && skill_dir != self.skills_dir
        {
            let mut companion_lines: Vec<String> = Vec::new();
            for subdir_name in COMPANION_SUBDIRS {
                let subdir = skill_dir.join(subdir_name);
                if let Ok(mut rd) = tokio::fs::read_dir(&subdir).await {
                    while let Ok(Some(entry)) = rd.next_entry().await {
                        if let Ok(ft) = entry.file_type().await
                            && ft.is_file()
                        {
                            // Relative path: <subdir>/<filename>
                            companion_lines.push(format!(
                                "{subdir_name}/{}",
                                entry.file_name().to_string_lossy()
                            ));
                        }
                    }
                }
            }
            if !companion_lines.is_empty() {
                companion_lines.sort();
                output.push_str("\nCompanion files:\n");
                for line in companion_lines {
                    output.push_str(&format!("  {line}\n"));
                }
            }
        }

        Ok(ToolOutput::success(output))
    }
}

// ── Tests ─────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use sera_types::tool::{ToolContext, ToolInput};
    use std::path::Path;

    fn make_input(name: &str, args: serde_json::Value) -> ToolInput {
        ToolInput {
            name: name.to_string(),
            arguments: args,
            call_id: "test-call".to_string(),
        }
    }

    async fn write_skill_md(path: &Path, name: &str, description: &str, body: &str) {
        let content = format!(
            "---\nname: {name}\ndescription: {description}\n---\n{body}"
        );
        tokio::fs::write(path, content).await.unwrap();
    }

    // ── discovery ────────────────────────────────────────────────────────────

    /// Discovery finds both a Hermes-layout skill (subdir/SKILL.md) and a flat
    /// *.md in the skills directory.
    #[tokio::test]
    async fn discovery_finds_hermes_and_flat_skills() {
        let dir = tempfile::tempdir().unwrap();
        let skills_dir = dir.path().to_path_buf();

        // Hermes layout: research-helper/SKILL.md
        let hermes_dir = skills_dir.join("research-helper");
        tokio::fs::create_dir(&hermes_dir).await.unwrap();
        write_skill_md(
            &hermes_dir.join("SKILL.md"),
            "research-helper",
            "Helps with research tasks",
            "Use this skill to do research.\n",
        )
        .await;

        // Flat layout: helper.md
        write_skill_md(
            &skills_dir.join("helper.md"),
            "helper",
            "General helper skill",
            "A helpful skill.\n",
        )
        .await;

        let skills = discover_skills(&skills_dir).await;
        let names: Vec<&str> = skills.iter().map(|(s, _)| s.name.as_str()).collect();
        assert!(names.contains(&"research-helper"), "missing research-helper");
        assert!(names.contains(&"helper"), "missing helper");
    }

    /// A depth-2 nested skill (`group/research/SKILL.md`) is listed by
    /// skills_list and viewable by skill_view.
    #[tokio::test]
    async fn depth_two_skill_is_listed_and_viewable() {
        let dir = tempfile::tempdir().unwrap();
        let skills_dir = dir.path().to_path_buf();

        let nested = skills_dir.join("group").join("research");
        tokio::fs::create_dir_all(&nested).await.unwrap();
        write_skill_md(
            &nested.join("SKILL.md"),
            "research",
            "Nested research skill",
            "Do nested research.\n",
        )
        .await;

        let list = SkillsList {
            skills_dir: skills_dir.clone(),
        };
        let list_out = list
            .execute(
                make_input("skills_list", serde_json::json!({})),
                ToolContext::default(),
            )
            .await
            .unwrap();
        assert!(
            list_out.content.contains("research"),
            "skills_list missing depth-2 skill: {}",
            list_out.content
        );

        let view = SkillView { skills_dir };
        let view_out = view
            .execute(
                make_input("skill_view", serde_json::json!({"name": "research"})),
                ToolContext::default(),
            )
            .await
            .unwrap();
        assert!(!view_out.is_error, "skill_view errored: {}", view_out.content);
        assert!(
            view_out.content.contains("Do nested research"),
            "skill_view missing body: {}",
            view_out.content
        );
    }

    /// A name collision (flat `foo.md` and `foo/SKILL.md` both named `foo`)
    /// is deduped: skills_list shows exactly one entry per name.
    #[tokio::test]
    async fn skills_list_dedups_name_collisions() {
        let dir = tempfile::tempdir().unwrap();
        let skills_dir = dir.path().to_path_buf();

        write_skill_md(&skills_dir.join("foo.md"), "foo", "flat foo", "").await;
        let foo_dir = skills_dir.join("foo");
        tokio::fs::create_dir(&foo_dir).await.unwrap();
        write_skill_md(&foo_dir.join("SKILL.md"), "foo", "hermes foo", "").await;

        let list = SkillsList { skills_dir };
        let out = list
            .execute(
                make_input("skills_list", serde_json::json!({})),
                ToolContext::default(),
            )
            .await
            .unwrap();

        let foo_lines = out
            .content
            .lines()
            .filter(|l| l.starts_with("foo "))
            .count();
        assert_eq!(
            foo_lines, 1,
            "collision should yield exactly one 'foo' line: {}",
            out.content
        );
    }

    // ── SkillsList ────────────────────────────────────────────────────────────

    /// skills_list output contains both skill names and descriptions.
    #[tokio::test]
    async fn skills_list_output_contains_names_and_descriptions() {
        let dir = tempfile::tempdir().unwrap();
        let skills_dir = dir.path().to_path_buf();

        let hermes_dir = skills_dir.join("research-helper");
        tokio::fs::create_dir(&hermes_dir).await.unwrap();
        write_skill_md(
            &hermes_dir.join("SKILL.md"),
            "research-helper",
            "Helps with research tasks",
            "",
        )
        .await;

        write_skill_md(
            &skills_dir.join("helper.md"),
            "helper",
            "General helper skill",
            "",
        )
        .await;

        let tool = SkillsList { skills_dir };
        let output = tool
            .execute(
                make_input("skills_list", serde_json::json!({})),
                ToolContext::default(),
            )
            .await
            .unwrap();

        assert!(!output.is_error);
        assert!(
            output.content.contains("research-helper"),
            "output missing 'research-helper': {}",
            output.content
        );
        assert!(
            output.content.contains("Helps with research tasks"),
            "output missing description: {}",
            output.content
        );
        assert!(
            output.content.contains("helper"),
            "output missing 'helper': {}",
            output.content
        );
        assert!(
            output.content.contains("General helper skill"),
            "output missing flat skill description: {}",
            output.content
        );
    }

    /// skills_list returns a success message when the directory is missing or empty.
    #[tokio::test]
    async fn skills_list_empty_dir_returns_no_skills_message() {
        let dir = tempfile::tempdir().unwrap();
        let tool = SkillsList {
            skills_dir: dir.path().to_path_buf(),
        };
        let output = tool
            .execute(
                make_input("skills_list", serde_json::json!({})),
                ToolContext::default(),
            )
            .await
            .unwrap();
        assert!(!output.is_error);
        assert!(
            output.content.contains("No skills"),
            "expected no-skills message, got: {}",
            output.content
        );
    }

    // ── SkillView ─────────────────────────────────────────────────────────────

    /// skill_view returns the body and lists companion files.
    #[tokio::test]
    async fn skill_view_returns_body_and_companion_files() {
        let dir = tempfile::tempdir().unwrap();
        let skills_dir = dir.path().to_path_buf();

        let skill_dir = skills_dir.join("research-helper");
        tokio::fs::create_dir(&skill_dir).await.unwrap();
        write_skill_md(
            &skill_dir.join("SKILL.md"),
            "research-helper",
            "Helps with research tasks",
            "Use this skill to do research.\n",
        )
        .await;

        // Add a companion file under references/
        let refs_dir = skill_dir.join("references");
        tokio::fs::create_dir(&refs_dir).await.unwrap();
        tokio::fs::write(refs_dir.join("guide.md"), "# Reference Guide\n")
            .await
            .unwrap();

        let tool = SkillView { skills_dir };
        let output = tool
            .execute(
                make_input("skill_view", serde_json::json!({"name": "research-helper"})),
                ToolContext::default(),
            )
            .await
            .unwrap();

        assert!(!output.is_error, "expected Ok, got error: {}", output.content);
        assert!(
            output.content.contains("Use this skill to do research"),
            "body missing: {}",
            output.content
        );
        assert!(
            output.content.contains("references/guide.md"),
            "companion file missing: {}",
            output.content
        );
    }

    /// skill_view with an unknown name returns InvalidInput error listing available names.
    #[tokio::test]
    async fn skill_view_unknown_name_errors_with_available() {
        let dir = tempfile::tempdir().unwrap();
        let skills_dir = dir.path().to_path_buf();

        write_skill_md(
            &skills_dir.join("helper.md"),
            "helper",
            "General helper skill",
            "",
        )
        .await;

        let tool = SkillView { skills_dir };
        let err = tool
            .execute(
                make_input("skill_view", serde_json::json!({"name": "nonexistent"})),
                ToolContext::default(),
            )
            .await
            .unwrap_err();

        match err {
            ToolError::InvalidInput(msg) => {
                assert!(
                    msg.contains("nonexistent"),
                    "error should mention requested name: {msg}"
                );
                assert!(
                    msg.contains("helper"),
                    "error should list available names: {msg}"
                );
            }
            other => panic!("expected InvalidInput, got {other:?}"),
        }
    }

    // ── metadata ──────────────────────────────────────────────────────────────

    /// Both tools must declare RiskLevel::Read (read-only filesystem access).
    #[test]
    fn metadata_risk_levels_are_read() {
        let skills_dir = PathBuf::from("/tmp/skills");
        assert_eq!(
            SkillsList { skills_dir: skills_dir.clone() }
                .metadata()
                .risk_level,
            RiskLevel::Read
        );
        assert_eq!(
            SkillView { skills_dir }.metadata().risk_level,
            RiskLevel::Read
        );
    }

    /// Both tools declare ToolScope::Read.
    #[test]
    fn metadata_scopes_are_read() {
        let skills_dir = PathBuf::from("/tmp/skills");
        assert_eq!(
            SkillsList { skills_dir: skills_dir.clone() }.metadata().scope,
            ToolScope::Read
        );
        assert_eq!(
            SkillView { skills_dir }.metadata().scope,
            ToolScope::Read
        );
    }
}

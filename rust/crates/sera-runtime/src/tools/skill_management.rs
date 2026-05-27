//! Agent-visible skill management tools — Phase 1 of the self-improvement loop.
//!
//! Three tools allow agents to discover, inspect, and mutate installed skills:
//!
//! - `skill-list`: progressive-disclosure metadata for all installed skills
//! - `skill-view`: full SKILL.md content for a named skill
//! - `skill-manage`: create new skills or patch existing ones
//!
//! All three share a [`SkillManagementContext`] injected at registry
//! construction time via [`super::TraitToolRegistry::with_skill_management`].

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

use async_trait::async_trait;
use sera_skills::knowledge_activity_log::{KnowledgeActivityEntry, KnowledgeActivityLog, KnowledgeOp};
use sera_skills::parse_skill_markdown_str;
use sera_skills::self_patch::{
    DefaultSelfPatchValidator, FsSelfPatchApplier, PatchKind, PatchPayload, SelfPatchApplier,
    SelfPatchValidator, SkillPatch,
};
use sera_types::tool::{
    ExecutionTarget, FunctionParameters, ParameterSchema, RiskLevel, Tool, ToolContext, ToolError,
    ToolInput, ToolMetadata, ToolOutput, ToolSchema, ToolScope,
};

const MAX_LIST_ENTRIES: usize = 100;
const MAX_SKILL_NAME_LEN: usize = 64;

// ── Shared context ──────────────────────────────────────────────────────────

/// Shared state for the three skill management tools.
pub struct SkillManagementContext {
    /// Ordered skill root paths for reading. First path wins on name collision.
    pub skill_roots: Vec<PathBuf>,
    /// Directory where new skills are written.
    pub write_root: PathBuf,
    /// In-memory activity log recording create/patch operations.
    /// Phase 2 adds persistence to disk/DB.
    pub activity_log: Mutex<KnowledgeActivityLog>,
}

impl SkillManagementContext {
    pub fn new(skill_roots: Vec<PathBuf>) -> Self {
        let write_root = skill_roots
            .first()
            .cloned()
            .unwrap_or_else(|| PathBuf::from("./skills"));
        Self {
            skill_roots,
            write_root,
            activity_log: Mutex::new(KnowledgeActivityLog::default()),
        }
    }

    pub fn with_write_root(mut self, root: PathBuf) -> Self {
        self.write_root = root;
        self
    }
}

// ── Name validation ─────────────────────────────────────────────────────────

/// Validate a skill name: `^[a-z0-9][a-z0-9._-]*$`, max 64 chars.
pub fn is_valid_skill_name(name: &str) -> bool {
    if name.is_empty() || name.len() > MAX_SKILL_NAME_LEN {
        return false;
    }
    let bytes = name.as_bytes();
    if !bytes[0].is_ascii_lowercase() && !bytes[0].is_ascii_digit() {
        return false;
    }
    bytes.iter().all(|&b| {
        b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'.' || b == b'_' || b == b'-'
    })
}

fn safe_skill_path(root: &Path, name: &str) -> Option<PathBuf> {
    if !is_valid_skill_name(name) {
        return None;
    }
    let path = root.join(name);
    if path.starts_with(root) {
        Some(path)
    } else {
        None
    }
}

/// Validate a knowledge filename: must be a plain basename with no path components.
fn is_valid_knowledge_filename(filename: &str) -> bool {
    !filename.is_empty()
        && filename.len() <= 255
        && !filename.contains('/')
        && !filename.contains('\\')
        && !filename.contains('\0')
        && !filename.contains("..")
}

/// Where a skill was found on disk.
enum SkillLocation {
    Directory(PathBuf),
    SingleFile(PathBuf),
}

// ── Skill discovery helpers ─────────────────────────────────────────────────

#[derive(Debug, serde::Serialize)]
struct SkillListEntry {
    name: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    description: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    version: Option<String>,
    source: String,
}

async fn discover_skills(roots: &[PathBuf], query: Option<&str>) -> Vec<SkillListEntry> {
    let mut entries = Vec::new();
    let mut seen_names = std::collections::HashSet::new();

    for root in roots {
        if !root.exists() {
            continue;
        }
        let Ok(mut reader) = tokio::fs::read_dir(root).await else {
            continue;
        };
        while let Ok(Some(entry)) = reader.next_entry().await {
            if entries.len() >= MAX_LIST_ENTRIES {
                break;
            }
            let path = entry.path();
            let Some(raw_name) = path.file_name().and_then(|n| n.to_str()) else {
                continue;
            };

            if raw_name.starts_with('_') || raw_name.starts_with('.') {
                continue;
            }

            let is_candidate = (path.is_dir() && path.join("SKILL.md").exists())
                || (path.is_file() && path.extension().and_then(|e| e.to_str()) == Some("md"));
            if !is_candidate {
                continue;
            }

            // Parse frontmatter to get the authoritative name the runtime
            // loader registers, not the path-derived basename.
            let Some(fm) = extract_skill_frontmatter(&path).await else {
                continue;
            };

            if !seen_names.insert(fm.name.clone()) {
                continue;
            }

            if let Some(q) = query
                && !fm.name.contains(q)
            {
                continue;
            }

            entries.push(SkillListEntry {
                name: fm.name,
                description: fm.description,
                version: fm.version,
                source: root.display().to_string(),
            });
        }
    }
    entries
}

struct SkillFrontmatter {
    name: String,
    description: Option<String>,
    version: Option<String>,
}

/// Parse skill frontmatter using the same parser as `SkillDispatchEngine::load_dir`.
/// Returns `None` if the file is missing, unparseable, or has an invalid name —
/// so `skill-list` only advertises skills the runtime actually loads.
async fn extract_skill_frontmatter(path: &Path) -> Option<SkillFrontmatter> {
    let content_path = if path.is_dir() {
        let skill_md = path.join("SKILL.md");
        if skill_md.exists() { skill_md } else { return None }
    } else {
        path.to_path_buf()
    };

    let content = tokio::fs::read_to_string(&content_path).await.ok()?;
    let parsed = parse_skill_markdown_str(&content, content_path).ok()?;
    Some(SkillFrontmatter {
        name: parsed.config.name,
        description: if parsed.config.description.is_empty() { None } else { Some(parsed.config.description) },
        version: if parsed.config.version.is_empty() { None } else { Some(parsed.config.version) },
    })
}

/// Scan roots for a skill whose frontmatter `name` matches `target_name`.
/// Returns the content file path and the root it was found in. Used as a
/// fallback when path-based lookup fails (basename != frontmatter name).
async fn find_by_frontmatter_name(roots: &[PathBuf], target_name: &str) -> Option<(PathBuf, PathBuf)> {
    for root in roots {
        if !root.exists() {
            continue;
        }
        let Ok(mut reader) = tokio::fs::read_dir(root).await else { continue };
        while let Ok(Some(entry)) = reader.next_entry().await {
            let path = entry.path();
            let content_path = if path.is_dir() {
                let skill_md = path.join("SKILL.md");
                if skill_md.exists() { skill_md } else { continue }
            } else if path.is_file() && path.extension().and_then(|e| e.to_str()) == Some("md") {
                path.clone()
            } else {
                continue;
            };
            if let Ok(content) = tokio::fs::read_to_string(&content_path).await
                && let Ok(parsed) = parse_skill_markdown_str(&content, content_path.clone())
                && parsed.config.name == target_name
            {
                return Some((content_path, root.clone()));
            }
        }
    }
    None
}

fn extract_frontmatter_field(content: &str, field: &str) -> Option<String> {
    if !content.starts_with("---") {
        return None;
    }
    let after_open = &content[3..];
    let close_pos = after_open.find("\n---")?;
    let frontmatter = &after_open[..close_pos];
    let prefix = format!("{field}:");
    for line in frontmatter.lines() {
        let trimmed = line.trim();
        if let Some(rest) = trimmed.strip_prefix(&prefix) {
            let val = rest.trim().trim_matches('"').trim_matches('\'');
            if !val.is_empty() {
                return Some(val.to_string());
            }
        }
    }
    None
}

fn extract_version_from_frontmatter(content: &str) -> Option<String> {
    extract_frontmatter_field(content, "version")
}

// ── SkillList ───────────────────────────────────────────────────────────────

pub struct SkillList {
    ctx: Arc<SkillManagementContext>,
}

impl SkillList {
    pub fn new(ctx: Arc<SkillManagementContext>) -> Self {
        Self { ctx }
    }
}

#[async_trait]
impl Tool for SkillList {
    fn metadata(&self) -> ToolMetadata {
        ToolMetadata {
            name: "skill-list".to_string(),
            description: "List installed skills with name, description, and source metadata"
                .to_string(),
            version: "1.0.0".to_string(),
            author: None,
            risk_level: RiskLevel::Read,
            execution_target: ExecutionTarget::InProcess,
            tags: vec!["skill".to_string(), "discovery".to_string()],
            scope: ToolScope::Read,
        }
    }

    fn schema(&self) -> ToolSchema {
        let mut properties: HashMap<String, ParameterSchema> = HashMap::new();
        properties.insert(
            "query".to_string(),
            ParameterSchema {
                schema_type: "string".to_string(),
                description: Some("Optional name filter".to_string()),
                enum_values: None,
                default: None,
            },
        );
        ToolSchema {
            parameters: FunctionParameters {
                schema_type: "object".to_string(),
                properties,
                required: vec![],
            },
        }
    }

    async fn execute(&self, input: ToolInput, _ctx: ToolContext) -> Result<ToolOutput, ToolError> {
        if self.ctx.skill_roots.is_empty() {
            return Err(ToolError::ExecutionFailed(
                "no skill roots configured".to_string(),
            ));
        }

        let query = input.arguments.get("query").and_then(|v| v.as_str());
        let entries = discover_skills(&self.ctx.skill_roots, query).await;

        let truncated = entries.len() >= MAX_LIST_ENTRIES;
        let output = serde_json::json!({
            "skills": entries,
            "count": entries.len(),
            "truncated": truncated,
        });
        serde_json::to_string_pretty(&output)
            .map(ToolOutput::success)
            .map_err(|e| ToolError::ExecutionFailed(format!("serialize: {e}")))
    }
}

// ── SkillView ───────────────────────────────────────────────────────────────

pub struct SkillView {
    ctx: Arc<SkillManagementContext>,
}

impl SkillView {
    pub fn new(ctx: Arc<SkillManagementContext>) -> Self {
        Self { ctx }
    }
}

#[async_trait]
impl Tool for SkillView {
    fn metadata(&self) -> ToolMetadata {
        ToolMetadata {
            name: "skill-view".to_string(),
            description: "View the full content of an installed skill by name".to_string(),
            version: "1.0.0".to_string(),
            author: None,
            risk_level: RiskLevel::Read,
            execution_target: ExecutionTarget::InProcess,
            tags: vec!["skill".to_string(), "discovery".to_string()],
            scope: ToolScope::Read,
        }
    }

    fn schema(&self) -> ToolSchema {
        let mut properties: HashMap<String, ParameterSchema> = HashMap::new();
        properties.insert(
            "name".to_string(),
            ParameterSchema {
                schema_type: "string".to_string(),
                description: Some("Skill name to view".to_string()),
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

    async fn execute(&self, input: ToolInput, _ctx: ToolContext) -> Result<ToolOutput, ToolError> {
        let name = input.arguments["name"]
            .as_str()
            .ok_or_else(|| ToolError::InvalidInput("missing 'name'".to_string()))?;

        if !is_valid_skill_name(name) {
            return Err(ToolError::InvalidInput(format!(
                "invalid skill name '{name}': must match [a-z0-9][a-z0-9._-]* and be ≤{MAX_SKILL_NAME_LEN} chars"
            )));
        }

        for root in &self.ctx.skill_roots {
            let Some(skill_path) = safe_skill_path(root, name) else {
                continue;
            };

            // Directory-style: <root>/<name>/SKILL.md
            let skill_md = skill_path.join("SKILL.md");
            if skill_md.is_file() {
                let content = tokio::fs::read_to_string(&skill_md)
                    .await
                    .map_err(|e| ToolError::ExecutionFailed(format!("read SKILL.md: {e}")))?;
                let output = serde_json::json!({
                    "name": name,
                    "source": root.display().to_string(),
                    "content": content,
                });
                return serde_json::to_string_pretty(&output)
                    .map(ToolOutput::success)
                    .map_err(|e| ToolError::ExecutionFailed(format!("serialize: {e}")));
            }

            // Single-file: <root>/<name>.md
            let md_file = root.join(format!("{name}.md"));
            if md_file.is_file() {
                let content = tokio::fs::read_to_string(&md_file)
                    .await
                    .map_err(|e| ToolError::ExecutionFailed(format!("read {name}.md: {e}")))?;
                let output = serde_json::json!({
                    "name": name,
                    "source": root.display().to_string(),
                    "content": content,
                });
                return serde_json::to_string_pretty(&output)
                    .map(ToolOutput::success)
                    .map_err(|e| ToolError::ExecutionFailed(format!("serialize: {e}")));
            }
        }

        // Fallback: scan roots for a skill whose frontmatter name matches
        // (handles basename != frontmatter name cases).
        if let Some((content_path, root)) = find_by_frontmatter_name(&self.ctx.skill_roots, name).await {
            let content = tokio::fs::read_to_string(&content_path)
                .await
                .map_err(|e| ToolError::ExecutionFailed(format!("read: {e}")))?;
            let output = serde_json::json!({
                "name": name,
                "source": root.display().to_string(),
                "content": content,
            });
            return serde_json::to_string_pretty(&output)
                .map(ToolOutput::success)
                .map_err(|e| ToolError::ExecutionFailed(format!("serialize: {e}")));
        }

        Err(ToolError::ExecutionFailed(format!(
            "skill '{name}' not found in any configured skill root"
        )))
    }
}

// ── SkillManage ─────────────────────────────────────────────────────────────

pub struct SkillManage {
    ctx: Arc<SkillManagementContext>,
}

impl SkillManage {
    pub fn new(ctx: Arc<SkillManagementContext>) -> Self {
        Self { ctx }
    }
}

#[async_trait]
impl Tool for SkillManage {
    fn metadata(&self) -> ToolMetadata {
        ToolMetadata {
            name: "skill-manage".to_string(),
            description: "Create or patch a skill (validated write with activity logging)"
                .to_string(),
            version: "1.0.0".to_string(),
            author: None,
            risk_level: RiskLevel::Write,
            execution_target: ExecutionTarget::InProcess,
            tags: vec!["skill".to_string(), "management".to_string()],
            scope: ToolScope::Write,
        }
    }

    fn schema(&self) -> ToolSchema {
        let mut properties: HashMap<String, ParameterSchema> = HashMap::new();
        properties.insert(
            "action".to_string(),
            ParameterSchema {
                schema_type: "string".to_string(),
                description: Some("Action: 'create' or 'patch'".to_string()),
                enum_values: Some(vec!["create".to_string(), "patch".to_string()]),
                default: None,
            },
        );
        properties.insert(
            "name".to_string(),
            ParameterSchema {
                schema_type: "string".to_string(),
                description: Some(
                    "Skill name (lowercase alphanumeric, dots, hyphens, underscores; max 64)"
                        .to_string(),
                ),
                enum_values: None,
                default: None,
            },
        );
        properties.insert(
            "body".to_string(),
            ParameterSchema {
                schema_type: "string".to_string(),
                description: Some("Skill content (SKILL.md for create; new body for patch)".to_string()),
                enum_values: None,
                default: None,
            },
        );
        properties.insert(
            "patch_kind".to_string(),
            ParameterSchema {
                schema_type: "string".to_string(),
                description: Some(
                    "Patch type: 'update_skill_md', 'add_knowledge', or 'update_metadata'"
                        .to_string(),
                ),
                enum_values: Some(vec![
                    "update_skill_md".to_string(),
                    "add_knowledge".to_string(),
                    "update_metadata".to_string(),
                ]),
                default: None,
            },
        );
        properties.insert(
            "filename".to_string(),
            ParameterSchema {
                schema_type: "string".to_string(),
                description: Some("Knowledge filename (for add_knowledge patch)".to_string()),
                enum_values: None,
                default: None,
            },
        );
        properties.insert(
            "field".to_string(),
            ParameterSchema {
                schema_type: "string".to_string(),
                description: Some("Metadata field name (for update_metadata patch)".to_string()),
                enum_values: None,
                default: None,
            },
        );
        properties.insert(
            "base_version".to_string(),
            ParameterSchema {
                schema_type: "string".to_string(),
                description: Some("Current version of skill being patched".to_string()),
                enum_values: None,
                default: None,
            },
        );
        ToolSchema {
            parameters: FunctionParameters {
                schema_type: "object".to_string(),
                properties,
                required: vec!["action".to_string(), "name".to_string()],
            },
        }
    }

    async fn execute(&self, input: ToolInput, _ctx: ToolContext) -> Result<ToolOutput, ToolError> {
        let args = &input.arguments;
        let action = args["action"]
            .as_str()
            .ok_or_else(|| ToolError::InvalidInput("missing 'action'".to_string()))?;
        let name = args["name"]
            .as_str()
            .ok_or_else(|| ToolError::InvalidInput("missing 'name'".to_string()))?;

        if !is_valid_skill_name(name) {
            return Err(ToolError::InvalidInput(format!(
                "invalid skill name '{name}': must match [a-z0-9][a-z0-9._-]* and be ≤{MAX_SKILL_NAME_LEN} chars"
            )));
        }

        match action {
            "create" => self.create_skill(name, args).await,
            "patch" => self.patch_skill(name, args).await,
            other => Err(ToolError::InvalidInput(format!(
                "unknown action '{other}': expected 'create' or 'patch'"
            ))),
        }
    }
}

impl SkillManage {
    async fn create_skill(
        &self,
        name: &str,
        args: &serde_json::Value,
    ) -> Result<ToolOutput, ToolError> {
        let body = args["body"]
            .as_str()
            .ok_or_else(|| ToolError::InvalidInput("missing 'body' for create".to_string()))?;

        // Validate the body is parseable SKILL.md before persisting.
        let parsed = parse_skill_markdown_str(body, PathBuf::from(format!("{name}/SKILL.md")))
            .map_err(|e| {
                ToolError::InvalidInput(format!("invalid SKILL.md body: {e}"))
            })?;
        if parsed.config.name != name {
            return Err(ToolError::InvalidInput(format!(
                "frontmatter name '{}' does not match requested skill name '{name}'",
                parsed.config.name
            )));
        }

        let skill_dir = safe_skill_path(&self.ctx.write_root, name)
            .ok_or_else(|| ToolError::InvalidInput(format!("unsafe skill path for '{name}'")))?;

        if skill_dir.exists() {
            return Err(ToolError::ExecutionFailed(format!(
                "skill '{name}' already exists at {}",
                skill_dir.display()
            )));
        }
        let single_file = self.ctx.write_root.join(format!("{name}.md"));
        if single_file.exists() {
            return Err(ToolError::ExecutionFailed(format!(
                "skill '{name}' already exists as {}",
                single_file.display()
            )));
        }

        tokio::fs::create_dir_all(&skill_dir)
            .await
            .map_err(|e| ToolError::ExecutionFailed(format!("mkdir: {e}")))?;

        if let Err(e) = tokio::fs::write(skill_dir.join("SKILL.md"), body).await {
            let _ = std::fs::remove_dir_all(&skill_dir);
            return Err(ToolError::ExecutionFailed(format!("write SKILL.md: {e}")));
        }

        if let Ok(mut log) = self.ctx.activity_log.lock() {
            log.append(
                KnowledgeActivityEntry::new(
                    KnowledgeOp::Store,
                    "skill-manage",
                    format!("created skill '{name}'"),
                )
                .with_page_id(name)
                .with_metadata(serde_json::json!({
                    "action": "create",
                    "bytes": body.len(),
                })),
            );
        }

        let output = serde_json::json!({
            "status": "created",
            "name": name,
            "path": skill_dir.display().to_string(),
            "bytes": body.len(),
        });
        serde_json::to_string_pretty(&output)
            .map(ToolOutput::success)
            .map_err(|e| ToolError::ExecutionFailed(format!("serialize: {e}")))
    }

    async fn patch_skill(
        &self,
        name: &str,
        args: &serde_json::Value,
    ) -> Result<ToolOutput, ToolError> {
        let patch_kind_str = args["patch_kind"]
            .as_str()
            .ok_or_else(|| ToolError::InvalidInput("missing 'patch_kind' for patch".to_string()))?;
        let body = args["body"]
            .as_str()
            .ok_or_else(|| ToolError::InvalidInput("missing 'body' for patch".to_string()))?;
        let base_version = args
            .get("base_version")
            .and_then(|v| v.as_str())
            .unwrap_or("1.0.0");

        let (location, _root) = self.find_skill(name).await?;

        if !matches!(&location, SkillLocation::Directory(_)) && patch_kind_str != "update_skill_md" {
            return Err(ToolError::InvalidInput(format!(
                "patch_kind '{patch_kind_str}' requires directory-style skill; '{name}' is a single-file skill"
            )));
        }

        let current = match &location {
            SkillLocation::Directory(dir) => load_skill_pack_from_dir(dir, name).await?,
            SkillLocation::SingleFile(path) => load_skill_pack_from_file(path, name).await?,
        };

        let (patch_kind, payload) = match patch_kind_str {
            "update_skill_md" => {
                let parsed = parse_skill_markdown_str(
                    body,
                    PathBuf::from(format!("{name}/SKILL.md")),
                )
                .map_err(|e| ToolError::InvalidInput(format!("invalid SKILL.md body: {e}")))?;
                if parsed.config.name != name {
                    return Err(ToolError::InvalidInput(format!(
                        "frontmatter name '{}' does not match skill '{name}'",
                        parsed.config.name
                    )));
                }
                (
                    PatchKind::UpdateSkillMd,
                    PatchPayload::SkillMd {
                        new_body: body.to_string(),
                    },
                )
            }
            "add_knowledge" => {
                let filename = args["filename"].as_str().ok_or_else(|| {
                    ToolError::InvalidInput("missing 'filename' for add_knowledge".to_string())
                })?;
                if !is_valid_knowledge_filename(filename) {
                    return Err(ToolError::InvalidInput(format!(
                        "invalid knowledge filename '{filename}': must be a plain basename with no path separators"
                    )));
                }
                (
                    PatchKind::AddKnowledgeBlock,
                    PatchPayload::Knowledge {
                        filename: filename.to_string(),
                        body: body.to_string(),
                    },
                )
            }
            "update_metadata" => {
                let field = args["field"].as_str().ok_or_else(|| {
                    ToolError::InvalidInput("missing 'field' for update_metadata".to_string())
                })?;
                (
                    PatchKind::UpdateMetadata,
                    PatchPayload::Metadata {
                        field: field.to_string(),
                        value: body.to_string(),
                    },
                )
            }
            other => {
                return Err(ToolError::InvalidInput(format!(
                    "unknown patch_kind '{other}'"
                )));
            }
        };

        // Use the directory basename as skill_id so FsSelfPatchApplier
        // targets the actual directory, not a re-derived path from the
        // frontmatter name (which may differ for legacy skills).
        let patch_skill_id = match &location {
            SkillLocation::Directory(dir) => dir
                .file_name()
                .and_then(|f| f.to_str())
                .unwrap_or(name)
                .to_string(),
            SkillLocation::SingleFile(_) => name.to_string(),
        };

        let patch = SkillPatch {
            skill_id: patch_skill_id,
            base_version: base_version.to_string(),
            patch_kind,
            payload,
        };

        let validator = DefaultSelfPatchValidator;
        let validated = validator
            .validate(&patch, &current)
            .map_err(|e| ToolError::ExecutionFailed(format!("patch validation failed: {e}")))?;

        let diff_summary = validated.diff_summary.clone();

        match &location {
            SkillLocation::Directory(skill_dir) => {
                let parent = skill_dir
                    .parent()
                    .ok_or_else(|| ToolError::ExecutionFailed("skill dir has no parent".to_string()))?;
                let applier = FsSelfPatchApplier::new(parent);
                applier
                    .apply(validated)
                    .map_err(|e| ToolError::ExecutionFailed(format!("patch apply failed: {e}")))?;
            }
            SkillLocation::SingleFile(path) => {
                tokio::fs::write(path, body)
                    .await
                    .map_err(|e| ToolError::ExecutionFailed(format!("write {}: {e}", path.display())))?;
            }
        }

        if let Ok(mut log) = self.ctx.activity_log.lock() {
            log.append(
                KnowledgeActivityEntry::new(
                    KnowledgeOp::Update,
                    "skill-manage",
                    format!("patched skill '{name}': {diff_summary}"),
                )
                .with_page_id(name)
                .with_metadata(serde_json::json!({
                    "action": "patch",
                    "patch_kind": patch_kind_str,
                })),
            );
        }

        let output = serde_json::json!({
            "status": "patched",
            "name": name,
            "diff_summary": diff_summary,
        });
        serde_json::to_string_pretty(&output)
            .map(ToolOutput::success)
            .map_err(|e| ToolError::ExecutionFailed(format!("serialize: {e}")))
    }

    async fn find_skill(&self, name: &str) -> Result<(SkillLocation, PathBuf), ToolError> {
        for root in &self.ctx.skill_roots {
            if let Some(path) = safe_skill_path(root, name)
                && path.is_dir()
                && path.join("SKILL.md").exists()
            {
                return Ok((SkillLocation::Directory(path), root.clone()));
            }
            let md_file = root.join(format!("{name}.md"));
            if md_file.is_file() {
                return Ok((SkillLocation::SingleFile(md_file), root.clone()));
            }
        }
        // Fallback: scan for a skill whose frontmatter name matches.
        if let Some((content_path, root)) = find_by_frontmatter_name(&self.ctx.skill_roots, name).await {
            if content_path.file_name().and_then(|f| f.to_str()) == Some("SKILL.md")
                && let Some(dir) = content_path.parent()
            {
                return Ok((SkillLocation::Directory(dir.to_path_buf()), root));
            }
            return Ok((SkillLocation::SingleFile(content_path), root));
        }
        Err(ToolError::ExecutionFailed(format!(
            "skill '{name}' not found for patching"
        )))
    }
}

async fn load_skill_pack_from_dir(
    dir: &Path,
    name: &str,
) -> Result<sera_skills::self_patch::SkillPack, ToolError> {
    let skill_md_path = dir.join("SKILL.md");
    let skill_md = if skill_md_path.exists() {
        tokio::fs::read_to_string(&skill_md_path)
            .await
            .map_err(|e| ToolError::ExecutionFailed(format!("read SKILL.md: {e}")))?
    } else {
        String::new()
    };

    let version =
        extract_version_from_frontmatter(&skill_md).unwrap_or_else(|| "0.0.0".to_string());

    let mut pack = sera_skills::self_patch::SkillPack::new(name, &version);
    pack.skill_md = skill_md;

    let knowledge_dir = dir.join("knowledge");
    if knowledge_dir.is_dir()
        && let Ok(mut reader) = tokio::fs::read_dir(&knowledge_dir).await
    {
        while let Ok(Some(entry)) = reader.next_entry().await {
            let path = entry.path();
            if path.is_file()
                && let Some(fname) = path.file_name().and_then(|n| n.to_str())
                && let Ok(content) = tokio::fs::read_to_string(&path).await
            {
                pack.knowledge.insert(fname.to_string(), content);
            }
        }
    }

    Ok(pack)
}

async fn load_skill_pack_from_file(
    path: &Path,
    name: &str,
) -> Result<sera_skills::self_patch::SkillPack, ToolError> {
    let skill_md = tokio::fs::read_to_string(path)
        .await
        .map_err(|e| ToolError::ExecutionFailed(format!("read {}: {e}", path.display())))?;

    let version =
        extract_version_from_frontmatter(&skill_md).unwrap_or_else(|| "0.0.0".to_string());

    let mut pack = sera_skills::self_patch::SkillPack::new(name, &version);
    pack.skill_md = skill_md;
    Ok(pack)
}

/// Build the three skill management tools from a shared context.
pub fn build_skill_management_tools(
    ctx: Arc<SkillManagementContext>,
) -> (SkillList, SkillView, SkillManage) {
    (
        SkillList::new(Arc::clone(&ctx)),
        SkillView::new(Arc::clone(&ctx)),
        SkillManage::new(ctx),
    )
}

/// Build a [`SkillManagementContext`] from `SERA_SKILLS_DIR` (or the default
/// `skills` directory used by the gateway dispatcher). Returns `None` only
/// when neither path exists on disk.
pub fn skill_management_context_from_env() -> Option<Arc<SkillManagementContext>> {
    let dir = std::env::var("SERA_SKILLS_DIR").unwrap_or_else(|_| "skills".to_string());
    let path = PathBuf::from(&dir);
    if path.is_dir() {
        Some(Arc::new(SkillManagementContext::new(vec![path])))
    } else {
        None
    }
}

// ── Tests ───────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use sera_types::principal::{PrincipalId, PrincipalRef};
    use sera_types::tool::{AuditHandle, CredentialBag, SessionRef, ToolPolicy, ToolProfile};

    fn make_ctx() -> ToolContext {
        ToolContext {
            session: SessionRef::new("test-session"),
            principal: PrincipalRef {
                id: PrincipalId("agent-001".to_string()),
                kind: sera_types::principal::PrincipalKind::Agent,
            },
            credentials: CredentialBag::new(),
            policy: ToolPolicy::from_profile(ToolProfile::Full),
            audit_handle: AuditHandle {
                trace_id: "trace-1".to_string(),
                span_id: "span-1".to_string(),
            },
            ..ToolContext::default()
        }
    }

    fn make_input(name: &str, args: serde_json::Value) -> ToolInput {
        ToolInput {
            name: name.to_string(),
            arguments: args,
            call_id: "call-test".to_string(),
        }
    }

    fn skill_body(name: &str, desc: &str) -> String {
        format!(
            "---\nname: {name}\nversion: 1.0.0\ndescription: {desc}\n---\n\n# {name}\nBody content.\n"
        )
    }

    async fn setup_skill_dir(root: &Path, name: &str, body: &str) {
        let dir = root.join(name);
        tokio::fs::create_dir_all(&dir).await.unwrap();
        tokio::fs::write(dir.join("SKILL.md"), body).await.unwrap();
    }

    // ── Name validation ─────────────────────────────────────────────────

    #[test]
    fn valid_skill_names() {
        assert!(is_valid_skill_name("a"));
        assert!(is_valid_skill_name("hello"));
        assert!(is_valid_skill_name("code-review"));
        assert!(is_valid_skill_name("my.skill"));
        assert!(is_valid_skill_name("my_skill"));
        assert!(is_valid_skill_name("0patch"));
        assert!(is_valid_skill_name("a-b.c_d"));
        assert!(is_valid_skill_name(&"a".repeat(64)));
    }

    #[test]
    fn invalid_skill_names() {
        assert!(!is_valid_skill_name(""));
        assert!(!is_valid_skill_name(&"a".repeat(65)));
        assert!(!is_valid_skill_name("Hello"));
        assert!(!is_valid_skill_name("-start"));
        assert!(!is_valid_skill_name(".hidden"));
        assert!(!is_valid_skill_name("_under"));
        assert!(!is_valid_skill_name("has space"));
        assert!(!is_valid_skill_name("has/slash"));
        assert!(!is_valid_skill_name("../traversal"));
        assert!(!is_valid_skill_name("has@symbol"));
    }

    #[test]
    fn safe_path_rejects_invalid_names() {
        let root = PathBuf::from("/skills");
        assert!(safe_skill_path(&root, "valid-name").is_some());
        assert!(safe_skill_path(&root, "../escape").is_none());
        assert!(safe_skill_path(&root, "has/slash").is_none());
        assert!(safe_skill_path(&root, "").is_none());
    }

    // ── SkillList ───────────────────────────────────────────────────────

    #[tokio::test]
    async fn list_skills_from_dir() {
        let tmp = tempfile::tempdir().unwrap();
        setup_skill_dir(tmp.path(), "alpha", &skill_body("alpha", "Alpha skill")).await;
        setup_skill_dir(tmp.path(), "beta", &skill_body("beta", "Beta skill")).await;

        let ctx = Arc::new(SkillManagementContext::new(vec![tmp.path().to_path_buf()]));
        let tool = SkillList::new(ctx);
        let input = make_input("skill-list", serde_json::json!({}));
        let output = tool.execute(input, make_ctx()).await.unwrap();

        assert!(!output.is_error);
        let parsed: serde_json::Value = serde_json::from_str(&output.content).unwrap();
        assert_eq!(parsed["count"], 2);
        assert!(!parsed["truncated"].as_bool().unwrap());

        let skills = parsed["skills"].as_array().unwrap();
        let names: Vec<&str> = skills
            .iter()
            .map(|s| s["name"].as_str().unwrap())
            .collect();
        assert!(names.contains(&"alpha"));
        assert!(names.contains(&"beta"));
    }

    #[tokio::test]
    async fn list_skills_with_query_filter() {
        let tmp = tempfile::tempdir().unwrap();
        setup_skill_dir(tmp.path(), "code-review", &skill_body("code-review", "Reviews code")).await;
        setup_skill_dir(tmp.path(), "deploy-helper", &skill_body("deploy-helper", "Deploys")).await;

        let ctx = Arc::new(SkillManagementContext::new(vec![tmp.path().to_path_buf()]));
        let tool = SkillList::new(ctx);
        let input = make_input("skill-list", serde_json::json!({"query": "code"}));
        let output = tool.execute(input, make_ctx()).await.unwrap();

        let parsed: serde_json::Value = serde_json::from_str(&output.content).unwrap();
        assert_eq!(parsed["count"], 1);
        assert_eq!(parsed["skills"][0]["name"], "code-review");
    }

    #[tokio::test]
    async fn list_skills_extracts_description() {
        let tmp = tempfile::tempdir().unwrap();
        setup_skill_dir(tmp.path(), "test-skill", &skill_body("test-skill", "A test skill")).await;

        let ctx = Arc::new(SkillManagementContext::new(vec![tmp.path().to_path_buf()]));
        let tool = SkillList::new(ctx);
        let input = make_input("skill-list", serde_json::json!({}));
        let output = tool.execute(input, make_ctx()).await.unwrap();

        let parsed: serde_json::Value = serde_json::from_str(&output.content).unwrap();
        assert_eq!(parsed["skills"][0]["description"], "A test skill");
    }

    #[tokio::test]
    async fn list_no_roots_returns_error() {
        let ctx = Arc::new(SkillManagementContext::new(vec![]));
        let tool = SkillList::new(ctx);
        let input = make_input("skill-list", serde_json::json!({}));
        let err = tool.execute(input, make_ctx()).await.unwrap_err();
        assert!(matches!(err, ToolError::ExecutionFailed(_)));
    }

    // ── SkillView ───────────────────────────────────────────────────────

    #[tokio::test]
    async fn view_existing_skill_dir() {
        let tmp = tempfile::tempdir().unwrap();
        let body = skill_body("viewer", "Viewer skill");
        setup_skill_dir(tmp.path(), "viewer", &body).await;

        let ctx = Arc::new(SkillManagementContext::new(vec![tmp.path().to_path_buf()]));
        let tool = SkillView::new(ctx);
        let input = make_input("skill-view", serde_json::json!({"name": "viewer"}));
        let output = tool.execute(input, make_ctx()).await.unwrap();

        assert!(!output.is_error);
        let parsed: serde_json::Value = serde_json::from_str(&output.content).unwrap();
        assert_eq!(parsed["name"], "viewer");
        assert!(parsed["content"].as_str().unwrap().contains("Viewer skill"));
    }

    #[tokio::test]
    async fn view_existing_single_file_skill() {
        let tmp = tempfile::tempdir().unwrap();
        let body = skill_body("single", "Single file skill");
        tokio::fs::write(tmp.path().join("single.md"), &body)
            .await
            .unwrap();

        let ctx = Arc::new(SkillManagementContext::new(vec![tmp.path().to_path_buf()]));
        let tool = SkillView::new(ctx);
        let input = make_input("skill-view", serde_json::json!({"name": "single"}));
        let output = tool.execute(input, make_ctx()).await.unwrap();

        assert!(!output.is_error);
        let parsed: serde_json::Value = serde_json::from_str(&output.content).unwrap();
        assert_eq!(parsed["name"], "single");
        assert!(parsed["content"].as_str().unwrap().contains("Single file skill"));
    }

    #[tokio::test]
    async fn view_missing_skill_returns_error() {
        let tmp = tempfile::tempdir().unwrap();
        let ctx = Arc::new(SkillManagementContext::new(vec![tmp.path().to_path_buf()]));
        let tool = SkillView::new(ctx);
        let input = make_input("skill-view", serde_json::json!({"name": "nonexistent"}));
        let err = tool.execute(input, make_ctx()).await.unwrap_err();
        assert!(matches!(err, ToolError::ExecutionFailed(_)));
    }

    #[tokio::test]
    async fn view_invalid_name_returns_input_error() {
        let tmp = tempfile::tempdir().unwrap();
        let ctx = Arc::new(SkillManagementContext::new(vec![tmp.path().to_path_buf()]));
        let tool = SkillView::new(ctx);
        let input = make_input("skill-view", serde_json::json!({"name": "../escape"}));
        let err = tool.execute(input, make_ctx()).await.unwrap_err();
        assert!(matches!(err, ToolError::InvalidInput(_)));
    }

    // ── SkillManage: create ─────────────────────────────────────────────

    #[tokio::test]
    async fn create_valid_skill() {
        let tmp = tempfile::tempdir().unwrap();
        let ctx = Arc::new(SkillManagementContext::new(vec![tmp.path().to_path_buf()]));
        let tool = SkillManage::new(Arc::clone(&ctx));

        let body = skill_body("new-skill", "A new skill");
        let input = make_input(
            "skill-manage",
            serde_json::json!({
                "action": "create",
                "name": "new-skill",
                "body": body,
            }),
        );
        let output = tool.execute(input, make_ctx()).await.unwrap();

        assert!(!output.is_error);
        let parsed: serde_json::Value = serde_json::from_str(&output.content).unwrap();
        assert_eq!(parsed["status"], "created");
        assert_eq!(parsed["name"], "new-skill");

        // Verify file exists
        let skill_md = tmp.path().join("new-skill").join("SKILL.md");
        assert!(skill_md.exists());
        let content = tokio::fs::read_to_string(&skill_md).await.unwrap();
        assert!(content.contains("A new skill"));
    }

    #[tokio::test]
    async fn create_rejects_invalid_name() {
        let tmp = tempfile::tempdir().unwrap();
        let ctx = Arc::new(SkillManagementContext::new(vec![tmp.path().to_path_buf()]));
        let tool = SkillManage::new(ctx);
        let input = make_input(
            "skill-manage",
            serde_json::json!({
                "action": "create",
                "name": "../escape",
                "body": "content",
            }),
        );
        let err = tool.execute(input, make_ctx()).await.unwrap_err();
        assert!(matches!(err, ToolError::InvalidInput(_)));
    }

    #[tokio::test]
    async fn create_rejects_duplicate() {
        let tmp = tempfile::tempdir().unwrap();
        setup_skill_dir(tmp.path(), "existing", &skill_body("existing", "Exists")).await;

        let ctx = Arc::new(SkillManagementContext::new(vec![tmp.path().to_path_buf()]));
        let tool = SkillManage::new(ctx);
        let input = make_input(
            "skill-manage",
            serde_json::json!({
                "action": "create",
                "name": "existing",
                "body": skill_body("existing", "Duplicate attempt"),
            }),
        );
        let err = tool.execute(input, make_ctx()).await.unwrap_err();
        assert!(matches!(err, ToolError::ExecutionFailed(_)));
    }

    #[tokio::test]
    async fn create_rejects_collision_with_single_file_skill() {
        let tmp = tempfile::tempdir().unwrap();
        tokio::fs::write(tmp.path().join("legacy.md"), &skill_body("legacy", "Legacy"))
            .await
            .unwrap();

        let ctx = Arc::new(SkillManagementContext::new(vec![tmp.path().to_path_buf()]));
        let tool = SkillManage::new(ctx);
        let input = make_input(
            "skill-manage",
            serde_json::json!({
                "action": "create",
                "name": "legacy",
                "body": skill_body("legacy", "New version"),
            }),
        );
        let err = tool.execute(input, make_ctx()).await.unwrap_err();
        assert!(matches!(err, ToolError::ExecutionFailed(_)));
        assert!(!tmp.path().join("legacy").exists(), "directory must not be created");
    }

    #[tokio::test]
    async fn list_skips_non_skill_directories() {
        let tmp = tempfile::tempdir().unwrap();
        // Real skill directory
        setup_skill_dir(tmp.path(), "real", &skill_body("real", "Real skill")).await;
        // Non-skill directory (no SKILL.md)
        tokio::fs::create_dir_all(tmp.path().join("stale-tmp")).await.unwrap();
        tokio::fs::write(tmp.path().join("stale-tmp").join("junk.txt"), "junk")
            .await
            .unwrap();

        let ctx = Arc::new(SkillManagementContext::new(vec![tmp.path().to_path_buf()]));
        let tool = SkillList::new(ctx);
        let input = make_input("skill-list", serde_json::json!({}));
        let output = tool.execute(input, make_ctx()).await.unwrap();

        let parsed: serde_json::Value = serde_json::from_str(&output.content).unwrap();
        assert_eq!(parsed["count"], 1);
        assert_eq!(parsed["skills"][0]["name"], "real");
    }

    #[tokio::test]
    async fn list_skips_non_skill_markdown_files() {
        let tmp = tempfile::tempdir().unwrap();
        // Valid single-file skill
        tokio::fs::write(tmp.path().join("valid.md"), &skill_body("valid", "Valid"))
            .await
            .unwrap();
        // README.md — no skill frontmatter name field
        tokio::fs::write(tmp.path().join("README.md"), "# Project Readme\nNo frontmatter.")
            .await
            .unwrap();
        // Malformed frontmatter — has --- but no name
        tokio::fs::write(tmp.path().join("broken.md"), "---\ndescription: no name\n---\nbody")
            .await
            .unwrap();

        let ctx = Arc::new(SkillManagementContext::new(vec![tmp.path().to_path_buf()]));
        let tool = SkillList::new(ctx);
        let input = make_input("skill-list", serde_json::json!({}));
        let output = tool.execute(input, make_ctx()).await.unwrap();

        let parsed: serde_json::Value = serde_json::from_str(&output.content).unwrap();
        assert_eq!(parsed["count"], 1, "only the valid skill should appear");
        assert_eq!(parsed["skills"][0]["name"], "valid");
    }

    #[tokio::test]
    async fn create_rejects_invalid_body_no_frontmatter() {
        let tmp = tempfile::tempdir().unwrap();
        let ctx = Arc::new(SkillManagementContext::new(vec![tmp.path().to_path_buf()]));
        let tool = SkillManage::new(ctx);
        let input = make_input(
            "skill-manage",
            serde_json::json!({
                "action": "create",
                "name": "bad-body",
                "body": "no frontmatter here, just plain text",
            }),
        );
        let err = tool.execute(input, make_ctx()).await.unwrap_err();
        assert!(matches!(err, ToolError::InvalidInput(_)));
        // No directory should have been created
        assert!(!tmp.path().join("bad-body").exists());
    }

    #[tokio::test]
    async fn create_rejects_name_mismatch() {
        let tmp = tempfile::tempdir().unwrap();
        let ctx = Arc::new(SkillManagementContext::new(vec![tmp.path().to_path_buf()]));
        let tool = SkillManage::new(ctx);
        let body = skill_body("wrong-name", "Description");
        let input = make_input(
            "skill-manage",
            serde_json::json!({
                "action": "create",
                "name": "actual-name",
                "body": body,
            }),
        );
        let err = tool.execute(input, make_ctx()).await.unwrap_err();
        assert!(matches!(err, ToolError::InvalidInput(_)));
        assert!(!tmp.path().join("actual-name").exists());
    }

    #[tokio::test]
    async fn created_skill_is_loadable_by_dispatch_engine() {
        let tmp = tempfile::tempdir().unwrap();
        let ctx = Arc::new(SkillManagementContext::new(vec![tmp.path().to_path_buf()]));
        let tool = SkillManage::new(ctx);

        let body = "---\nname: greet\nversion: 1.0.0\ndescription: Greeting skill\ntriggers:\n  - hello\n---\n\nSay hello.\n";
        let input = make_input(
            "skill-manage",
            serde_json::json!({
                "action": "create",
                "name": "greet",
                "body": body,
            }),
        );
        tool.execute(input, make_ctx()).await.unwrap();

        // Verify the dispatch engine can load it (simulates fresh session)
        let engine = crate::skill_dispatch::SkillDispatchEngine::new();
        let loaded = engine.load_dir(tmp.path()).await.unwrap();
        assert_eq!(loaded, 1, "dispatch engine must load the created skill");
        assert_eq!(engine.registered_count(), 1);

        let fired = engine.on_turn("hello there");
        assert_eq!(fired.len(), 1);
        assert_eq!(fired[0].name, "greet");
    }

    // ── SkillManage: patch ──────────────────────────────────────────────

    #[tokio::test]
    async fn patch_updates_skill_md() {
        let tmp = tempfile::tempdir().unwrap();
        let original = skill_body("patchme", "Original content");
        setup_skill_dir(tmp.path(), "patchme", &original).await;

        let ctx = Arc::new(SkillManagementContext::new(vec![tmp.path().to_path_buf()]));
        let tool = SkillManage::new(Arc::clone(&ctx));

        let new_body = skill_body("patchme", "Updated content");
        let input = make_input(
            "skill-manage",
            serde_json::json!({
                "action": "patch",
                "name": "patchme",
                "patch_kind": "update_skill_md",
                "body": new_body,
                "base_version": "1.0.0",
            }),
        );
        let output = tool.execute(input, make_ctx()).await.unwrap();

        assert!(!output.is_error);
        let parsed: serde_json::Value = serde_json::from_str(&output.content).unwrap();
        assert_eq!(parsed["status"], "patched");

        // Verify content changed on disk
        let content = tokio::fs::read_to_string(tmp.path().join("patchme").join("SKILL.md"))
            .await
            .unwrap();
        assert!(content.contains("Updated content"));
    }

    #[tokio::test]
    async fn patch_rejects_version_mismatch() {
        let tmp = tempfile::tempdir().unwrap();
        setup_skill_dir(tmp.path(), "versioned", &skill_body("versioned", "V1")).await;

        let ctx = Arc::new(SkillManagementContext::new(vec![tmp.path().to_path_buf()]));
        let tool = SkillManage::new(ctx);

        let new_body = skill_body("versioned", "V2");
        let input = make_input(
            "skill-manage",
            serde_json::json!({
                "action": "patch",
                "name": "versioned",
                "patch_kind": "update_skill_md",
                "body": new_body,
                "base_version": "2.0.0",
            }),
        );
        let err = tool.execute(input, make_ctx()).await.unwrap_err();
        assert!(matches!(err, ToolError::ExecutionFailed(_)));
    }

    #[tokio::test]
    async fn patch_rejects_name_mismatch_in_body() {
        let tmp = tempfile::tempdir().unwrap();
        setup_skill_dir(tmp.path(), "foo", &skill_body("foo", "Original")).await;

        let ctx = Arc::new(SkillManagementContext::new(vec![tmp.path().to_path_buf()]));
        let tool = SkillManage::new(ctx);
        let bad_body = skill_body("bar", "Sneaky rename");
        let input = make_input(
            "skill-manage",
            serde_json::json!({
                "action": "patch",
                "name": "foo",
                "patch_kind": "update_skill_md",
                "body": bad_body,
                "base_version": "1.0.0",
            }),
        );
        let err = tool.execute(input, make_ctx()).await.unwrap_err();
        assert!(matches!(err, ToolError::InvalidInput(_)));
        // Original content must be unchanged
        let content = tokio::fs::read_to_string(tmp.path().join("foo").join("SKILL.md"))
            .await
            .unwrap();
        assert!(content.contains("Original"));
    }

    #[tokio::test]
    async fn patch_rejects_path_traversal_in_knowledge_filename() {
        let tmp = tempfile::tempdir().unwrap();
        setup_skill_dir(tmp.path(), "target", &skill_body("target", "Target")).await;

        let ctx = Arc::new(SkillManagementContext::new(vec![tmp.path().to_path_buf()]));
        let tool = SkillManage::new(ctx);
        let input = make_input(
            "skill-manage",
            serde_json::json!({
                "action": "patch",
                "name": "target",
                "patch_kind": "add_knowledge",
                "body": "malicious content",
                "filename": "../../etc/pwn",
                "base_version": "1.0.0",
            }),
        );
        let err = tool.execute(input, make_ctx()).await.unwrap_err();
        assert!(matches!(err, ToolError::InvalidInput(_)));
    }

    #[test]
    fn knowledge_filename_validation() {
        assert!(is_valid_knowledge_filename("notes.md"));
        assert!(is_valid_knowledge_filename("my-file.txt"));
        assert!(!is_valid_knowledge_filename(""));
        assert!(!is_valid_knowledge_filename("../escape"));
        assert!(!is_valid_knowledge_filename("sub/dir.md"));
        assert!(!is_valid_knowledge_filename("back\\slash"));
        assert!(!is_valid_knowledge_filename("has\0null"));
    }

    #[tokio::test]
    async fn patch_single_file_skill_update_skill_md() {
        let tmp = tempfile::tempdir().unwrap();
        let original = skill_body("single", "Original single-file");
        tokio::fs::write(tmp.path().join("single.md"), &original)
            .await
            .unwrap();

        let ctx = Arc::new(SkillManagementContext::new(vec![tmp.path().to_path_buf()]));
        let tool = SkillManage::new(ctx);

        let new_body = skill_body("single", "Updated single-file");
        let input = make_input(
            "skill-manage",
            serde_json::json!({
                "action": "patch",
                "name": "single",
                "patch_kind": "update_skill_md",
                "body": new_body,
                "base_version": "1.0.0",
            }),
        );
        let output = tool.execute(input, make_ctx()).await.unwrap();
        assert!(!output.is_error);

        let content = tokio::fs::read_to_string(tmp.path().join("single.md"))
            .await
            .unwrap();
        assert!(content.contains("Updated single-file"));
    }

    #[tokio::test]
    async fn patch_single_file_rejects_add_knowledge() {
        let tmp = tempfile::tempdir().unwrap();
        tokio::fs::write(tmp.path().join("simple.md"), &skill_body("simple", "Simple"))
            .await
            .unwrap();

        let ctx = Arc::new(SkillManagementContext::new(vec![tmp.path().to_path_buf()]));
        let tool = SkillManage::new(ctx);
        let input = make_input(
            "skill-manage",
            serde_json::json!({
                "action": "patch",
                "name": "simple",
                "patch_kind": "add_knowledge",
                "body": "some knowledge",
                "filename": "notes.md",
                "base_version": "1.0.0",
            }),
        );
        let err = tool.execute(input, make_ctx()).await.unwrap_err();
        assert!(matches!(err, ToolError::InvalidInput(_)));
    }

    // ── Activity log ────────────────────────────────────────────────────

    #[tokio::test]
    async fn activity_log_records_create_and_patch() {
        let tmp = tempfile::tempdir().unwrap();
        let ctx = Arc::new(SkillManagementContext::new(vec![tmp.path().to_path_buf()]));
        let tool = SkillManage::new(Arc::clone(&ctx));

        // Create
        let body = skill_body("logged", "Logged skill");
        let input = make_input(
            "skill-manage",
            serde_json::json!({
                "action": "create",
                "name": "logged",
                "body": body,
            }),
        );
        tool.execute(input, make_ctx()).await.unwrap();

        // Patch
        let new_body = skill_body("logged", "Updated logged");
        let input = make_input(
            "skill-manage",
            serde_json::json!({
                "action": "patch",
                "name": "logged",
                "patch_kind": "update_skill_md",
                "body": new_body,
                "base_version": "1.0.0",
            }),
        );
        tool.execute(input, make_ctx()).await.unwrap();

        let log = ctx.activity_log.lock().unwrap();
        assert_eq!(log.len(), 2);
        let entries: Vec<_> = log.iter().collect();
        assert_eq!(entries[0].op, KnowledgeOp::Store);
        assert!(entries[0].summary.contains("created"));
        assert_eq!(entries[1].op, KnowledgeOp::Update);
        assert!(entries[1].summary.contains("patched"));
    }

    // ── Metadata ────────────────────────────────────────────────────────

    #[test]
    fn tool_metadata_risk_and_scope() {
        let ctx = Arc::new(SkillManagementContext::new(vec![]));
        let list = SkillList::new(Arc::clone(&ctx));
        let view = SkillView::new(Arc::clone(&ctx));
        let manage = SkillManage::new(ctx);

        assert_eq!(list.metadata().risk_level, RiskLevel::Read);
        assert_eq!(list.metadata().scope, ToolScope::Read);
        assert_eq!(view.metadata().risk_level, RiskLevel::Read);
        assert_eq!(view.metadata().scope, ToolScope::Read);
        assert_eq!(manage.metadata().risk_level, RiskLevel::Write);
        assert_eq!(manage.metadata().scope, ToolScope::Write);
    }

    #[test]
    fn tool_names_are_correct() {
        let ctx = Arc::new(SkillManagementContext::new(vec![]));
        let list = SkillList::new(Arc::clone(&ctx));
        let view = SkillView::new(Arc::clone(&ctx));
        let manage = SkillManage::new(ctx);

        assert_eq!(list.metadata().name, "skill-list");
        assert_eq!(view.metadata().name, "skill-view");
        assert_eq!(manage.metadata().name, "skill-manage");
    }

    // ── Production wire-up ────────────────────────────────────────────

    #[test]
    fn context_from_env_returns_none_when_default_dir_missing() {
        unsafe { std::env::remove_var("SERA_SKILLS_DIR") };
        // When unset, falls back to "./skills" which likely doesn't exist in test cwd.
        // This test just verifies it doesn't panic and returns None for a missing default.
        let _ = skill_management_context_from_env();
    }

    #[test]
    fn context_from_env_returns_some_when_dir_exists() {
        let tmp = tempfile::tempdir().unwrap();
        unsafe { std::env::set_var("SERA_SKILLS_DIR", tmp.path().as_os_str()) };
        let ctx = skill_management_context_from_env();
        unsafe { std::env::remove_var("SERA_SKILLS_DIR") };
        assert!(ctx.is_some());
        let ctx = ctx.unwrap();
        assert_eq!(ctx.skill_roots, vec![tmp.path().to_path_buf()]);
    }

    #[test]
    fn context_from_env_returns_none_when_dir_missing() {
        unsafe { std::env::set_var("SERA_SKILLS_DIR", "/tmp/__sera_nonexistent_skills_dir__") };
        let ctx = skill_management_context_from_env();
        unsafe { std::env::remove_var("SERA_SKILLS_DIR") };
        assert!(ctx.is_none());
    }

    #[test]
    fn registry_includes_skill_tools_when_configured() {
        let tmp = tempfile::tempdir().unwrap();
        let ctx = Arc::new(SkillManagementContext::new(vec![tmp.path().to_path_buf()]));
        let registry = crate::tools::TraitToolRegistry::with_builtins().with_skill_management(ctx);
        assert!(registry.get("skill-list").is_some());
        assert!(registry.get("skill-view").is_some());
        assert!(registry.get("skill-manage").is_some());
        assert_eq!(registry.list().len(), 14 + 3);
    }

    #[test]
    fn registry_excludes_skill_tools_when_not_configured() {
        let registry = crate::tools::TraitToolRegistry::with_builtins();
        assert!(registry.get("skill-list").is_none());
        assert!(registry.get("skill-view").is_none());
        assert!(registry.get("skill-manage").is_none());
        assert_eq!(registry.list().len(), 14);
    }

    // ── Dogfood scaffold ────────────────────────────────────────────────

    /// Phase 2 expectation: a fresh session should use an improved skill
    /// after a correction/background review loop updates it. This test
    /// scaffolds the end-to-end flow without the background review trigger.
    #[tokio::test]
    #[ignore = "Phase 2: requires background review loop + fresh session spawn"]
    async fn dogfood_fresh_session_uses_improved_skill() {
        let tmp = tempfile::tempdir().unwrap();
        let ctx = Arc::new(SkillManagementContext::new(vec![tmp.path().to_path_buf()]));
        let manage = SkillManage::new(Arc::clone(&ctx));
        let view = SkillView::new(Arc::clone(&ctx));

        // 1. Create initial skill
        let v1 = skill_body("greeting", "Says hello");
        manage
            .execute(
                make_input(
                    "skill-manage",
                    serde_json::json!({"action": "create", "name": "greeting", "body": v1}),
                ),
                make_ctx(),
            )
            .await
            .unwrap();

        // 2. Simulate correction: patch the skill with improved content
        let v2 = skill_body("greeting", "Says hello warmly with context");
        manage
            .execute(
                make_input(
                    "skill-manage",
                    serde_json::json!({
                        "action": "patch",
                        "name": "greeting",
                        "patch_kind": "update_skill_md",
                        "body": v2,
                        "base_version": "1.0.0",
                    }),
                ),
                make_ctx(),
            )
            .await
            .unwrap();

        // 3. Verify the updated skill is visible (simulates fresh session load)
        let output = view
            .execute(
                make_input("skill-view", serde_json::json!({"name": "greeting"})),
                make_ctx(),
            )
            .await
            .unwrap();
        let parsed: serde_json::Value = serde_json::from_str(&output.content).unwrap();
        let content = parsed["content"].as_str().unwrap();
        assert!(
            content.contains("warmly with context"),
            "fresh session should see the improved skill"
        );

        // Phase 2 TODO: spawn a fresh DefaultRuntime session, load skills,
        // and verify the agent's behaviour reflects the updated skill content.
    }
}

//! SKILL.md loader — human-authorable single-file skill format.
//!
//! `SKILL.md` is a markdown-first variant of the skill definition format (see
//! issue #511). Users write skills the same way OMC skills are authored: YAML
//! frontmatter between `---` fences, followed by a free-form markdown body.
//!
//! # Example
//!
//! ```markdown
//! ---
//! name: lookup-invoice
//! description: Find an invoice by its external id via the finance API
//! inputs:
//!   invoice_id: string
//! tier: 1
//! ---
//!
//! # Behaviour
//! When asked about invoices, call the finance API ...
//! ```
//!
//! # Relationship to [`crate::markdown`]
//!
//! The `markdown` module is the original strict AgentSkills parser used by
//! `MarkdownSkillPack`. This loader targets a smaller, user-friendly subset
//! with permissive semantics:
//!
//! - `name` and `description` are required; everything else is optional.
//! - Unknown frontmatter keys are logged at `warn` level and ignored —
//!   typos don't explode user-authored files.
//! - `tier` defaults to [`DEFAULT_TIER`] when absent.
//! - `inputs` is a free-form `HashMap<String, String>`; shape is not enforced.

use std::collections::HashMap;
use std::path::{Path, PathBuf};

use serde::Deserialize;
use serde_yaml::Value;
use tokio::fs;
use tracing::warn;

use crate::error::SkillsError;

/// Default tier assigned to a SKILL.md when the frontmatter omits `tier`.
///
/// Mirrors the current runtime default — tier 1 = standard capability, matches
/// SERA's tier-1 sandbox (`sandbox-boundaries/tier-1.yaml`).
pub const DEFAULT_TIER: u8 = 1;

/// Recognised frontmatter keys. Anything outside this set is logged and
/// dropped so legacy / experimental fields don't cause loads to fail.
const KNOWN_KEYS: &[&str] = &["name", "description", "inputs", "tier"];

/// A parsed SKILL.md file.
///
/// Carries the four load-bearing fields plus the raw markdown body, which
/// downstream code injects into the agent prompt.
#[derive(Debug, Clone)]
pub struct Skill {
    /// Stable identifier for the skill; matches the file's `name` field.
    pub name: String,
    /// Human-readable description / trigger hint.
    pub description: String,
    /// Free-form input schema — typically `{ "invoice_id": "string" }`.
    /// Not validated here; runtime consumers interpret the map.
    pub inputs: HashMap<String, String>,
    /// Sandbox tier (1, 2, or 3). See `sandbox-boundaries/tier-*.yaml`.
    pub tier: u8,
    /// Markdown body after the frontmatter fence. Used verbatim as the
    /// skill prompt.
    pub body: String,
    /// Absolute path the skill was loaded from, when known.
    pub source_path: Option<PathBuf>,
}

/// Intermediate frontmatter shape. Mirrors the public SKILL.md schema.
#[derive(Debug, Default, Deserialize)]
struct Frontmatter {
    #[serde(default)]
    name: Option<String>,
    #[serde(default)]
    description: Option<String>,
    #[serde(default)]
    inputs: HashMap<String, String>,
    #[serde(default)]
    tier: Option<u8>,
}

/// Split raw SKILL.md text at the leading `---` fences.
///
/// The fences are matched case-insensitively-agnostic (only `---` is legal —
/// markdown fences aren't case-sensitive by nature, but we accept arbitrary
/// trailing whitespace on the fence line so `---   ` parses). Returns
/// `(yaml, body)`.
fn split_frontmatter(raw: &str) -> Result<(String, String), SkillsError> {
    let trimmed = raw.strip_prefix('\u{FEFF}').unwrap_or(raw);

    // Normalise CRLF → LF for processing.
    let normalised = trimmed.replace("\r\n", "\n");

    let mut lines = normalised.split_inclusive('\n');

    // First non-empty line must be `---` (possibly with trailing whitespace).
    let first = lines
        .next()
        .ok_or_else(|| SkillsError::Format("empty SKILL.md file".to_string()))?;
    if !is_fence_line(first) {
        return Err(SkillsError::Format(
            "SKILL.md must begin with `---` frontmatter fence".to_string(),
        ));
    }

    // Collect yaml lines until the closing fence.
    let mut yaml = String::new();
    let mut found_close = false;
    let mut body = String::new();
    for line in lines.by_ref() {
        if is_fence_line(line) {
            found_close = true;
            break;
        }
        yaml.push_str(line);
    }
    if !found_close {
        return Err(SkillsError::Format(
            "SKILL.md frontmatter missing closing `---` fence".to_string(),
        ));
    }
    // Remaining lines are the body.
    for line in lines {
        body.push_str(line);
    }
    Ok((yaml, body))
}

/// Returns `true` when `line` is a valid `---` fence.
///
/// The fence must be exactly three dashes followed only by whitespace on that
/// line — this matches YAML's document-separator rules while being lenient
/// about a stray space before the newline.
fn is_fence_line(line: &str) -> bool {
    let stripped = line.trim_end_matches('\n').trim_end_matches('\r');
    let trimmed = stripped.trim_end();
    trimmed == "---"
}

/// Parse a SKILL.md string. `source_path` is retained on the returned
/// [`Skill`] for diagnostics but is not touched on disk.
pub fn parse_skill_md(raw: &str, source_path: Option<PathBuf>) -> Result<Skill, SkillsError> {
    let (yaml, body) = split_frontmatter(raw)?;

    // Parse loosely into a Value first so we can spot unknown keys before
    // coercing into the strong type. A fully untyped parse is the only way
    // to emit a `warn!` without failing.
    let raw_value: Value = serde_yaml::from_str(&yaml).map_err(|e| {
        SkillsError::Format(format!(
            "invalid YAML frontmatter{}: {e}",
            source_path
                .as_deref()
                .map(|p| format!(" in {}", p.display()))
                .unwrap_or_default()
        ))
    })?;

    warn_on_unknown_keys(&raw_value, source_path.as_deref());

    let fm: Frontmatter = serde_yaml::from_value(raw_value).map_err(|e| {
        SkillsError::Format(format!(
            "invalid SKILL.md frontmatter{}: {e}",
            source_path
                .as_deref()
                .map(|p| format!(" in {}", p.display()))
                .unwrap_or_default()
        ))
    })?;

    let name = fm
        .name
        .ok_or_else(|| SkillsError::Format("SKILL.md frontmatter missing `name`".to_string()))?;
    let description = fm.description.ok_or_else(|| {
        SkillsError::Format("SKILL.md frontmatter missing `description`".to_string())
    })?;

    if name.trim().is_empty() {
        return Err(SkillsError::Format(
            "SKILL.md `name` must not be empty".to_string(),
        ));
    }
    if description.trim().is_empty() {
        return Err(SkillsError::Format(
            "SKILL.md `description` must not be empty".to_string(),
        ));
    }

    let tier = fm.tier.unwrap_or(DEFAULT_TIER);
    if !matches!(tier, 1..=3) {
        return Err(SkillsError::Format(format!(
            "SKILL.md `tier` must be 1, 2, or 3 (got {tier})",
        )));
    }

    Ok(Skill {
        name,
        description,
        inputs: fm.inputs,
        tier,
        body,
        source_path,
    })
}

/// Load a SKILL.md file from disk.
///
/// Errors bubble up through [`SkillsError`]; IO errors become
/// [`SkillsError::Io`], format errors become [`SkillsError::Format`].
pub async fn load_skill_md(path: &Path) -> Result<Skill, SkillsError> {
    let raw = fs::read_to_string(path).await?;
    parse_skill_md(&raw, Some(path.to_path_buf()))
}

fn warn_on_unknown_keys(value: &Value, source_path: Option<&Path>) {
    if let Value::Mapping(map) = value {
        for (k, _v) in map {
            if let Value::String(key) = k
                && !KNOWN_KEYS.contains(&key.as_str())
            {
                match source_path {
                    Some(p) => warn!(
                        path = %p.display(),
                        key = %key,
                        "SKILL.md: ignoring unknown frontmatter key"
                    ),
                    None => warn!(
                        key = %key,
                        "SKILL.md: ignoring unknown frontmatter key"
                    ),
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const VALID_SKILL: &str = "---\nname: lookup-invoice\ndescription: Find an invoice by its external id\ninputs:\n  invoice_id: string\ntier: 1\n---\n\n# Behaviour\nWhen asked about invoices ...\n";

    #[test]
    fn parses_valid_skill_md() {
        let skill = parse_skill_md(VALID_SKILL, None).unwrap();
        assert_eq!(skill.name, "lookup-invoice");
        assert_eq!(skill.description, "Find an invoice by its external id");
        assert_eq!(skill.inputs.get("invoice_id").map(String::as_str), Some("string"));
        assert_eq!(skill.tier, 1);
        assert!(skill.body.contains("# Behaviour"));
    }

    #[test]
    fn missing_frontmatter_fence_errors() {
        let raw = "# just a markdown file\nno frontmatter here\n";
        let err = parse_skill_md(raw, None).unwrap_err();
        match err {
            SkillsError::Format(msg) => assert!(msg.contains("`---`")),
            other => panic!("expected Format error, got {other:?}"),
        }
    }

    #[test]
    fn missing_closing_fence_errors() {
        let raw = "---\nname: nope\ndescription: never closes\n\nbody without closing fence\n";
        let err = parse_skill_md(raw, None).unwrap_err();
        match err {
            SkillsError::Format(msg) => assert!(msg.contains("closing")),
            other => panic!("expected Format error, got {other:?}"),
        }
    }

    #[test]
    fn missing_name_errors() {
        let raw = "---\ndescription: no name\n---\nbody\n";
        let err = parse_skill_md(raw, None).unwrap_err();
        match err {
            SkillsError::Format(msg) => assert!(msg.contains("`name`")),
            other => panic!("expected Format error, got {other:?}"),
        }
    }

    #[test]
    fn missing_description_errors() {
        let raw = "---\nname: x\n---\nbody\n";
        let err = parse_skill_md(raw, None).unwrap_err();
        match err {
            SkillsError::Format(msg) => assert!(msg.contains("`description`")),
            other => panic!("expected Format error, got {other:?}"),
        }
    }

    #[test]
    fn empty_name_rejected() {
        let raw = "---\nname: \"\"\ndescription: ok\n---\nbody\n";
        let err = parse_skill_md(raw, None).unwrap_err();
        assert!(matches!(err, SkillsError::Format(_)));
    }

    #[test]
    fn empty_description_rejected() {
        let raw = "---\nname: ok\ndescription: \"\"\n---\nbody\n";
        let err = parse_skill_md(raw, None).unwrap_err();
        assert!(matches!(err, SkillsError::Format(_)));
    }

    #[test]
    fn body_only_no_frontmatter_errors() {
        let raw = "just a body, no leading fence\n";
        let err = parse_skill_md(raw, None).unwrap_err();
        assert!(matches!(err, SkillsError::Format(_)));
    }

    #[test]
    fn unknown_frontmatter_key_warns_but_loads() {
        // Extra `triggers` key should be ignored, not cause a failure.
        let raw = "---\nname: ok\ndescription: ok\ntriggers:\n  - review\nsome_other_field: whatever\n---\nbody\n";
        let skill = parse_skill_md(raw, None).unwrap();
        assert_eq!(skill.name, "ok");
    }

    #[test]
    fn crlf_line_endings_accepted() {
        let raw = "---\r\nname: crlf-ok\r\ndescription: works on windows\r\n---\r\nbody\r\n";
        let skill = parse_skill_md(raw, None).unwrap();
        assert_eq!(skill.name, "crlf-ok");
    }

    #[test]
    fn tier_defaults_when_absent() {
        let raw = "---\nname: defaults\ndescription: no tier set\n---\nbody\n";
        let skill = parse_skill_md(raw, None).unwrap();
        assert_eq!(skill.tier, DEFAULT_TIER);
    }

    #[test]
    fn invalid_tier_rejected() {
        let raw = "---\nname: ok\ndescription: ok\ntier: 9\n---\nbody\n";
        let err = parse_skill_md(raw, None).unwrap_err();
        assert!(matches!(err, SkillsError::Format(_)));
    }

    #[test]
    fn fence_with_trailing_whitespace_accepted() {
        let raw = "---   \nname: fence-ws\ndescription: trailing space on fence\n---\t\nbody\n";
        let skill = parse_skill_md(raw, None).unwrap();
        assert_eq!(skill.name, "fence-ws");
    }

    #[test]
    fn bom_prefix_stripped() {
        let raw = "\u{FEFF}---\nname: bom\ndescription: has a BOM\n---\nbody\n";
        let skill = parse_skill_md(raw, None).unwrap();
        assert_eq!(skill.name, "bom");
    }

    #[test]
    fn body_preserved_verbatim() {
        let raw = "---\nname: x\ndescription: y\n---\nLine one\nLine two\n";
        let skill = parse_skill_md(raw, None).unwrap();
        assert_eq!(skill.body, "Line one\nLine two\n");
    }

    #[test]
    fn empty_body_allowed() {
        let raw = "---\nname: empty-body\ndescription: no content\n---\n";
        let skill = parse_skill_md(raw, None).unwrap();
        assert_eq!(skill.body, "");
    }

    #[test]
    fn empty_file_errors() {
        let err = parse_skill_md("", None).unwrap_err();
        assert!(matches!(err, SkillsError::Format(_)));
    }

    #[tokio::test]
    async fn load_from_disk_reads_file() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("SKILL.md");
        tokio::fs::write(&path, VALID_SKILL).await.unwrap();
        let skill = load_skill_md(&path).await.unwrap();
        assert_eq!(skill.name, "lookup-invoice");
        assert_eq!(skill.source_path.as_deref(), Some(path.as_path()));
    }

    #[tokio::test]
    async fn load_from_disk_missing_file_errors() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("does-not-exist.md");
        let err = load_skill_md(&path).await.unwrap_err();
        assert!(matches!(err, SkillsError::Io(_)));
    }
}

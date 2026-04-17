//! AgentSkills markdown skill file parser.
//!
//! Parses single-file `.md` skill definitions with YAML frontmatter. The
//! frontmatter describes skill metadata (name, version, triggers, tools,
//! MCP servers, etc.); the body is the raw prompt text injected into the
//! agent's context window when the skill is active.
//!
//! # Example input
//!
//! ```markdown
//! ---
//! name: code-review
//! version: 1.0.0
//! description: Review code for correctness and style
//! triggers: [review, audit]
//! tools: [read_file, search_code]
//! ---
//!
//! You are a senior code reviewer...
//! ```
//!
//! See `docs/skill-format.md` for the full schema.

use std::collections::HashMap;
use std::path::{Path, PathBuf};

use pulldown_cmark::{Options, Parser};
use regex::Regex;
use serde::Deserialize;
use tokio::fs;

use sera_types::skill::{
    SkillConfig, SkillDefinition, SkillMcpServer, SkillMcpTransport, SkillMode, SkillTrigger,
};

use crate::error::SkillsError;

/// Parsed representation of a single AgentSkills markdown file.
///
/// The caller receives both a `SkillDefinition` (with the markdown body
/// attached) and a derived `SkillConfig` suitable for registration in
/// `SkillRegistry`. `body_raw` preserves the exact markdown bytes after
/// frontmatter for downstream rendering.
#[derive(Debug, Clone)]
pub struct ParsedSkillMarkdown {
    pub definition: SkillDefinition,
    pub config: SkillConfig,
    pub body_raw: String,
    pub path: PathBuf,
}

/// Intermediate frontmatter shape mirroring the AgentSkills YAML schema.
///
/// All fields except `name` are optional — absent fields map to `None`
/// or empty collections on the produced `SkillDefinition`.
#[derive(Debug, Deserialize, Default)]
#[serde(deny_unknown_fields)]
struct Frontmatter {
    name: String,

    #[serde(default)]
    version: Option<String>,

    #[serde(default)]
    description: Option<String>,

    #[serde(default)]
    triggers: Vec<String>,

    #[serde(default)]
    tools: Vec<String>,

    #[serde(default)]
    mcp_tools: Option<McpToolsFrontmatter>,

    #[serde(default)]
    model: Option<String>,

    #[serde(default)]
    context_budget_tokens: Option<u32>,

    #[serde(default)]
    source: Option<String>,
}

#[derive(Debug, Deserialize, Default)]
#[serde(deny_unknown_fields)]
struct McpToolsFrontmatter {
    #[serde(default)]
    stdio_servers: Vec<StdioServerFrontmatter>,
    #[serde(default)]
    sse_servers: Vec<UrlServerFrontmatter>,
    #[serde(default)]
    streamable_http_servers: Vec<UrlServerFrontmatter>,
}

#[derive(Debug, Deserialize, Default)]
#[serde(deny_unknown_fields)]
struct StdioServerFrontmatter {
    name: String,
    #[serde(default)]
    command: Option<String>,
    #[serde(default)]
    args: Vec<String>,
    #[serde(default)]
    env: HashMap<String, String>,
}

#[derive(Debug, Deserialize, Default)]
#[serde(deny_unknown_fields)]
struct UrlServerFrontmatter {
    name: String,
    url: String,
    #[serde(default)]
    env: HashMap<String, String>,
}

fn name_regex() -> &'static Regex {
    use std::sync::OnceLock;
    static NAME_RE: OnceLock<Regex> = OnceLock::new();
    NAME_RE.get_or_init(|| Regex::new(r"^[a-z0-9-]{1,64}$").unwrap())
}

fn version_regex() -> &'static Regex {
    use std::sync::OnceLock;
    static VERSION_RE: OnceLock<Regex> = OnceLock::new();
    // Semver-lite: MAJOR.MINOR.PATCH with optional prerelease / build metadata.
    VERSION_RE.get_or_init(|| {
        Regex::new(r"^\d+\.\d+\.\d+(?:-[0-9A-Za-z.-]+)?(?:\+[0-9A-Za-z.-]+)?$").unwrap()
    })
}

/// Split a raw file into `(yaml_frontmatter, body)`.
///
/// The file MUST start with a `---` line, followed by YAML, a closing
/// `---` line, and then the body. Either `\n` or `\r\n` line endings work.
fn split_frontmatter(raw: &str) -> Result<(&str, &str), SkillsError> {
    // Strip BOM if present.
    let trimmed = raw.strip_prefix('\u{FEFF}').unwrap_or(raw);

    // Accept both "---\n" and "---\r\n".
    let after_open = trimmed
        .strip_prefix("---\n")
        .or_else(|| trimmed.strip_prefix("---\r\n"))
        .ok_or_else(|| {
            SkillsError::Format(
                "skill markdown must start with `---` frontmatter fence".to_string(),
            )
        })?;

    // Find the closing fence on its own line.
    let mut search_start = 0usize;
    loop {
        let slice = &after_open[search_start..];
        let idx = slice.find("---").ok_or_else(|| {
            SkillsError::Format(
                "skill markdown frontmatter missing closing `---` fence".to_string(),
            )
        })?;
        let absolute = search_start + idx;
        let at_line_start = absolute == 0 || after_open.as_bytes()[absolute - 1] == b'\n';
        // Require fence to terminate with \n or \r\n or EOF.
        let after_fence = absolute + 3;
        let terminates = after_open.len() == after_fence
            || after_open.as_bytes()[after_fence] == b'\n'
            || (after_open.as_bytes()[after_fence] == b'\r'
                && after_open.len() > after_fence + 1
                && after_open.as_bytes()[after_fence + 1] == b'\n');

        if at_line_start && terminates {
            let yaml = &after_open[..absolute];
            // Skip past the fence line terminator.
            let body_start = if after_open.len() == after_fence {
                after_fence
            } else if after_open.as_bytes()[after_fence] == b'\n' {
                after_fence + 1
            } else {
                after_fence + 2
            };
            return Ok((yaml, &after_open[body_start..]));
        }
        search_start = absolute + 3;
    }
}

/// Parse a skill markdown string. `source_path` is used for error messages
/// and to populate [`ParsedSkillMarkdown::path`].
pub fn parse_skill_markdown_str(
    raw: &str,
    source_path: PathBuf,
) -> Result<ParsedSkillMarkdown, SkillsError> {
    let (yaml_section, body) = split_frontmatter(raw)?;

    let fm: Frontmatter = serde_yaml::from_str(yaml_section).map_err(|e| {
        SkillsError::Format(format!(
            "invalid YAML frontmatter in {}: {}",
            source_path.display(),
            e
        ))
    })?;

    // Validate name.
    if !name_regex().is_match(&fm.name) {
        return Err(SkillsError::Format(format!(
            "invalid skill name `{}` in {}: must match [a-z0-9-]{{1,64}}",
            fm.name,
            source_path.display()
        )));
    }

    // Validate version if present (semver-lite). Missing version is allowed
    // for now; a follow-up bead can raise this to required.
    if let Some(ref v) = fm.version
        && !version_regex().is_match(v)
    {
        return Err(SkillsError::Format(format!(
            "invalid version `{}` in {}: expected MAJOR.MINOR.PATCH",
            v,
            source_path.display()
        )));
    }

    // Validate description: if present, must contain at least one non-whitespace character.
    if let Some(ref desc) = fm.description
        && desc.trim().is_empty()
    {
        return Err(SkillsError::Format(format!(
            "field `description` in {} must be non-empty when provided (got an empty or whitespace-only string)",
            source_path.display()
        )));
    }

    // Validate triggers: each element must be non-empty and non-whitespace.
    for (i, trigger) in fm.triggers.iter().enumerate() {
        if trigger.trim().is_empty() {
            return Err(SkillsError::Format(format!(
                "field `triggers[{i}]` in {} must be a non-empty string (got {:?})",
                source_path.display(),
                trigger
            )));
        }
    }

    // Validate context_budget_tokens: 0 is not a useful budget.
    if fm.context_budget_tokens == Some(0) {
        return Err(SkillsError::Format(format!(
            "field `context_budget_tokens` in {} must be a positive integer (got 0)",
            source_path.display()
        )));
    }

    // Validate MCP server names and URLs.
    if let Some(ref mcp) = fm.mcp_tools {
        for s in &mcp.stdio_servers {
            if s.name.trim().is_empty() {
                return Err(SkillsError::Format(format!(
                    "field `mcp_tools.stdio_servers[].name` in {} must be non-empty",
                    source_path.display()
                )));
            }
        }
        for s in &mcp.sse_servers {
            if s.name.trim().is_empty() {
                return Err(SkillsError::Format(format!(
                    "field `mcp_tools.sse_servers[].name` in {} must be non-empty",
                    source_path.display()
                )));
            }
            if !s.url.starts_with("http://") && !s.url.starts_with("https://") {
                return Err(SkillsError::Format(format!(
                    "field `mcp_tools.sse_servers[].url` in {} must start with http:// or https:// (got {:?})",
                    source_path.display(),
                    s.url
                )));
            }
        }
        for s in &mcp.streamable_http_servers {
            if s.name.trim().is_empty() {
                return Err(SkillsError::Format(format!(
                    "field `mcp_tools.streamable_http_servers[].name` in {} must be non-empty",
                    source_path.display()
                )));
            }
            if !s.url.starts_with("http://") && !s.url.starts_with("https://") {
                return Err(SkillsError::Format(format!(
                    "field `mcp_tools.streamable_http_servers[].url` in {} must start with http:// or https:// (got {:?})",
                    source_path.display(),
                    s.url
                )));
            }
        }
    }

    // Validate body is syntactically valid markdown. We only scan (and drop)
    // the events; the raw body text is what the LLM will see.
    let mut opts = Options::empty();
    opts.insert(Options::ENABLE_TABLES);
    opts.insert(Options::ENABLE_FOOTNOTES);
    opts.insert(Options::ENABLE_STRIKETHROUGH);
    opts.insert(Options::ENABLE_TASKLISTS);
    for _event in Parser::new_ext(body, opts) {
        // Scanning is enough: pulldown-cmark is infallible in that it does
        // not return Result; a panic-free iteration is our validation.
    }

    // Assemble MCP server list from the three transport buckets.
    let mut mcp_servers: Vec<SkillMcpServer> = Vec::new();
    if let Some(mcp) = fm.mcp_tools.as_ref() {
        for s in &mcp.stdio_servers {
            mcp_servers.push(SkillMcpServer {
                name: s.name.clone(),
                transport: SkillMcpTransport::Stdio,
                command: s.command.clone(),
                args: s.args.clone(),
                url: None,
                env: s.env.clone(),
            });
        }
        for s in &mcp.sse_servers {
            mcp_servers.push(SkillMcpServer {
                name: s.name.clone(),
                transport: SkillMcpTransport::Sse,
                command: None,
                args: vec![],
                url: Some(s.url.clone()),
                env: s.env.clone(),
            });
        }
        for s in &mcp.streamable_http_servers {
            mcp_servers.push(SkillMcpServer {
                name: s.name.clone(),
                transport: SkillMcpTransport::StreamableHttp,
                command: None,
                args: vec![],
                url: Some(s.url.clone()),
                env: s.env.clone(),
            });
        }
    }

    let definition = SkillDefinition {
        name: fm.name.clone(),
        description: fm.description.clone(),
        version: fm.version.clone(),
        parameters: None,
        source: fm.source.clone(),
        body: Some(body.to_string()),
        triggers: fm.triggers.clone(),
        model_override: fm.model.clone(),
        context_budget_tokens: fm.context_budget_tokens,
        tool_bindings: fm.tools.clone(),
        mcp_servers,
    };

    // Derive a SkillConfig. Defaults for markdown skills:
    //   mode    = OnDemand (activation decided at runtime)
    //   trigger = Always if any keyword triggers are declared, else Manual
    //   tools   = frontmatter tools
    //   context_injection = None (body lives on the definition)
    let trigger = if fm.triggers.is_empty() {
        SkillTrigger::Manual
    } else {
        SkillTrigger::Always
    };

    let config = SkillConfig {
        name: fm.name.clone(),
        version: fm.version.clone().unwrap_or_else(|| "0.0.0".to_string()),
        description: fm.description.clone().unwrap_or_default(),
        mode: SkillMode::OnDemand,
        trigger,
        tools: fm.tools.clone(),
        context_injection: None,
        config: serde_json::json!({}),
    };

    Ok(ParsedSkillMarkdown {
        definition,
        config,
        body_raw: body.to_string(),
        path: source_path,
    })
}

/// Parse a skill markdown file from disk.
pub async fn parse_skill_markdown_file(path: &Path) -> Result<ParsedSkillMarkdown, SkillsError> {
    let raw = fs::read_to_string(path).await?;
    parse_skill_markdown_str(&raw, path.to_path_buf())
}

#[cfg(test)]
mod tests {
    use super::*;

    const VALID_SKILL: &str = r#"---
name: code-review
version: 1.0.0
description: Review code for correctness, style, and security issues
triggers:
  - review
  - audit
tools:
  - read_file
  - search_code
mcp_tools:
  stdio_servers:
    - name: github
      command: npx
      args: ["-y", "@modelcontextprotocol/server-github"]
model: claude-opus-4
context_budget_tokens: 4096
---

You are a senior code reviewer. When activated, you systematically examine code.

## Checklist

- Correctness
- Style
- Security
"#;

    #[test]
    fn parses_valid_frontmatter_and_body() {
        let parsed =
            parse_skill_markdown_str(VALID_SKILL, PathBuf::from("code-review.md")).unwrap();
        assert_eq!(parsed.definition.name, "code-review");
        assert_eq!(parsed.definition.version.as_deref(), Some("1.0.0"));
        assert_eq!(parsed.definition.triggers, vec!["review", "audit"]);
        assert_eq!(parsed.definition.tool_bindings, vec!["read_file", "search_code"]);
        assert_eq!(parsed.definition.model_override.as_deref(), Some("claude-opus-4"));
        assert_eq!(parsed.definition.context_budget_tokens, Some(4096));

        // MCP server decoded.
        assert_eq!(parsed.definition.mcp_servers.len(), 1);
        let server = &parsed.definition.mcp_servers[0];
        assert_eq!(server.name, "github");
        assert_eq!(server.transport, SkillMcpTransport::Stdio);
        assert_eq!(server.command.as_deref(), Some("npx"));
        assert_eq!(server.args, vec!["-y", "@modelcontextprotocol/server-github"]);

        // Body preserved byte-for-byte.
        assert!(parsed.body_raw.starts_with("\nYou are a senior code reviewer."));
        assert_eq!(parsed.definition.body.as_deref(), Some(parsed.body_raw.as_str()));

        // Derived config.
        assert_eq!(parsed.config.name, "code-review");
        assert_eq!(parsed.config.mode, SkillMode::OnDemand);
        assert_eq!(parsed.config.trigger, SkillTrigger::Always);
        assert_eq!(parsed.config.tools, vec!["read_file", "search_code"]);
    }

    #[test]
    fn missing_frontmatter_fails() {
        let err = parse_skill_markdown_str(
            "Just markdown without frontmatter.\n",
            PathBuf::from("x.md"),
        )
        .unwrap_err();
        assert!(matches!(err, SkillsError::Format(_)));
    }

    #[test]
    fn bad_name_rejected() {
        let raw = "---\nname: Invalid Name!\nversion: 1.0.0\n---\n\nbody\n";
        let err = parse_skill_markdown_str(raw, PathBuf::from("x.md")).unwrap_err();
        match err {
            SkillsError::Format(msg) => assert!(msg.contains("invalid skill name")),
            other => panic!("expected Format error, got {other:?}"),
        }
    }

    #[test]
    fn bad_version_rejected() {
        let raw = "---\nname: ok-name\nversion: not-semver\n---\n\nbody\n";
        let err = parse_skill_markdown_str(raw, PathBuf::from("x.md")).unwrap_err();
        match err {
            SkillsError::Format(msg) => assert!(msg.contains("invalid version")),
            other => panic!("expected Format error, got {other:?}"),
        }
    }

    #[test]
    fn missing_version_allowed() {
        let raw = "---\nname: ok-name\n---\n\nbody\n";
        let parsed = parse_skill_markdown_str(raw, PathBuf::from("x.md")).unwrap();
        assert!(parsed.definition.version.is_none());
        assert_eq!(parsed.config.version, "0.0.0");
    }

    #[test]
    fn manual_trigger_when_no_keywords() {
        let raw = "---\nname: manual-skill\nversion: 0.1.0\n---\n\nbody\n";
        let parsed = parse_skill_markdown_str(raw, PathBuf::from("x.md")).unwrap();
        assert_eq!(parsed.config.trigger, SkillTrigger::Manual);
    }

    #[test]
    fn mcp_server_populated() {
        let parsed =
            parse_skill_markdown_str(VALID_SKILL, PathBuf::from("code-review.md")).unwrap();
        let servers = &parsed.definition.mcp_servers;
        assert_eq!(servers.len(), 1);
        assert_eq!(servers[0].name, "github");
    }

    #[test]
    fn tool_bindings_populated() {
        let parsed =
            parse_skill_markdown_str(VALID_SKILL, PathBuf::from("code-review.md")).unwrap();
        assert_eq!(parsed.definition.tool_bindings.len(), 2);
    }

    #[test]
    fn body_preserved_byte_for_byte() {
        let raw = "---\nname: x\nversion: 1.0.0\n---\nExact\nbody\nbytes\n";
        let parsed = parse_skill_markdown_str(raw, PathBuf::from("x.md")).unwrap();
        assert_eq!(parsed.body_raw, "Exact\nbody\nbytes\n");
    }

    #[test]
    fn crlf_frontmatter_supported() {
        let raw = "---\r\nname: crlf-ok\r\nversion: 1.0.0\r\n---\r\nbody\r\n";
        let parsed = parse_skill_markdown_str(raw, PathBuf::from("x.md")).unwrap();
        assert_eq!(parsed.definition.name, "crlf-ok");
    }

    #[test]
    fn name_length_limit_enforced() {
        let long = "a".repeat(65);
        let raw = format!("---\nname: {long}\nversion: 1.0.0\n---\n\nbody\n");
        let err = parse_skill_markdown_str(&raw, PathBuf::from("x.md")).unwrap_err();
        assert!(matches!(err, SkillsError::Format(_)));
    }

    #[test]
    fn unknown_frontmatter_field_rejected() {
        let raw = "---\nname: x\nversion: 1.0.0\nunknown_field: 42\n---\n\nbody\n";
        let err = parse_skill_markdown_str(raw, PathBuf::from("x.md")).unwrap_err();
        assert!(matches!(err, SkillsError::Format(_)));
    }

    // --- additional gap-filling tests ---

    #[test]
    fn bom_prefix_stripped_before_parsing() {
        // UTF-8 BOM (\u{FEFF}) must be silently removed.
        let raw = "\u{FEFF}---\nname: bom-skill\nversion: 1.0.0\n---\n\nbody\n";
        let parsed = parse_skill_markdown_str(raw, PathBuf::from("bom.md")).unwrap();
        assert_eq!(parsed.definition.name, "bom-skill");
    }

    #[test]
    fn name_at_exact_max_length_accepted() {
        // The regex allows exactly 64 characters.
        let name = "a".repeat(64);
        let raw = format!("---\nname: {name}\nversion: 1.0.0\n---\n\nbody\n");
        let parsed = parse_skill_markdown_str(&raw, PathBuf::from("x.md")).unwrap();
        assert_eq!(parsed.definition.name, name);
    }

    #[test]
    fn empty_body_accepted() {
        // A skill with frontmatter but zero body bytes is valid.
        let raw = "---\nname: empty-body\nversion: 0.1.0\n---\n";
        let parsed = parse_skill_markdown_str(raw, PathBuf::from("x.md")).unwrap();
        assert_eq!(parsed.body_raw, "");
        assert_eq!(parsed.definition.body.as_deref(), Some(""));
    }

    #[test]
    fn sse_server_decoded() {
        let raw = "---\nname: sse-skill\nversion: 1.0.0\nmcp_tools:\n  sse_servers:\n    - name: remote\n      url: https://example.com/sse\n---\nbody\n";
        let parsed = parse_skill_markdown_str(raw, PathBuf::from("x.md")).unwrap();
        assert_eq!(parsed.definition.mcp_servers.len(), 1);
        let s = &parsed.definition.mcp_servers[0];
        assert_eq!(s.name, "remote");
        assert_eq!(s.transport, SkillMcpTransport::Sse);
        assert_eq!(s.url.as_deref(), Some("https://example.com/sse"));
        assert!(s.command.is_none());
    }

    #[test]
    fn streamable_http_server_decoded() {
        let raw = "---\nname: http-skill\nversion: 1.0.0\nmcp_tools:\n  streamable_http_servers:\n    - name: api\n      url: https://api.example.com/mcp\n---\nbody\n";
        let parsed = parse_skill_markdown_str(raw, PathBuf::from("x.md")).unwrap();
        assert_eq!(parsed.definition.mcp_servers.len(), 1);
        let s = &parsed.definition.mcp_servers[0];
        assert_eq!(s.transport, SkillMcpTransport::StreamableHttp);
        assert_eq!(s.url.as_deref(), Some("https://api.example.com/mcp"));
    }

    #[test]
    fn multiple_mcp_transport_types_combined() {
        let raw = r#"---
name: multi-mcp
version: 1.0.0
mcp_tools:
  stdio_servers:
    - name: stdio-one
      command: npx
  sse_servers:
    - name: sse-one
      url: https://sse.example.com
  streamable_http_servers:
    - name: http-one
      url: https://http.example.com
---
body
"#;
        let parsed = parse_skill_markdown_str(raw, PathBuf::from("x.md")).unwrap();
        assert_eq!(parsed.definition.mcp_servers.len(), 3);
    }

    #[test]
    fn missing_closing_fence_is_error() {
        let raw = "---\nname: x\nversion: 1.0.0\n\nbody without closing fence\n";
        let err = parse_skill_markdown_str(raw, PathBuf::from("x.md")).unwrap_err();
        assert!(matches!(err, SkillsError::Format(_)));
    }

    #[test]
    fn empty_string_input_does_not_panic() {
        let err = parse_skill_markdown_str("", PathBuf::from("x.md")).unwrap_err();
        assert!(matches!(err, SkillsError::Format(_)));
    }

    #[test]
    fn binary_garbage_wrapped_in_utf8_does_not_panic() {
        // Build a string from ASCII control characters (valid UTF-8, junk content).
        let garbage: String = (0u8..=31)
            .filter(|b| *b != b'\n' && *b != b'\r')
            .map(|b| b as char)
            .collect::<String>()
            + " not yaml at all \u{FFFD}";
        // Just ensure no panic; error is expected.
        let _ = parse_skill_markdown_str(&garbage, PathBuf::from("garbage.md"));
    }

    #[test]
    fn deeply_nested_yaml_does_not_panic() {
        // Construct deeply nested YAML that serde_yaml must handle.
        let nested = "a:\n  ".repeat(50) + "b: 1";
        let raw = format!("---\nname: deep\nversion: 1.0.0\nextra:\n  {nested}\n---\nbody\n");
        // deny_unknown_fields will reject it; we just want no panic.
        let _ = parse_skill_markdown_str(&raw, PathBuf::from("deep.md"));
    }

    #[test]
    fn config_version_defaults_to_zero_when_absent() {
        let raw = "---\nname: no-version\n---\nbody\n";
        let parsed = parse_skill_markdown_str(raw, PathBuf::from("x.md")).unwrap();
        assert_eq!(parsed.config.version, "0.0.0");
        assert!(parsed.definition.version.is_none());
    }

    #[test]
    fn source_field_propagated_to_definition() {
        let raw = "---\nname: sourced\nversion: 1.0.0\nsource: ghcr.io/org/pack\n---\nbody\n";
        let parsed = parse_skill_markdown_str(raw, PathBuf::from("x.md")).unwrap();
        assert_eq!(parsed.definition.source.as_deref(), Some("ghcr.io/org/pack"));
    }

    #[test]
    fn context_budget_tokens_propagated() {
        let raw = "---\nname: budgeted\nversion: 1.0.0\ncontext_budget_tokens: 8192\n---\nbody\n";
        let parsed = parse_skill_markdown_str(raw, PathBuf::from("x.md")).unwrap();
        assert_eq!(parsed.definition.context_budget_tokens, Some(8192));
    }

    // --- new gap-closing validation tests ---

    #[test]
    fn empty_description_rejected() {
        // description: "" must be rejected; field must be non-empty when present.
        let raw = "---\nname: x\nversion: 1.0.0\ndescription: \"\"\n---\nbody\n";
        let err = parse_skill_markdown_str(raw, PathBuf::from("x.md")).unwrap_err();
        match err {
            SkillsError::Format(msg) => {
                assert!(msg.contains("`description`"), "msg was: {msg}");
                assert!(msg.contains("non-empty"), "msg was: {msg}");
            }
            other => panic!("expected Format error, got {other:?}"),
        }
    }

    #[test]
    fn whitespace_only_description_rejected() {
        // description: "   " (only spaces) must also be rejected.
        let raw = "---\nname: x\nversion: 1.0.0\ndescription: \"   \"\n---\nbody\n";
        let err = parse_skill_markdown_str(raw, PathBuf::from("x.md")).unwrap_err();
        assert!(matches!(err, SkillsError::Format(_)));
    }

    #[test]
    fn absent_description_allowed() {
        // Omitting description entirely is still valid.
        let raw = "---\nname: x\nversion: 1.0.0\n---\nbody\n";
        let parsed = parse_skill_markdown_str(raw, PathBuf::from("x.md")).unwrap();
        assert!(parsed.definition.description.is_none());
    }

    #[test]
    fn empty_trigger_string_rejected() {
        // triggers: [""] — an empty element must be rejected.
        let raw = "---\nname: x\nversion: 1.0.0\ntriggers:\n  - \"\"\n---\nbody\n";
        let err = parse_skill_markdown_str(raw, PathBuf::from("x.md")).unwrap_err();
        match err {
            SkillsError::Format(msg) => {
                assert!(msg.contains("`triggers[0]`"), "msg was: {msg}");
            }
            other => panic!("expected Format error, got {other:?}"),
        }
    }

    #[test]
    fn whitespace_only_trigger_rejected() {
        // triggers: ["   "] — whitespace-only element must be rejected.
        let raw = "---\nname: x\nversion: 1.0.0\ntriggers:\n  - \"   \"\n---\nbody\n";
        let err = parse_skill_markdown_str(raw, PathBuf::from("x.md")).unwrap_err();
        assert!(matches!(err, SkillsError::Format(_)));
    }

    #[test]
    fn context_budget_tokens_zero_rejected() {
        // context_budget_tokens: 0 is not a useful budget.
        let raw = "---\nname: x\nversion: 1.0.0\ncontext_budget_tokens: 0\n---\nbody\n";
        let err = parse_skill_markdown_str(raw, PathBuf::from("x.md")).unwrap_err();
        match err {
            SkillsError::Format(msg) => {
                assert!(msg.contains("`context_budget_tokens`"), "msg was: {msg}");
                assert!(msg.contains("positive"), "msg was: {msg}");
            }
            other => panic!("expected Format error, got {other:?}"),
        }
    }

    #[test]
    fn mcp_stdio_server_empty_name_rejected() {
        let raw = "---\nname: x\nversion: 1.0.0\nmcp_tools:\n  stdio_servers:\n    - name: \"\"\n      command: npx\n---\nbody\n";
        let err = parse_skill_markdown_str(raw, PathBuf::from("x.md")).unwrap_err();
        match err {
            SkillsError::Format(msg) => {
                assert!(msg.contains("stdio_servers"), "msg was: {msg}");
                assert!(msg.contains("non-empty"), "msg was: {msg}");
            }
            other => panic!("expected Format error, got {other:?}"),
        }
    }

    #[test]
    fn mcp_sse_server_empty_name_rejected() {
        let raw = "---\nname: x\nversion: 1.0.0\nmcp_tools:\n  sse_servers:\n    - name: \"\"\n      url: https://example.com\n---\nbody\n";
        let err = parse_skill_markdown_str(raw, PathBuf::from("x.md")).unwrap_err();
        match err {
            SkillsError::Format(msg) => {
                assert!(msg.contains("sse_servers"), "msg was: {msg}");
            }
            other => panic!("expected Format error, got {other:?}"),
        }
    }

    #[test]
    fn mcp_sse_server_bad_url_rejected() {
        // URL that doesn't start with http:// or https:// must be rejected.
        let raw = "---\nname: x\nversion: 1.0.0\nmcp_tools:\n  sse_servers:\n    - name: bad\n      url: ftp://example.com\n---\nbody\n";
        let err = parse_skill_markdown_str(raw, PathBuf::from("x.md")).unwrap_err();
        match err {
            SkillsError::Format(msg) => {
                assert!(msg.contains("sse_servers"), "msg was: {msg}");
                assert!(msg.contains("http://") || msg.contains("https://"), "msg was: {msg}");
            }
            other => panic!("expected Format error, got {other:?}"),
        }
    }

    #[test]
    fn mcp_sse_server_empty_url_rejected() {
        let raw = "---\nname: x\nversion: 1.0.0\nmcp_tools:\n  sse_servers:\n    - name: ok\n      url: \"\"\n---\nbody\n";
        let err = parse_skill_markdown_str(raw, PathBuf::from("x.md")).unwrap_err();
        assert!(matches!(err, SkillsError::Format(_)));
    }

    #[test]
    fn mcp_streamable_http_server_bad_url_rejected() {
        let raw = "---\nname: x\nversion: 1.0.0\nmcp_tools:\n  streamable_http_servers:\n    - name: ok\n      url: ws://example.com\n---\nbody\n";
        let err = parse_skill_markdown_str(raw, PathBuf::from("x.md")).unwrap_err();
        match err {
            SkillsError::Format(msg) => {
                assert!(msg.contains("streamable_http_servers"), "msg was: {msg}");
            }
            other => panic!("expected Format error, got {other:?}"),
        }
    }

    #[test]
    fn mcp_http_server_http_url_accepted() {
        // Plain http:// (not https) must also be valid.
        let raw = "---\nname: x\nversion: 1.0.0\nmcp_tools:\n  streamable_http_servers:\n    - name: local\n      url: http://localhost:8080/mcp\n---\nbody\n";
        let parsed = parse_skill_markdown_str(raw, PathBuf::from("x.md")).unwrap();
        assert_eq!(parsed.definition.mcp_servers.len(), 1);
    }
}

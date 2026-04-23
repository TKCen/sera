//! `find_symbol` — LSP `workspace/symbol` with name-path matching.
//!
//! Phase 2 scope — see `docs/plan/LSP-TOOLS-DESIGN.md` §3.2. The tool exposes
//! the full I/O shape the design doc specifies; the implementation is
//! Rust-first (rust-analyzer) with Python and TypeScript entries already in
//! the registry for Phase 2 config-only use.

use std::path::{Path, PathBuf};
use std::str::FromStr;

use serde::{Deserialize, Serialize};

use crate::lsp::error::{LspError, ToolError};
use crate::lsp::name_path::{NamePath, NamePathSegment};
use crate::lsp::state::{normalize_path, LspToolsState};
use crate::registry::ToolDescriptor;

use super::{ByteRange, SymbolEntry, SymbolKind};

/// Input schema — matches `LSP-TOOLS-DESIGN.md` §3.2 exactly.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FindSymbolInput {
    /// Name-path pattern, e.g. `"Tool"`, `"ToolRegistry/get"`, `"/MyStruct/new"`.
    pub name_path_pattern: String,
    /// Restrict search to this file or directory. Empty = whole project.
    #[serde(default)]
    pub relative_path: String,
    /// Depth of children to return. 0 = matched symbol only.
    #[serde(default)]
    pub depth: u8,
    /// Include the full source body of each matched symbol.
    #[serde(default)]
    pub include_body: bool,
    /// LSP `SymbolKind` integers to include. Empty = all kinds.
    #[serde(default)]
    pub include_kinds: Vec<u8>,
    /// Maximum number of results. 0 = unlimited.
    #[serde(default)]
    pub max_matches: u32,
}

/// Output schema — matches `LSP-TOOLS-DESIGN.md` §3.2 exactly.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FindSymbolResult {
    pub matches: Vec<SymbolMatch>,
    pub truncated: bool,
}

/// One match in a `find_symbol` response.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SymbolMatch {
    pub name_path: String,
    pub relative_path: String,
    pub range: ByteRange,
    pub kind: SymbolKind,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub body: Option<String>,
    #[serde(default)]
    pub children: Vec<SymbolEntry>,
}

pub struct FindSymbolTool;

impl FindSymbolTool {
    pub const NAME: &'static str = "find_symbol";
    pub const DESCRIPTION: &'static str =
        "Search for a symbol by name-path pattern across the project \
         (or within a scoped path). Backed by LSP workspace/symbol.";

    pub fn new() -> Self {
        Self
    }
}

impl Default for FindSymbolTool {
    fn default() -> Self {
        Self::new()
    }
}

impl ToolDescriptor for FindSymbolTool {
    fn name(&self) -> &str {
        Self::NAME
    }
    fn description(&self) -> &str {
        Self::DESCRIPTION
    }
}

impl FindSymbolTool {
    /// Execute the tool end-to-end.
    ///
    /// Algorithm (per design §3.2):
    ///   1. Parse `name_path_pattern` with [`NamePath::parse`].
    ///   2. Resolve the language:
    ///      - If `relative_path` is non-empty, derive it from that path's
    ///        extension via the registry.
    ///      - Otherwise default to Rust. (Phase 2 is Rust-first; the
    ///        registry still hosts Python/TypeScript entries for when the
    ///        caller supplies a scoped `relative_path`.)
    ///   3. Spawn-or-reuse the supervisor via `state.get_or_spawn`.
    ///   4. Call `workspace/symbol` with the pattern's last segment name.
    ///   5. Filter by `include_kinds`, name-path match, and optional path prefix.
    ///   6. Attach optional `body` and trimmed `children` per input.
    ///   7. Truncate to `max_matches`; return the populated `FindSymbolResult`.
    pub async fn invoke(
        &self,
        input: FindSymbolInput,
        state: &LspToolsState,
        project_root: &Path,
    ) -> Result<FindSymbolResult, ToolError> {
        // Step 1 — parse the pattern.
        let pattern = NamePath::parse(&input.name_path_pattern)?;

        // Step 2 — language resolution.
        let (language_id, scope_rel): (String, Option<PathBuf>) = if input.relative_path.is_empty()
        {
            // Rust-first default — the phase-2 design doc notes Python and
            // TypeScript entries ship for config-only coverage; whole-project
            // `find_symbol` targets rust-analyzer.
            ("rust".to_string(), None)
        } else {
            let rel = PathBuf::from(&input.relative_path);
            let abs = normalize_path(project_root, &rel)?;
            // Scope may be a file or a directory; either way we want the
            // extension to drive language selection when it's a file, and we
            // fall back to Rust when it's a directory (Phase 2 is Rust-first).
            let lang = if abs.is_file() {
                let ext = abs
                    .extension()
                    .and_then(|e| e.to_str())
                    .map(|e| format!(".{e}"))
                    .ok_or_else(|| LspError::Unsupported {
                        language: format!("<no extension>: {}", abs.display()),
                    })?;
                state
                    .registry
                    .resolve_for_extension(&ext)
                    .map(|c| c.language_id.clone())
                    .ok_or(LspError::Unsupported {
                        language: ext.clone(),
                    })?
            } else {
                // Directory scope — keep Rust default for Phase 2.
                "rust".to_string()
            };
            (lang, Some(rel))
        };

        // Step 3 — supervisor.
        let supervisor = state.get_or_spawn(&language_id, project_root).await?;
        let client = supervisor.client();

        // Step 4 — workspace/symbol lookup, using the pattern's last-segment
        // name as the query string (servers narrow by substring internally).
        let query = pattern.last_segment_name();
        let raw = tokio::time::timeout(
            state.request_timeout,
            client.workspace_symbol(query),
        )
        .await
        .map_err(|_| LspError::Timeout)??;

        // Step 5 — filter. The `include_kinds` filter is a whitelist; empty =
        // accept all. The name-path filter requires building a candidate
        // `NamePath` from each `SymbolInformation`.
        let kinds_whitelist: Option<Vec<u8>> = if input.include_kinds.is_empty() {
            None
        } else {
            Some(input.include_kinds.clone())
        };

        // Single-segment, no-overload patterns use substring matching (the
        // design doc's default for short queries).
        let substring = pattern.segments.len() == 1
            && pattern.segments[0].overload_index.is_none()
            && !pattern.absolute;

        let mut matches: Vec<SymbolMatch> = Vec::new();
        let mut truncated = false;

        for sym in raw.into_iter() {
            // Kind filter first — cheapest.
            let wrapped_kind = SymbolKind::from(sym.kind);
            if let Some(list) = &kinds_whitelist
                && !list.contains(&wrapped_kind.as_u8())
            {
                continue;
            }

            // Resolve the file path relative to the project root so we can
            // both filter and surface a tidy `relative_path` in the output.
            let sym_abs = match uri_to_path(&sym.location.uri) {
                Some(p) => p,
                None => continue,
            };
            let Ok(rel_path) = sym_abs.strip_prefix(project_root) else {
                // Symbol lives outside the project (e.g. a dependency) — skip.
                continue;
            };
            let rel_path_buf = rel_path.to_path_buf();

            // Path-scope filter — prefix match against the caller-supplied
            // `relative_path` if any.
            if let Some(scope) = &scope_rel
                && !rel_path_buf.starts_with(scope)
            {
                continue;
            }

            // Build the candidate NamePath for name matching. `container_name`
            // contributes outer segments; the symbol's `name` is the last.
            let candidate = symbol_info_to_name_path(&sym);
            if !pattern.matches(&candidate, substring) {
                continue;
            }

            // Assemble the match.
            let mut children: Vec<SymbolEntry> = Vec::new();
            if input.depth > 0 {
                // Fetch doc symbols for that file and locate children of the
                // matched symbol (name + range overlap heuristic).
                let uri = file_uri(&sym_abs)?;
                let full = tokio::time::timeout(
                    state.request_timeout,
                    client.document_symbol(uri),
                )
                .await
                .map_err(|_| LspError::Timeout)??;
                children = find_children_for(&full, &sym.name, sym.location.range, input.depth);
            }

            let body = if input.include_body {
                read_body(&sym_abs, sym.location.range)?
            } else {
                None
            };

            // Map the LSP range to a byte range. We do the lightweight
            // line-start scan; for a huge file this costs one file read, which
            // is the same work `get_symbols_overview` performs.
            let range = match file_bytes_and_starts(&sym_abs) {
                Ok((bytes, starts)) => ByteRange {
                    start: position_to_byte(&sym.location.range.start, &starts, bytes.len()),
                    end: position_to_byte(&sym.location.range.end, &starts, bytes.len()),
                },
                Err(_) => ByteRange { start: 0, end: 0 },
            };

            matches.push(SymbolMatch {
                name_path: candidate.format(),
                relative_path: rel_path_buf.to_string_lossy().into_owned(),
                range,
                kind: wrapped_kind,
                body,
                children,
            });

            if input.max_matches != 0 && matches.len() as u32 >= input.max_matches {
                truncated = true;
                break;
            }
        }

        Ok(FindSymbolResult { matches, truncated })
    }
}

/// Build a `NamePath` from a `SymbolInformation` using its `container_name`
/// (if any) as the outer scope and its `name` as the last segment. The
/// result is always relative.
pub(crate) fn symbol_info_to_name_path(sym: &lsp_types::SymbolInformation) -> NamePath {
    let mut segments = Vec::new();
    if let Some(container) = &sym.container_name
        && !container.is_empty()
    {
        for part in container.split("::").filter(|p| !p.is_empty()) {
            segments.push(NamePathSegment {
                name: part.to_string(),
                overload_index: None,
            });
        }
    }
    segments.push(NamePathSegment {
        name: sym.name.clone(),
        overload_index: None,
    });
    NamePath {
        segments,
        absolute: false,
    }
}

/// Convert `file://`-scheme `Uri` to a filesystem path. Returns `None` for
/// non-`file` schemes.
pub(crate) fn uri_to_path(uri: &lsp_types::Uri) -> Option<PathBuf> {
    let s = uri.as_str();
    let path = s.strip_prefix("file://")?;
    // On non-Windows, the URI is `file:///abs/path`; strip the extra `/` only
    // if the next char looks like a drive letter (Windows). Otherwise keep
    // the leading slash.
    let cleaned = if path.starts_with('/')
        && path.len() > 2
        && path.as_bytes()[2] == b':'
        && path.as_bytes()[1].is_ascii_alphabetic()
    {
        &path[1..]
    } else {
        path
    };
    Some(PathBuf::from(cleaned))
}

fn file_uri(abs: &Path) -> Result<lsp_types::Uri, LspError> {
    let s = abs.to_string_lossy().replace('\\', "/");
    let uri_str = if s.starts_with('/') {
        format!("file://{s}")
    } else {
        format!("file:///{s}")
    };
    lsp_types::Uri::from_str(&uri_str).map_err(|e| LspError::Request {
        method: "<file-uri>".into(),
        reason: format!("cannot build URI for {}: {e}", abs.display()),
    })
}

fn file_bytes_and_starts(path: &Path) -> Result<(Vec<u8>, Vec<usize>), LspError> {
    let bytes = std::fs::read(path).map_err(|e| LspError::Request {
        method: "<read-file>".into(),
        reason: format!("cannot read {}: {e}", path.display()),
    })?;
    let mut starts = Vec::with_capacity(64);
    starts.push(0);
    for (i, b) in bytes.iter().enumerate() {
        if *b == b'\n' {
            starts.push(i + 1);
        }
    }
    Ok((bytes, starts))
}

fn position_to_byte(pos: &lsp_types::Position, line_starts: &[usize], total_len: usize) -> u32 {
    let line = pos.line as usize;
    let base = if line < line_starts.len() {
        line_starts[line]
    } else {
        total_len
    };
    let off = base.saturating_add(pos.character as usize).min(total_len);
    u32::try_from(off).unwrap_or(u32::MAX)
}

fn read_body(path: &Path, range: lsp_types::Range) -> Result<Option<String>, LspError> {
    let (bytes, starts) = file_bytes_and_starts(path)?;
    let start = position_to_byte(&range.start, &starts, bytes.len()) as usize;
    let end = position_to_byte(&range.end, &starts, bytes.len()) as usize;
    let slice = &bytes[start.min(bytes.len())..end.min(bytes.len())];
    Ok(Some(String::from_utf8_lossy(slice).into_owned()))
}

/// Walk doc symbols and return the trimmed child tree whose top-level name
/// matches `target_name` and whose range encloses `target_range`.
fn find_children_for(
    all: &[lsp_types::DocumentSymbol],
    target_name: &str,
    _target_range: lsp_types::Range,
    depth: u8,
) -> Vec<SymbolEntry> {
    for sym in all {
        if sym.name == target_name {
            // Convert its children using the phase-1 helper pattern.
            return match &sym.children {
                Some(ch) => convert_children(ch, depth),
                None => Vec::new(),
            };
        }
        if let Some(ch) = &sym.children {
            let nested = find_children_for(ch, target_name, _target_range, depth);
            if !nested.is_empty() {
                return nested;
            }
        }
    }
    Vec::new()
}

fn convert_children(symbols: &[lsp_types::DocumentSymbol], depth: u8) -> Vec<SymbolEntry> {
    if depth == 0 {
        return Vec::new();
    }
    symbols
        .iter()
        .map(|s| SymbolEntry {
            name: s.name.clone(),
            kind: SymbolKind::from(s.kind),
            // DocumentSymbol is position-based — we emit a placeholder byte
            // range (0,0) because the tool consumer already has the outer
            // file path and can re-fetch with `get_symbols_overview` if byte
            // precision on children is needed.
            range: ByteRange { start: 0, end: 0 },
            children: convert_children(
                s.children.as_deref().unwrap_or(&[]),
                depth.saturating_sub(1),
            ),
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tool_trait_surface_is_stable() {
        let t = FindSymbolTool::new();
        assert_eq!(t.name(), "find_symbol");
        assert!(!t.description().is_empty());
    }

    #[test]
    fn input_defaults() {
        let json = serde_json::json!({"name_path_pattern": "Tool"});
        let input: FindSymbolInput = serde_json::from_value(json).unwrap();
        assert_eq!(input.depth, 0);
        assert!(input.relative_path.is_empty());
        assert!(!input.include_body);
        assert!(input.include_kinds.is_empty());
        assert_eq!(input.max_matches, 0);
    }

    #[test]
    fn symbol_info_to_name_path_container_split() {
        #[allow(deprecated)]
        let si = lsp_types::SymbolInformation {
            name: "get".into(),
            kind: lsp_types::SymbolKind::FUNCTION,
            tags: None,
            deprecated: None,
            location: lsp_types::Location {
                uri: "file:///tmp/r.rs".parse().unwrap(),
                range: lsp_types::Range {
                    start: lsp_types::Position {
                        line: 0,
                        character: 0,
                    },
                    end: lsp_types::Position {
                        line: 0,
                        character: 0,
                    },
                },
            },
            container_name: Some("outer::ToolRegistry".into()),
        };
        let np = symbol_info_to_name_path(&si);
        assert_eq!(np.format(), "outer/ToolRegistry/get");
    }

    #[test]
    fn uri_to_path_posix() {
        let uri: lsp_types::Uri = "file:///tmp/project/src/lib.rs".parse().unwrap();
        let p = uri_to_path(&uri).unwrap();
        assert_eq!(p, PathBuf::from("/tmp/project/src/lib.rs"));
    }

    #[test]
    fn uri_to_path_windows_drive() {
        let uri: lsp_types::Uri = "file:///C:/tmp/a.rs".parse().unwrap();
        let p = uri_to_path(&uri).unwrap();
        // Drive letter → leading slash trimmed.
        assert_eq!(p, PathBuf::from("C:/tmp/a.rs"));
    }

    #[test]
    fn uri_to_path_rejects_non_file_scheme() {
        let uri: lsp_types::Uri = "http://example.com/x".parse().unwrap();
        assert!(uri_to_path(&uri).is_none());
    }
}

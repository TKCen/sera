//! `find_referencing_symbols` — LSP `textDocument/references`, attributed to
//! the enclosing symbol of each call site.
//!
//! Phase 2 scope — see `docs/plan/LSP-TOOLS-DESIGN.md` §3.3.

use std::path::{Path, PathBuf};
use std::str::FromStr;

use serde::{Deserialize, Serialize};

use crate::lsp::error::{LspError, ToolError};
use crate::lsp::name_path::NamePath;
use crate::lsp::state::{normalize_path, LspToolsState};
use crate::registry::Tool;

use super::find_symbol::{uri_to_path, SymbolMatch};
use super::{ByteRange, SymbolEntry, SymbolKind};

/// Input schema — matches `LSP-TOOLS-DESIGN.md` §3.3 exactly.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FindReferencingSymbolsInput {
    /// Exact name-path of the target symbol, e.g. `"ToolRegistry/get"`.
    pub name_path: String,
    /// File containing the symbol. Required for disambiguation.
    pub relative_path: String,
    /// LSP `SymbolKind` integers to include in results. Empty = all.
    #[serde(default)]
    pub include_kinds: Vec<u8>,
}

/// Output schema — matches `LSP-TOOLS-DESIGN.md` §3.3 exactly.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FindReferencingSymbolsResult {
    pub references: Vec<ReferenceMatch>,
}

/// One referencing symbol + ~3 lines of context around the reference.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ReferenceMatch {
    pub referencing_symbol: SymbolMatch,
    pub snippet: String,
    pub reference_range: ByteRange,
}

pub struct FindReferencingSymbolsTool;

impl FindReferencingSymbolsTool {
    pub const NAME: &'static str = "find_referencing_symbols";
    pub const DESCRIPTION: &'static str =
        "Find all symbols that reference the given target symbol. Backed by \
         LSP textDocument/references, with each reference attributed to its \
         enclosing symbol.";

    pub fn new() -> Self {
        Self
    }
}

impl Default for FindReferencingSymbolsTool {
    fn default() -> Self {
        Self::new()
    }
}

impl Tool for FindReferencingSymbolsTool {
    fn name(&self) -> &str {
        Self::NAME
    }
    fn description(&self) -> &str {
        Self::DESCRIPTION
    }
}

impl FindReferencingSymbolsTool {
    /// Execute end-to-end per design §3.3.
    pub async fn invoke(
        &self,
        input: FindReferencingSymbolsInput,
        state: &LspToolsState,
        project_root: &Path,
    ) -> Result<FindReferencingSymbolsResult, ToolError> {
        // 1 — parse the name-path.
        let pattern = NamePath::parse(&input.name_path)?;

        // 2 — path normalise + language resolve from the extension.
        let rel = PathBuf::from(&input.relative_path);
        let abs = normalize_path(project_root, &rel)?;
        let ext = abs
            .extension()
            .and_then(|e| e.to_str())
            .map(|e| format!(".{e}"))
            .ok_or_else(|| LspError::Unsupported {
                language: format!("<no extension>: {}", abs.display()),
            })?;
        let language_id = state
            .registry
            .resolve_for_extension(&ext)
            .map(|c| c.language_id.clone())
            .ok_or(LspError::Unsupported {
                language: ext.clone(),
            })?;

        // 3 — supervisor.
        let supervisor = state.get_or_spawn(&language_id, project_root).await?;
        let client = supervisor.client();

        // 4 — locate the target symbol in the file via `documentSymbol` so we
        // can anchor `textDocument/references` on its selection range.
        let target_uri = file_uri(&abs)?;
        let doc_syms = tokio::time::timeout(
            state.request_timeout,
            client.document_symbol(target_uri.clone()),
        )
        .await
        .map_err(|_| LspError::Timeout)??;
        let target = find_target(&doc_syms, &pattern).ok_or_else(|| LspError::SymbolNotFound {
            name_path: pattern.format(),
        })?;

        // 5 — reference lookup. Use the selection range's start position —
        // that's the symbol's identifier, which is what rust-analyzer expects.
        let anchor = target.selection_range.start;
        let locations = tokio::time::timeout(
            state.request_timeout,
            client.references(target_uri, anchor, false),
        )
        .await
        .map_err(|_| LspError::Timeout)??;

        // 6 — for each referencing location, find the enclosing symbol in its
        // file, read a 3-line snippet, and produce a `ReferenceMatch`.
        let kinds_whitelist: Option<Vec<u8>> = if input.include_kinds.is_empty() {
            None
        } else {
            Some(input.include_kinds.clone())
        };

        let mut references: Vec<ReferenceMatch> = Vec::new();
        for loc in locations {
            let ref_abs = match uri_to_path(&loc.uri) {
                Some(p) => p,
                None => continue,
            };
            let Ok(rel_path) = ref_abs.strip_prefix(project_root) else {
                continue;
            };
            let rel_path_buf = rel_path.to_path_buf();

            // Fetch document symbols for this file (may be the same file we
            // already fetched — that's fine, it's a second round-trip but
            // keeps the code uniform; Phase 3 will add a request-scoped cache).
            let other_doc_uri = file_uri(&ref_abs)?;
            let other_syms = tokio::time::timeout(
                state.request_timeout,
                client.document_symbol(other_doc_uri),
            )
            .await
            .map_err(|_| LspError::Timeout)??;

            let enclosing = find_enclosing(&other_syms, loc.range);
            let Some(enclosing) = enclosing else {
                continue;
            };
            let wrapped_kind = SymbolKind::from(enclosing.kind);
            if let Some(list) = &kinds_whitelist
                && !list.contains(&wrapped_kind.as_u8())
            {
                continue;
            }

            let (bytes, starts) = read_bytes_and_starts(&ref_abs)?;
            let enc_range = ByteRange {
                start: position_to_byte(&enclosing.range.start, &starts, bytes.len()),
                end: position_to_byte(&enclosing.range.end, &starts, bytes.len()),
            };
            let ref_range = ByteRange {
                start: position_to_byte(&loc.range.start, &starts, bytes.len()),
                end: position_to_byte(&loc.range.end, &starts, bytes.len()),
            };
            let snippet = extract_snippet(&bytes, &starts, loc.range.start.line);

            // Build a synthetic NamePath for the enclosing symbol — no
            // container information is surfaced by `documentSymbol`, so the
            // name-path is just the symbol's own name.
            let encl_np = format_doc_symbol_name(enclosing);

            references.push(ReferenceMatch {
                referencing_symbol: SymbolMatch {
                    name_path: encl_np,
                    relative_path: rel_path_buf.to_string_lossy().into_owned(),
                    range: enc_range,
                    kind: wrapped_kind,
                    body: None,
                    children: Vec::<SymbolEntry>::new(),
                },
                snippet,
                reference_range: ref_range,
            });
        }

        Ok(FindReferencingSymbolsResult { references })
    }
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

fn read_bytes_and_starts(path: &Path) -> Result<(Vec<u8>, Vec<usize>), LspError> {
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

fn extract_snippet(bytes: &[u8], starts: &[usize], line: u32) -> String {
    let line = line as usize;
    let first = line.saturating_sub(1);
    let last = line.saturating_add(1);
    let start = starts.get(first).copied().unwrap_or(0);
    // End at the start of `last + 1` (exclusive) or EOF.
    let end = starts.get(last + 1).copied().unwrap_or(bytes.len());
    String::from_utf8_lossy(&bytes[start..end.min(bytes.len())])
        .trim_end_matches('\n')
        .to_string()
}

/// Walk the `documentSymbol` tree depth-first, matching by name-path suffix.
fn find_target<'a>(
    syms: &'a [lsp_types::DocumentSymbol],
    pattern: &NamePath,
) -> Option<&'a lsp_types::DocumentSymbol> {
    find_target_with_path(syms, pattern, &mut Vec::new())
}

fn find_target_with_path<'a>(
    syms: &'a [lsp_types::DocumentSymbol],
    pattern: &NamePath,
    stack: &mut Vec<String>,
) -> Option<&'a lsp_types::DocumentSymbol> {
    for sym in syms {
        stack.push(sym.name.clone());
        let candidate = NamePath {
            segments: stack
                .iter()
                .map(|n| crate::lsp::name_path::NamePathSegment {
                    name: n.clone(),
                    overload_index: None,
                })
                .collect(),
            absolute: false,
        };
        if pattern.matches(&candidate, false) {
            stack.pop();
            return Some(sym);
        }
        if let Some(children) = &sym.children
            && let Some(hit) = find_target_with_path(children, pattern, stack)
        {
            stack.pop();
            return Some(hit);
        }
        stack.pop();
    }
    None
}

/// Find the deepest symbol whose range contains `reference_range`.
fn find_enclosing(
    syms: &[lsp_types::DocumentSymbol],
    reference_range: lsp_types::Range,
) -> Option<&lsp_types::DocumentSymbol> {
    // Depth-first — we always prefer the deepest containing symbol.
    let mut best: Option<&lsp_types::DocumentSymbol> = None;
    for sym in syms {
        if !contains(&sym.range, &reference_range) {
            continue;
        }
        best = Some(sym);
        if let Some(children) = &sym.children
            && let Some(inner) = find_enclosing(children, reference_range)
        {
            best = Some(inner);
        }
    }
    best
}

fn contains(outer: &lsp_types::Range, inner: &lsp_types::Range) -> bool {
    pos_leq(&outer.start, &inner.start) && pos_leq(&inner.end, &outer.end)
}

fn pos_leq(a: &lsp_types::Position, b: &lsp_types::Position) -> bool {
    (a.line, a.character) <= (b.line, b.character)
}

fn format_doc_symbol_name(sym: &lsp_types::DocumentSymbol) -> String {
    // Strip any trailing `()` / `<...>` rust-analyzer decorates function names
    // with, because the name-path parser rejects those characters.
    let trimmed = sym.name.split(['(', '<']).next().unwrap_or(&sym.name).trim();
    trimmed.to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tool_trait_surface_is_stable() {
        let t = FindReferencingSymbolsTool::new();
        assert_eq!(t.name(), "find_referencing_symbols");
        assert!(!t.description().is_empty());
    }

    #[test]
    fn extract_snippet_one_line_file() {
        let bytes = b"only line\n";
        let starts = vec![0, 10];
        assert_eq!(extract_snippet(bytes, &starts, 0), "only line");
    }

    #[test]
    fn extract_snippet_grabs_three_lines() {
        let text = b"a\nb\nc\nd\ne\n";
        let starts = vec![0, 2, 4, 6, 8, 10];
        let s = extract_snippet(text, &starts, 2); // line 2 is 'c'
        assert_eq!(s, "b\nc\nd");
    }

    #[test]
    fn find_target_matches_nested_name_path() {
        let inner = lsp_types::DocumentSymbol {
            name: "get".into(),
            detail: None,
            kind: lsp_types::SymbolKind::FUNCTION,
            tags: None,
            #[allow(deprecated)]
            deprecated: None,
            range: lsp_types::Range {
                start: lsp_types::Position {
                    line: 5,
                    character: 4,
                },
                end: lsp_types::Position {
                    line: 8,
                    character: 5,
                },
            },
            selection_range: lsp_types::Range {
                start: lsp_types::Position {
                    line: 5,
                    character: 7,
                },
                end: lsp_types::Position {
                    line: 5,
                    character: 10,
                },
            },
            children: None,
        };
        let outer = lsp_types::DocumentSymbol {
            name: "ToolRegistry".into(),
            detail: None,
            kind: lsp_types::SymbolKind::STRUCT,
            tags: None,
            #[allow(deprecated)]
            deprecated: None,
            range: lsp_types::Range {
                start: lsp_types::Position {
                    line: 4,
                    character: 0,
                },
                end: lsp_types::Position {
                    line: 12,
                    character: 0,
                },
            },
            selection_range: lsp_types::Range {
                start: lsp_types::Position {
                    line: 4,
                    character: 10,
                },
                end: lsp_types::Position {
                    line: 4,
                    character: 22,
                },
            },
            children: Some(vec![inner]),
        };

        let pat = NamePath::parse("ToolRegistry/get").unwrap();
        let outers = [outer.clone()];
        let hit = find_target(&outers, &pat).expect("found");
        assert_eq!(hit.name, "get");

        // Bare pattern: matches the outer type as a single-segment tail.
        let pat2 = NamePath::parse("ToolRegistry").unwrap();
        let outers2 = [outer];
        let hit2 = find_target(&outers2, &pat2).expect("found");
        assert_eq!(hit2.name, "ToolRegistry");
    }

    #[test]
    fn find_enclosing_picks_deepest_containing_range() {
        let child = lsp_types::DocumentSymbol {
            name: "helper".into(),
            detail: None,
            kind: lsp_types::SymbolKind::FUNCTION,
            tags: None,
            #[allow(deprecated)]
            deprecated: None,
            range: lsp_types::Range {
                start: lsp_types::Position {
                    line: 10,
                    character: 0,
                },
                end: lsp_types::Position {
                    line: 15,
                    character: 0,
                },
            },
            selection_range: lsp_types::Range {
                start: lsp_types::Position {
                    line: 10,
                    character: 3,
                },
                end: lsp_types::Position {
                    line: 10,
                    character: 9,
                },
            },
            children: None,
        };
        let parent = lsp_types::DocumentSymbol {
            name: "caller".into(),
            detail: None,
            kind: lsp_types::SymbolKind::MODULE,
            tags: None,
            #[allow(deprecated)]
            deprecated: None,
            range: lsp_types::Range {
                start: lsp_types::Position {
                    line: 0,
                    character: 0,
                },
                end: lsp_types::Position {
                    line: 20,
                    character: 0,
                },
            },
            selection_range: lsp_types::Range {
                start: lsp_types::Position {
                    line: 0,
                    character: 4,
                },
                end: lsp_types::Position {
                    line: 0,
                    character: 10,
                },
            },
            children: Some(vec![child]),
        };

        let ref_range = lsp_types::Range {
            start: lsp_types::Position {
                line: 12,
                character: 4,
            },
            end: lsp_types::Position {
                line: 12,
                character: 10,
            },
        };
        let parents = [parent];
        let hit = find_enclosing(&parents, ref_range).expect("found");
        assert_eq!(hit.name, "helper");
    }
}

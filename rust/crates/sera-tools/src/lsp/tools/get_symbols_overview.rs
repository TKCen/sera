//! `get_symbols_overview` — top-level symbols of a file or directory.
//!
//! Phase 1b scope — Rust-only via rust-analyzer, end-to-end invocation. See
//! `docs/plan/LSP-TOOLS-DESIGN.md` §3.1.

use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use crate::lsp::cache::CacheKey;
use crate::lsp::error::{LspError, ToolError};
use crate::lsp::state::{normalize_path, LspToolsState};
use crate::registry::Tool;

#[cfg(test)]
use std::sync::Arc;
#[cfg(test)]
use std::time::Duration;
#[cfg(test)]
use tokio::io::{AsyncRead, AsyncWrite};
#[cfg(test)]
use crate::lsp::client::LspClient;

use super::{ByteRange, SymbolEntry, SymbolKind, SymbolsOverview};

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

/// Tool implementation — Phase 1b wires `invoke` end-to-end through the
/// supervisor registry, per-request timeout, and symbol cache.
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
    /// Fetch a top-level (or depth-trimmed) symbol overview for one file.
    ///
    /// Algorithm (see `docs/plan/LSP-TOOLS-DESIGN.md` §3.1):
    ///   1. Normalise `input.path` against `project_root` (rejects `..` / absolute).
    ///   2. Reject directories with `Unsupported { language: "directory" }`
    ///      (Phase 2 will add aggregation).
    ///   3. Resolve the language-server config by extension.
    ///   4. Cache lookup by `(project_root, rel_path, server_version, mtime)`.
    ///      Hit → return cached vector unchanged.
    ///   5. Miss → spawn/reuse a supervisor, call `textDocument/documentSymbol`
    ///      under a `state.request_timeout` budget.
    ///   6. Convert LSP ranges to byte offsets via a one-shot line-start table,
    ///      trim to the requested `depth`, store in cache, return.
    pub async fn invoke(
        &self,
        input: GetSymbolsOverviewInput,
        state: &LspToolsState,
        project_root: &Path,
    ) -> Result<SymbolsOverview, ToolError> {
        let rel_path = PathBuf::from(&input.path);
        let abs_path = normalize_path(project_root, &rel_path)?;

        // Step 2: directory rejection with explicit logging.
        let metadata = std::fs::metadata(&abs_path).map_err(|e| LspError::Request {
            method: "<stat>".into(),
            reason: format!("cannot stat {}: {e}", abs_path.display()),
        })?;
        if metadata.is_dir() {
            tracing::debug!(
                path = %abs_path.display(),
                "get_symbols_overview rejecting directory input — directory aggregation is Phase 2"
            );
            return Err(LspError::Unsupported {
                language: "directory".into(),
            });
        }

        // Step 3: extension → language resolution.
        let ext = abs_path
            .extension()
            .and_then(|e| e.to_str())
            .map(|e| format!(".{e}"))
            .ok_or_else(|| LspError::Unsupported {
                language: format!("<no extension>: {}", abs_path.display()),
            })?;
        let (language_id, server_command) = {
            let cfg = state.registry.resolve_for_extension(&ext).ok_or_else(|| {
                LspError::Unsupported {
                    language: ext.clone(),
                }
            })?;
            (cfg.language_id.clone(), cfg.command.clone())
        };

        // Step 4: cache lookup. We key on the `server_command` string per the
        // bead — once the supervisor is live we will also mix in the `serverInfo`
        // version, but `command` alone is enough to invalidate across `rust-analyzer`
        // path changes. `mtime` covers file-level invalidation.
        let mtime = metadata.modified().map_err(|e| LspError::Request {
            method: "<stat-mtime>".into(),
            reason: format!("cannot read mtime for {}: {e}", abs_path.display()),
        })?;
        let cache_key = CacheKey {
            project_root: project_root.to_path_buf(),
            relative_path: rel_path.clone(),
            server_version: server_command,
            mtime,
        };
        if let Some(cached) = state.cache.get(&cache_key) {
            let trimmed = trim_depth(&cached, input.depth);
            return Ok(SymbolsOverview {
                path: input.path,
                language: language_id,
                symbols: trimmed,
            });
        }

        // Step 5: spawn/reuse a supervisor, issue the request under a timeout.
        let supervisor = state.get_or_spawn(&language_id, project_root).await?;
        let client = supervisor.client();
        let uri = file_uri(&abs_path)?;
        let doc_symbols = tokio::time::timeout(
            state.request_timeout,
            client.document_symbol(uri),
        )
        .await
        .map_err(|_| LspError::Timeout)??;

        // Step 6: convert to byte ranges, trim to requested depth, cache, return.
        let file_bytes = std::fs::read(&abs_path).map_err(|e| LspError::Request {
            method: "<read-file>".into(),
            reason: format!("cannot read {}: {e}", abs_path.display()),
        })?;
        let line_starts = build_line_starts(&file_bytes);
        let full = convert_symbols(&doc_symbols, &line_starts, file_bytes.len());
        state.cache.put(cache_key, full.clone());
        let trimmed = trim_depth(&full, input.depth);
        Ok(SymbolsOverview {
            path: input.path,
            language: language_id,
            symbols: trimmed,
        })
    }

    /// Test-only helper that skips registry+spawn and drives a caller-supplied
    /// client. Kept behind `cfg(test)` so it cannot leak into production code
    /// that would bypass cache/timeout wiring.
    #[cfg(test)]
    pub(crate) async fn invoke_with_client<W, R>(
        input: GetSymbolsOverviewInput,
        state: &LspToolsState,
        project_root: &Path,
        language_id: &str,
        server_version: &str,
        client: Arc<LspClient<W, R>>,
    ) -> Result<SymbolsOverview, ToolError>
    where
        W: AsyncWrite + Unpin + Send + 'static,
        R: AsyncRead + Unpin + Send + 'static,
    {
        let rel_path = PathBuf::from(&input.path);
        let abs_path = normalize_path(project_root, &rel_path)?;
        let metadata = std::fs::metadata(&abs_path).map_err(|e| LspError::Request {
            method: "<stat>".into(),
            reason: format!("cannot stat {}: {e}", abs_path.display()),
        })?;
        if metadata.is_dir() {
            return Err(LspError::Unsupported {
                language: "directory".into(),
            });
        }
        let mtime = metadata.modified().map_err(|e| LspError::Request {
            method: "<stat-mtime>".into(),
            reason: format!("cannot read mtime for {}: {e}", abs_path.display()),
        })?;
        let cache_key = CacheKey {
            project_root: project_root.to_path_buf(),
            relative_path: rel_path.clone(),
            server_version: server_version.to_string(),
            mtime,
        };
        if let Some(cached) = state.cache.get(&cache_key) {
            let trimmed = trim_depth(&cached, input.depth);
            return Ok(SymbolsOverview {
                path: input.path,
                language: language_id.to_string(),
                symbols: trimmed,
            });
        }
        let uri = file_uri(&abs_path)?;
        let doc_symbols = tokio::time::timeout(
            state.request_timeout,
            client.document_symbol(uri),
        )
        .await
        .map_err(|_| LspError::Timeout)??;
        let file_bytes = std::fs::read(&abs_path).map_err(|e| LspError::Request {
            method: "<read-file>".into(),
            reason: format!("cannot read {}: {e}", abs_path.display()),
        })?;
        let line_starts = build_line_starts(&file_bytes);
        let full = convert_symbols(&doc_symbols, &line_starts, file_bytes.len());
        state.cache.put(cache_key, full.clone());
        let trimmed = trim_depth(&full, input.depth);
        Ok(SymbolsOverview {
            path: input.path,
            language: language_id.to_string(),
            symbols: trimmed,
        })
    }
}

/// Build `file://`-scheme URI for an absolute path.
fn file_uri(abs: &Path) -> Result<lsp_types::Uri, LspError> {
    use std::str::FromStr;
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

/// One-shot line-start table — `line_starts[n]` is the byte offset at which
/// line `n` (0-indexed) begins. Used to convert LSP `Position { line, character }`
/// to byte offsets. Per-call only; not a persistent cache.
fn build_line_starts(bytes: &[u8]) -> Vec<usize> {
    let mut starts = Vec::with_capacity(64);
    starts.push(0);
    for (i, b) in bytes.iter().enumerate() {
        if *b == b'\n' {
            starts.push(i + 1);
        }
    }
    starts
}

/// Convert an LSP `Position { line, character }` into a byte offset, clamped
/// to the file's total byte length. `character` is treated as UTF-16 code
/// units per the LSP spec, which matches what rust-analyzer emits; for ASCII
/// files (the vast majority of Rust source) this equals byte offset.
///
/// Phase 1b uses character-as-byte. Multi-byte character precision is a
/// Phase 2 follow-up — this matches `LSP-TOOLS-DESIGN.md` §3.1 note about
/// UTF-16 fidelity landing later.
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

/// Convert `Vec<DocumentSymbol>` to `Vec<SymbolEntry>` with full child trees.
/// Depth trimming is applied separately by [`trim_depth`].
fn convert_symbols(
    symbols: &[lsp_types::DocumentSymbol],
    line_starts: &[usize],
    total_len: usize,
) -> Vec<SymbolEntry> {
    symbols
        .iter()
        .map(|s| SymbolEntry {
            name: s.name.clone(),
            kind: SymbolKind::from(s.kind),
            range: ByteRange {
                start: position_to_byte(&s.range.start, line_starts, total_len),
                end: position_to_byte(&s.range.end, line_starts, total_len),
            },
            children: match &s.children {
                Some(ch) => convert_symbols(ch, line_starts, total_len),
                None => Vec::new(),
            },
        })
        .collect()
}

/// Trim every `SymbolEntry` tree so that no node deeper than `depth` carries
/// children. `depth == 0` means drop all children; `depth == 1` keeps one
/// level of children; and so on.
fn trim_depth(entries: &[SymbolEntry], depth: u8) -> Vec<SymbolEntry> {
    entries
        .iter()
        .map(|e| trim_depth_one(e, depth))
        .collect()
}

fn trim_depth_one(entry: &SymbolEntry, depth: u8) -> SymbolEntry {
    if depth == 0 {
        return SymbolEntry {
            name: entry.name.clone(),
            kind: entry.kind,
            range: entry.range.clone(),
            children: Vec::new(),
        };
    }
    SymbolEntry {
        name: entry.name.clone(),
        kind: entry.kind,
        range: entry.range.clone(),
        children: entry
            .children
            .iter()
            .map(|c| trim_depth_one(c, depth - 1))
            .collect(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::lsp::client::LspTransport;
    use crate::lsp::jsonrpc::{read_framed, write_framed};
    use crate::lsp::registry::{LspServerConfig, LspServerRegistry};
    use std::io::Write;
    use std::sync::atomic::{AtomicUsize, Ordering};
    use tempfile::TempDir;
    use tokio::io::BufReader;

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

    #[test]
    fn build_line_starts_empty_file() {
        let starts = build_line_starts(b"");
        assert_eq!(starts, vec![0]);
    }

    #[test]
    fn build_line_starts_multiple_lines() {
        //                    0 1 2 3 4 5 6
        let bytes = b"ab\ncd\ne";
        let starts = build_line_starts(bytes);
        assert_eq!(starts, vec![0, 3, 6]);
    }

    #[test]
    fn position_to_byte_clamps_past_eof() {
        let total = 10;
        let pos = lsp_types::Position {
            line: 99,
            character: 99,
        };
        assert_eq!(position_to_byte(&pos, &[0], total), total as u32);
    }

    #[test]
    fn trim_depth_zero_drops_all_children() {
        let s = SymbolEntry {
            name: "root".into(),
            kind: SymbolKind::new(23),
            range: ByteRange { start: 0, end: 1 },
            children: vec![SymbolEntry {
                name: "child".into(),
                kind: SymbolKind::new(8),
                range: ByteRange { start: 2, end: 3 },
                children: vec![],
            }],
        };
        let got = trim_depth(&[s], 0);
        assert_eq!(got.len(), 1);
        assert!(got[0].children.is_empty());
    }

    #[test]
    fn trim_depth_one_keeps_immediate_children_only() {
        let s = SymbolEntry {
            name: "root".into(),
            kind: SymbolKind::new(23),
            range: ByteRange { start: 0, end: 1 },
            children: vec![SymbolEntry {
                name: "child".into(),
                kind: SymbolKind::new(8),
                range: ByteRange { start: 2, end: 3 },
                children: vec![SymbolEntry {
                    name: "grandchild".into(),
                    kind: SymbolKind::new(8),
                    range: ByteRange { start: 4, end: 5 },
                    children: vec![],
                }],
            }],
        };
        let got = trim_depth(&[s], 1);
        assert_eq!(got[0].children.len(), 1);
        assert!(got[0].children[0].children.is_empty());
    }

    // -----------------------------------------------------------------------
    // Mock LSP server infrastructure
    // -----------------------------------------------------------------------

    /// Build a pipe-pair backed `LspClient` plus an invocation counter.
    /// The server task responds to one `textDocument/documentSymbol` request
    /// with the supplied canned JSON result, then exits.
    async fn spawn_mock_with_result(
        result: serde_json::Value,
        delay: Option<Duration>,
    ) -> (
        Arc<LspClient<tokio::io::DuplexStream, tokio::io::DuplexStream>>,
        Arc<AtomicUsize>,
    ) {
        let (client_w, mut server_r) = tokio::io::duplex(8192);
        let (mut server_w, client_r) = tokio::io::duplex(8192);
        let counter = Arc::new(AtomicUsize::new(0));
        let counter_task = counter.clone();

        tokio::spawn(async move {
            let mut reader = BufReader::new(&mut server_r);
            let Ok(body) = read_framed(&mut reader).await else {
                return;
            };
            counter_task.fetch_add(1, Ordering::SeqCst);
            let req: serde_json::Value = serde_json::from_slice(&body).unwrap();
            let id = req["id"].as_i64().unwrap();
            if let Some(d) = delay {
                tokio::time::sleep(d).await;
            }
            let resp = serde_json::json!({
                "jsonrpc": "2.0",
                "id": id,
                "result": result,
            });
            let _ = write_framed(&mut server_w, &resp).await;
        });

        let transport = Arc::new(LspTransport::new(client_w, client_r));
        let client = Arc::new(LspClient::new(transport));
        (client, counter)
    }

    /// Create a temp-dir project with `src/lib.rs` containing `content`.
    fn fixture_project(content: &str) -> TempDir {
        let dir = tempfile::tempdir().expect("tempdir");
        let src = dir.path().join("src");
        std::fs::create_dir_all(&src).unwrap();
        let mut f = std::fs::File::create(src.join("lib.rs")).unwrap();
        f.write_all(content.as_bytes()).unwrap();
        f.sync_all().unwrap();
        dir
    }

    fn rust_state() -> LspToolsState {
        let mut reg = LspServerRegistry::new();
        reg.register(LspServerConfig {
            language_id: "rust".into(),
            command: "rust-analyzer-mock".into(),
            args: vec![],
            extensions: vec![".rs".into()],
            initialization_options: serde_json::json!({}),
        });
        LspToolsState::new(reg)
    }

    #[tokio::test]
    async fn invoke_with_client_converts_symbols_and_trims_depth() {
        let dir = fixture_project("pub struct Foo;\npub fn bar() {}\n");
        let state = rust_state();
        let canned = serde_json::json!([
            {
                "name": "Foo",
                "kind": 23,
                "range": {"start": {"line": 0, "character": 0}, "end": {"line": 0, "character": 15}},
                "selectionRange": {"start": {"line": 0, "character": 11}, "end": {"line": 0, "character": 14}},
                "children": [
                    {
                        "name": "inner",
                        "kind": 8,
                        "range": {"start": {"line": 0, "character": 11}, "end": {"line": 0, "character": 14}},
                        "selectionRange": {"start": {"line": 0, "character": 11}, "end": {"line": 0, "character": 14}}
                    }
                ]
            },
            {
                "name": "bar",
                "kind": 12,
                "range": {"start": {"line": 1, "character": 0}, "end": {"line": 1, "character": 15}},
                "selectionRange": {"start": {"line": 1, "character": 7}, "end": {"line": 1, "character": 10}}
            }
        ]);
        let (client, _counter) = spawn_mock_with_result(canned, None).await;
        let overview = GetSymbolsOverviewTool::invoke_with_client(
            GetSymbolsOverviewInput {
                path: "src/lib.rs".into(),
                depth: 0,
            },
            &state,
            dir.path(),
            "rust",
            "rust-analyzer-mock",
            client,
        )
        .await
        .expect("ok");
        assert_eq!(overview.language, "rust");
        assert_eq!(overview.symbols.len(), 2);
        assert_eq!(overview.symbols[0].name, "Foo");
        assert_eq!(overview.symbols[0].kind, SymbolKind::new(23));
        // depth=0 dropped children
        assert!(overview.symbols[0].children.is_empty());
        // Byte ranges computed from line-start table
        assert_eq!(overview.symbols[0].range.start, 0);
        assert_eq!(overview.symbols[1].name, "bar");
        // Cache populated
        assert_eq!(state.cache.len(), 1);
    }

    #[tokio::test]
    async fn invoke_rejects_path_traversal() {
        let dir = fixture_project("pub fn x() {}\n");
        let state = rust_state();
        let tool = GetSymbolsOverviewTool::new();
        let err = tool
            .invoke(
                GetSymbolsOverviewInput {
                    path: "../escape.rs".into(),
                    depth: 0,
                },
                &state,
                dir.path(),
            )
            .await
            .expect_err("must reject");
        assert!(matches!(err, LspError::PathTraversal));
    }

    #[tokio::test]
    async fn invoke_rejects_unknown_extension() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("notes.txt"), "hello").unwrap();
        let state = rust_state();
        let tool = GetSymbolsOverviewTool::new();
        let err = tool
            .invoke(
                GetSymbolsOverviewInput {
                    path: "notes.txt".into(),
                    depth: 0,
                },
                &state,
                dir.path(),
            )
            .await
            .expect_err("unknown ext");
        match err {
            LspError::Unsupported { language } => assert_eq!(language, ".txt"),
            other => panic!("unexpected: {other:?}"),
        }
    }

    #[tokio::test]
    async fn invoke_rejects_directory_input() {
        let dir = fixture_project("pub fn x() {}\n");
        let state = rust_state();
        let tool = GetSymbolsOverviewTool::new();
        let err = tool
            .invoke(
                GetSymbolsOverviewInput {
                    path: "src".into(),
                    depth: 0,
                },
                &state,
                dir.path(),
            )
            .await
            .expect_err("directory");
        match err {
            LspError::Unsupported { language } => assert_eq!(language, "directory"),
            other => panic!("unexpected: {other:?}"),
        }
    }

    #[tokio::test]
    async fn invoke_cache_hit_skips_lsp_call() {
        let dir = fixture_project("pub fn x() {}\n");
        let state = rust_state();

        // Pre-populate cache for the exact key `invoke_with_client` will compute.
        let abs = dir.path().join("src/lib.rs");
        let mtime = std::fs::metadata(&abs).unwrap().modified().unwrap();
        let key = CacheKey {
            project_root: dir.path().to_path_buf(),
            relative_path: PathBuf::from("src/lib.rs"),
            server_version: "rust-analyzer-mock".into(),
            mtime,
        };
        let cached = vec![SymbolEntry {
            name: "CachedHit".into(),
            kind: SymbolKind::new(23),
            range: ByteRange { start: 0, end: 5 },
            children: vec![],
        }];
        state.cache.put(key, cached);

        // Build a mock that would panic the test if actually called — we assert
        // the counter stays at zero.
        let (client, counter) =
            spawn_mock_with_result(serde_json::json!([]), None).await;

        let overview = GetSymbolsOverviewTool::invoke_with_client(
            GetSymbolsOverviewInput {
                path: "src/lib.rs".into(),
                depth: 0,
            },
            &state,
            dir.path(),
            "rust",
            "rust-analyzer-mock",
            client,
        )
        .await
        .expect("cache hit");
        assert_eq!(overview.symbols.len(), 1);
        assert_eq!(overview.symbols[0].name, "CachedHit");
        assert_eq!(
            counter.load(Ordering::SeqCst),
            0,
            "mock supervisor must NOT be called on cache hit"
        );
    }

    #[tokio::test]
    async fn invoke_timeout_exceeds_budget() {
        let dir = fixture_project("pub fn x() {}\n");
        let mut state = rust_state();
        state.request_timeout = Duration::from_millis(50);

        // Server that sleeps 200ms before responding — well past 50ms budget.
        let (client, _counter) = spawn_mock_with_result(
            serde_json::json!([]),
            Some(Duration::from_millis(200)),
        )
        .await;

        let err = GetSymbolsOverviewTool::invoke_with_client(
            GetSymbolsOverviewInput {
                path: "src/lib.rs".into(),
                depth: 0,
            },
            &state,
            dir.path(),
            "rust",
            "rust-analyzer-mock",
            client,
        )
        .await
        .expect_err("must time out");
        assert!(matches!(err, LspError::Timeout), "got: {err:?}");
    }
}

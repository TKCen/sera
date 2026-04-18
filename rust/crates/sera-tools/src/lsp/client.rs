//! Thin LSP client facade — parses JSON-RPC responses into `lsp-types` shapes.
//!
//! Phase 1 surface:
//! * `initialize` — performs the LSP handshake on a fresh pipe pair.
//! * `document_symbol` — resolves `textDocument/documentSymbol` into a
//!   `Vec<DocumentSymbol>`.
//! * `workspace_symbol` — stubbed for Phase 2; returns `Unsupported`.
//!
//! The client is generic over its `AsyncRead`/`AsyncWrite` streams so that
//! unit tests can drive it over in-memory `tokio::io::duplex` pipes rather
//! than launching a real language server.

use std::sync::Arc;
use tokio::io::{AsyncRead, AsyncWrite, BufReader};
use tokio::sync::Mutex;

use lsp_types::{
    ClientCapabilities, DocumentSymbol, DocumentSymbolParams, DocumentSymbolResponse,
    InitializeParams, InitializeResult, InitializedParams, Location, OneOf, PartialResultParams,
    Position, ReferenceContext, ReferenceParams, SymbolInformation, TextDocumentIdentifier,
    TextDocumentPositionParams, Uri, WorkDoneProgressParams, WorkspaceSymbol,
    WorkspaceSymbolParams, WorkspaceSymbolResponse,
};

use super::error::LspError;
use super::jsonrpc::{
    read_framed, write_framed, RequestIdGen, RpcNotification, RpcRequest, RpcResponse,
};

/// A duplex LSP transport: write to `stdin`, read from `stdout`.
pub struct LspTransport<W, R>
where
    W: AsyncWrite + Unpin + Send + 'static,
    R: AsyncRead + Unpin + Send + 'static,
{
    pub writer: Mutex<W>,
    pub reader: Mutex<BufReader<R>>,
}

impl<W, R> LspTransport<W, R>
where
    W: AsyncWrite + Unpin + Send + 'static,
    R: AsyncRead + Unpin + Send + 'static,
{
    pub fn new(writer: W, reader: R) -> Self {
        Self {
            writer: Mutex::new(writer),
            reader: Mutex::new(BufReader::new(reader)),
        }
    }
}

/// Thin LSP client facade. Owned by `LspProcessSupervisor` in production.
pub struct LspClient<W, R>
where
    W: AsyncWrite + Unpin + Send + 'static,
    R: AsyncRead + Unpin + Send + 'static,
{
    transport: Arc<LspTransport<W, R>>,
    ids: RequestIdGen,
}

impl<W, R> LspClient<W, R>
where
    W: AsyncWrite + Unpin + Send + 'static,
    R: AsyncRead + Unpin + Send + 'static,
{
    pub fn new(transport: Arc<LspTransport<W, R>>) -> Self {
        Self {
            transport,
            ids: RequestIdGen::default(),
        }
    }

    /// Perform the LSP `initialize` handshake plus `initialized` notification.
    ///
    /// The response is returned so callers can stash the server's declared
    /// version into the `SymbolCache` key.
    pub async fn initialize(
        &self,
        params: InitializeParams,
    ) -> Result<InitializeResult, LspError> {
        let result = self
            .request::<_, InitializeResult>("initialize", params)
            .await?;
        self.notify("initialized", InitializedParams {}).await?;
        Ok(result)
    }

    /// Send `textDocument/documentSymbol` and collect the flat or nested
    /// response into `Vec<DocumentSymbol>`.
    pub async fn document_symbol(&self, uri: Uri) -> Result<Vec<DocumentSymbol>, LspError> {
        let params = DocumentSymbolParams {
            text_document: TextDocumentIdentifier { uri },
            work_done_progress_params: Default::default(),
            partial_result_params: Default::default(),
        };
        let resp = self
            .request::<_, Option<DocumentSymbolResponse>>("textDocument/documentSymbol", params)
            .await?;
        Ok(match resp {
            None => Vec::new(),
            Some(DocumentSymbolResponse::Nested(v)) => v,
            Some(DocumentSymbolResponse::Flat(flat)) => flat
                .into_iter()
                .map(|si| {
                    #[allow(deprecated)]
                    DocumentSymbol {
                        name: si.name,
                        detail: None,
                        kind: si.kind,
                        tags: si.tags,
                        deprecated: si.deprecated,
                        range: si.location.range,
                        selection_range: si.location.range,
                        children: None,
                    }
                })
                .collect(),
        })
    }

    /// Send `workspace/symbol` and normalise the response to
    /// `Vec<SymbolInformation>`.
    ///
    /// LSP 3.17 introduced `WorkspaceSymbol` alongside the legacy
    /// `SymbolInformation`; servers may respond with either shape. rust-analyzer
    /// emits the flat `SymbolInformation` variant as of this writing. We accept
    /// both and flatten to the older, location-bearing shape so downstream
    /// tools always have a concrete `Location`. `WorkspaceSymbol` entries with
    /// a range-less `WorkspaceLocation` become `SymbolInformation` with a
    /// zero-width range anchored at `(0, 0)` — the caller will have to issue
    /// `workspace/symbol/resolve` to fill it in (phase 3).
    pub async fn workspace_symbol(
        &self,
        query: &str,
    ) -> Result<Vec<SymbolInformation>, LspError> {
        let params = WorkspaceSymbolParams {
            query: query.to_string(),
            partial_result_params: PartialResultParams::default(),
            work_done_progress_params: WorkDoneProgressParams::default(),
        };
        let resp = self
            .request::<_, Option<WorkspaceSymbolResponse>>("workspace/symbol", params)
            .await?;
        Ok(match resp {
            None => Vec::new(),
            Some(WorkspaceSymbolResponse::Flat(v)) => v,
            Some(WorkspaceSymbolResponse::Nested(v)) => v
                .into_iter()
                .map(workspace_symbol_to_flat)
                .collect(),
        })
    }

    /// Send `textDocument/references` and return the list of `Location`s.
    ///
    /// Mirrors the LSP method name. `include_declaration = false` matches the
    /// design-doc default for `find_referencing_symbols`.
    pub async fn references(
        &self,
        uri: Uri,
        position: Position,
        include_declaration: bool,
    ) -> Result<Vec<Location>, LspError> {
        let params = ReferenceParams {
            text_document_position: TextDocumentPositionParams {
                text_document: TextDocumentIdentifier { uri },
                position,
            },
            work_done_progress_params: WorkDoneProgressParams::default(),
            partial_result_params: PartialResultParams::default(),
            context: ReferenceContext {
                include_declaration,
            },
        };
        let resp = self
            .request::<_, Option<Vec<Location>>>("textDocument/references", params)
            .await?;
        Ok(resp.unwrap_or_default())
    }

    /// Issue a typed JSON-RPC request and decode the `result` field.
    async fn request<P, T>(&self, method: &str, params: P) -> Result<T, LspError>
    where
        P: serde::Serialize,
        T: serde::de::DeserializeOwned,
    {
        let id = self.ids.next();
        let req = RpcRequest {
            jsonrpc: "2.0",
            id,
            method,
            params,
        };

        {
            let mut w = self.transport.writer.lock().await;
            write_framed(&mut *w, &req).await?;
        }

        // Loop until we see a response whose id matches ours. Server-to-client
        // requests ($/progress, window/logMessage, …) are drained and ignored.
        let mut reader = self.transport.reader.lock().await;
        loop {
            let body = read_framed(&mut *reader).await?;
            let resp: RpcResponse =
                serde_json::from_slice(&body).map_err(|e| LspError::Request {
                    method: method.to_string(),
                    reason: format!("malformed response: {e}"),
                })?;
            if resp.id != Some(id) {
                continue; // not for us
            }
            if let Some(err) = resp.error {
                return Err(LspError::Request {
                    method: method.to_string(),
                    reason: err.message,
                });
            }
            let value = resp.result.unwrap_or(serde_json::Value::Null);
            return serde_json::from_value(value).map_err(|e| LspError::Request {
                method: method.to_string(),
                reason: format!("result decode failed: {e}"),
            });
        }
    }

    /// Send a JSON-RPC notification (no id, no response expected).
    async fn notify<P: serde::Serialize>(&self, method: &str, params: P) -> Result<(), LspError> {
        let n = RpcNotification {
            jsonrpc: "2.0",
            method,
            params,
        };
        let mut w = self.transport.writer.lock().await;
        write_framed(&mut *w, &n).await
    }
}

/// Flatten a 3.17 `WorkspaceSymbol` to the older `SymbolInformation` shape
/// so downstream callers can treat the result uniformly.
#[allow(deprecated)]
fn workspace_symbol_to_flat(sym: WorkspaceSymbol) -> SymbolInformation {
    let location = match sym.location {
        OneOf::Left(loc) => loc,
        OneOf::Right(wloc) => Location {
            uri: wloc.uri,
            range: lsp_types::Range {
                start: Position {
                    line: 0,
                    character: 0,
                },
                end: Position {
                    line: 0,
                    character: 0,
                },
            },
        },
    };
    SymbolInformation {
        name: sym.name,
        kind: sym.kind,
        tags: sym.tags,
        deprecated: None,
        location,
        container_name: sym.container_name,
    }
}

/// Minimal `InitializeParams` with the capabilities the tools actually need.
pub fn default_initialize_params(root_uri: Option<Uri>) -> InitializeParams {
    #[allow(deprecated)]
    InitializeParams {
        process_id: Some(std::process::id()),
        root_path: None,
        root_uri,
        initialization_options: None,
        capabilities: ClientCapabilities::default(),
        trace: None,
        workspace_folders: None,
        client_info: None,
        locale: None,
        work_done_progress_params: Default::default(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::lsp::jsonrpc::{read_framed, write_framed};

    /// A one-shot hand-written mock LSP server. It runs in a spawned task,
    /// reads one request from the client's `stdin` pipe and writes a canned
    /// response to the client's `stdout` pipe. See module comment above.
    async fn mock_document_symbol_server(
        mut client_to_server: tokio::io::DuplexStream,
        mut server_to_client: tokio::io::DuplexStream,
        canned: serde_json::Value,
    ) {
        let mut reader = BufReader::new(&mut client_to_server);
        let body = read_framed(&mut reader).await.expect("read request");
        let req: serde_json::Value = serde_json::from_slice(&body).unwrap();
        let id = req["id"].as_i64().unwrap();
        let resp = serde_json::json!({
            "jsonrpc": "2.0",
            "id": id,
            "result": canned,
        });
        write_framed(&mut server_to_client, &resp).await.unwrap();
    }

    #[tokio::test]
    async fn document_symbol_parses_realistic_rust_analyzer_response() {
        // Two duplex pipes simulate the server's stdin/stdout.
        let (client_w, server_r) = tokio::io::duplex(4096);
        let (server_w, client_r) = tokio::io::duplex(4096);

        // rust-analyzer returns nested DocumentSymbol[].
        let canned = serde_json::json!([
            {
                "name": "Tool",
                "kind": 11,
                "range": {"start": {"line": 4, "character": 0}, "end": {"line": 12, "character": 0}},
                "selectionRange": {"start": {"line": 4, "character": 10}, "end": {"line": 4, "character": 14}},
                "children": [
                    {
                        "name": "execute",
                        "kind": 6,
                        "range": {"start": {"line": 5, "character": 4}, "end": {"line": 8, "character": 5}},
                        "selectionRange": {"start": {"line": 5, "character": 7}, "end": {"line": 5, "character": 14}}
                    }
                ]
            },
            {
                "name": "ToolRegistry",
                "kind": 23,
                "range": {"start": {"line": 14, "character": 0}, "end": {"line": 42, "character": 1}},
                "selectionRange": {"start": {"line": 14, "character": 11}, "end": {"line": 14, "character": 23}}
            }
        ]);

        tokio::spawn(mock_document_symbol_server(server_r, server_w, canned));

        let transport = Arc::new(LspTransport::new(client_w, client_r));
        let client = LspClient::new(transport);

        let symbols = client
            .document_symbol("file:///tmp/lib.rs".parse::<Uri>().unwrap())
            .await
            .expect("document_symbol must succeed");

        assert_eq!(symbols.len(), 2);
        assert_eq!(symbols[0].name, "Tool");
        assert_eq!(symbols[0].kind, lsp_types::SymbolKind::INTERFACE); // 11
        let children = symbols[0].children.as_ref().unwrap();
        assert_eq!(children.len(), 1);
        assert_eq!(children[0].name, "execute");
        assert_eq!(symbols[1].name, "ToolRegistry");
        assert_eq!(symbols[1].kind, lsp_types::SymbolKind::STRUCT); // 23
    }

    #[tokio::test]
    async fn null_result_yields_empty_vec() {
        let (client_w, server_r) = tokio::io::duplex(1024);
        let (server_w, client_r) = tokio::io::duplex(1024);
        tokio::spawn(mock_document_symbol_server(
            server_r,
            server_w,
            serde_json::Value::Null,
        ));

        let transport = Arc::new(LspTransport::new(client_w, client_r));
        let client = LspClient::new(transport);
        let symbols = client
            .document_symbol("file:///empty.rs".parse::<Uri>().unwrap())
            .await
            .unwrap();
        assert!(symbols.is_empty());
    }

    #[tokio::test]
    async fn server_error_is_surfaced() {
        let (mut client_w, mut server_r) = tokio::io::duplex(1024);
        let (mut server_w, client_r) = tokio::io::duplex(1024);

        tokio::spawn(async move {
            let mut reader = BufReader::new(&mut server_r);
            let body = read_framed(&mut reader).await.unwrap();
            let req: serde_json::Value = serde_json::from_slice(&body).unwrap();
            let id = req["id"].as_i64().unwrap();
            let resp = serde_json::json!({
                "jsonrpc": "2.0",
                "id": id,
                "error": {"code": -32601, "message": "method not found"},
            });
            write_framed(&mut server_w, &resp).await.unwrap();
        });

        let transport = Arc::new(LspTransport::new(
            // swap into the single-writer pipe
            {
                let stream: &mut tokio::io::DuplexStream = &mut client_w;
                let _ = stream; // silence unused warning
                client_w
            },
            client_r,
        ));
        let client = LspClient::new(transport);
        let err = client
            .document_symbol("file:///x.rs".parse::<Uri>().unwrap())
            .await
            .expect_err("must fail");
        match err {
            LspError::Request { method, reason } => {
                assert_eq!(method, "textDocument/documentSymbol");
                assert!(reason.contains("method not found"));
            }
            other => panic!("unexpected error: {other:?}"),
        }
    }

    /// Generic one-shot mock — drives the client-side pipes and replies with
    /// `canned` under the request's own id. Used by the phase 2 workspace
    /// symbol / references tests so they can share the same pipe plumbing as
    /// `mock_document_symbol_server`.
    async fn mock_one_shot(
        mut client_to_server: tokio::io::DuplexStream,
        mut server_to_client: tokio::io::DuplexStream,
        canned: serde_json::Value,
    ) {
        let mut reader = BufReader::new(&mut client_to_server);
        let body = read_framed(&mut reader).await.expect("read request");
        let req: serde_json::Value = serde_json::from_slice(&body).unwrap();
        let id = req["id"].as_i64().unwrap();
        let resp = serde_json::json!({
            "jsonrpc": "2.0",
            "id": id,
            "result": canned,
        });
        write_framed(&mut server_to_client, &resp).await.unwrap();
    }

    #[tokio::test]
    async fn workspace_symbol_parses_flat_response() {
        let (client_w, server_r) = tokio::io::duplex(4096);
        let (server_w, client_r) = tokio::io::duplex(4096);

        // rust-analyzer typically replies with the flat SymbolInformation[] shape.
        let canned = serde_json::json!([
            {
                "name": "Tool",
                "kind": 11,
                "location": {
                    "uri": "file:///tmp/project/src/tool.rs",
                    "range": {
                        "start": {"line": 4, "character": 0},
                        "end": {"line": 4, "character": 14}
                    }
                },
                "containerName": "registry"
            },
            {
                "name": "ToolRegistry",
                "kind": 23,
                "location": {
                    "uri": "file:///tmp/project/src/registry.rs",
                    "range": {
                        "start": {"line": 14, "character": 0},
                        "end": {"line": 42, "character": 1}
                    }
                }
            }
        ]);
        tokio::spawn(mock_one_shot(server_r, server_w, canned));

        let transport = Arc::new(LspTransport::new(client_w, client_r));
        let client = LspClient::new(transport);
        let syms = client
            .workspace_symbol("Tool")
            .await
            .expect("workspace_symbol must succeed");
        assert_eq!(syms.len(), 2);
        assert_eq!(syms[0].name, "Tool");
        assert_eq!(syms[0].kind, lsp_types::SymbolKind::INTERFACE);
        assert_eq!(syms[0].container_name.as_deref(), Some("registry"));
        assert_eq!(syms[1].name, "ToolRegistry");
    }

    #[tokio::test]
    async fn workspace_symbol_accepts_nested_3_17_shape() {
        let (client_w, server_r) = tokio::io::duplex(4096);
        let (server_w, client_r) = tokio::io::duplex(4096);
        // 3.17 `WorkspaceSymbol[]` with a real Location.
        let canned = serde_json::json!([
            {
                "name": "Widget",
                "kind": 23,
                "location": {
                    "uri": "file:///tmp/a.rs",
                    "range": {
                        "start": {"line": 0, "character": 0},
                        "end": {"line": 0, "character": 6}
                    }
                }
            }
        ]);
        tokio::spawn(mock_one_shot(server_r, server_w, canned));

        let transport = Arc::new(LspTransport::new(client_w, client_r));
        let client = LspClient::new(transport);
        let syms = client.workspace_symbol("Widget").await.expect("ok");
        assert_eq!(syms.len(), 1);
        assert_eq!(syms[0].name, "Widget");
    }

    #[tokio::test]
    async fn workspace_symbol_null_result_is_empty() {
        let (client_w, server_r) = tokio::io::duplex(1024);
        let (server_w, client_r) = tokio::io::duplex(1024);
        tokio::spawn(mock_one_shot(server_r, server_w, serde_json::Value::Null));

        let transport = Arc::new(LspTransport::new(client_w, client_r));
        let client = LspClient::new(transport);
        let syms = client.workspace_symbol("nope").await.unwrap();
        assert!(syms.is_empty());
    }

    #[tokio::test]
    async fn references_roundtrips_locations() {
        let (client_w, server_r) = tokio::io::duplex(4096);
        let (server_w, client_r) = tokio::io::duplex(4096);
        let canned = serde_json::json!([
            {
                "uri": "file:///tmp/callers.rs",
                "range": {
                    "start": {"line": 10, "character": 4},
                    "end": {"line": 10, "character": 10}
                }
            },
            {
                "uri": "file:///tmp/other.rs",
                "range": {
                    "start": {"line": 0, "character": 0},
                    "end": {"line": 0, "character": 3}
                }
            }
        ]);
        tokio::spawn(mock_one_shot(server_r, server_w, canned));

        let transport = Arc::new(LspTransport::new(client_w, client_r));
        let client = LspClient::new(transport);
        let locs = client
            .references(
                "file:///tmp/target.rs".parse::<Uri>().unwrap(),
                lsp_types::Position {
                    line: 5,
                    character: 4,
                },
                false,
            )
            .await
            .expect("references must succeed");
        assert_eq!(locs.len(), 2);
        assert_eq!(locs[0].range.start.line, 10);
    }

    #[tokio::test]
    async fn references_null_result_is_empty() {
        let (client_w, server_r) = tokio::io::duplex(512);
        let (server_w, client_r) = tokio::io::duplex(512);
        tokio::spawn(mock_one_shot(server_r, server_w, serde_json::Value::Null));
        let transport = Arc::new(LspTransport::new(client_w, client_r));
        let client = LspClient::new(transport);
        let locs = client
            .references(
                "file:///tmp/x.rs".parse::<Uri>().unwrap(),
                lsp_types::Position {
                    line: 0,
                    character: 0,
                },
                false,
            )
            .await
            .unwrap();
        assert!(locs.is_empty());
    }
}

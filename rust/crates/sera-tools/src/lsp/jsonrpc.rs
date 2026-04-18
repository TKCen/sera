//! Minimal JSON-RPC 2.0 framing for the LSP transport.
//!
//! Phase 1 ships a small hand-written framer so that unit tests can drive the
//! supervisor/client over `tokio::io::duplex` pipes without launching a real
//! language-server subprocess. The crate also pulls in `async-lsp` as a
//! workspace dependency for richer framing needs in Phase 2.
//!
//! Protocol reference: <https://microsoft.github.io/language-server-protocol/specifications/base/>

use serde::{Deserialize, Serialize};
use std::sync::atomic::{AtomicI64, Ordering};
use tokio::io::{AsyncBufReadExt, AsyncReadExt, AsyncWriteExt, BufReader};

use super::error::LspError;

/// A JSON-RPC 2.0 request envelope.
#[derive(Debug, Serialize)]
pub struct RpcRequest<'a, P: Serialize> {
    pub jsonrpc: &'static str,
    pub id: i64,
    pub method: &'a str,
    pub params: P,
}

/// A JSON-RPC 2.0 notification (fire-and-forget — no `id` field).
#[derive(Debug, Serialize)]
pub struct RpcNotification<'a, P: Serialize> {
    pub jsonrpc: &'static str,
    pub method: &'a str,
    pub params: P,
}

/// A JSON-RPC 2.0 response envelope.
#[derive(Debug, Deserialize)]
pub struct RpcResponse {
    #[allow(dead_code)]
    pub jsonrpc: String,
    pub id: Option<i64>,
    #[serde(default)]
    pub result: Option<serde_json::Value>,
    #[serde(default)]
    pub error: Option<RpcError>,
}

#[derive(Debug, Deserialize)]
pub struct RpcError {
    #[allow(dead_code)]
    pub code: i64,
    pub message: String,
}

/// Monotonic request-id generator, one per connection.
#[derive(Debug, Default)]
pub struct RequestIdGen(AtomicI64);

impl RequestIdGen {
    pub fn next(&self) -> i64 {
        self.0.fetch_add(1, Ordering::Relaxed).saturating_add(1)
    }
}

/// Write one JSON-RPC message with LSP framing: `Content-Length: N\r\n\r\n` + body.
pub async fn write_framed<W: AsyncWriteExt + Unpin, T: Serialize>(
    writer: &mut W,
    value: &T,
) -> Result<(), LspError> {
    let body = serde_json::to_vec(value)
        .map_err(|e| LspError::Request {
            method: "<serialize>".into(),
            reason: e.to_string(),
        })?;
    let header = format!("Content-Length: {}\r\n\r\n", body.len());
    writer
        .write_all(header.as_bytes())
        .await
        .map_err(|e| LspError::Request {
            method: "<io>".into(),
            reason: e.to_string(),
        })?;
    writer
        .write_all(&body)
        .await
        .map_err(|e| LspError::Request {
            method: "<io>".into(),
            reason: e.to_string(),
        })?;
    writer.flush().await.map_err(|e| LspError::Request {
        method: "<flush>".into(),
        reason: e.to_string(),
    })?;
    Ok(())
}

/// Read one framed JSON-RPC message. Returns the raw body bytes.
pub async fn read_framed<R: tokio::io::AsyncRead + Unpin>(
    reader: &mut BufReader<R>,
) -> Result<Vec<u8>, LspError> {
    let mut content_length: Option<usize> = None;

    loop {
        let mut line = String::new();
        let n = reader.read_line(&mut line).await.map_err(|e| LspError::Request {
            method: "<read-header>".into(),
            reason: e.to_string(),
        })?;
        if n == 0 {
            return Err(LspError::Request {
                method: "<read-header>".into(),
                reason: "unexpected EOF reading LSP header".into(),
            });
        }
        let trimmed = line.trim_end_matches(['\r', '\n']);
        if trimmed.is_empty() {
            break; // end of headers
        }
        if let Some(rest) = trimmed.strip_prefix("Content-Length:") {
            content_length = Some(rest.trim().parse::<usize>().map_err(|e| LspError::Request {
                method: "<parse-content-length>".into(),
                reason: e.to_string(),
            })?);
        }
        // Other headers (e.g. Content-Type) are ignored per LSP spec.
    }

    let len = content_length.ok_or_else(|| LspError::Request {
        method: "<read-header>".into(),
        reason: "missing Content-Length header".into(),
    })?;
    let mut body = vec![0u8; len];
    reader
        .read_exact(&mut body)
        .await
        .map_err(|e| LspError::Request {
            method: "<read-body>".into(),
            reason: e.to_string(),
        })?;
    Ok(body)
}

#[cfg(test)]
mod tests {
    use super::*;
    use tokio::io::BufReader;

    #[tokio::test]
    async fn write_then_read_roundtrip() {
        let (mut w, r) = tokio::io::duplex(1024);
        let mut reader = BufReader::new(r);

        let req = RpcRequest {
            jsonrpc: "2.0",
            id: 7,
            method: "ping",
            params: serde_json::json!({"hello": "world"}),
        };
        write_framed(&mut w, &req).await.unwrap();

        let body = read_framed(&mut reader).await.unwrap();
        let parsed: serde_json::Value = serde_json::from_slice(&body).unwrap();
        assert_eq!(parsed["id"], 7);
        assert_eq!(parsed["method"], "ping");
        assert_eq!(parsed["params"]["hello"], "world");
    }

    #[test]
    fn request_id_gen_is_monotonic() {
        let ids = RequestIdGen::default();
        let a = ids.next();
        let b = ids.next();
        assert!(b > a);
    }
}

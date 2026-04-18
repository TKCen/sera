//! Server-Sent Events (SSE) client helper for streaming `/api/chat` turns.
//!
//! The autonomous gateway's `/api/chat` endpoint supports streaming when the
//! request body contains `"stream": true`.  It emits SSE frames of the shape:
//!
//! ```text
//! event: message
//! data: {"delta":"hello ", "session_id":"...", "message_id":"..."}
//!
//! event: done
//! data: {"status":"complete","usage":{...}}
//! ```
//!
//! This module wraps a `reqwest::Response::bytes_stream()` in a manual
//! line-based parser (SSE framing is simple enough that pulling in
//! `eventsource-stream` for two event types would be overkill).  The parser
//! emits [`StreamEvent`] variants the caller can render one-shot.
//!
//! The design deliberately mirrors `sera-tui::client::StreamEvent` (which
//! reads the same SSE stream) so the two crates produce identical output
//! shapes; a future refactor can fold them into a shared `sera-client`
//! crate — filed as a follow-up, out of scope for this bead.

use std::pin::Pin;

use anyhow::{Context, Result};
use futures_util::stream::Stream;
use futures_util::StreamExt;
use reqwest::Client;
use serde_json::Value;

/// A decoded streaming event.
///
/// The variants align with the gateway's emitted SSE event names plus a
/// synthetic `Other` for forward compatibility.  Unrecognised events are
/// preserved rather than dropped so the REPL can display novel signals
/// (memory pressure, tool invocations, ...) as the gateway grows.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum StreamEvent {
    /// A partial token chunk of the assistant reply.
    Token { delta: String, session_id: String },
    /// A tool invocation step (e.g. the gateway fired `bash`).
    ToolCall { name: String, args: String },
    /// Tool call completed — carries the observation/result string.
    ToolResult { name: String, result: String },
    /// A HITL permission request has been raised and is awaiting approval.
    HitlPending { id: String },
    /// Memory budget crossed a threshold — carries a human-readable message.
    MemoryPressure { message: String },
    /// Terminal event — the turn is complete; optional usage stats.
    Done { usage: Option<Value> },
    /// The gateway reported an error during the turn.
    Error { message: String },
    /// A recognised event whose kind didn't map to a known variant.
    Other { event: String, data: Value },
}

impl StreamEvent {
    /// Parse a single SSE frame (one or more lines ending in `\n\n`) into
    /// a [`StreamEvent`].  Returns `None` for keep-alives, comments, or
    /// frames whose `data:` payload is malformed / missing.
    pub fn parse_frame(frame: &str) -> Option<Self> {
        let mut event_name = "message".to_owned();
        let mut data_buf = String::new();
        for line in frame.lines() {
            if line.is_empty() || line.starts_with(':') {
                continue; // comment / keep-alive
            }
            if let Some(v) = line.strip_prefix("event:") {
                event_name = v.trim().to_owned();
            } else if let Some(v) = line.strip_prefix("data:") {
                if !data_buf.is_empty() {
                    data_buf.push('\n');
                }
                data_buf.push_str(v.trim_start());
            }
        }
        if data_buf.trim().is_empty() {
            return None;
        }
        let json: Value = serde_json::from_str(data_buf.trim()).ok()?;
        Some(Self::from_event(&event_name, json))
    }

    fn from_event(event: &str, data: Value) -> Self {
        let str_at = |key: &str| -> String {
            data.get(key)
                .and_then(|v| v.as_str())
                .unwrap_or("")
                .to_owned()
        };

        match event {
            "message" | "token" => {
                // The gateway emits {"delta":"word ", "session_id":"..", "message_id":".."}
                let delta = str_at("delta");
                let session_id = str_at("session_id");
                StreamEvent::Token { delta, session_id }
            }
            "done" => StreamEvent::Done {
                usage: data.get("usage").cloned(),
            },
            "tool_call" => StreamEvent::ToolCall {
                name: str_at("name"),
                args: data.get("args").map(|v| v.to_string()).unwrap_or_default(),
            },
            "tool_result" => StreamEvent::ToolResult {
                name: str_at("name"),
                result: data.get("result").map(|v| match v {
                    Value::String(s) => s.clone(),
                    other => other.to_string(),
                }).unwrap_or_default(),
            },
            "hitl_pending" => StreamEvent::HitlPending {
                id: str_at("id"),
            },
            "memory_pressure" => StreamEvent::MemoryPressure {
                message: str_at("message"),
            },
            "error" => StreamEvent::Error {
                message: str_at("message"),
            },
            other => StreamEvent::Other {
                event: other.to_owned(),
                data,
            },
        }
    }
}

/// Thin HTTP-SSE client.
///
/// A new client is constructed per `stream()` call rather than held on the
/// struct because `reqwest::Client` already pools connections internally.
pub struct SseClient {
    http: Client,
    base_url: String,
}

impl SseClient {
    pub fn new(http: Client, base_url: impl Into<String>) -> Self {
        Self {
            http,
            base_url: base_url.into().trim_end_matches('/').to_owned(),
        }
    }

    /// POST `path` with `body` and return a stream of parsed SSE events.
    ///
    /// The caller is responsible for sending the correct JSON body — for
    /// the gateway's `/api/chat` that means `{"agent": "...", "message":
    /// "...", "stream": true}`.
    pub async fn post_stream(
        &self,
        path: &str,
        body: Value,
    ) -> Result<Pin<Box<dyn Stream<Item = Result<StreamEvent>> + Send>>> {
        let url = format!("{}{}", self.base_url, path);
        let resp = self
            .http
            .post(&url)
            .header("Accept", "text/event-stream")
            .header("Content-Type", "application/json")
            .json(&body)
            .send()
            .await
            .with_context(|| format!("POST {url} failed"))?;

        let status = resp.status();
        if !status.is_success() {
            let body_text = resp.text().await.unwrap_or_default();
            anyhow::bail!("gateway returned HTTP {status}: {body_text}");
        }

        Ok(Box::pin(parse_event_stream(resp.bytes_stream())))
    }

    /// GET `path` and stream SSE events (used by `chat` to subscribe to an
    /// existing session's stream if the gateway exposes one).
    #[allow(dead_code)]
    pub async fn get_stream(
        &self,
        path: &str,
    ) -> Result<Pin<Box<dyn Stream<Item = Result<StreamEvent>> + Send>>> {
        let url = format!("{}{}", self.base_url, path);
        let resp = self
            .http
            .get(&url)
            .header("Accept", "text/event-stream")
            .send()
            .await
            .with_context(|| format!("GET {url} failed"))?;

        let status = resp.status();
        if !status.is_success() {
            let body_text = resp.text().await.unwrap_or_default();
            anyhow::bail!("gateway returned HTTP {status}: {body_text}");
        }

        Ok(Box::pin(parse_event_stream(resp.bytes_stream())))
    }
}

/// Adapter: take a byte stream of HTTP response body and emit parsed
/// [`StreamEvent`]s.  Frames are delimited by a blank line (`\n\n`).
/// Malformed frames are skipped with a `tracing::warn` but do not error
/// out the whole stream — the REPL needs to survive partial data.
pub fn parse_event_stream<S>(
    bytes: S,
) -> impl Stream<Item = Result<StreamEvent>> + Send
where
    S: Stream<Item = reqwest::Result<bytes::Bytes>> + Send + Unpin + 'static,
{
    use async_stream::stream;
    stream! {
        let mut bytes = bytes;
        let mut buffer = String::new();
        while let Some(chunk) = bytes.next().await {
            let chunk = match chunk {
                Ok(c) => c,
                Err(e) => {
                    yield Err(anyhow::anyhow!("transport error: {e}"));
                    return;
                }
            };
            let Ok(text) = std::str::from_utf8(&chunk) else {
                tracing::warn!("non-utf8 chunk in SSE stream; skipping");
                continue;
            };
            buffer.push_str(text);
            // Frames are separated by "\n\n" (spec allows "\r\n\r\n" too).
            // Normalise CRLF to LF before splitting.
            while let Some(end) = find_frame_end(&buffer) {
                let frame: String = buffer.drain(..end).collect();
                // Remove the trailing blank-line separator from the buffer.
                if buffer.starts_with("\n\n") {
                    buffer.drain(..2);
                } else if buffer.starts_with("\r\n\r\n") {
                    buffer.drain(..4);
                }
                match StreamEvent::parse_frame(&frame) {
                    Some(ev) => {
                        let is_done = matches!(ev, StreamEvent::Done { .. });
                        yield Ok(ev);
                        if is_done {
                            return;
                        }
                    }
                    None => {
                        tracing::warn!(frame = %frame.chars().take(120).collect::<String>(), "skipping malformed SSE frame");
                    }
                }
            }
        }
    }
}

/// Return the byte index of the start of the trailing blank line for the
/// first complete SSE frame in `buf`, or `None` if no frame is complete yet.
fn find_frame_end(buf: &str) -> Option<usize> {
    // Support both "\n\n" (unix/SSE spec) and "\r\n\r\n" (some proxies).
    let lf = buf.find("\n\n");
    let crlf = buf.find("\r\n\r\n");
    match (lf, crlf) {
        (Some(a), Some(b)) => Some(a.min(b)),
        (Some(a), None) => Some(a),
        (None, Some(b)) => Some(b),
        (None, None) => None,
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use futures_util::stream;

    #[test]
    fn parse_frame_token_event() {
        let frame = "event: message\ndata: {\"delta\":\"hello \",\"session_id\":\"s1\"}";
        let ev = StreamEvent::parse_frame(frame).unwrap();
        assert_eq!(
            ev,
            StreamEvent::Token {
                delta: "hello ".to_owned(),
                session_id: "s1".to_owned()
            }
        );
    }

    #[test]
    fn parse_frame_done_event() {
        let frame = "event: done\ndata: {\"status\":\"complete\",\"usage\":{\"total_tokens\":42}}";
        let ev = StreamEvent::parse_frame(frame).unwrap();
        match ev {
            StreamEvent::Done { usage } => {
                let u = usage.unwrap();
                assert_eq!(u["total_tokens"], 42);
            }
            other => panic!("expected Done, got {other:?}"),
        }
    }

    #[test]
    fn parse_frame_hitl_pending() {
        let frame = "event: hitl_pending\ndata: {\"id\":\"req-123\"}";
        let ev = StreamEvent::parse_frame(frame).unwrap();
        assert_eq!(ev, StreamEvent::HitlPending { id: "req-123".to_owned() });
    }

    #[test]
    fn parse_frame_memory_pressure() {
        let frame = "event: memory_pressure\ndata: {\"message\":\"3 turns over budget\"}";
        let ev = StreamEvent::parse_frame(frame).unwrap();
        assert_eq!(
            ev,
            StreamEvent::MemoryPressure {
                message: "3 turns over budget".to_owned()
            }
        );
    }

    #[test]
    fn parse_frame_tool_call_and_result() {
        let call = "event: tool_call\ndata: {\"name\":\"bash\",\"args\":{\"cmd\":\"ls\"}}";
        let ev = StreamEvent::parse_frame(call).unwrap();
        assert!(matches!(ev, StreamEvent::ToolCall { .. }));

        let res = "event: tool_result\ndata: {\"name\":\"bash\",\"result\":\"file1.txt\"}";
        let ev = StreamEvent::parse_frame(res).unwrap();
        assert_eq!(
            ev,
            StreamEvent::ToolResult {
                name: "bash".to_owned(),
                result: "file1.txt".to_owned()
            }
        );
    }

    #[test]
    fn parse_frame_unknown_event_preserved_as_other() {
        let frame = "event: novel_event\ndata: {\"foo\":\"bar\"}";
        let ev = StreamEvent::parse_frame(frame).unwrap();
        match ev {
            StreamEvent::Other { event, data } => {
                assert_eq!(event, "novel_event");
                assert_eq!(data["foo"], "bar");
            }
            other => panic!("expected Other, got {other:?}"),
        }
    }

    #[test]
    fn parse_frame_missing_data_returns_none() {
        assert!(StreamEvent::parse_frame("event: message").is_none());
        assert!(StreamEvent::parse_frame(": keep-alive").is_none());
        assert!(StreamEvent::parse_frame("").is_none());
    }

    #[test]
    fn parse_frame_malformed_json_returns_none() {
        let frame = "event: message\ndata: not json";
        assert!(StreamEvent::parse_frame(frame).is_none());
    }

    #[test]
    fn parse_frame_default_event_is_message() {
        let frame = "data: {\"delta\":\"x\"}";
        let ev = StreamEvent::parse_frame(frame).unwrap();
        assert!(matches!(ev, StreamEvent::Token { .. }));
    }

    #[tokio::test]
    async fn parse_event_stream_emits_and_closes_on_done() {
        let payload =
            "event: message\ndata: {\"delta\":\"hello \",\"session_id\":\"s1\"}\n\n\
             event: message\ndata: {\"delta\":\"world\",\"session_id\":\"s1\"}\n\n\
             event: done\ndata: {\"status\":\"complete\"}\n\n";
        let chunks: Vec<reqwest::Result<bytes::Bytes>> =
            vec![Ok(bytes::Bytes::from(payload.to_owned()))];
        let s = stream::iter(chunks);
        let events = parse_event_stream(s);
        tokio::pin!(events);

        let ev = events.next().await.unwrap().unwrap();
        assert!(matches!(ev, StreamEvent::Token { .. }));
        let ev = events.next().await.unwrap().unwrap();
        assert!(matches!(ev, StreamEvent::Token { .. }));
        let ev = events.next().await.unwrap().unwrap();
        assert!(matches!(ev, StreamEvent::Done { .. }));
        // Stream closes after Done.
        assert!(events.next().await.is_none());
    }

    #[tokio::test]
    async fn parse_event_stream_skips_malformed_frames() {
        let payload = "event: message\ndata: not-json\n\n\
             event: message\ndata: {\"delta\":\"ok\"}\n\n\
             event: done\ndata: {}\n\n";
        let chunks: Vec<reqwest::Result<bytes::Bytes>> =
            vec![Ok(bytes::Bytes::from(payload.to_owned()))];
        let s = stream::iter(chunks);
        let events = parse_event_stream(s);
        tokio::pin!(events);

        // First frame (malformed) is skipped; second frame arrives.
        let ev = events.next().await.unwrap().unwrap();
        match ev {
            StreamEvent::Token { delta, .. } => assert_eq!(delta, "ok"),
            other => panic!("expected Token, got {other:?}"),
        }
        // Then Done.
        let ev = events.next().await.unwrap().unwrap();
        assert!(matches!(ev, StreamEvent::Done { .. }));
    }

    #[tokio::test]
    async fn parse_event_stream_handles_split_chunks() {
        // The first chunk ends mid-frame; the parser must buffer until the
        // separator arrives.
        let c1 = "event: message\ndata: {\"delta\":\"part";
        let c2 = "ial\",\"session_id\":\"s1\"}\n\n\
                  event: done\ndata: {}\n\n";
        let chunks: Vec<reqwest::Result<bytes::Bytes>> = vec![
            Ok(bytes::Bytes::from(c1.to_owned())),
            Ok(bytes::Bytes::from(c2.to_owned())),
        ];
        let s = stream::iter(chunks);
        let events = parse_event_stream(s);
        tokio::pin!(events);

        let ev = events.next().await.unwrap().unwrap();
        match ev {
            StreamEvent::Token { delta, .. } => assert_eq!(delta, "partial"),
            other => panic!("expected Token, got {other:?}"),
        }
    }

    #[tokio::test]
    async fn parse_event_stream_handles_crlf_separators() {
        let payload = "event: message\r\ndata: {\"delta\":\"a\"}\r\n\r\n\
             event: done\r\ndata: {}\r\n\r\n";
        let chunks: Vec<reqwest::Result<bytes::Bytes>> =
            vec![Ok(bytes::Bytes::from(payload.to_owned()))];
        let s = stream::iter(chunks);
        let events = parse_event_stream(s);
        tokio::pin!(events);

        let ev = events.next().await.unwrap().unwrap();
        assert!(matches!(ev, StreamEvent::Token { .. }));
    }
}

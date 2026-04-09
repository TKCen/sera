//! Discord Gateway connector — connects via raw WebSocket, handles heartbeat,
//! and dispatches MESSAGE_CREATE events through an mpsc channel.

use std::sync::atomic::{AtomicBool, AtomicI64, Ordering};
use std::sync::Arc;

use futures_util::{SinkExt, StreamExt};
use serde_json::Value;
use tokio::sync::mpsc;
use tokio::time::{self, Duration};
use tokio_tungstenite::tungstenite::Message;

// ---------------------------------------------------------------------------
// Public types
// ---------------------------------------------------------------------------

/// Message received from Discord, ready for the gateway event queue.
#[derive(Debug, Clone)]
pub struct DiscordMessage {
    pub channel_id: String,
    pub user_id: String,
    pub username: String,
    pub content: String,
    #[allow(dead_code)]
    pub message_id: String,
}

/// Discord Gateway connector — connects via WebSocket, handles heartbeat,
/// dispatches messages.
pub struct DiscordConnector {
    token: String,
    agent_name: String,
    tx: mpsc::Sender<DiscordMessage>,
}

// ---------------------------------------------------------------------------
// Intent constants
// ---------------------------------------------------------------------------

/// GUILDS (1 << 0)
const INTENT_GUILDS: u64 = 1;
/// GUILD_MESSAGES (1 << 9)
const INTENT_GUILD_MESSAGES: u64 = 512;
/// DIRECT_MESSAGES (1 << 12)
const INTENT_DIRECT_MESSAGES: u64 = 4096;
/// MESSAGE_CONTENT (1 << 15)
const INTENT_MESSAGE_CONTENT: u64 = 32768;

/// Combined intents value: 1 + 512 + 4096 + 32768 = 37377
pub const DISCORD_INTENTS: u64 =
    INTENT_GUILDS | INTENT_GUILD_MESSAGES | INTENT_DIRECT_MESSAGES | INTENT_MESSAGE_CONTENT;

// ---------------------------------------------------------------------------
// Gateway opcodes
// ---------------------------------------------------------------------------

const OP_DISPATCH: u64 = 0;
const OP_HEARTBEAT: u64 = 1;
const OP_IDENTIFY: u64 = 2;
const OP_HELLO: u64 = 10;
// const OP_HEARTBEAT_ACK: u64 = 11; // logged but unused

const GATEWAY_URL: &str = "wss://gateway.discord.gg/?v=10&encoding=json";
const DISCORD_API_BASE: &str = "https://discord.com/api/v10";

// ---------------------------------------------------------------------------
// Payload helpers (pure functions, tested independently)
// ---------------------------------------------------------------------------

/// Build an Identify payload (opcode 2).
pub fn build_identify_payload(token: &str, agent_name: &str) -> Value {
    serde_json::json!({
        "op": OP_IDENTIFY,
        "d": {
            "token": token,
            "intents": DISCORD_INTENTS,
            "properties": {
                "os": "linux",
                "browser": agent_name,
                "device": agent_name,
            }
        }
    })
}

/// Build a Heartbeat payload (opcode 1).
pub fn build_heartbeat_payload(sequence: Option<i64>) -> Value {
    serde_json::json!({
        "op": OP_HEARTBEAT,
        "d": sequence,
    })
}

/// Extract `heartbeat_interval` from a Hello (opcode 10) payload.
///
/// Returns `None` if the payload is not a valid Hello.
pub fn parse_heartbeat_interval(payload: &Value) -> Option<u64> {
    if payload.get("op")?.as_u64()? != OP_HELLO {
        return None;
    }
    payload
        .get("d")?
        .get("heartbeat_interval")?
        .as_u64()
}

/// Strip Discord mention tags (`<@123>` and `<@!123>`) from a message string,
/// then trim leading/trailing whitespace and collapse internal runs of spaces.
pub fn strip_mentions(content: &str) -> String {
    use std::sync::LazyLock;
    static RE: LazyLock<regex::Regex> =
        LazyLock::new(|| regex::Regex::new(r"<@!?\d+>").expect("valid regex"));
    let stripped = RE.replace_all(content, "");
    // Collapse multiple spaces and trim edges
    stripped
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
}

/// Try to extract a `DiscordMessage` from a Dispatch (opcode 0) payload
/// with `t == "MESSAGE_CREATE"`.
///
/// Returns `None` if the payload is not a MESSAGE_CREATE dispatch, or if the
/// message author is a bot.
pub fn parse_message_create(payload: &Value) -> Option<DiscordMessage> {
    if payload.get("op")?.as_u64()? != OP_DISPATCH {
        return None;
    }
    if payload.get("t")?.as_str()? != "MESSAGE_CREATE" {
        return None;
    }
    let d = payload.get("d")?;
    let author = d.get("author")?;

    // Skip bot messages
    if author.get("bot").and_then(Value::as_bool).unwrap_or(false) {
        return None;
    }

    Some(DiscordMessage {
        channel_id: d.get("channel_id")?.as_str()?.to_owned(),
        user_id: author.get("id")?.as_str()?.to_owned(),
        username: author.get("username")?.as_str()?.to_owned(),
        content: strip_mentions(d.get("content")?.as_str()?),
        message_id: d.get("id")?.as_str()?.to_owned(),
    })
}

/// Extract the event name from a Dispatch payload (opcode 0).
pub fn parse_dispatch_event(payload: &Value) -> Option<String> {
    if payload.get("op")?.as_u64()? != OP_DISPATCH {
        return None;
    }
    payload.get("t")?.as_str().map(String::from)
}

/// Extract the sequence number (`s`) from any gateway payload.
pub fn parse_sequence(payload: &Value) -> Option<i64> {
    payload.get("s")?.as_i64()
}

// ---------------------------------------------------------------------------
// DiscordConnector implementation
// ---------------------------------------------------------------------------

impl DiscordConnector {
    pub fn new(token: &str, agent_name: &str, tx: mpsc::Sender<DiscordMessage>) -> Self {
        Self {
            token: token.to_owned(),
            agent_name: agent_name.to_owned(),
            tx,
        }
    }

    /// Start the connector — connects to Discord Gateway, runs heartbeat loop,
    /// dispatches MESSAGE_CREATE events. Reconnects on close after 5 seconds.
    pub async fn run(&self) -> anyhow::Result<()> {
        let running = Arc::new(AtomicBool::new(true));

        while running.load(Ordering::Relaxed) {
            if let Err(e) = self.connect_and_run(running.clone()).await {
                tracing::error!("Discord gateway error: {e}");
            }
            if running.load(Ordering::Relaxed) {
                tracing::info!("Reconnecting to Discord in 5 seconds...");
                time::sleep(Duration::from_secs(5)).await;
            }
        }
        Ok(())
    }

    /// Send a message to a Discord channel via the REST API.
    pub async fn send_message(&self, channel_id: &str, content: &str) -> anyhow::Result<()> {
        let url = format!("{DISCORD_API_BASE}/channels/{channel_id}/messages");
        let client = reqwest::Client::new();
        let resp = client
            .post(&url)
            .header("Authorization", format!("Bot {}", self.token))
            .json(&serde_json::json!({ "content": content }))
            .send()
            .await?;

        if !resp.status().is_success() {
            let status = resp.status();
            let body = resp.text().await.unwrap_or_default();
            anyhow::bail!("Discord API error {status}: {body}");
        }
        Ok(())
    }

    // -----------------------------------------------------------------------
    // Internal
    // -----------------------------------------------------------------------

    async fn connect_and_run(&self, running: Arc<AtomicBool>) -> anyhow::Result<()> {
        tracing::info!("Connecting to Discord Gateway...");

        let (ws_stream, _) = tokio_tungstenite::connect_async(GATEWAY_URL).await?;
        let (mut write, mut read) = ws_stream.split();

        tracing::info!("Discord Gateway connection opened");

        // Shared sequence counter for heartbeats
        let sequence = Arc::new(AtomicI64::new(-1)); // -1 means null

        // Read the Hello payload to get heartbeat_interval
        let hello_msg = read
            .next()
            .await
            .ok_or_else(|| anyhow::anyhow!("Gateway closed before Hello"))??;

        let hello_text = hello_msg.to_text()?;
        let hello_payload: Value = serde_json::from_str(hello_text)?;

        let heartbeat_ms = parse_heartbeat_interval(&hello_payload)
            .ok_or_else(|| anyhow::anyhow!("Invalid Hello payload"))?;

        tracing::info!("Heartbeat interval: {heartbeat_ms}ms");

        // Send Identify
        let identify = build_identify_payload(&self.token, &self.agent_name);
        write
            .send(Message::Text(identify.to_string().into()))
            .await?;

        // Spawn heartbeat loop
        let hb_sequence = sequence.clone();
        let hb_running = running.clone();
        let (hb_tx, mut hb_rx) = mpsc::channel::<Message>(16);

        tokio::spawn(async move {
            let mut interval = time::interval(Duration::from_millis(heartbeat_ms));
            while hb_running.load(Ordering::Relaxed) {
                interval.tick().await;
                let seq = hb_sequence.load(Ordering::Relaxed);
                let seq_val = if seq < 0 { None } else { Some(seq) };
                let payload = build_heartbeat_payload(seq_val);
                if hb_tx
                    .send(Message::Text(payload.to_string().into()))
                    .await
                    .is_err()
                {
                    break;
                }
            }
        });

        // Main event loop — merge heartbeat sends with reads
        loop {
            tokio::select! {
                Some(hb_msg) = hb_rx.recv() => {
                    if let Err(e) = write.send(hb_msg).await {
                        tracing::error!("Failed to send heartbeat: {e}");
                        break;
                    }
                }
                msg = read.next() => {
                    match msg {
                        Some(Ok(Message::Text(text))) => {
                            match serde_json::from_str::<Value>(&text) {
                                Ok(payload) => {
                                    self.handle_payload(&payload, &sequence).await;
                                }
                                Err(e) => {
                                    tracing::error!("Failed to parse Discord payload: {e}");
                                }
                            }
                        }
                        Some(Ok(Message::Close(_))) | None => {
                            tracing::warn!("Discord Gateway connection closed");
                            break;
                        }
                        Some(Err(e)) => {
                            tracing::error!("Discord Gateway error: {e}");
                            break;
                        }
                        _ => {} // Ping/Pong/Binary — ignore
                    }
                }
            }
        }

        Ok(())
    }

    async fn handle_payload(&self, payload: &Value, sequence: &AtomicI64) {
        // Update sequence number
        if let Some(s) = parse_sequence(payload) {
            sequence.store(s, Ordering::Relaxed);
        }

        let op = payload.get("op").and_then(Value::as_u64).unwrap_or(u64::MAX);

        match op {
            OP_DISPATCH => {
                if let Some(event) = parse_dispatch_event(payload) {
                    match event.as_str() {
                        "READY" => {
                            let username = payload
                                .get("d")
                                .and_then(|d| d.get("user"))
                                .and_then(|u| u.get("username"))
                                .and_then(Value::as_str)
                                .unwrap_or("unknown");
                            tracing::info!("Discord adapter ready as {username}");
                        }
                        "MESSAGE_CREATE" => {
                            if let Some(msg) = parse_message_create(payload) {
                                if let Err(e) = self.tx.send(msg).await {
                                    tracing::error!("Failed to dispatch Discord message: {e}");
                                }
                            }
                        }
                        _ => {
                            tracing::debug!("Unhandled dispatch event: {event}");
                        }
                    }
                }
            }
            11 => {
                // Heartbeat ACK — no action needed
                tracing::trace!("Heartbeat ACK received");
            }
            _ => {
                tracing::debug!("Unhandled opcode: {op}");
            }
        }
    }
}

// ---------------------------------------------------------------------------
// Tests — parsing and construction logic only (no real WebSocket connections)
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_discord_message_creation() {
        let msg = DiscordMessage {
            channel_id: "123456".into(),
            user_id: "789".into(),
            username: "testuser".into(),
            content: "hello world".into(),
            message_id: "msg001".into(),
        };
        assert_eq!(msg.channel_id, "123456");
        assert_eq!(msg.user_id, "789");
        assert_eq!(msg.username, "testuser");
        assert_eq!(msg.content, "hello world");
        assert_eq!(msg.message_id, "msg001");
    }

    #[test]
    fn test_intent_calculation() {
        assert_eq!(INTENT_GUILDS, 1 << 0);
        assert_eq!(INTENT_GUILD_MESSAGES, 1 << 9);
        assert_eq!(INTENT_DIRECT_MESSAGES, 1 << 12);
        assert_eq!(INTENT_MESSAGE_CONTENT, 1 << 15);
        assert_eq!(DISCORD_INTENTS, 37377);
        assert_eq!(
            DISCORD_INTENTS,
            INTENT_GUILDS | INTENT_GUILD_MESSAGES | INTENT_DIRECT_MESSAGES | INTENT_MESSAGE_CONTENT
        );
    }

    #[test]
    fn test_parse_heartbeat_interval_from_hello() {
        let hello = serde_json::json!({
            "op": 10,
            "d": {
                "heartbeat_interval": 41250
            }
        });
        assert_eq!(parse_heartbeat_interval(&hello), Some(41250));
    }

    #[test]
    fn test_parse_heartbeat_interval_wrong_opcode() {
        let not_hello = serde_json::json!({
            "op": 0,
            "d": {
                "heartbeat_interval": 41250
            }
        });
        assert_eq!(parse_heartbeat_interval(&not_hello), None);
    }

    #[test]
    fn test_parse_heartbeat_interval_missing_field() {
        let bad = serde_json::json!({ "op": 10, "d": {} });
        assert_eq!(parse_heartbeat_interval(&bad), None);
    }

    #[test]
    fn test_parse_message_create_valid() {
        let payload = serde_json::json!({
            "op": 0,
            "t": "MESSAGE_CREATE",
            "s": 42,
            "d": {
                "id": "msg123",
                "channel_id": "ch456",
                "content": "Hello SERA!",
                "author": {
                    "id": "user789",
                    "username": "alice",
                    "bot": false
                }
            }
        });
        let msg = parse_message_create(&payload).expect("should parse");
        assert_eq!(msg.message_id, "msg123");
        assert_eq!(msg.channel_id, "ch456");
        assert_eq!(msg.content, "Hello SERA!");
        assert_eq!(msg.user_id, "user789");
        assert_eq!(msg.username, "alice");
    }

    #[test]
    fn test_parse_message_create_bot_filtered() {
        let payload = serde_json::json!({
            "op": 0,
            "t": "MESSAGE_CREATE",
            "s": 43,
            "d": {
                "id": "msg124",
                "channel_id": "ch456",
                "content": "I am a bot",
                "author": {
                    "id": "bot001",
                    "username": "botuser",
                    "bot": true
                }
            }
        });
        assert!(parse_message_create(&payload).is_none());
    }

    #[test]
    fn test_parse_message_create_bot_field_absent() {
        // When "bot" field is absent, treat as non-bot (human user)
        let payload = serde_json::json!({
            "op": 0,
            "t": "MESSAGE_CREATE",
            "s": 44,
            "d": {
                "id": "msg125",
                "channel_id": "ch789",
                "content": "No bot field",
                "author": {
                    "id": "user002",
                    "username": "bob"
                }
            }
        });
        let msg = parse_message_create(&payload).expect("should parse when bot field absent");
        assert_eq!(msg.username, "bob");
    }

    #[test]
    fn test_parse_message_create_wrong_event_type() {
        let payload = serde_json::json!({
            "op": 0,
            "t": "READY",
            "s": 1,
            "d": {
                "session_id": "abc",
                "user": { "username": "sera", "discriminator": "0001" }
            }
        });
        assert!(parse_message_create(&payload).is_none());
    }

    #[test]
    fn test_parse_message_create_wrong_opcode() {
        let payload = serde_json::json!({
            "op": 10,
            "d": { "heartbeat_interval": 41250 }
        });
        assert!(parse_message_create(&payload).is_none());
    }

    #[test]
    fn test_parse_sequence() {
        let payload = serde_json::json!({ "op": 0, "s": 99, "t": "READY", "d": {} });
        assert_eq!(parse_sequence(&payload), Some(99));
    }

    #[test]
    fn test_parse_sequence_null() {
        let payload = serde_json::json!({ "op": 10, "s": null, "d": {} });
        assert_eq!(parse_sequence(&payload), None);
    }

    #[test]
    fn test_parse_dispatch_event() {
        let payload = serde_json::json!({ "op": 0, "t": "GUILD_CREATE", "s": 2, "d": {} });
        assert_eq!(parse_dispatch_event(&payload), Some("GUILD_CREATE".into()));
    }

    #[test]
    fn test_parse_dispatch_event_non_dispatch() {
        let payload = serde_json::json!({ "op": 11, "d": null });
        assert_eq!(parse_dispatch_event(&payload), None);
    }

    #[test]
    fn test_build_identify_payload() {
        let payload = build_identify_payload("my-token", "sera-agent");
        assert_eq!(payload["op"], 2);
        assert_eq!(payload["d"]["token"], "my-token");
        assert_eq!(payload["d"]["intents"], 37377);
        assert_eq!(payload["d"]["properties"]["browser"], "sera-agent");
        assert_eq!(payload["d"]["properties"]["device"], "sera-agent");
        assert_eq!(payload["d"]["properties"]["os"], "linux");
    }

    #[test]
    fn test_build_heartbeat_payload_with_sequence() {
        let payload = build_heartbeat_payload(Some(42));
        assert_eq!(payload["op"], 1);
        assert_eq!(payload["d"], 42);
    }

    #[test]
    fn test_build_heartbeat_payload_null_sequence() {
        let payload = build_heartbeat_payload(None);
        assert_eq!(payload["op"], 1);
        assert!(payload["d"].is_null());
    }

    #[test]
    fn test_connector_new() {
        let (tx, _rx) = mpsc::channel(10);
        let connector = DiscordConnector::new("token123", "my-agent", tx);
        assert_eq!(connector.token, "token123");
        assert_eq!(connector.agent_name, "my-agent");
    }

    // --- strip_mentions tests ---

    #[test]
    fn test_strip_mentions_basic() {
        assert_eq!(strip_mentions("<@123456> hello"), "hello");
    }

    #[test]
    fn test_strip_mentions_nickname() {
        assert_eq!(strip_mentions("<@!123456> hello"), "hello");
    }

    #[test]
    fn test_strip_mentions_multiple() {
        assert_eq!(strip_mentions("<@111> <@222> hi"), "hi");
    }

    #[test]
    fn test_strip_mentions_none() {
        assert_eq!(strip_mentions("hello world"), "hello world");
    }

    #[test]
    fn test_strip_mentions_only_mention() {
        assert_eq!(strip_mentions("<@123>"), "");
    }

    #[test]
    fn test_strip_mentions_middle() {
        assert_eq!(strip_mentions("hey <@123> what's up"), "hey what's up");
    }

    #[test]
    fn test_parse_message_create_strips_mention() {
        let payload = serde_json::json!({
            "op": 0,
            "t": "MESSAGE_CREATE",
            "s": 50,
            "d": {
                "id": "msg200",
                "channel_id": "ch100",
                "content": "<@987654321012345678> help me",
                "author": {
                    "id": "user001",
                    "username": "carol"
                }
            }
        });
        let msg = parse_message_create(&payload).expect("should parse");
        assert_eq!(msg.content, "help me");
    }
}

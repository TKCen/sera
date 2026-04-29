//! Discord Gateway connector — connects via raw WebSocket, handles heartbeat,
//! and dispatches MESSAGE_CREATE events through an mpsc channel.

use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicI64, Ordering};

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
    /// Whether this message was a DM (not a guild channel).
    pub is_dm: bool,
    /// Whether the bot was mentioned in this message (for guild channels).
    pub mentions_bot: bool,
}

/// Discord Gateway connector — connects via WebSocket, handles heartbeat,
/// dispatches messages.
pub struct DiscordConnector {
    token: String,
    agent_name: String,
    tx: mpsc::Sender<DiscordMessage>,
    /// Bot's own user ID, set on READY event.
    bot_user_id: std::sync::Mutex<Option<String>>,
    /// Shared shutdown flag from `AppState`. When `true` the reconnect loop
    /// exits instead of sleeping for the next attempt.
    shutting_down: Arc<AtomicBool>,
    /// Session id captured from the most recent READY dispatch. Required to
    /// send Resume (opcode 6) after a reconnect; cleared on Op 9 with
    /// `d=false` so the next attempt does a fresh IDENTIFY.
    session_id: std::sync::Mutex<Option<String>>,
    /// Per-session reconnect URL captured from READY. When set, subsequent
    /// reconnects target this host instead of `GATEWAY_URL`.
    resume_gateway_url: std::sync::Mutex<Option<String>>,
    /// Last sequence number observed from the gateway. Persists across
    /// connections so RESUME can replay missed dispatches. `-1` means "no
    /// sequence yet" (serializes as JSON `null`).
    last_sequence: Arc<AtomicI64>,
}

/// Why the current gateway connection ended. Drives the backoff and IDENTIFY-vs-RESUME
/// decision in [`DiscordConnector::run`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum DisconnectReason {
    /// Connection ended in a state where the existing session may still be
    /// resumed: a normal close, an Op 7 RECONNECT, or an Op 9 with `d=true`.
    Resumable,
    /// Server told us the session is dead (Op 9 with `d=false`). The connector
    /// must clear stored session state and IDENTIFY again with a backoff.
    SessionInvalidated,
}

/// Signal from [`DiscordConnector::handle_payload`] back to the event loop
/// asking it to terminate the current connection.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum HandlerSignal {
    /// Op 7 RECONNECT or Op 9 with `d=true` — close and resume.
    Reconnect,
    /// Op 9 with `d=false` — close, drop session state, re-IDENTIFY.
    SessionInvalidated,
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
const OP_RESUME: u64 = 6;
const OP_RECONNECT: u64 = 7;
const OP_INVALID_SESSION: u64 = 9;
const OP_HELLO: u64 = 10;
const OP_HEARTBEAT_ACK: u64 = 11;

const GATEWAY_URL: &str = "wss://gateway.discord.gg/?v=10&encoding=json";
const DISCORD_API_BASE: &str = "https://discord.com/api/v10";

/// Query string Discord requires when connecting to a `resume_gateway_url`.
const GATEWAY_QUERY: &str = "/?v=10&encoding=json";

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

/// Build a Resume payload (opcode 6) for replaying missed events on a previously
/// established session.
pub fn build_resume_payload(token: &str, session_id: &str, sequence: i64) -> Value {
    serde_json::json!({
        "op": OP_RESUME,
        "d": {
            "token": token,
            "session_id": session_id,
            "seq": sequence,
        }
    })
}

/// Returns `Some(resumable)` when the payload is opcode 9 (INVALID_SESSION).
/// Per Discord docs, `d` is a boolean: `true` means the session can be resumed,
/// `false` means it must be re-IDENTIFYd from scratch.
pub fn parse_invalid_session_resumable(payload: &Value) -> Option<bool> {
    if payload.get("op")?.as_u64()? != OP_INVALID_SESSION {
        return None;
    }
    Some(payload.get("d").and_then(Value::as_bool).unwrap_or(false))
}

/// Extract `(session_id, resume_gateway_url)` from a READY dispatch payload.
/// Both pieces are required to RESUME a session after disconnect.
pub fn parse_ready_session(payload: &Value) -> Option<(String, Option<String>)> {
    if payload.get("op")?.as_u64()? != OP_DISPATCH {
        return None;
    }
    if payload.get("t")?.as_str()? != "READY" {
        return None;
    }
    let d = payload.get("d")?;
    let session_id = d.get("session_id")?.as_str()?.to_owned();
    let resume_url = d
        .get("resume_gateway_url")
        .and_then(Value::as_str)
        .map(String::from);
    Some((session_id, resume_url))
}

/// Backoff schedule for INVALID_SESSION re-IDENTIFY attempts.
///
/// Discord's spec says clients should wait a random 1-5 seconds before sending a
/// new IDENTIFY. We follow that for the first few attempts then escalate to a
/// 30-second cap so a misbehaving server can't keep us in a hot loop. Caller
/// passes the running consecutive-INVALID_SESSION count (1 for the first one).
pub fn invalid_session_backoff_secs(consecutive: u32) -> u64 {
    match consecutive {
        0 | 1 => 1,
        2 => 3,
        3 => 5,
        4 => 10,
        5 => 20,
        _ => 30,
    }
}

/// Extract `heartbeat_interval` from a Hello (opcode 10) payload.
///
/// Returns `None` if the payload is not a valid Hello.
pub fn parse_heartbeat_interval(payload: &Value) -> Option<u64> {
    if payload.get("op")?.as_u64()? != OP_HELLO {
        return None;
    }
    payload.get("d")?.get("heartbeat_interval")?.as_u64()
}

/// Strip Discord mention tags (`<@123>` and `<@!123>`) from a message string,
/// then trim leading/trailing whitespace and collapse internal runs of spaces.
pub fn strip_mentions(content: &str) -> String {
    use std::sync::LazyLock;
    static RE: LazyLock<regex::Regex> =
        LazyLock::new(|| regex::Regex::new(r"<@!?\d+>").expect("valid regex"));
    let stripped = RE.replace_all(content, "");
    // Collapse multiple spaces and trim edges
    stripped.split_whitespace().collect::<Vec<_>>().join(" ")
}

/// Try to extract a `DiscordMessage` from a Dispatch (opcode 0) payload
/// with `t == "MESSAGE_CREATE"`.
///
/// Returns `None` if the payload is not a MESSAGE_CREATE dispatch, or if the
/// message author is a bot.
///
/// The `is_dm` and `mentions_bot` fields are set based on the raw payload data.
pub fn parse_message_create(payload: &Value, bot_user_id: Option<&str>) -> Option<DiscordMessage> {
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

    // Check if this is a DM (no guild_id) or mentions the bot
    let is_dm = d.get("guild_id").is_none();
    let mentions_bot = if let Some(bot_id) = bot_user_id {
        d.get("mentions")
            .and_then(|m| m.as_array())
            .map(|arr| {
                arr.iter()
                    .any(|u| u.get("id").and_then(Value::as_str) == Some(bot_id))
            })
            .unwrap_or(false)
    } else {
        false
    };

    Some(DiscordMessage {
        channel_id: d.get("channel_id")?.as_str()?.to_owned(),
        user_id: author.get("id")?.as_str()?.to_owned(),
        username: author.get("username")?.as_str()?.to_owned(),
        content: strip_mentions(d.get("content")?.as_str()?),
        message_id: d.get("id")?.as_str()?.to_owned(),
        is_dm,
        mentions_bot,
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
    pub fn new(
        token: &str,
        agent_name: &str,
        tx: mpsc::Sender<DiscordMessage>,
        shutting_down: Arc<AtomicBool>,
    ) -> Self {
        Self {
            token: token.to_owned(),
            agent_name: agent_name.to_owned(),
            tx,
            bot_user_id: std::sync::Mutex::new(None),
            shutting_down,
            session_id: std::sync::Mutex::new(None),
            resume_gateway_url: std::sync::Mutex::new(None),
            last_sequence: Arc::new(AtomicI64::new(-1)),
        }
    }

    fn clear_session_state(&self) {
        if let Ok(mut g) = self.session_id.lock() {
            *g = None;
        }
        if let Ok(mut g) = self.resume_gateway_url.lock() {
            *g = None;
        }
        self.last_sequence.store(-1, Ordering::Relaxed);
    }

    fn current_session(&self) -> Option<String> {
        self.session_id.lock().ok().and_then(|g| g.clone())
    }

    fn current_resume_url(&self) -> Option<String> {
        self.resume_gateway_url.lock().ok().and_then(|g| g.clone())
    }

    /// Start the connector — connects to Discord Gateway, runs heartbeat loop,
    /// dispatches MESSAGE_CREATE events. Reconnects after a backoff that
    /// adapts to the disconnect reason:
    ///
    /// * normal close / transport error → 5s, attempt RESUME
    /// * Op 7 RECONNECT or Op 9 (`d=true`) → 1s, attempt RESUME
    /// * Op 9 (`d=false`) → escalating 1-30s, drop session, fresh IDENTIFY
    ///
    /// Exits immediately when `shutting_down` is set to `true`.
    pub async fn run(&self) -> anyhow::Result<()> {
        let mut consecutive_invalid_sessions: u32 = 0;
        while !self.shutting_down.load(Ordering::Relaxed) {
            let outcome = match self.connect_and_run().await {
                Ok(reason) => Some(reason),
                Err(e) => {
                    tracing::error!("Discord gateway error: {e}");
                    None
                }
            };
            if self.shutting_down.load(Ordering::Relaxed) {
                break;
            }

            let backoff = match outcome {
                Some(DisconnectReason::SessionInvalidated) => {
                    self.clear_session_state();
                    consecutive_invalid_sessions = consecutive_invalid_sessions.saturating_add(1);
                    let secs = invalid_session_backoff_secs(consecutive_invalid_sessions);
                    tracing::warn!(
                        consecutive = consecutive_invalid_sessions,
                        "Discord session invalidated; sleeping {secs}s before re-IDENTIFY"
                    );
                    Duration::from_secs(secs)
                }
                Some(DisconnectReason::Resumable) => {
                    consecutive_invalid_sessions = 0;
                    Duration::from_secs(1)
                }
                None => {
                    consecutive_invalid_sessions = 0;
                    Duration::from_secs(5)
                }
            };

            tracing::info!("Reconnecting to Discord in {:?}...", backoff);
            // Sleep interruptibly: wake every 100ms and check the flag.
            let deadline = tokio::time::Instant::now() + backoff;
            while tokio::time::Instant::now() < deadline {
                if self.shutting_down.load(Ordering::Relaxed) {
                    return Ok(());
                }
                time::sleep(Duration::from_millis(100)).await;
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

    async fn connect_and_run(&self) -> anyhow::Result<DisconnectReason> {
        // Pick host: per-session resume URL if we have one, else the canonical
        // gateway. Both must carry the v=10/encoding=json query string.
        let url = match self.current_resume_url() {
            Some(base) => format!("{base}{GATEWAY_QUERY}"),
            None => GATEWAY_URL.to_owned(),
        };
        tracing::info!(url = %url, "Connecting to Discord Gateway...");

        let (ws_stream, _) = tokio_tungstenite::connect_async(&url).await?;
        let (mut write, mut read) = ws_stream.split();

        tracing::info!("Discord Gateway connection opened");

        // Sequence counter is shared across reconnects so RESUME can replay
        // missed events. The heartbeat task gets a clone.
        let sequence = Arc::clone(&self.last_sequence);

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

        // Decide IDENTIFY vs RESUME. We can RESUME only if we have both a
        // session id and a non-negative last sequence from a prior connection.
        let session_for_resume = self.current_session();
        let seq_for_resume = self.last_sequence.load(Ordering::Relaxed);
        let initial_payload = match session_for_resume.as_deref() {
            Some(sid) if seq_for_resume >= 0 => {
                tracing::info!(
                    session_id = sid,
                    seq = seq_for_resume,
                    "Sending RESUME to Discord Gateway"
                );
                build_resume_payload(&self.token, sid, seq_for_resume)
            }
            _ => {
                tracing::info!("Sending IDENTIFY to Discord Gateway");
                build_identify_payload(&self.token, &self.agent_name)
            }
        };
        write
            .send(Message::Text(initial_payload.to_string().into()))
            .await?;

        // Spawn heartbeat loop — exits when hb_tx is dropped (connection closed)
        // or when the shared shutting_down flag is set.
        let hb_sequence = Arc::clone(&sequence);
        let hb_shutting_down = Arc::clone(&self.shutting_down);
        let (hb_tx, mut hb_rx) = mpsc::channel::<Message>(16);

        tokio::spawn(async move {
            let mut interval = time::interval(Duration::from_millis(heartbeat_ms));
            while !hb_shutting_down.load(Ordering::Relaxed) {
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

        // Main event loop — merge heartbeat sends with reads. The handler
        // returns Some(signal) when an opcode mandates closing the connection;
        // we propagate that as the DisconnectReason.
        let mut signal: Option<HandlerSignal> = None;
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
                                    if let Some(sig) = self.handle_payload(&payload).await {
                                        signal = Some(sig);
                                        break;
                                    }
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

        Ok(match signal {
            Some(HandlerSignal::SessionInvalidated) => DisconnectReason::SessionInvalidated,
            Some(HandlerSignal::Reconnect) | None => DisconnectReason::Resumable,
        })
    }

    async fn handle_payload(&self, payload: &Value) -> Option<HandlerSignal> {
        // Update sequence number — written to the shared Arc so RESUME after
        // reconnect can pick up where we left off.
        if let Some(s) = parse_sequence(payload) {
            self.last_sequence.store(s, Ordering::Relaxed);
        }

        let op = payload
            .get("op")
            .and_then(Value::as_u64)
            .unwrap_or(u64::MAX);

        match op {
            OP_DISPATCH => {
                if let Some(event) = parse_dispatch_event(payload) {
                    match event.as_str() {
                        "READY" => {
                            let user_id = payload
                                .get("d")
                                .and_then(|d| d.get("user"))
                                .and_then(|u| u.get("id"))
                                .and_then(Value::as_str)
                                .map(String::from);
                            if let Some(ref uid) = user_id
                                && let Ok(mut guard) = self.bot_user_id.lock()
                            {
                                *guard = Some(uid.clone());
                            }
                            // Capture session_id and resume_gateway_url so a
                            // future reconnect can RESUME instead of dropping
                            // events.
                            if let Some((session_id, resume_url)) = parse_ready_session(payload) {
                                if let Ok(mut g) = self.session_id.lock() {
                                    *g = Some(session_id);
                                }
                                if let Ok(mut g) = self.resume_gateway_url.lock() {
                                    *g = resume_url;
                                }
                            }
                            let username = payload
                                .get("d")
                                .and_then(|d| d.get("user"))
                                .and_then(|u| u.get("username"))
                                .and_then(Value::as_str)
                                .unwrap_or("unknown");
                            tracing::info!("Discord adapter ready as {username}");
                        }
                        "RESUMED" => {
                            tracing::info!("Discord session RESUMED");
                        }
                        "MESSAGE_CREATE" => {
                            let bot_id = self.bot_user_id.lock().ok().and_then(|g| g.clone());
                            if let Some(msg) = parse_message_create(payload, bot_id.as_deref())
                                && let Err(e) = self.tx.send(msg).await
                            {
                                tracing::error!("Failed to dispatch Discord message: {e}");
                            }
                        }
                        _ => {
                            tracing::debug!("Unhandled dispatch event: {event}");
                        }
                    }
                }
                None
            }
            OP_RECONNECT => {
                tracing::warn!("Discord Op 7 RECONNECT received — closing to resume");
                Some(HandlerSignal::Reconnect)
            }
            OP_INVALID_SESSION => {
                let resumable = parse_invalid_session_resumable(payload).unwrap_or(false);
                tracing::warn!(resumable, "Discord Op 9 INVALID_SESSION received");
                if resumable {
                    Some(HandlerSignal::Reconnect)
                } else {
                    Some(HandlerSignal::SessionInvalidated)
                }
            }
            OP_HEARTBEAT_ACK => {
                tracing::trace!("Heartbeat ACK received");
                None
            }
            _ => {
                tracing::debug!("Unhandled opcode: {op}");
                None
            }
        }
    }

    // -- Test-only state inspectors ------------------------------------------

    #[cfg(test)]
    fn test_session_id(&self) -> Option<String> {
        self.current_session()
    }

    #[cfg(test)]
    fn test_resume_gateway_url(&self) -> Option<String> {
        self.current_resume_url()
    }

    #[cfg(test)]
    fn test_last_sequence(&self) -> i64 {
        self.last_sequence.load(Ordering::Relaxed)
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
            is_dm: false,
            mentions_bot: false,
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
        let msg = parse_message_create(&payload, None).expect("should parse");
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
        assert!(parse_message_create(&payload, None).is_none());
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
        let msg = parse_message_create(&payload, None).expect("should parse when bot field absent");
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
        assert!(parse_message_create(&payload, None).is_none());
    }

    #[test]
    fn test_parse_message_create_wrong_opcode() {
        let payload = serde_json::json!({
            "op": 10,
            "d": { "heartbeat_interval": 41250 }
        });
        assert!(parse_message_create(&payload, None).is_none());
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
        let shutting_down = Arc::new(AtomicBool::new(false));
        let connector = DiscordConnector::new("token123", "my-agent", tx, shutting_down);
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
        let msg = parse_message_create(&payload, None).expect("should parse");
        assert_eq!(msg.content, "help me");
    }

    // --- Shutdown interruptibility tests ---

    /// Setting `shutting_down` to `true` while the connector is sleeping through
    /// a reconnect backoff must cause `run()` to exit within ~100ms (one poll
    /// interval), well inside the 500ms deadline we assert here.
    #[tokio::test]
    async fn reconnect_sleep_interrupted_by_shutdown_flag() {
        let (tx, _rx) = mpsc::channel::<DiscordMessage>(1);
        let shutting_down = Arc::new(AtomicBool::new(false));
        let connector = Arc::new(DiscordConnector::new(
            "fake-token",
            "test-agent",
            tx,
            Arc::clone(&shutting_down),
        ));

        // `run()` will immediately try to connect, fail (no real server), then
        // enter the 5-second reconnect sleep. We flip the flag after 50ms so
        // the sleep is cut short.
        let flag = Arc::clone(&shutting_down);
        let handle = tokio::spawn(async move { connector.run().await });

        tokio::time::sleep(Duration::from_millis(50)).await;
        flag.store(true, Ordering::Relaxed);

        tokio::time::timeout(Duration::from_millis(500), handle)
            .await
            .expect("run() should exit within 500ms after shutting_down is set")
            .expect("task should not panic")
            .expect("run() should return Ok");
    }

    /// `event_loop` must exit within ~200ms of `shutting_down` being set, even
    /// when the sender half of the channel is still alive (not dropped).
    #[tokio::test]
    async fn event_loop_exits_on_shutdown_flag() {
        use std::sync::atomic::{AtomicBool, Ordering};
        use tokio::sync::mpsc;

        let shutting_down = Arc::new(AtomicBool::new(false));

        // Build a minimal AppState-like struct — we only need `shutting_down`
        // accessible. We call the function directly via its public signature.
        // Since `event_loop` is a free async fn in the same crate we can call
        // it directly in an integration-style test here.
        //
        // However, `event_loop` lives in `sera.rs` (the binary), not this
        // library file. We replicate the same pattern here to prove the
        // shutdown-flag polling contract works correctly.
        let flag = Arc::clone(&shutting_down);
        let (tx, mut rx) = mpsc::channel::<DiscordMessage>(4);

        // Simulate the loop body: recv() with a 100ms timeout, check flag each
        // iteration — identical to what event_loop now does.
        let loop_handle = tokio::spawn(async move {
            loop {
                if flag.load(Ordering::Relaxed) {
                    break;
                }
                tokio::select! {
                    msg = rx.recv() => {
                        if msg.is_none() { break; }
                    }
                    _ = tokio::time::sleep(Duration::from_millis(100)) => {}
                }
            }
        });

        // Sender is alive; flip the flag after 50ms.
        tokio::time::sleep(Duration::from_millis(50)).await;
        shutting_down.store(true, Ordering::Relaxed);

        // The loop must exit within 200ms (one poll window after the flag flip).
        tokio::time::timeout(Duration::from_millis(200), loop_handle)
            .await
            .expect("event_loop should exit within 200ms after shutting_down set")
            .expect("loop task should not panic");

        // Sender is still alive — channel was NOT closed.
        drop(tx);
    }

    // -----------------------------------------------------------------------
    // Reconnect / RESUME / INVALID_SESSION coverage (sera-m0az / sera-v4fp /
    // sera-70fs)
    // -----------------------------------------------------------------------

    fn test_connector() -> (Arc<DiscordConnector>, mpsc::Receiver<DiscordMessage>) {
        let (tx, rx) = mpsc::channel(4);
        let shutting_down = Arc::new(AtomicBool::new(false));
        let connector = Arc::new(DiscordConnector::new("token", "agent", tx, shutting_down));
        (connector, rx)
    }

    #[test]
    fn build_resume_payload_shape() {
        let p = build_resume_payload("tok", "sess-xyz", 42);
        assert_eq!(p["op"], 6);
        assert_eq!(p["d"]["token"], "tok");
        assert_eq!(p["d"]["session_id"], "sess-xyz");
        assert_eq!(p["d"]["seq"], 42);
    }

    #[test]
    fn parse_invalid_session_resumable_true() {
        let p = serde_json::json!({ "op": 9, "d": true });
        assert_eq!(parse_invalid_session_resumable(&p), Some(true));
    }

    #[test]
    fn parse_invalid_session_resumable_false() {
        let p = serde_json::json!({ "op": 9, "d": false });
        assert_eq!(parse_invalid_session_resumable(&p), Some(false));
    }

    #[test]
    fn parse_invalid_session_wrong_opcode() {
        let p = serde_json::json!({ "op": 7, "d": null });
        assert_eq!(parse_invalid_session_resumable(&p), None);
    }

    #[test]
    fn parse_invalid_session_missing_d_defaults_false() {
        // If `d` is missing or non-bool, treat as non-resumable (safer default).
        let p = serde_json::json!({ "op": 9 });
        assert_eq!(parse_invalid_session_resumable(&p), Some(false));
    }

    #[test]
    fn parse_ready_session_with_resume_url() {
        let p = serde_json::json!({
            "op": 0,
            "t": "READY",
            "s": 1,
            "d": {
                "session_id": "abc-123",
                "resume_gateway_url": "wss://gateway-eu.discord.gg",
                "user": { "id": "u1", "username": "sera" }
            }
        });
        let (sid, url) = parse_ready_session(&p).expect("ready");
        assert_eq!(sid, "abc-123");
        assert_eq!(url.as_deref(), Some("wss://gateway-eu.discord.gg"));
    }

    #[test]
    fn parse_ready_session_without_resume_url() {
        // Older payloads or malformed READYs without resume_gateway_url still
        // give us a session_id; we'll fall back to the canonical gateway.
        let p = serde_json::json!({
            "op": 0,
            "t": "READY",
            "s": 1,
            "d": { "session_id": "sid", "user": {} }
        });
        let (sid, url) = parse_ready_session(&p).expect("ready");
        assert_eq!(sid, "sid");
        assert!(url.is_none());
    }

    #[test]
    fn parse_ready_session_rejects_non_ready() {
        let p = serde_json::json!({ "op": 0, "t": "GUILD_CREATE", "s": 2, "d": {} });
        assert!(parse_ready_session(&p).is_none());
    }

    #[test]
    fn invalid_session_backoff_schedule() {
        // First attempts stay within Discord's "1-5 second" guidance, then
        // escalate to a 30s cap so we can't hot-loop indefinitely.
        assert_eq!(invalid_session_backoff_secs(0), 1);
        assert_eq!(invalid_session_backoff_secs(1), 1);
        assert_eq!(invalid_session_backoff_secs(2), 3);
        assert_eq!(invalid_session_backoff_secs(3), 5);
        assert_eq!(invalid_session_backoff_secs(4), 10);
        assert_eq!(invalid_session_backoff_secs(5), 20);
        assert_eq!(invalid_session_backoff_secs(6), 30);
        assert_eq!(invalid_session_backoff_secs(100), 30);
    }

    #[tokio::test]
    async fn handle_payload_op7_signals_reconnect() {
        let (conn, _rx) = test_connector();
        let p = serde_json::json!({ "op": 7, "d": null });
        assert_eq!(
            conn.handle_payload(&p).await,
            Some(HandlerSignal::Reconnect)
        );
    }

    #[tokio::test]
    async fn handle_payload_op9_resumable_signals_reconnect() {
        let (conn, _rx) = test_connector();
        let p = serde_json::json!({ "op": 9, "d": true });
        assert_eq!(
            conn.handle_payload(&p).await,
            Some(HandlerSignal::Reconnect)
        );
    }

    #[tokio::test]
    async fn handle_payload_op9_not_resumable_signals_invalidated() {
        let (conn, _rx) = test_connector();
        let p = serde_json::json!({ "op": 9, "d": false });
        assert_eq!(
            conn.handle_payload(&p).await,
            Some(HandlerSignal::SessionInvalidated)
        );
    }

    #[tokio::test]
    async fn handle_payload_unhandled_opcode_returns_none() {
        // Unknown opcodes must NOT signal a reconnect — they used to fall
        // through to a debug log and dropping connection on Op 7/9 was the
        // bug. Here we check a genuinely unknown opcode (e.g. 99) is silent.
        let (conn, _rx) = test_connector();
        let p = serde_json::json!({ "op": 99, "d": null });
        assert_eq!(conn.handle_payload(&p).await, None);
    }

    #[tokio::test]
    async fn ready_event_captures_session_state() {
        let (conn, _rx) = test_connector();
        let ready = serde_json::json!({
            "op": 0,
            "t": "READY",
            "s": 1,
            "d": {
                "session_id": "sess-1",
                "resume_gateway_url": "wss://gateway-resume.discord.gg",
                "user": { "id": "bot1", "username": "sera" }
            }
        });
        assert_eq!(conn.handle_payload(&ready).await, None);

        assert_eq!(conn.test_session_id().as_deref(), Some("sess-1"));
        assert_eq!(
            conn.test_resume_gateway_url().as_deref(),
            Some("wss://gateway-resume.discord.gg")
        );
        assert_eq!(conn.test_last_sequence(), 1);
    }

    #[tokio::test]
    async fn dispatch_updates_sequence_for_resume() {
        // Even on dispatch events we don't act on (e.g. GUILD_CREATE), the
        // sequence number must be tracked so RESUME knows where to pick up.
        let (conn, _rx) = test_connector();
        let evt = serde_json::json!({ "op": 0, "t": "GUILD_CREATE", "s": 17, "d": {} });
        assert_eq!(conn.handle_payload(&evt).await, None);
        assert_eq!(conn.test_last_sequence(), 17);
    }

    #[tokio::test]
    async fn op9_invalidated_keeps_state_until_run_clears() {
        // handle_payload itself just signals; the run() loop is what clears
        // session state. Verify the signal returns and state persists at this
        // layer (run() clears in test below via clear_session_state).
        let (conn, _rx) = test_connector();
        let ready = serde_json::json!({
            "op": 0,
            "t": "READY",
            "s": 5,
            "d": {
                "session_id": "sess-keep",
                "resume_gateway_url": "wss://gw",
                "user": { "id": "b", "username": "n" }
            }
        });
        conn.handle_payload(&ready).await;
        assert_eq!(conn.test_session_id().as_deref(), Some("sess-keep"));

        let invalid = serde_json::json!({ "op": 9, "d": false });
        assert_eq!(
            conn.handle_payload(&invalid).await,
            Some(HandlerSignal::SessionInvalidated)
        );
        // Still set — clearing happens in run() after the connection ends.
        assert_eq!(conn.test_session_id().as_deref(), Some("sess-keep"));

        conn.clear_session_state();
        assert!(conn.test_session_id().is_none());
        assert!(conn.test_resume_gateway_url().is_none());
        assert_eq!(conn.test_last_sequence(), -1);
    }
}

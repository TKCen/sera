//! Discord Gateway connector — connects via raw WebSocket, handles heartbeat,
//! and dispatches MESSAGE_CREATE events through an mpsc channel.
//!
//! ## Threat model (sera-h1iq)
//!
//! Inbound payloads arrive over a TLS WebSocket so "hostile" payloads are not
//! the normal case. The parser is still expected to survive misbehaving
//! gateway regions or MITM scenarios that deliver malformed JSON: every
//! `parse_*` helper here returns `Option` (never panics) on missing fields,
//! wrong types, truncated payloads, or unicode-heavy strings. Inbound
//! `MESSAGE_CREATE` content is hard-bounded by
//! [`MAX_MESSAGE_CONTENT_BYTES`] so a hostile gateway cannot OOM the runtime
//! via a single 1MB payload.
//!
//! ## Reconnect / lock primitives (sera-bib9)
//!
//! `bot_user_id` is set exactly once on the first READY dispatch and never
//! cleared (it is the bot's own immutable Discord user id). It is stored in
//! [`std::sync::OnceLock`] so async handlers can read it without grabbing a
//! blocking `Mutex` on the tokio executor thread. The remaining session
//! state (`session_id`, `resume_gateway_url`) is still gated by short-held
//! `std::sync::Mutex` critical sections that never span an `.await`; the
//! clippy `await_holding_lock` lint will catch any future regression.

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

/// One user mentioned in an inbound Discord MESSAGE_CREATE payload. Captured
/// verbatim from the raw `d.mentions` array so the routing layer can decide
/// which mentions are safe to expose to the runtime / echo on outbound (the
/// connector-specific `peer_bots` allowlist filters this list — arbitrary
/// user mentions are never preserved past that filter). See sera-yeg.4.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DiscordMentionedUser {
    pub id: String,
    pub username: String,
}

/// A peer-bot binding observed in an inbound message and confirmed against
/// the connector's `peer_bots` allowlist. The runtime sees the message with
/// `<@id>` tags rewritten to `@<handle>` text tokens; outbound replies that
/// reference the same `@<handle>` are rewritten back to `<@id>` so Discord
/// renders an actual mention. This is the data carrier for the reverse
/// peer-mention handoff path (sera-yeg.4).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PeerMentionBinding {
    /// Discord user id (used to render `<@id>` outbound mention tags).
    pub user_id: String,
    /// Configured peer-bot handle (the form runtime/LLM transcript sees).
    pub handle: String,
}

/// Message received from Discord, ready for the gateway event queue.
#[derive(Debug, Clone)]
pub struct DiscordMessage {
    pub channel_id: String,
    pub user_id: String,
    pub username: String,
    /// Mention-stripped content (legacy field, kept for back-compat with the
    /// pre-sera-yeg.4 routing layer and tests that synthesize messages
    /// without filling `raw_content`). When `raw_content` is non-empty the
    /// routing layer prefers it so the configured peer-bot allowlist can
    /// drive normalization (`normalize_content_for_runtime`).
    pub content: String,
    pub message_id: String,
    /// Whether this message was a DM (not a guild channel).
    pub is_dm: bool,
    /// Whether the bot was mentioned in this message (for guild channels).
    pub mentions_bot: bool,
    /// Whether the message author is itself a Discord bot. SERA's own bot
    /// messages are always filtered upstream in [`parse_message_create`];
    /// peer bots make it through with this flag set so the routing layer
    /// (`process_message`) can apply the configured peer-bot policy.
    pub is_bot: bool,
    /// Manifest name of the [`DiscordConnector`] that produced this message.
    /// Set by [`DiscordConnector`] as it dispatches into the shared mpsc;
    /// `None` only for synthetic test fixtures. The routing layer uses this
    /// to look up the *connector-specific* `peer_bots` allowlist rather than
    /// taking the first parsed discord connector in manifests (sera-yeg.1
    /// repair, Codex comment 3274130773).
    pub connector_name: Option<String>,
    /// Raw message content as received from Discord, with `<@id>` mention
    /// tags intact. `Some` for messages produced by [`parse_message_create`];
    /// synthetic test fixtures may leave this empty, in which case the
    /// routing layer falls back to `content`.
    pub raw_content: String,
    /// Users mentioned in the raw payload (one entry per `d.mentions`
    /// element). Empty when no `<@id>` tags were resolved by Discord. The
    /// routing layer filters this through the connector's `peer_bots`
    /// allowlist before exposing any mention to the runtime / echoing it on
    /// outbound replies (sera-yeg.4).
    pub mentions: Vec<DiscordMentionedUser>,
}

/// Discord Gateway connector — connects via WebSocket, handles heartbeat,
/// dispatches messages.
pub struct DiscordConnector {
    token: String,
    agent_name: String,
    /// Manifest name of the connector this instance was spawned for. Used to
    /// stamp outgoing [`DiscordMessage`]s so the routing layer can look up
    /// per-connector configuration (e.g. `peer_bots`) instead of folding
    /// across all discord connectors.
    connector_name: String,
    tx: mpsc::Sender<DiscordMessage>,
    /// Bot's own user ID, set on READY event. Single-write: populated the
    /// first time the gateway emits a READY dispatch and never mutated
    /// thereafter (the bot's own id is immutable). Stored in `OnceLock` so
    /// async handlers can read it lock-free without parking the executor
    /// thread (sera-bib9).
    bot_user_id: std::sync::OnceLock<String>,
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
    /// Set to `true` the first time the current connection receives a READY
    /// dispatch. The reconnect loop checks this after `connect_and_run`
    /// returns: if the connection ever reached READY, the consecutive
    /// connect-failure counter resets so a transient transport drop after a
    /// healthy run does not pay full exponential backoff (sera-dxmv).
    observed_ready: Arc<AtomicBool>,
}

/// Why the current gateway connection ended. Drives the backoff and IDENTIFY-vs-RESUME
/// decision in [`DiscordConnector::run`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum DisconnectReason {
    /// Server explicitly asked us to reconnect (Op 7 RECONNECT) or said the
    /// session was invalid-but-resumable (Op 9 with `d=true`). Keep session
    /// state and apply a short 1s backoff before reconnecting + RESUMing.
    OpcodeResumable,
    /// Server told us the session is dead (Op 9 with `d=false`). The connector
    /// must clear stored session state and IDENTIFY again with a long backoff.
    SessionInvalidated,
    /// We sent RESUME on this connection, the server never sent a `RESUMED`
    /// dispatch confirming the resume succeeded, and the loop ended without
    /// an opcode signal. This straddles two real cases: Discord rejecting
    /// RESUME via a close code (4007 invalid seq, 4009 session timeout, etc.)
    /// and a flaky transport drop before any opcode arrived. `run()`
    /// distinguishes them by counting consecutive occurrences — a single
    /// occurrence retries RESUME (preserving a still-valid session through
    /// transient drops), repeated occurrences clear session state and fall
    /// back to IDENTIFY.
    ///
    /// A close after `RESUMED` is observed is `GenericClose`, not this
    /// variant — at that point the resume has already succeeded and the
    /// disconnect is unrelated to resume status.
    ResumeFailedNoSignal,
    /// IDENTIFY-path connection ended without an opcode signal — generic
    /// transport close, IO error, or server-side close without an opcode.
    /// Apply the standard 5s backoff and keep whatever session state a READY
    /// may have populated (so the next attempt can RESUME if applicable).
    GenericClose,
}

impl DisconnectReason {
    /// Pure classifier so the mapping is unit-testable without a real socket.
    ///
    /// * `signal` — opcode-driven termination from the event loop.
    /// * `attempted_resume` — `true` if this connection was opened with a RESUME
    ///   payload (vs IDENTIFY).
    /// * `resume_succeeded` — `true` once we have observed a `RESUMED` dispatch
    ///   on this connection, indicating the resume was accepted by the server.
    ///   After that point a no-signal close is just a transport drop on a
    ///   confirmed session, not a resume failure.
    fn from_signal(
        signal: Option<HandlerSignal>,
        attempted_resume: bool,
        resume_succeeded: bool,
    ) -> Self {
        match signal {
            Some(HandlerSignal::SessionInvalidated) => Self::SessionInvalidated,
            Some(HandlerSignal::Reconnect) => Self::OpcodeResumable,
            None if attempted_resume && !resume_succeeded => Self::ResumeFailedNoSignal,
            None => Self::GenericClose,
        }
    }
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

/// Number of consecutive RESUME-without-opcode-signal disconnects we tolerate
/// before clearing session state and falling back to IDENTIFY. A single
/// occurrence is treated as a likely transient transport drop and triggers a
/// RESUME retry; further occurrences are treated as Discord rejecting RESUME
/// via a close code and force a fresh IDENTIFY.
const RESUME_NO_SIGNAL_CLEAR_THRESHOLD: u32 = 2;

/// Returns `true` if `run()` should clear session state and IDENTIFY on the
/// next attempt instead of retrying RESUME. Extracted so the retry policy is
/// unit-testable without spinning up the full gateway loop.
fn resume_no_signal_should_clear(consecutive: u32) -> bool {
    consecutive >= RESUME_NO_SIGNAL_CLEAR_THRESHOLD
}

/// Number of consecutive connect-stage errors against `resume_gateway_url` we
/// tolerate before falling back to the canonical gateway URL. Discord's
/// canonical gateway accepts RESUME for the same `session_id`, so clearing
/// only the resume host (not the session) recovers from a stale or
/// regionally-degraded resume URL without forcing an IDENTIFY.
const RESUME_URL_FAILURE_THRESHOLD: u32 = 2;

/// Returns `true` if `run()` should clear `resume_gateway_url` and target the
/// canonical gateway on the next attempt. A single error preserves the
/// optimization for transient blips; repeated errors release a stuck host.
fn resume_url_should_clear(consecutive: u32) -> bool {
    consecutive >= RESUME_URL_FAILURE_THRESHOLD
}

/// Hard upper bound on the byte length of the `content` field
/// [`parse_message_create`] forwards into the runtime. Discord's own message
/// ceiling is 4000 bytes (Nitro) but a misbehaving gateway region or a
/// hostile intermediary could deliver an arbitrarily large payload — without
/// this clamp the connector would happily copy a 1MB string into every
/// downstream log line and the runtime mpsc queue (sera-h1iq).
///
/// Set to 16 KiB so legitimate code-block messages still pass through
/// uncut while malicious oversized payloads get a single truncation event.
pub const MAX_MESSAGE_CONTENT_BYTES: usize = 16 * 1024;

/// Truncate `content` to at most [`MAX_MESSAGE_CONTENT_BYTES`], preserving
/// UTF-8 char boundaries so the returned string never sits inside a
/// multi-byte sequence. Returns the (possibly truncated) string and `true`
/// if truncation actually happened. Public for unit testing (sera-h1iq).
pub fn clamp_message_content(content: &str) -> (String, bool) {
    if content.len() <= MAX_MESSAGE_CONTENT_BYTES {
        return (content.to_owned(), false);
    }
    let mut end = MAX_MESSAGE_CONTENT_BYTES;
    while end > 0 && !content.is_char_boundary(end) {
        end -= 1;
    }
    (content[..end].to_owned(), true)
}

/// Truncated-exponential backoff schedule for generic-close / transport-error
/// reconnects (sera-dxmv).
///
/// Discord's developer guidance for gateway reconnects is "truncated
/// exponential with jitter, 1s base, 30s cap". This function returns the
/// *deterministic* base seconds for an attempt index so the schedule can be
/// asserted in unit tests without depending on randomness. Callers apply
/// jitter via [`apply_backoff_jitter`] before sleeping.
///
/// `attempt` is 1-based: `1 → 1s`, `2 → 2s`, `3 → 4s`, `4 → 8s`, `5 → 16s`,
/// then capped at 30s for every subsequent failure. `0` returns `1` so a
/// caller that hasn't incremented yet still gets the base delay.
pub fn connect_backoff_secs(attempt: u32) -> u64 {
    if attempt == 0 {
        return 1;
    }
    // 1 << 31 already overflows u64::MAX once multiplied by base, so saturate
    // at 30s long before that point.
    let shift = attempt.saturating_sub(1).min(30);
    let value = 1u64.checked_shl(shift).unwrap_or(30);
    value.min(30)
}

/// Apply ±`jitter_pct` percent jitter to `base_secs`, drawing from `rng`.
/// Returns a duration in seconds bounded to `[1, 60]` so a buggy rng or
/// extreme base can't make us sleep forever or fire instantly.
///
/// Extracted as a pure function so the deterministic-mode unit test can
/// pass `jitter_pct = 0` and observe the unjittered schedule exactly
/// (sera-dxmv acceptance #3).
pub fn apply_backoff_jitter<R: rand::Rng>(base_secs: u64, jitter_pct: u8, rng: &mut R) -> u64 {
    if jitter_pct == 0 || base_secs == 0 {
        return base_secs.clamp(1, 60);
    }
    let pct = (jitter_pct as f64).min(100.0) / 100.0;
    let base = base_secs as f64;
    let jitter = rng.gen_range(-pct..=pct);
    let jittered = (base * (1.0 + jitter)).round();
    let bounded = jittered.clamp(1.0, 60.0);
    bounded as u64
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

/// Compiled regex matching Discord user mention tags, capturing the numeric
/// id. Shared by [`normalize_content_for_runtime`] so allow- vs deny-list
/// decisions key off the same parse the rest of the connector uses.
fn mention_tag_re() -> &'static regex::Regex {
    use std::sync::LazyLock;
    static RE: LazyLock<regex::Regex> =
        LazyLock::new(|| regex::Regex::new(r"<@!?(\d+)>").expect("valid regex"));
    &RE
}

/// Filter the inbound `mentions` array through the connector's `peer_bots`
/// allowlist (matched by id OR username, same as [`decide_bot_policy`]).
/// SERA's own bot id is excluded so the self mention used for routing never
/// leaks into the runtime view (sera-yeg.4).
///
/// Returns the bindings the routing layer can safely expose to the runtime
/// and use to render outbound mention tags. Empty when nothing matched.
pub fn peer_directory_from_mentions(
    mentions: &[DiscordMentionedUser],
    self_bot_id: Option<&str>,
    peer_bots: &[String],
) -> Vec<PeerMentionBinding> {
    mentions
        .iter()
        .filter(|m| match self_bot_id {
            Some(sid) => m.id != sid,
            None => true,
        })
        .filter(|m| {
            peer_bots
                .iter()
                .any(|p| !p.is_empty() && (p == &m.id || p == &m.username))
        })
        .map(|m| PeerMentionBinding {
            user_id: m.id.clone(),
            handle: m.username.clone(),
        })
        .collect()
}

/// Build the runtime-visible content from raw Discord text:
/// * `<@self_bot_id>` (and `<@!self_bot_id>`) → stripped — the self mention
///   is used for inbound routing only and must not appear in transcript.
/// * `<@peer_id>` where `peer_id` is in `peer_directory` → replaced with
///   `@<handle>` so the LLM sees the peer reference as plain text.
/// * Every other `<@id>` tag → stripped. Arbitrary user mentions must never
///   reach the runtime: that would enable mention-spam / prompt-injection
///   for any guild member that happens to ping SERA.
///
/// Whitespace is collapsed and trimmed to match [`strip_mentions`].
pub fn normalize_content_for_runtime(
    raw: &str,
    self_bot_id: Option<&str>,
    peer_directory: &[PeerMentionBinding],
) -> String {
    let replaced = mention_tag_re().replace_all(raw, |caps: &regex::Captures<'_>| {
        let id = &caps[1];
        if let Some(sid) = self_bot_id
            && id == sid
        {
            return String::new();
        }
        if let Some(binding) = peer_directory.iter().find(|b| b.user_id == id) {
            return format!("@{}", binding.handle);
        }
        String::new()
    });
    replaced.split_whitespace().collect::<Vec<_>>().join(" ")
}

/// Render an outbound Discord message by substituting `@<handle>` tokens
/// with `<@user_id>` for each entry in `peer_directory`. Other text passes
/// through unchanged so arbitrary user-supplied `@names` cannot become
/// outbound pings — only allowlisted peer-bot handles get rewritten.
///
/// Longer handles are processed first so `@Sera 2.0` is preferred over
/// `@Sera` when both appear in the directory (sera-yeg.4 acceptance #2).
pub fn render_outbound_content(content: &str, peer_directory: &[PeerMentionBinding]) -> String {
    let mut entries: Vec<&PeerMentionBinding> = peer_directory.iter().collect();
    entries.sort_by_key(|b| std::cmp::Reverse(b.handle.len()));
    let mut out = content.to_owned();
    for binding in entries {
        let needle = format!("@{}", binding.handle);
        let replacement = format!("<@{}>", binding.user_id);
        out = out.replace(&needle, &replacement);
    }
    out
}

/// Try to extract a `DiscordMessage` from a Dispatch (opcode 0) payload
/// with `t == "MESSAGE_CREATE"`.
///
/// Returns `None` if the payload is not a MESSAGE_CREATE dispatch, or if the
/// message author is SERA's own bot (loop-safety). Other bot authors are
/// surfaced with `is_bot = true` so the routing layer can apply the
/// configured peer-bot policy (sera-yeg.1).
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
    let author_id = author.get("id")?.as_str()?;

    // Hard loop-safety: never surface SERA's own bot messages to the routing
    // layer. This is the only filter applied here; unrelated bots and peer
    // bots are surfaced with `is_bot = true` so policy can decide.
    if let Some(self_id) = bot_user_id
        && author_id == self_id
    {
        return None;
    }

    let is_bot = author.get("bot").and_then(Value::as_bool).unwrap_or(false);

    // Check if this is a DM (no guild_id) or mentions the bot
    let is_dm = d.get("guild_id").is_none();
    let mentions: Vec<DiscordMentionedUser> = d
        .get("mentions")
        .and_then(|m| m.as_array())
        .map(|arr| {
            arr.iter()
                .filter_map(|u| {
                    let id = u.get("id")?.as_str()?.to_owned();
                    let username = u.get("username")?.as_str()?.to_owned();
                    Some(DiscordMentionedUser { id, username })
                })
                .collect()
        })
        .unwrap_or_default();
    let mentions_bot = match bot_user_id {
        Some(bot_id) => mentions.iter().any(|m| m.id == bot_id),
        None => false,
    };

    // Bound the content the connector forwards so a hostile or misbehaving
    // gateway can't OOM the runtime / log pipeline via a single oversized
    // payload (sera-h1iq).
    let raw_content_str = d.get("content")?.as_str()?;
    let (raw_content, truncated) = clamp_message_content(raw_content_str);
    if truncated {
        tracing::warn!(
            original_len = raw_content_str.len(),
            kept_len = raw_content.len(),
            "Inbound Discord message content exceeded MAX_MESSAGE_CONTENT_BYTES; truncated before forwarding",
        );
    }

    Some(DiscordMessage {
        channel_id: d.get("channel_id")?.as_str()?.to_owned(),
        user_id: author_id.to_owned(),
        username: author.get("username")?.as_str()?.to_owned(),
        content: strip_mentions(&raw_content),
        message_id: d.get("id")?.as_str()?.to_owned(),
        is_dm,
        mentions_bot,
        is_bot,
        // Stamped by the dispatching `DiscordConnector` (which knows its
        // own connector_name); the pure parser deliberately stays
        // identity-agnostic so it remains unit-testable without one.
        connector_name: None,
        raw_content,
        mentions,
    })
}

/// Routing decision for a Discord message authored by a bot.
///
/// Pure data — no I/O — so the policy is unit-testable without standing up
/// a gateway. `decide_bot_policy` is the sole producer; `process_message`
/// is the sole consumer.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BotPolicyDecision {
    /// Author is human or a configured peer bot with explicit handoff.
    Accept,
    /// Author is SERA's own bot. Defense-in-depth — `parse_message_create`
    /// already drops these.
    IgnoreSelf,
    /// Author is a bot we don't recognize as a peer.
    IgnoreUnrelated,
    /// Author is a configured peer bot but the message lacks an explicit
    /// handoff (DM or @-mention of SERA).
    IgnoreNoHandoff,
}

impl BotPolicyDecision {
    /// Human-readable tag for audit metadata.
    pub fn audit_reason(self) -> &'static str {
        match self {
            BotPolicyDecision::Accept => "accept",
            BotPolicyDecision::IgnoreSelf => "ignored_self_bot",
            BotPolicyDecision::IgnoreUnrelated => "ignored_unrelated_bot",
            BotPolicyDecision::IgnoreNoHandoff => "ignored_peer_bot_no_handoff",
        }
    }
}

/// Top-level routing gate for an inbound Discord message. Composes the
/// peer-bot policy with the human DM/mention filter so the gate order is
/// expressed in one pure function instead of inline in `process_message`.
///
/// Rationale (sera-yeg.1 repair): the original `process_message` returned
/// early on `!is_explicit_handoff` *before* running `decide_bot_policy`,
/// which silently dropped unrelated bots in public channels and peer bots
/// without handoff — including the audit log entry the spec requires.
/// Routing now goes through this helper so bot-authored messages always
/// run through the peer-bot policy (auditable) while human messages keep
/// the cheap "not a DM, not mentioned → ignore" fast path.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MessageGateDecision {
    /// Message should proceed to the turn pipeline.
    Accept,
    /// Human author posting in a guild channel without @-mention — drop
    /// silently (debug log only) to keep public channels low-noise.
    IgnoreHumanNoHandoff,
    /// Bot author was denied by the peer-bot policy; carries the reason so
    /// the caller can audit-log it under `discord_message_ignored`.
    IgnoreBot(BotPolicyDecision),
}

/// Decide whether to admit an inbound Discord message to the turn pipeline.
///
/// Order of gates (sera-yeg.1):
/// * Bot authors always go through [`decide_bot_policy`] regardless of
///   DM/mention status, so self bots, unrelated bots, and peer bots
///   without explicit handoff all produce an [`IgnoreBot`] decision the
///   caller must audit. This is what makes `ignored_peer_bot_no_handoff`
///   and `ignored_unrelated_bot` actually appear in the live audit log
///   instead of being dropped silently by the human-channel filter.
/// * Human authors keep the original behavior: non-DM messages without an
///   @-mention return [`IgnoreHumanNoHandoff`] (silent ignore, no audit
///   row), everything else returns [`Accept`].
///
/// `is_explicit_handoff` must be `msg.is_dm || msg.mentions_bot` — the
/// caller computes it once and passes it in.
///
/// [`IgnoreBot`]: MessageGateDecision::IgnoreBot
/// [`IgnoreHumanNoHandoff`]: MessageGateDecision::IgnoreHumanNoHandoff
/// [`Accept`]: MessageGateDecision::Accept
pub fn gate_message(
    is_bot: bool,
    author_id: &str,
    username: &str,
    self_bot_id: Option<&str>,
    is_explicit_handoff: bool,
    peer_bots: &[String],
) -> MessageGateDecision {
    if is_bot {
        let policy = decide_bot_policy(
            true,
            author_id,
            username,
            self_bot_id,
            is_explicit_handoff,
            peer_bots,
        );
        return match policy {
            BotPolicyDecision::Accept => MessageGateDecision::Accept,
            denied => MessageGateDecision::IgnoreBot(denied),
        };
    }
    if !is_explicit_handoff {
        return MessageGateDecision::IgnoreHumanNoHandoff;
    }
    MessageGateDecision::Accept
}

/// Decide whether to accept a Discord message under the peer-bot policy.
///
/// Loop-safety contract (sera-yeg.1):
/// * Human authors are always accepted.
/// * SERA's own bot id is always ignored (already filtered upstream).
/// * Other bot authors are accepted only if `author_id` or `username` matches
///   an entry in `peer_bots` AND the message is an explicit handoff to SERA
///   (DM or @-mention).
pub fn decide_bot_policy(
    is_bot: bool,
    author_id: &str,
    username: &str,
    self_bot_id: Option<&str>,
    is_explicit_handoff: bool,
    peer_bots: &[String],
) -> BotPolicyDecision {
    if !is_bot {
        return BotPolicyDecision::Accept;
    }
    if let Some(self_id) = self_bot_id
        && author_id == self_id
    {
        return BotPolicyDecision::IgnoreSelf;
    }
    let is_peer = peer_bots
        .iter()
        .any(|p| !p.is_empty() && (p == author_id || p == username));
    if !is_peer {
        return BotPolicyDecision::IgnoreUnrelated;
    }
    if !is_explicit_handoff {
        return BotPolicyDecision::IgnoreNoHandoff;
    }
    BotPolicyDecision::Accept
}

/// Build the REST URL for the Discord typing-indicator endpoint.
/// Extracted for unit testability (no live Discord required).
pub fn typing_indicator_url(channel_id: &str) -> String {
    format!("{DISCORD_API_BASE}/channels/{channel_id}/typing")
}

/// Build the REST URL for adding/removing the bot's own reaction on a
/// message. Discord expects the emoji URL-encoded (`%F0%9F%91%80` for `👀`).
pub fn own_reaction_url(channel_id: &str, message_id: &str, emoji: &str) -> String {
    let emoji_enc = urlencoding::encode(emoji);
    format!(
        "{DISCORD_API_BASE}/channels/{channel_id}/messages/{message_id}/reactions/{emoji_enc}/@me"
    )
}

// ---------------------------------------------------------------------------
// Status-card session surface (sera-yeg.6)
//
// A status card is a single Discord message — optionally posted inside a
// thread anchored on the inbound operator message — that SERA edits in place
// across the lifecycle of a turn. It gives operators a non-noisy "workcell"
// surface showing terminal state (accepted/running/blocked/done/failed)
// instead of the bot looking silent while a long turn is in flight.
//
// The state machine + renderer are pure and unit-tested here; the REST
// methods on `DiscordConnector` and the wire-up in the message-processing
// pipeline are best-effort UX affordances — failures degrade silently and
// never propagate as turn failures, matching the typing/reaction lifecycle
// from sera-yeg.2.
// ---------------------------------------------------------------------------

/// Build the REST URL for starting a thread from an existing message.
/// Endpoint: `POST /channels/{channel_id}/messages/{message_id}/threads`.
pub fn start_thread_from_message_url(channel_id: &str, message_id: &str) -> String {
    format!("{DISCORD_API_BASE}/channels/{channel_id}/messages/{message_id}/threads")
}

/// Build the REST URL for posting messages to a channel (or thread —
/// Discord exposes threads as channels for message ops).
pub fn channel_messages_url(channel_id: &str) -> String {
    format!("{DISCORD_API_BASE}/channels/{channel_id}/messages")
}

/// Build the REST URL for editing the bot's own message.
/// Endpoint: `PATCH /channels/{channel_id}/messages/{message_id}`.
pub fn edit_message_url(channel_id: &str, message_id: &str) -> String {
    format!("{DISCORD_API_BASE}/channels/{channel_id}/messages/{message_id}")
}

/// Lifecycle state of a Discord status card. Drives the rendered text and
/// the terminal/non-terminal classification used by the message-processing
/// pipeline to decide whether further edits are allowed.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum StatusCardState {
    /// Turn was accepted by the connector gate and is queued / starting.
    Accepted,
    /// Turn is actively executing (post-pre-route, runtime running).
    Running,
    /// Turn is waiting on an external dependency (hook redirect, HITL, etc.).
    /// Carries a short operator-facing reason (sanitized at render time).
    ///
    /// Reserved for the hook/HITL wait integrations that will hang off the
    /// existing pre_route / pre_tool chains — not constructed by the live
    /// pipeline today (current paths terminate in `Done`/`Failed`), but is
    /// part of the state machine contract and exercised by unit tests.
    #[allow(dead_code)]
    Blocked(String),
    /// Turn ran to completion and a reply was sent.
    Done,
    /// Turn ended in an error (hook reject, runtime failure, transport).
    /// Carries a short operator-facing reason (sanitized at render time).
    Failed(String),
}

impl StatusCardState {
    /// True for `Done`/`Failed`. Terminal states must not be edited further;
    /// the pipeline must drop further status updates once a card is terminal.
    pub fn is_terminal(&self) -> bool {
        matches!(self, StatusCardState::Done | StatusCardState::Failed(_))
    }
}

/// A status-card session anchored on the inbound Discord message. Owns the
/// posted message id (after first publish) and the current state — the
/// pipeline calls `transition` and re-renders to drive edits.
#[derive(Debug, Clone)]
pub struct StatusCard {
    /// Short audit id displayed on the card. Trimmed and length-clamped by
    /// [`sanitize_audit_id_for_display`] before storage so the renderer
    /// never needs to re-sanitize.
    audit_id: String,
    /// Current lifecycle state. Mutated through [`StatusCard::transition`].
    state: StatusCardState,
}

impl StatusCard {
    /// Create a new card in [`StatusCardState::Accepted`] with a display-safe
    /// audit id derived from the input.
    pub fn new(audit_id: &str) -> Self {
        Self {
            audit_id: sanitize_audit_id_for_display(audit_id),
            state: StatusCardState::Accepted,
        }
    }

    pub fn state(&self) -> &StatusCardState {
        &self.state
    }

    pub fn audit_id(&self) -> &str {
        &self.audit_id
    }

    /// Apply a state transition. Returns `Err` with the current state if the
    /// card is already terminal — callers should drop the update rather than
    /// editing the live Discord message past a final ✅/❌ state.
    pub fn transition(&mut self, next: StatusCardState) -> Result<(), StatusCardState> {
        if self.state.is_terminal() {
            return Err(self.state.clone());
        }
        self.state = next;
        Ok(())
    }

    /// Render the card as a single Discord message body — privacy-clean
    /// (no raw channel ids, file paths, tokens, transcripts) and short
    /// enough to fit comfortably under Discord's 2000-char message limit.
    pub fn render(&self) -> String {
        render_status_card(&self.audit_id, &self.state)
    }
}

/// Trim a raw audit id / session id to a display-safe short prefix. Keeps
/// roughly the first 12 chars (matches the short-sha convention used in
/// other SERA operator surfaces) and strips characters Discord markdown
/// would interpret (` ``` ` and backticks specifically). Non-ascii input
/// is preserved verbatim up to the limit so callers can pass UUIDs or
/// other ascii identifiers without quoting concerns.
pub fn sanitize_audit_id_for_display(raw: &str) -> String {
    let trimmed = raw.trim();
    let cleaned: String = trimmed
        .chars()
        .filter(|c| *c != '`' && *c != '\n' && *c != '\r')
        .collect();
    if cleaned.is_empty() {
        return "unknown".to_owned();
    }
    // Clamp at 12 chars — long enough to disambiguate sessions in operator
    // surfaces, short enough to keep the card compact.
    cleaned.chars().take(12).collect()
}

/// Sanitize an operator-facing reason string for inclusion on a status card.
/// Strips control characters, collapses whitespace, clamps length, and
/// removes Discord code-fence backticks so a malformed reason cannot break
/// the card's markdown.
pub fn sanitize_status_reason(raw: &str) -> String {
    let cleaned: String = raw
        .chars()
        .map(|c| {
            if c == '`' || c == '\n' || c == '\r' || c == '\t' {
                ' '
            } else {
                c
            }
        })
        .filter(|c| !c.is_control())
        .collect();
    // Collapse runs of whitespace.
    let mut out = String::with_capacity(cleaned.len());
    let mut prev_space = false;
    for c in cleaned.chars() {
        if c == ' ' {
            if !prev_space {
                out.push(' ');
            }
            prev_space = true;
        } else {
            out.push(c);
            prev_space = false;
        }
    }
    let trimmed = out.trim();
    // Clamp at 200 chars so the card stays compact even on long reasons.
    if trimmed.chars().count() > 200 {
        let mut clamped: String = trimmed.chars().take(197).collect();
        clamped.push('…');
        clamped
    } else {
        trimmed.to_owned()
    }
}

/// Build the JSON body for a status-card create/edit request. Disables
/// all mention parsing so a sanitized-but-mention-shaped reason cannot
/// generate Discord notifications (`@everyone`, `<@id>`, role pings,
/// etc.). Exposed for unit-testability of the wire shape.
pub fn status_card_payload(content: &str) -> Value {
    serde_json::json!({
        "content": content,
        "allowed_mentions": { "parse": [] },
    })
}

/// Render a status card body. Pure function exposed for unit testing —
/// callers should normally hold a [`StatusCard`] and call `render`.
pub fn render_status_card(audit_id: &str, state: &StatusCardState) -> String {
    let (label, emoji) = match state {
        StatusCardState::Accepted => ("Accepted", "📥"),
        StatusCardState::Running => ("Running", "⚙️"),
        StatusCardState::Blocked(_) => ("Blocked", "⏸"),
        StatusCardState::Done => ("Done", "✅"),
        StatusCardState::Failed(_) => ("Failed", "❌"),
    };
    let mut body = format!("**{emoji} SERA · {label}** · `{audit_id}`");
    if let StatusCardState::Blocked(reason) | StatusCardState::Failed(reason) = state {
        let sanitized = sanitize_status_reason(reason);
        if !sanitized.is_empty() {
            body.push('\n');
            body.push_str(&sanitized);
        }
    }
    body
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
// Outbound long-message chunking (sera-yeg.8)
//
// Discord rejects message `content` over 2000 unicode characters with a 400.
// A runtime reply that overflows the limit would either disappear entirely or
// surface as an opaque transport error to the operator. The chunker splits an
// overlong reply into ordered `[i/N]`-labeled parts at safe boundaries
// (paragraph → newline → whitespace), capped at a small policy ceiling so a
// runaway reply cannot dump dozens of messages into a channel. When the cap
// is hit, the unsupported case is made visible with a final ⚠ notice and
// audited via `tracing::warn`.
// ---------------------------------------------------------------------------

/// Discord's hard limit for the `content` field of a message (in unicode
/// characters). Source: Discord API docs — `CREATE_MESSAGE`.
pub const DISCORD_CONTENT_LIMIT: usize = 2000;

/// Reservation for the `\n[i/N]` continuation suffix appended to each chunk
/// when content spans multiple messages. Worst-case `\n[6/6]` is 6 chars; we
/// reserve 12 so a future cap bump (or a misbehaving formatter that adds an
/// extra newline) still leaves headroom under [`DISCORD_CONTENT_LIMIT`].
pub const DISCORD_CHUNK_SUFFIX_RESERVE: usize = 12;

/// Per-chunk payload budget — content limit minus the suffix reservation.
pub const DISCORD_CHUNK_BUDGET: usize = DISCORD_CONTENT_LIMIT - DISCORD_CHUNK_SUFFIX_RESERVE;

/// Maximum number of content chunks the gateway will emit for a single
/// reply before falling over to the visible-truncation path. Six gives a
/// realistic long answer (~12 KB) room to land in-channel while still
/// stopping a pathological reply from spamming the operator.
pub const DISCORD_MAX_CHUNKS: usize = 6;

/// Audit classification produced by [`chunk_for_discord`]. Drives the
/// `tracing` level used by [`DiscordConnector::send_message_chunked`] so
/// the chunked vs truncated paths land in distinct log buckets.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ChunkAudit {
    /// Reply fits in a single message; sent verbatim.
    SingleMessage,
    /// Reply was split into `parts` ordered chunks (`parts >= 2`); every
    /// part of the original content was delivered.
    Chunked { parts: usize },
    /// Reply exceeded the policy cap. `kept` chunks were emitted with a
    /// trailing visible ⚠ notice; `dropped_chunks` parts of the original
    /// content did not reach the channel.
    Truncated { kept: usize, dropped_chunks: usize },
}

/// Plan for delivering an outbound reply to Discord. `parts` is the
/// ordered list of message bodies the connector will POST; `audit`
/// describes which path the chunker took for logging/test assertions.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ChunkPlan {
    pub parts: Vec<String>,
    pub audit: ChunkAudit,
}

/// Render an outbound reply into one or more Discord-shaped messages,
/// each under [`DISCORD_CONTENT_LIMIT`]. Returns a [`ChunkPlan`] whose
/// `parts` are ready to POST verbatim and whose `audit` records whether
/// the content was sent whole, split, or truncated with a visible notice.
///
/// Boundary preference (descending): paragraph break (`\n\n`), single
/// newline, whitespace, then a hard cut at the byte budget when nothing
/// softer exists. Trailing whitespace on each chunk is trimmed so the
/// `[i/N]` suffix renders cleanly. Pure function — no I/O.
pub fn chunk_for_discord(content: &str) -> ChunkPlan {
    let trimmed = content.trim_end_matches(['\n', '\r', ' ', '\t']);
    if trimmed.is_empty() {
        return ChunkPlan {
            parts: vec![String::new()],
            audit: ChunkAudit::SingleMessage,
        };
    }
    if trimmed.chars().count() <= DISCORD_CONTENT_LIMIT {
        return ChunkPlan {
            parts: vec![trimmed.to_owned()],
            audit: ChunkAudit::SingleMessage,
        };
    }

    let raw_chunks = split_for_chunking(trimmed, DISCORD_CHUNK_BUDGET);
    let total_proposed = raw_chunks.len();
    let mut iter = raw_chunks.into_iter();
    let kept: Vec<String> = (&mut iter).take(DISCORD_MAX_CHUNKS).collect();
    let dropped = total_proposed.saturating_sub(kept.len());

    let visible_total = kept.len() + usize::from(dropped > 0);
    let mut parts: Vec<String> = Vec::with_capacity(visible_total);
    for (i, body) in kept.iter().enumerate() {
        if visible_total == 1 {
            parts.push(body.clone());
        } else {
            // Strip only trailing newlines on the body so the `\n[i/N]`
            // suffix renders adjacently; intentional leading whitespace
            // (Markdown indentation, code blocks, list items) is left
            // intact across the boundary.
            let base = body.trim_end_matches(['\n', '\r']);
            parts.push(format!("{base}\n[{}/{}]", i + 1, visible_total));
        }
    }

    if dropped > 0 {
        parts.push(format!(
            "⚠ output truncated — {dropped} part(s) omitted\n[{}/{}]",
            visible_total, visible_total
        ));
        ChunkPlan {
            parts,
            audit: ChunkAudit::Truncated {
                kept: kept.len(),
                dropped_chunks: dropped,
            },
        }
    } else {
        ChunkPlan {
            parts,
            audit: ChunkAudit::Chunked { parts: kept.len() },
        }
    }
}

/// Split `content` into chunks each within `budget` unicode characters.
/// Prefers paragraph/newline/whitespace boundaries inside the budget
/// window; falls back to a hard cut only when no soft boundary exists in
/// the leading `budget` chars. Boundary characters stay with the head,
/// and chunk content is otherwise returned verbatim — intentional
/// leading whitespace (Markdown indentation, code blocks, list items) on
/// the tail is preserved so chunked output formats identically to the
/// single-message path.
fn split_for_chunking(content: &str, budget: usize) -> Vec<String> {
    let mut chunks: Vec<String> = Vec::new();
    let mut remaining = content;
    while !remaining.is_empty() {
        if remaining.chars().count() <= budget {
            chunks.push(remaining.to_owned());
            break;
        }
        let bytes_budget = remaining
            .char_indices()
            .nth(budget)
            .map(|(i, _)| i)
            .unwrap_or(remaining.len());
        let window = &remaining[..bytes_budget];
        let cut = window
            .rfind("\n\n")
            .map(|i| i + 2)
            .or_else(|| window.rfind('\n').map(|i| i + 1))
            .or_else(|| window.rfind(char::is_whitespace).map(|i| i + 1))
            .filter(|i| *i > 0)
            .unwrap_or(bytes_budget);
        let (head, tail) = remaining.split_at(cut);
        chunks.push(head.to_owned());
        remaining = tail;
    }
    chunks
}

// ---------------------------------------------------------------------------
// Reaction-driven HITL controls (sera-yeg.7, sera-q66q)
//
// A *narrow* slice of Discord-as-operator-console: an emoji reaction on a
// SERA-owned message (typically a status card) is parsed into a typed
// `ControlIntent` (Approve/Deny/Cancel/Retry/Pause/Resume). The intent is
// then authorized against the connector's configured operator allowlist and
// the inbound message's target principal. Unauthorized reactions surface as
// `ControlGateDecision::Denied(..)` so the caller can emit an
// `discord_control_denied` audit row instead of silently dropping the event.
//
// Everything in this module is pure data + functions — no I/O. The intent is
// that callers wire the live MESSAGE_REACTION_ADD dispatch into
// `gate_control_reaction` and then route accepted intents through the
// existing ApprovalMatrix / GoalRun control surface rather than mutating
// session state directly. The state machine + policy boundary is therefore
// the same as the bot-policy gate (`gate_message`) — pure decision, audit
// reason on the way out, no policy bypass.
// ---------------------------------------------------------------------------

/// Typed control intent derived from a Discord reaction. Names mirror the
/// HITL / GoalRun control surface (`approve`, `deny`, `cancel`, `retry`,
/// `pause`, `resume`) so the audit + downstream routing path can use a
/// single vocabulary regardless of which channel surface produced it.
///
/// `#[allow(dead_code)]`: only constructed from tests + the not-yet-wired
/// `gate_control_reaction` (see its rustdoc). Lifted once the bin wires
/// MESSAGE_REACTION_ADD into the gate.
#[allow(dead_code)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ControlIntent {
    /// Operator approves the pending approval / HITL ticket.
    Approve,
    /// Operator denies the pending approval / HITL ticket.
    Deny,
    /// Operator cancels the in-flight turn / GoalRun.
    Cancel,
    /// Operator asks SERA to retry the last failed step.
    Retry,
    /// Operator pauses the in-flight turn / GoalRun (non-terminal).
    Pause,
    /// Operator resumes a previously paused turn / GoalRun.
    Resume,
}

impl ControlIntent {
    /// Stable audit tag for this intent. Used by callers when writing a
    /// `discord_control_*` audit row so log analysis and tests don't have
    /// to depend on Debug formatting. See [`gate_control_reaction`] for
    /// the dead-code rationale that applies to the whole control slice.
    #[allow(dead_code)]
    pub fn audit_tag(self) -> &'static str {
        match self {
            ControlIntent::Approve => "approve",
            ControlIntent::Deny => "deny",
            ControlIntent::Cancel => "cancel",
            ControlIntent::Retry => "retry",
            ControlIntent::Pause => "pause",
            ControlIntent::Resume => "resume",
        }
    }
}

/// One configured reaction-to-intent binding. Operators express the mapping
/// in the connector manifest; this is the in-memory carrier the gate uses.
///
/// `emoji` is matched byte-equal against the inbound payload's emoji name
/// (Discord sends `name` = the unicode char for unicode emoji, or the
/// custom emoji name otherwise). The match is case-sensitive so a manifest
/// typo doesn't silently widen the set of accepted reactions.
#[allow(dead_code)]
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ReactionControlBinding {
    pub emoji: String,
    pub intent: ControlIntent,
}

/// A single inbound reaction event, parsed from MESSAGE_REACTION_ADD. The
/// `target_message_id` is the bot-owned message the reaction landed on —
/// the gate uses it to anchor the resulting control to the corresponding
/// HITL ticket / status card.
#[allow(dead_code)]
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DiscordReactionAdd {
    pub channel_id: String,
    pub target_message_id: String,
    pub user_id: String,
    pub emoji: String,
}

/// Parse a MESSAGE_REACTION_ADD Dispatch payload into a
/// [`DiscordReactionAdd`]. Returns `None` if the payload is the wrong
/// event type, lacks required fields, or carries a reaction from SERA's
/// own bot (loop-safety — bot self-reacts must never drive control).
///
/// Reactions on messages that are *not* bot-owned are out of scope for
/// the control gate (an operator @-tagging a peer's message has no
/// meaningful target), but this parser deliberately stays
/// principal-agnostic — the caller filters by `target_message_id` against
/// known bot-owned ids.
#[allow(dead_code)]
pub fn parse_reaction_add(payload: &Value, bot_user_id: Option<&str>) -> Option<DiscordReactionAdd> {
    if payload.get("op")?.as_u64()? != OP_DISPATCH {
        return None;
    }
    if payload.get("t")?.as_str()? != "MESSAGE_REACTION_ADD" {
        return None;
    }
    let d = payload.get("d")?;
    let user_id = d.get("user_id")?.as_str()?.to_owned();
    // Loop safety: self-reactions never produce a control intent. The bot
    // adds 👀 / ✅ / ❌ to its own messages as UX affordances — those must
    // not be interpreted as operator control input.
    if let Some(self_id) = bot_user_id
        && user_id == self_id
    {
        return None;
    }
    let channel_id = d.get("channel_id")?.as_str()?.to_owned();
    let target_message_id = d.get("message_id")?.as_str()?.to_owned();
    let emoji = d.get("emoji")?.get("name")?.as_str()?.to_owned();
    Some(DiscordReactionAdd {
        channel_id,
        target_message_id,
        user_id,
        emoji,
    })
}

/// Outcome of the reaction-control gate. Mirrors the shape of
/// [`MessageGateDecision`] so the audit pattern is uniform across the
/// inbound message gate and the reaction-control gate.
#[allow(dead_code)]
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ControlGateDecision {
    /// Reaction maps to a known intent AND the reacting principal is on
    /// the configured operator allowlist. The caller must route the intent
    /// through ApprovalMatrix / GoalRun controls — this gate deliberately
    /// does NOT mutate session state.
    Accepted(ControlIntent),
    /// Reaction maps to a known intent but the reacting principal is not
    /// authorized. Caller MUST emit a `discord_control_denied` audit row
    /// with reason `unauthorized_principal`.
    Denied(ControlDenyReason),
    /// Reaction does not map to any configured intent — silently dropped
    /// to keep low-noise channels usable for non-control reactions
    /// (operators reacting with hearts, etc.). No audit row.
    Unmapped,
}

/// Reason a reaction-driven control was denied. Distinct from the
/// `Unmapped` (silent) path so the audit pattern stays expressive.
#[allow(dead_code)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ControlDenyReason {
    /// Reacting Discord user id is not on the configured operator
    /// allowlist. Default-deny: empty allowlist denies everyone.
    UnauthorizedPrincipal,
    /// Reaction target message is not a bot-owned control surface. Guards
    /// against an operator reacting to a peer-bot message with the same
    /// emoji and inadvertently driving a SERA control.
    UntargetedSurface,
}

impl ControlDenyReason {
    /// Audit tag for `discord_control_denied` rows. Stable across versions.
    #[allow(dead_code)]
    pub fn audit_reason(self) -> &'static str {
        match self {
            ControlDenyReason::UnauthorizedPrincipal => "unauthorized_principal",
            ControlDenyReason::UntargetedSurface => "untargeted_surface",
        }
    }
}

/// Map a reaction emoji to a configured intent, if any. Pure lookup over
/// the manifest bindings — case-sensitive, byte-equal on `emoji`. Multiple
/// bindings with the same emoji resolve to the first match (manifest
/// ordering wins) so operators can override defaults by listing them
/// earlier.
#[allow(dead_code)]
pub fn map_reaction_to_intent(
    emoji: &str,
    bindings: &[ReactionControlBinding],
) -> Option<ControlIntent> {
    bindings
        .iter()
        .find(|b| b.emoji == emoji)
        .map(|b| b.intent)
}

/// Authorize a reacting principal against the configured operator
/// allowlist. Default-deny: an empty allowlist denies everyone, which is
/// the safe default for fresh deployments where the operator hasn't
/// configured a roster yet.
///
/// Matches by exact Discord user id. Username matching is deliberately not
/// supported here — usernames are mutable and an operator typo would
/// either lock everyone out or admit a stranger. Ids are the stable
/// principal identifier on Discord.
#[allow(dead_code)]
pub fn authorize_control_principal(user_id: &str, allowed_operators: &[String]) -> bool {
    if user_id.is_empty() {
        return false;
    }
    allowed_operators
        .iter()
        .any(|id| !id.is_empty() && id == user_id)
}

/// Top-level gate for a parsed reaction-control event. Composes
/// [`map_reaction_to_intent`] and [`authorize_control_principal`] under a
/// single decision so the call site has one pure entry point to test.
///
/// `bot_owned_message_ids` is the set of message ids this connector
/// considers control surfaces (typically: posted status cards, HITL
/// prompts). Empty set means "no surfaces tracked" — all reactions are
/// gated `UntargetedSurface` so the caller can audit instead of silently
/// dropping.
///
/// `#[allow(dead_code)]`: the bin doesn't wire MESSAGE_REACTION_ADD into
/// this gate yet — this lands the pure decision + tests first, matching
/// the status-card slice (sera-yeg.6) where helpers shipped a PR ahead
/// of the live pipeline integration.
#[allow(dead_code)]
pub fn gate_control_reaction(
    reaction: &DiscordReactionAdd,
    bindings: &[ReactionControlBinding],
    bot_owned_message_ids: &[String],
    allowed_operators: &[String],
) -> ControlGateDecision {
    let Some(intent) = map_reaction_to_intent(&reaction.emoji, bindings) else {
        return ControlGateDecision::Unmapped;
    };
    let is_bot_owned = bot_owned_message_ids
        .iter()
        .any(|id| !id.is_empty() && id == &reaction.target_message_id);
    if !is_bot_owned {
        return ControlGateDecision::Denied(ControlDenyReason::UntargetedSurface);
    }
    if !authorize_control_principal(&reaction.user_id, allowed_operators) {
        return ControlGateDecision::Denied(ControlDenyReason::UnauthorizedPrincipal);
    }
    ControlGateDecision::Accepted(intent)
}

// ---------------------------------------------------------------------------
// DiscordConnector implementation
// ---------------------------------------------------------------------------

impl DiscordConnector {
    pub fn new(
        token: &str,
        agent_name: &str,
        connector_name: &str,
        tx: mpsc::Sender<DiscordMessage>,
        shutting_down: Arc<AtomicBool>,
    ) -> Self {
        Self {
            token: token.to_owned(),
            agent_name: agent_name.to_owned(),
            connector_name: connector_name.to_owned(),
            tx,
            bot_user_id: std::sync::OnceLock::new(),
            shutting_down,
            session_id: std::sync::Mutex::new(None),
            resume_gateway_url: std::sync::Mutex::new(None),
            last_sequence: Arc::new(AtomicI64::new(-1)),
            observed_ready: Arc::new(AtomicBool::new(false)),
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

    /// Drop only the per-session resume host so the next reconnect targets the
    /// canonical gateway. Keeps `session_id` and `last_sequence` so RESUME on
    /// canonical gateway can still recover the session.
    fn clear_resume_gateway_url(&self) {
        if let Ok(mut g) = self.resume_gateway_url.lock() {
            *g = None;
        }
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
    /// * normal close / transport error → truncated-exponential 1-30s + ±25%
    ///   jitter (sera-dxmv), attempt RESUME; counter resets on a successful
    ///   READY observed during the previous connection
    /// * Op 7 RECONNECT or Op 9 (`d=true`) → 1s, attempt RESUME
    /// * Op 9 (`d=false`) → escalating 1-30s, drop session, fresh IDENTIFY
    ///
    /// Exits immediately when `shutting_down` is set to `true`.
    pub async fn run(&self) -> anyhow::Result<()> {
        let mut consecutive_invalid_sessions: u32 = 0;
        let mut consecutive_resume_no_signal: u32 = 0;
        let mut consecutive_resume_url_errors: u32 = 0;
        // Generic-close / transport-error attempt counter. Survives across
        // loop iterations so repeated flaps escalate via the exponential
        // schedule; reset once a connection has observed READY (sera-dxmv).
        let mut consecutive_connect_failures: u32 = 0;
        while !self.shutting_down.load(Ordering::Relaxed) {
            let attempted_resume_url = self.current_resume_url().is_some();
            // Reset the per-attempt READY observer before each connection
            // so the counter only resets when the *current* connect actually
            // made it past Hello+IDENTIFY/RESUME.
            self.observed_ready.store(false, Ordering::Relaxed);
            let outcome = match self.connect_and_run().await {
                Ok(reason) => {
                    consecutive_resume_url_errors = 0;
                    Some(reason)
                }
                Err(e) => {
                    tracing::error!("Discord gateway error: {e}");
                    if attempted_resume_url {
                        consecutive_resume_url_errors =
                            consecutive_resume_url_errors.saturating_add(1);
                        if resume_url_should_clear(consecutive_resume_url_errors) {
                            tracing::warn!(
                                consecutive = consecutive_resume_url_errors,
                                "Resume gateway host has failed repeatedly; falling back to canonical gateway URL while preserving session"
                            );
                            self.clear_resume_gateway_url();
                            consecutive_resume_url_errors = 0;
                        }
                    } else {
                        consecutive_resume_url_errors = 0;
                    }
                    None
                }
            };
            if self.observed_ready.load(Ordering::Relaxed) {
                consecutive_connect_failures = 0;
            }
            if self.shutting_down.load(Ordering::Relaxed) {
                break;
            }

            let backoff = match outcome {
                Some(DisconnectReason::SessionInvalidated) => {
                    self.clear_session_state();
                    consecutive_invalid_sessions = consecutive_invalid_sessions.saturating_add(1);
                    consecutive_resume_no_signal = 0;
                    let secs = invalid_session_backoff_secs(consecutive_invalid_sessions);
                    tracing::warn!(
                        consecutive = consecutive_invalid_sessions,
                        "Discord session invalidated; sleeping {secs}s before re-IDENTIFY"
                    );
                    Duration::from_secs(secs)
                }
                Some(DisconnectReason::ResumeFailedNoSignal) => {
                    // RESUME-attempt connection died without an opcode signal —
                    // ambiguous: could be Discord rejecting RESUME via close
                    // code (4007 invalid seq, 4009 session timeout) OR a flaky
                    // transport drop before any opcode arrived. Retry RESUME
                    // once before giving up so transient network blips don't
                    // discard a still-valid session, but bail to IDENTIFY on
                    // repeat failures so we don't hot-loop a RESUME the server
                    // keeps refusing.
                    consecutive_resume_no_signal =
                        consecutive_resume_no_signal.saturating_add(1);
                    if resume_no_signal_should_clear(consecutive_resume_no_signal) {
                        tracing::warn!(
                            consecutive = consecutive_resume_no_signal,
                            "RESUME ended without opcode signal repeatedly; clearing session and falling back to IDENTIFY"
                        );
                        self.clear_session_state();
                        consecutive_invalid_sessions =
                            consecutive_invalid_sessions.saturating_add(1);
                        consecutive_resume_no_signal = 0;
                        let secs = invalid_session_backoff_secs(consecutive_invalid_sessions);
                        Duration::from_secs(secs)
                    } else {
                        tracing::warn!(
                            "RESUME ended without opcode signal; retrying RESUME before falling back to IDENTIFY"
                        );
                        Duration::from_secs(5)
                    }
                }
                Some(DisconnectReason::OpcodeResumable) => {
                    consecutive_invalid_sessions = 0;
                    consecutive_resume_no_signal = 0;
                    Duration::from_secs(1)
                }
                Some(DisconnectReason::GenericClose) | None => {
                    consecutive_invalid_sessions = 0;
                    consecutive_resume_no_signal = 0;
                    consecutive_connect_failures =
                        consecutive_connect_failures.saturating_add(1);
                    let base = connect_backoff_secs(consecutive_connect_failures);
                    // ±25% jitter avoids synchronized reconnect storms across
                    // multiple bots when Discord rolls a gateway change
                    // (sera-dxmv acceptance #1).
                    let secs = apply_backoff_jitter(base, 25, &mut rand::thread_rng());
                    tracing::warn!(
                        attempt = consecutive_connect_failures,
                        base_secs = base,
                        jittered_secs = secs,
                        "Discord gateway closed without opcode signal; sleeping before reconnect",
                    );
                    Duration::from_secs(secs)
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

    /// Send an outbound reply, chunking it via [`chunk_for_discord`] if the
    /// content overflows Discord's per-message limit. Returns the chunker's
    /// audit classification so callers can record provenance / surface the
    /// truncated case in their own logs. Failures abort on the first
    /// failed POST so a partial overflow does not leave a half-rendered
    /// reply in-channel without an error in the caller's log.
    ///
    /// The truncation path is intentionally visible: the final chunk
    /// carries a `⚠ output truncated` notice and the connector logs at
    /// `warn` so the unsupported case is auditable rather than silent
    /// (sera-yeg.8).
    pub async fn send_message_chunked(
        &self,
        channel_id: &str,
        content: &str,
    ) -> anyhow::Result<ChunkAudit> {
        let plan = chunk_for_discord(content);
        match &plan.audit {
            ChunkAudit::SingleMessage => {}
            ChunkAudit::Chunked { parts } => {
                tracing::info!(
                    parts = *parts,
                    channel_id = %channel_id,
                    "Discord reply split across multiple messages",
                );
            }
            ChunkAudit::Truncated {
                kept,
                dropped_chunks,
            } => {
                tracing::warn!(
                    kept = *kept,
                    dropped = *dropped_chunks,
                    channel_id = %channel_id,
                    "Discord reply exceeded chunk cap; appending visible truncation notice",
                );
            }
        }
        for part in &plan.parts {
            self.send_message(channel_id, part).await?;
        }
        Ok(plan.audit)
    }

    /// Trigger the Discord typing indicator on a channel.
    ///
    /// The indicator self-expires after ~10 seconds, so callers must
    /// re-trigger periodically while processing. UX affordance only —
    /// failures (missing permission, transport error) are returned to the
    /// caller for low-noise logging and never propagated as turn failures
    /// (sera-yeg.2).
    pub async fn trigger_typing(&self, channel_id: &str) -> anyhow::Result<()> {
        let url = typing_indicator_url(channel_id);
        let client = reqwest::Client::new();
        let resp = client
            .post(&url)
            .header("Authorization", format!("Bot {}", self.token))
            .send()
            .await?;
        if !resp.status().is_success() {
            let status = resp.status();
            anyhow::bail!("Discord typing error {status}");
        }
        Ok(())
    }

    /// Add an emoji reaction (the bot's own) to a message.
    ///
    /// UX affordance only — failures are returned for low-noise logging by
    /// the caller and never propagated as turn failures (sera-yeg.2).
    pub async fn add_reaction(
        &self,
        channel_id: &str,
        message_id: &str,
        emoji: &str,
    ) -> anyhow::Result<()> {
        let url = own_reaction_url(channel_id, message_id, emoji);
        let client = reqwest::Client::new();
        let resp = client
            .put(&url)
            .header("Authorization", format!("Bot {}", self.token))
            .header("Content-Length", "0")
            .send()
            .await?;
        if !resp.status().is_success() {
            let status = resp.status();
            anyhow::bail!("Discord add_reaction error {status}");
        }
        Ok(())
    }

    /// Remove the bot's own reaction from a message.
    ///
    /// UX affordance only — failures are returned for low-noise logging by
    /// the caller and never propagated as turn failures (sera-yeg.2).
    pub async fn remove_own_reaction(
        &self,
        channel_id: &str,
        message_id: &str,
        emoji: &str,
    ) -> anyhow::Result<()> {
        let url = own_reaction_url(channel_id, message_id, emoji);
        let client = reqwest::Client::new();
        let resp = client
            .delete(&url)
            .header("Authorization", format!("Bot {}", self.token))
            .send()
            .await?;
        if !resp.status().is_success() {
            let status = resp.status();
            anyhow::bail!("Discord remove_reaction error {status}");
        }
        Ok(())
    }

    /// Start a public thread anchored on an existing message
    /// (`POST /channels/{channel_id}/messages/{message_id}/threads`).
    /// Returns the new thread's channel id on success. UX affordance only —
    /// failures degrade silently in the caller (sera-yeg.6).
    pub async fn start_thread_from_message(
        &self,
        channel_id: &str,
        message_id: &str,
        name: &str,
        auto_archive_duration_minutes: u32,
    ) -> anyhow::Result<String> {
        let url = start_thread_from_message_url(channel_id, message_id);
        let client = reqwest::Client::new();
        let resp = client
            .post(&url)
            .header("Authorization", format!("Bot {}", self.token))
            .json(&serde_json::json!({
                "name": name,
                "auto_archive_duration": auto_archive_duration_minutes,
            }))
            .send()
            .await?;
        if !resp.status().is_success() {
            let status = resp.status();
            anyhow::bail!("Discord start_thread error {status}");
        }
        let body: Value = resp.json().await?;
        let id = body
            .get("id")
            .and_then(|v| v.as_str())
            .ok_or_else(|| anyhow::anyhow!("Discord start_thread response missing id"))?
            .to_owned();
        Ok(id)
    }

    /// Send a message and return Discord's `id` for it. Used to anchor a
    /// status card so subsequent edits can target the same message
    /// (sera-yeg.6). UX affordance only.
    ///
    /// Mention parsing is disabled (`allowed_mentions.parse = []`) so a
    /// sanitized-but-still-mention-shaped reason in the rendered card
    /// (e.g. `@everyone` or `<@id>`) cannot ping anyone — the status
    /// surface is informational, not interactive (Codex P1 on PR #1278).
    pub async fn send_message_with_id(
        &self,
        channel_id: &str,
        content: &str,
    ) -> anyhow::Result<String> {
        let url = channel_messages_url(channel_id);
        let client = reqwest::Client::new();
        let resp = client
            .post(&url)
            .header("Authorization", format!("Bot {}", self.token))
            .json(&status_card_payload(content))
            .send()
            .await?;
        if !resp.status().is_success() {
            let status = resp.status();
            let body = resp.text().await.unwrap_or_default();
            anyhow::bail!("Discord send_message error {status}: {body}");
        }
        let body: Value = resp.json().await?;
        let id = body
            .get("id")
            .and_then(|v| v.as_str())
            .ok_or_else(|| anyhow::anyhow!("Discord send_message response missing id"))?
            .to_owned();
        Ok(id)
    }

    /// Edit an existing bot-owned message in place. Used to update the
    /// status card as the turn lifecycle progresses (sera-yeg.6).
    ///
    /// Mention parsing is disabled on the edit body for the same reason
    /// as [`Self::send_message_with_id`] — edits to a Failed/Blocked card
    /// must not generate notifications even if the reason text survives
    /// sanitization in a mention-shaped form.
    pub async fn edit_message(
        &self,
        channel_id: &str,
        message_id: &str,
        content: &str,
    ) -> anyhow::Result<()> {
        let url = edit_message_url(channel_id, message_id);
        let client = reqwest::Client::new();
        let resp = client
            .patch(&url)
            .header("Authorization", format!("Bot {}", self.token))
            .json(&status_card_payload(content))
            .send()
            .await?;
        if !resp.status().is_success() {
            let status = resp.status();
            anyhow::bail!("Discord edit_message error {status}");
        }
        Ok(())
    }

    /// The bot's own Discord user id, populated on the first READY dispatch.
    /// `None` until the gateway has connected. Reads from a `OnceLock`, so
    /// the lookup is lock-free and safe to call from async contexts
    /// (sera-bib9).
    pub fn bot_user_id(&self) -> Option<String> {
        self.bot_user_id.get().cloned()
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
        let (initial_payload, attempted_resume) = match session_for_resume.as_deref() {
            Some(sid) if seq_for_resume >= 0 => {
                tracing::info!(
                    session_id = sid,
                    seq = seq_for_resume,
                    "Sending RESUME to Discord Gateway"
                );
                (build_resume_payload(&self.token, sid, seq_for_resume), true)
            }
            _ => {
                tracing::info!("Sending IDENTIFY to Discord Gateway");
                (
                    build_identify_payload(&self.token, &self.agent_name),
                    false,
                )
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
        // we propagate that as the DisconnectReason. We also track whether a
        // RESUMED dispatch was observed so that no-signal closes after a
        // successful resume aren't misclassified as resume failures.
        let mut signal: Option<HandlerSignal> = None;
        let mut resume_succeeded = false;
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
                                    if attempted_resume
                                        && !resume_succeeded
                                        && parse_dispatch_event(&payload).as_deref()
                                            == Some("RESUMED")
                                    {
                                        resume_succeeded = true;
                                    }
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

        Ok(DisconnectReason::from_signal(
            signal,
            attempted_resume,
            resume_succeeded,
        ))
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
                            if let Some(ref uid) = user_id {
                                // OnceLock::set is idempotent — subsequent
                                // READYs (e.g. after IDENTIFY) carry the same
                                // immutable bot id, so a second set is a
                                // harmless no-op (sera-bib9).
                                let _ = self.bot_user_id.set(uid.clone());
                            }
                            // Signal to the reconnect loop that this
                            // connection actually reached a healthy state
                            // (sera-dxmv: reset connect-failure counter on
                            // successful READY).
                            self.observed_ready.store(true, Ordering::Relaxed);
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
                            // A successful RESUME is just as healthy as a
                            // fresh READY for backoff purposes — the
                            // resume_gateway_url accepted us and replayed
                            // missed events. Without this, reconnects that
                            // recover via RESUME would never clear the
                            // generic-close counter and later transport
                            // drops would keep escalating toward the 30s
                            // cap (sera-dxmv P2 repair, PR #1282 review).
                            self.observed_ready.store(true, Ordering::Relaxed);
                        }
                        "MESSAGE_CREATE" => {
                            let bot_id = self.bot_user_id.get().cloned();
                            if let Some(mut msg) = parse_message_create(payload, bot_id.as_deref())
                            {
                                // Stamp the message with this connector's
                                // manifest name so the routing layer applies
                                // the *connector-specific* peer-bot policy
                                // (sera-yeg.1 repair — Codex 3274130773).
                                msg.connector_name = Some(self.connector_name.clone());
                                if let Err(e) = self.tx.send(msg).await {
                                    tracing::error!(
                                        "Failed to dispatch Discord message: {e}"
                                    );
                                }
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
            is_bot: false,
            connector_name: None,
            raw_content: "hello world".into(),
            mentions: Vec::new(),
        };
        assert_eq!(msg.channel_id, "123456");
        assert_eq!(msg.user_id, "789");
        assert_eq!(msg.username, "testuser");
        assert_eq!(msg.content, "hello world");
        assert_eq!(msg.message_id, "msg001");
        assert!(!msg.is_bot);
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
    fn test_parse_message_create_self_bot_dropped() {
        // SERA's own bot id is always dropped at parse time (loop safety).
        let payload = serde_json::json!({
            "op": 0,
            "t": "MESSAGE_CREATE",
            "s": 43,
            "d": {
                "id": "msg124",
                "channel_id": "ch456",
                "content": "self echo",
                "author": {
                    "id": "self-bot",
                    "username": "sera",
                    "bot": true
                }
            }
        });
        assert!(parse_message_create(&payload, Some("self-bot")).is_none());
    }

    #[test]
    fn test_parse_message_create_peer_bot_surfaced() {
        // Peer-bot messages survive parse with `is_bot = true` so the
        // routing layer can apply the configured peer-bot policy
        // (sera-yeg.1). The previous behavior of dropping all bots at
        // parse time made the peer-bot policy impossible to implement.
        let payload = serde_json::json!({
            "op": 0,
            "t": "MESSAGE_CREATE",
            "s": 43,
            "d": {
                "id": "msg124",
                "channel_id": "ch456",
                "content": "I am a peer bot",
                "author": {
                    "id": "bot001",
                    "username": "hermes",
                    "bot": true
                }
            }
        });
        let msg = parse_message_create(&payload, Some("other-self-id"))
            .expect("peer-bot messages should surface for policy");
        assert!(msg.is_bot);
        assert_eq!(msg.user_id, "bot001");
        assert_eq!(msg.username, "hermes");
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
        let connector =
            DiscordConnector::new("token123", "my-agent", "discord-main", tx, shutting_down);
        assert_eq!(connector.token, "token123");
        assert_eq!(connector.agent_name, "my-agent");
        assert_eq!(connector.connector_name, "discord-main");
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
            "discord-test",
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

        // 3s budget rather than 500ms so the test absorbs CI variance on
        // the real WSS handshake to `gateway.discord.gg` that
        // `connect_and_run` performs before it can observe the flag — that
        // handshake can take 300-1000ms on slower runners. The 100ms
        // interruptible-sleep tick still bounds the post-flag exit, so a
        // genuine regression in the wake-up path will still fail.
        tokio::time::timeout(Duration::from_secs(3), handle)
            .await
            .expect("run() should exit within 3s after shutting_down is set")
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
        let connector = Arc::new(DiscordConnector::new(
            "token",
            "agent",
            "discord-test",
            tx,
            shutting_down,
        ));
        (connector, rx)
    }

    // DisconnectReason classifier — pure mapping so we don't need a real socket
    // to verify each (signal, attempted_resume) pair lands on the right branch.

    #[test]
    fn classify_session_invalidated_signal_overrides_attempted_resume() {
        // Op 9 (d=false) must invalidate regardless of attempted_resume /
        // resume_succeeded.
        for &attempted_resume in &[false, true] {
            for &resume_succeeded in &[false, true] {
                assert_eq!(
                    DisconnectReason::from_signal(
                        Some(HandlerSignal::SessionInvalidated),
                        attempted_resume,
                        resume_succeeded
                    ),
                    DisconnectReason::SessionInvalidated
                );
            }
        }
    }

    #[test]
    fn classify_opcode_reconnect_signal_overrides_attempted_resume() {
        // Op 7 RECONNECT or Op 9 (d=true) — keep session, short backoff.
        for &attempted_resume in &[false, true] {
            for &resume_succeeded in &[false, true] {
                assert_eq!(
                    DisconnectReason::from_signal(
                        Some(HandlerSignal::Reconnect),
                        attempted_resume,
                        resume_succeeded
                    ),
                    DisconnectReason::OpcodeResumable
                );
            }
        }
    }

    #[test]
    fn classify_no_signal_after_resume_is_resume_failed_no_signal() {
        // RESUME-attempt close without opcode AND without a prior RESUMED
        // confirmation is ambiguous: could be Discord rejecting via
        // close-code (4007/4009) or a flaky transport drop. The classifier
        // just labels it; `run()` decides retry-vs-clear via
        // `resume_no_signal_should_clear`.
        assert_eq!(
            DisconnectReason::from_signal(None, true, false),
            DisconnectReason::ResumeFailedNoSignal
        );
    }

    #[test]
    fn classify_no_signal_after_resumed_is_generic_close() {
        // The Codex P1 case: once we've seen a RESUMED dispatch, the resume
        // has already succeeded. A later transport-level close is a normal
        // disconnect, not a resume failure — must classify as GenericClose so
        // run() doesn't increment resume-no-signal counter and eventually
        // clear a still-valid session.
        assert_eq!(
            DisconnectReason::from_signal(None, true, true),
            DisconnectReason::GenericClose
        );
    }

    #[test]
    fn resume_url_failure_threshold_falls_back_after_repeats() {
        // First connect-stage error against `resume_gateway_url` is treated as
        // a transient blip; we keep the resume host. Repeated errors release
        // the stuck host so `run()` falls back to the canonical gateway URL
        // (still attempting RESUME with the cached session_id).
        assert!(!resume_url_should_clear(0));
        assert!(!resume_url_should_clear(1));
        assert!(resume_url_should_clear(2));
        assert!(resume_url_should_clear(3));
        assert!(resume_url_should_clear(u32::MAX));
    }

    #[tokio::test]
    async fn clear_resume_gateway_url_keeps_session_id_and_sequence() {
        // The fallback path must clear ONLY the resume host. session_id and
        // last_sequence persist so the next attempt RESUMEs the same session
        // on the canonical gateway URL.
        let (conn, _rx) = test_connector();
        let ready = serde_json::json!({
            "op": 0,
            "t": "READY",
            "s": 7,
            "d": {
                "session_id": "sess-fallback",
                "resume_gateway_url": "wss://gateway-bad.discord.gg",
                "user": { "id": "b", "username": "n" }
            }
        });
        conn.handle_payload(&ready).await;
        assert_eq!(conn.test_session_id().as_deref(), Some("sess-fallback"));
        assert_eq!(
            conn.test_resume_gateway_url().as_deref(),
            Some("wss://gateway-bad.discord.gg")
        );
        assert_eq!(conn.test_last_sequence(), 7);

        conn.clear_resume_gateway_url();

        assert_eq!(conn.test_session_id().as_deref(), Some("sess-fallback"));
        assert!(conn.test_resume_gateway_url().is_none());
        assert_eq!(conn.test_last_sequence(), 7);
    }

    #[test]
    fn resume_no_signal_threshold_retries_once_then_clears() {
        // First RESUME-without-signal disconnect must NOT clear session — a
        // single transient transport drop should retry RESUME so we don't
        // discard a still-valid session and lose replayable events.
        assert!(!resume_no_signal_should_clear(0));
        assert!(!resume_no_signal_should_clear(1));
        // Second consecutive occurrence flips to clear-and-IDENTIFY so we
        // can't hot-loop a RESUME the server keeps refusing.
        assert!(resume_no_signal_should_clear(2));
        assert!(resume_no_signal_should_clear(3));
        assert!(resume_no_signal_should_clear(u32::MAX));
    }

    #[test]
    fn classify_no_signal_after_identify_is_generic_close() {
        // The P2 review case: a transport-level close on an IDENTIFY-path
        // connection should not get the 1s "happy path" backoff.
        // resume_succeeded is irrelevant on the IDENTIFY path.
        for &resume_succeeded in &[false, true] {
            assert_eq!(
                DisconnectReason::from_signal(None, false, resume_succeeded),
                DisconnectReason::GenericClose
            );
        }
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

    // -----------------------------------------------------------------------
    // Peer-bot policy decisions (sera-yeg.1)
    // -----------------------------------------------------------------------

    #[test]
    fn bot_policy_human_always_accepted() {
        assert_eq!(
            decide_bot_policy(false, "user-1", "alice", Some("self"), false, &[]),
            BotPolicyDecision::Accept,
        );
        // Even with no peer_bots configured, a human user is fine.
        assert_eq!(
            decide_bot_policy(false, "user-1", "alice", Some("self"), true, &[]),
            BotPolicyDecision::Accept,
        );
    }

    #[test]
    fn bot_policy_self_bot_is_ignored() {
        // Defense in depth: even if parse_message_create lets the self id
        // through, the policy says ignore.
        let peers: Vec<String> = vec!["self-id".into()];
        assert_eq!(
            decide_bot_policy(true, "self-id", "sera", Some("self-id"), true, &peers),
            BotPolicyDecision::IgnoreSelf,
        );
    }

    #[test]
    fn bot_policy_unrelated_bot_is_ignored() {
        let peers: Vec<String> = vec!["allowed-peer".into()];
        assert_eq!(
            decide_bot_policy(true, "stranger-bot", "stranger", Some("self"), true, &peers),
            BotPolicyDecision::IgnoreUnrelated,
        );
        // Empty peer-bots list = strict default, all bots ignored.
        assert_eq!(
            decide_bot_policy(true, "any-bot", "any", Some("self"), true, &[]),
            BotPolicyDecision::IgnoreUnrelated,
        );
    }

    #[test]
    fn bot_policy_peer_bot_without_handoff_is_ignored() {
        let peers: Vec<String> = vec!["peer-1".into()];
        assert_eq!(
            decide_bot_policy(true, "peer-1", "hermes", Some("self"), false, &peers),
            BotPolicyDecision::IgnoreNoHandoff,
        );
    }

    #[test]
    fn bot_policy_peer_bot_with_explicit_handoff_accepted() {
        let peers: Vec<String> = vec!["peer-1".into()];
        assert_eq!(
            decide_bot_policy(true, "peer-1", "hermes", Some("self"), true, &peers),
            BotPolicyDecision::Accept,
        );
    }

    #[test]
    fn bot_policy_peer_bot_match_by_username() {
        // Operators may configure peer bots by friendly name (e.g.
        // `@Sera 2.0` on the Hermes side); the policy matches either id
        // or username so both work.
        let peers: Vec<String> = vec!["Sera 2.0".into()];
        assert_eq!(
            decide_bot_policy(true, "12345", "Sera 2.0", Some("self"), true, &peers),
            BotPolicyDecision::Accept,
        );
    }

    #[test]
    fn bot_policy_ignores_empty_string_peer_entries() {
        // Empty strings in the peers list must not match anyone — guards
        // against an operator typo collapsing the strict default.
        let peers: Vec<String> = vec!["".into(), "real-peer".into()];
        assert_eq!(
            decide_bot_policy(true, "", "", Some("self"), true, &peers),
            BotPolicyDecision::IgnoreUnrelated,
        );
        assert_eq!(
            decide_bot_policy(true, "real-peer", "x", Some("self"), true, &peers),
            BotPolicyDecision::Accept,
        );
    }

    #[test]
    fn bot_policy_audit_reasons_are_distinct() {
        assert_eq!(BotPolicyDecision::Accept.audit_reason(), "accept");
        assert_eq!(BotPolicyDecision::IgnoreSelf.audit_reason(), "ignored_self_bot");
        assert_eq!(
            BotPolicyDecision::IgnoreUnrelated.audit_reason(),
            "ignored_unrelated_bot",
        );
        assert_eq!(
            BotPolicyDecision::IgnoreNoHandoff.audit_reason(),
            "ignored_peer_bot_no_handoff",
        );
    }

    // -----------------------------------------------------------------------
    // Routing gate composition (sera-yeg.1 repair)
    //
    // These tests model the gate order in `process_message`: bots must run
    // through the peer-bot policy *before* the generic human/public-channel
    // filter, so unrelated bots and peer-without-handoff in public channels
    // still produce auditable decisions.
    // -----------------------------------------------------------------------

    #[test]
    fn gate_human_dm_accepted() {
        assert_eq!(
            gate_message(false, "u1", "alice", Some("self"), true, &[]),
            MessageGateDecision::Accept,
        );
    }

    #[test]
    fn gate_human_mention_accepted() {
        // is_explicit_handoff carries either is_dm OR mentions_bot.
        assert_eq!(
            gate_message(false, "u1", "alice", Some("self"), true, &[]),
            MessageGateDecision::Accept,
        );
    }

    #[test]
    fn gate_human_no_handoff_silently_ignored() {
        // Public channel + no mention → silent ignore (no audit row).
        // This preserves the original low-noise behavior for humans.
        assert_eq!(
            gate_message(false, "u1", "alice", Some("self"), false, &[]),
            MessageGateDecision::IgnoreHumanNoHandoff,
        );
    }

    #[test]
    fn gate_self_bot_ignored_even_in_public_channel() {
        // Loop safety: self bot id is hard-ignored regardless of channel
        // context. Parse-time filter already drops these, but the gate
        // is defense-in-depth and must still emit an auditable decision.
        let peers: Vec<String> = vec!["self-id".into()];
        assert_eq!(
            gate_message(true, "self-id", "sera", Some("self-id"), false, &peers),
            MessageGateDecision::IgnoreBot(BotPolicyDecision::IgnoreSelf),
        );
    }

    #[test]
    fn gate_unrelated_bot_in_public_channel_audited_not_silent() {
        // BUG REPAIR: before this fix, an unrelated bot posting in a public
        // channel (is_dm=false, mentions_bot=false) was silently dropped by
        // the `!is_explicit_handoff` early return. It must now surface as
        // IgnoreBot so the caller writes a `discord_message_ignored` audit
        // row with reason `ignored_unrelated_bot`.
        let peers: Vec<String> = vec!["allowed-peer".into()];
        assert_eq!(
            gate_message(true, "stranger", "stranger", Some("self"), false, &peers),
            MessageGateDecision::IgnoreBot(BotPolicyDecision::IgnoreUnrelated),
        );
    }

    #[test]
    fn gate_peer_bot_without_handoff_in_public_channel_audited_not_silent() {
        // BUG REPAIR: a configured peer bot that posts in a public channel
        // without DM/mention used to disappear via the human early return.
        // It must now surface as IgnoreBot with `IgnoreNoHandoff` so the
        // caller audits the failure mode under
        // `ignored_peer_bot_no_handoff`.
        let peers: Vec<String> = vec!["peer-1".into()];
        assert_eq!(
            gate_message(true, "peer-1", "hermes", Some("self"), false, &peers),
            MessageGateDecision::IgnoreBot(BotPolicyDecision::IgnoreNoHandoff),
        );
    }

    #[test]
    fn gate_peer_bot_with_explicit_handoff_accepted() {
        let peers: Vec<String> = vec!["peer-1".into()];
        assert_eq!(
            gate_message(true, "peer-1", "hermes", Some("self"), true, &peers),
            MessageGateDecision::Accept,
        );
    }

    #[test]
    fn gate_bot_runs_before_human_handoff_check() {
        // Sanity: even though a bot author lacks is_explicit_handoff (i.e.
        // would have been silently dropped by the old early return), the
        // gate routes via bot policy first. The decision must be policy-
        // driven (IgnoreUnrelated here), never the silent human path.
        let peers: Vec<String> = vec![];
        let d = gate_message(true, "x", "y", Some("self"), false, &peers);
        assert!(matches!(d, MessageGateDecision::IgnoreBot(_)));
        assert_ne!(d, MessageGateDecision::IgnoreHumanNoHandoff);
    }

    // -----------------------------------------------------------------------
    // REST URL construction (sera-yeg.2)
    // -----------------------------------------------------------------------

    #[test]
    fn typing_indicator_url_shape() {
        assert_eq!(
            typing_indicator_url("ch-42"),
            "https://discord.com/api/v10/channels/ch-42/typing",
        );
    }

    #[test]
    fn own_reaction_url_url_encodes_emoji() {
        // `👀` is U+1F440 → UTF-8 `F0 9F 91 80` → percent-encoded
        // `%F0%9F%91%80`. Without encoding, Discord rejects the route
        // with 400 INVALID_FORM_BODY.
        assert_eq!(
            own_reaction_url("ch-1", "msg-1", "👀"),
            "https://discord.com/api/v10/channels/ch-1/messages/msg-1/reactions/%F0%9F%91%80/@me",
        );
        assert_eq!(
            own_reaction_url("ch-1", "msg-1", "✅"),
            "https://discord.com/api/v10/channels/ch-1/messages/msg-1/reactions/%E2%9C%85/@me",
        );
        assert_eq!(
            own_reaction_url("ch-1", "msg-1", "❌"),
            "https://discord.com/api/v10/channels/ch-1/messages/msg-1/reactions/%E2%9D%8C/@me",
        );
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

    // -----------------------------------------------------------------------
    // Reverse peer-mention handoff (sera-yeg.4)
    //
    // Inbound: configured non-self peer mentions are preserved as
    // `@<handle>` text so the runtime sees the reference; SERA's own self
    // mention is stripped (used only for routing); arbitrary user mentions
    // are stripped so they can't leak into the runtime or be echoed.
    //
    // Outbound: when SERA's reply contains `@<peer-handle>` for a binding
    // captured on the inbound, it is rewritten to `<@id>` so Discord
    // renders an actual mention (the reverse handoff goal).
    // -----------------------------------------------------------------------

    #[test]
    fn parse_message_create_captures_raw_content_and_mentions() {
        let payload = serde_json::json!({
            "op": 0,
            "t": "MESSAGE_CREATE",
            "s": 60,
            "d": {
                "id": "msg-yeg4-1",
                "channel_id": "ch-yeg4",
                "content": "<@111> please ping <@222> for me",
                "mentions": [
                    {"id": "111", "username": "Sera"},
                    {"id": "222", "username": "Sera 2.0"}
                ],
                "author": {
                    "id": "user-yeg4",
                    "username": "operator",
                    "bot": false
                }
            }
        });
        let msg = parse_message_create(&payload, Some("111")).expect("parse");
        // raw_content keeps mention tags intact for downstream normalization.
        assert_eq!(msg.raw_content, "<@111> please ping <@222> for me");
        // Legacy `content` is still the strip_mentions output (back-compat).
        assert_eq!(msg.content, "please ping for me");
        assert_eq!(msg.mentions.len(), 2);
        assert_eq!(
            msg.mentions[0],
            DiscordMentionedUser {
                id: "111".into(),
                username: "Sera".into()
            }
        );
        assert_eq!(
            msg.mentions[1],
            DiscordMentionedUser {
                id: "222".into(),
                username: "Sera 2.0".into()
            }
        );
        // Self mention drives the mentions_bot routing flag.
        assert!(msg.mentions_bot);
    }

    #[test]
    fn peer_directory_filters_through_allowlist_and_drops_self() {
        let mentions = vec![
            DiscordMentionedUser {
                id: "111".into(),
                username: "Sera".into(),
            },
            DiscordMentionedUser {
                id: "222".into(),
                username: "Sera 2.0".into(),
            },
            DiscordMentionedUser {
                id: "333".into(),
                username: "random-user".into(),
            },
        ];
        // peer_bots configured by username; self id is "111".
        let peers: Vec<String> = vec!["Sera 2.0".into()];
        let dir = peer_directory_from_mentions(&mentions, Some("111"), &peers);
        // Self ("111") and the unallowlisted random user ("333") are dropped.
        assert_eq!(
            dir,
            vec![PeerMentionBinding {
                user_id: "222".into(),
                handle: "Sera 2.0".into(),
            }],
        );
    }

    #[test]
    fn peer_directory_matches_by_id_or_username() {
        // peer_bots can list either the Discord user id (stable across
        // username changes) OR the human-friendly username; both must
        // resolve into a binding.
        let mentions = vec![
            DiscordMentionedUser {
                id: "by-id".into(),
                username: "Friendly".into(),
            },
            DiscordMentionedUser {
                id: "by-name".into(),
                username: "Hermes 2.0".into(),
            },
        ];
        let peers: Vec<String> = vec!["by-id".into(), "Hermes 2.0".into()];
        let dir = peer_directory_from_mentions(&mentions, None, &peers);
        assert_eq!(dir.len(), 2);
        assert!(dir.iter().any(|b| b.user_id == "by-id"));
        assert!(dir.iter().any(|b| b.user_id == "by-name"));
    }

    #[test]
    fn peer_directory_empty_when_no_peer_bots_configured() {
        // Strict default: with peer_bots empty, no inbound mention is
        // surfaced — even bots are anonymized away.
        let mentions = vec![DiscordMentionedUser {
            id: "222".into(),
            username: "Sera 2.0".into(),
        }];
        let dir = peer_directory_from_mentions(&mentions, Some("self"), &[]);
        assert!(dir.is_empty());
    }

    #[test]
    fn normalize_content_strips_self_and_unknown_keeps_peer() {
        let dir = vec![PeerMentionBinding {
            user_id: "222".into(),
            handle: "Sera 2.0".into(),
        }];
        // Self <@111>: stripped. Peer <@222>: replaced with @Sera 2.0.
        // Unknown <@333>: stripped (arbitrary mention spam guard).
        let raw = "<@111> hi <@222> and <@333> please";
        let runtime = normalize_content_for_runtime(raw, Some("111"), &dir);
        assert_eq!(runtime, "hi @Sera 2.0 and please");
    }

    #[test]
    fn normalize_content_handles_nickname_form() {
        // Discord uses `<@!id>` when the mention targets a member with a
        // server nickname. Must be treated identically to `<@id>`.
        let dir = vec![PeerMentionBinding {
            user_id: "222".into(),
            handle: "Sera 2.0".into(),
        }];
        let runtime = normalize_content_for_runtime("<@!222> ping", None, &dir);
        assert_eq!(runtime, "@Sera 2.0 ping");
    }

    #[test]
    fn normalize_content_strips_all_when_no_directory() {
        // No peer directory and no self id: behave like strip_mentions —
        // every mention tag is removed so nothing leaks to runtime.
        let runtime = normalize_content_for_runtime("<@111> <@222> raw", None, &[]);
        assert_eq!(runtime, "raw");
    }

    #[test]
    fn render_outbound_substitutes_only_allowlisted_handles() {
        let dir = vec![PeerMentionBinding {
            user_id: "222".into(),
            handle: "Sera 2.0".into(),
        }];
        // The allowlisted handle is rewritten; @stranger is left as plain
        // text so it cannot become an outbound ping.
        let out = render_outbound_content("hello @Sera 2.0 and @stranger", &dir);
        assert_eq!(out, "hello <@222> and @stranger");
    }

    #[test]
    fn render_outbound_prefers_longer_handle_on_overlap() {
        // Two peers, one a prefix of the other. The longer handle must win
        // so `@Sera 2.0` doesn't collapse to `<@sera_id> 2.0`.
        let dir = vec![
            PeerMentionBinding {
                user_id: "AAA".into(),
                handle: "Sera".into(),
            },
            PeerMentionBinding {
                user_id: "BBB".into(),
                handle: "Sera 2.0".into(),
            },
        ];
        let out = render_outbound_content("@Sera 2.0 and @Sera", &dir);
        assert_eq!(out, "<@BBB> and <@AAA>");
    }

    #[test]
    fn render_outbound_noop_when_directory_empty() {
        // No allowlisted peers -> reply passes through untouched. This is
        // the loop-safety contract for the common case (human user, no
        // reverse handoff): nothing in the reply gets turned into a ping.
        assert_eq!(
            render_outbound_content("@Sera 2.0 hi", &[]),
            "@Sera 2.0 hi",
        );
    }

    #[test]
    fn render_outbound_does_not_introduce_self_ping() {
        // SERA's own bot id is not in any peer_directory (peer_directory
        // strips self at construction). If the LLM ever produces `@sera`
        // text, the renderer must leave it alone — preventing accidental
        // self-pings that could trip loop-safety telemetry.
        let dir = vec![PeerMentionBinding {
            user_id: "222".into(),
            handle: "Sera 2.0".into(),
        }];
        let out = render_outbound_content("@sera will think about it", &dir);
        assert_eq!(out, "@sera will think about it");
    }

    #[test]
    fn reverse_handoff_round_trip_inbound_to_outbound() {
        // End-to-end on the pure layer: inbound MESSAGE_CREATE mentioning
        // both @Sera (self) and @Sera 2.0 (allowlisted peer). After
        // normalization the runtime sees the peer reference as plain text.
        // The LLM echoes that reference into its reply. The outbound
        // renderer turns it back into a Discord `<@id>` tag — closing the
        // reverse-mention loop without disabling any loop guard.
        let payload = serde_json::json!({
            "op": 0,
            "t": "MESSAGE_CREATE",
            "s": 70,
            "d": {
                "id": "msg-yeg4-rt",
                "channel_id": "ch-yeg4-rt",
                "content": "<@111> tell <@222> that the reverse path works",
                "mentions": [
                    {"id": "111", "username": "Sera"},
                    {"id": "222", "username": "Sera 2.0"}
                ],
                "author": {"id": "user-rt", "username": "operator", "bot": false}
            }
        });
        let msg = parse_message_create(&payload, Some("111")).expect("parse");
        let peers: Vec<String> = vec!["Sera 2.0".into()];
        let dir = peer_directory_from_mentions(&msg.mentions, Some("111"), &peers);
        let runtime = normalize_content_for_runtime(&msg.raw_content, Some("111"), &dir);
        assert_eq!(runtime, "tell @Sera 2.0 that the reverse path works");

        // Simulated LLM reply that echoes the peer handle.
        let reply = "@Sera 2.0 acknowledged: reverse path works";
        let outbound = render_outbound_content(reply, &dir);
        assert_eq!(outbound, "<@222> acknowledged: reverse path works");
    }

    // -----------------------------------------------------------------------
    // Status-card session surface (sera-yeg.6)
    // -----------------------------------------------------------------------

    #[test]
    fn start_thread_from_message_url_shape() {
        assert_eq!(
            start_thread_from_message_url("ch-9", "msg-77"),
            "https://discord.com/api/v10/channels/ch-9/messages/msg-77/threads",
        );
    }

    #[test]
    fn channel_messages_url_shape() {
        assert_eq!(
            channel_messages_url("ch-9"),
            "https://discord.com/api/v10/channels/ch-9/messages",
        );
    }

    #[test]
    fn edit_message_url_shape() {
        assert_eq!(
            edit_message_url("ch-9", "msg-77"),
            "https://discord.com/api/v10/channels/ch-9/messages/msg-77",
        );
    }

    #[test]
    fn status_card_initial_state_is_accepted() {
        let card = StatusCard::new("abcdef1234567890");
        assert_eq!(*card.state(), StatusCardState::Accepted);
        // Trimmed to the 12-char display prefix.
        assert_eq!(card.audit_id(), "abcdef123456");
    }

    #[test]
    fn status_card_transition_running_then_done() {
        let mut card = StatusCard::new("sess-001");
        card.transition(StatusCardState::Running).unwrap();
        assert_eq!(*card.state(), StatusCardState::Running);
        card.transition(StatusCardState::Done).unwrap();
        assert!(card.state().is_terminal());
    }

    #[test]
    fn status_card_terminal_rejects_further_transitions() {
        let mut card = StatusCard::new("sess-002");
        card.transition(StatusCardState::Running).unwrap();
        card.transition(StatusCardState::Done).unwrap();
        // Done is terminal — must not allow further edits.
        let err = card
            .transition(StatusCardState::Failed("late failure".into()))
            .expect_err("terminal card must reject further transitions");
        assert_eq!(err, StatusCardState::Done);
        assert_eq!(*card.state(), StatusCardState::Done);
    }

    #[test]
    fn status_card_failed_is_terminal() {
        let mut card = StatusCard::new("sess-003");
        card.transition(StatusCardState::Failed("hook reject".into()))
            .unwrap();
        assert!(card.state().is_terminal());
        // Subsequent transitions are rejected.
        assert!(card.transition(StatusCardState::Done).is_err());
    }

    #[test]
    fn status_card_blocked_allows_continuation() {
        // Blocked is NOT terminal — operator can later resume / fail / finish.
        let mut card = StatusCard::new("sess-004");
        card.transition(StatusCardState::Blocked("waiting on hitl".into()))
            .unwrap();
        assert!(!card.state().is_terminal());
        card.transition(StatusCardState::Running).unwrap();
        card.transition(StatusCardState::Done).unwrap();
        assert!(card.state().is_terminal());
    }

    #[test]
    fn render_status_card_includes_label_and_audit_prefix() {
        let body = render_status_card("abcdef123456", &StatusCardState::Accepted);
        assert!(body.contains("SERA · Accepted"));
        assert!(body.contains("`abcdef123456`"));
    }

    #[test]
    fn render_status_card_running_has_no_reason_line() {
        let body = render_status_card("abc", &StatusCardState::Running);
        // Running carries no reason — body must be single-line.
        assert!(!body.contains('\n'), "rendered body had newline: {body}");
        assert!(body.contains("Running"));
    }

    #[test]
    fn render_status_card_blocked_includes_sanitized_reason() {
        let body =
            render_status_card("abc", &StatusCardState::Blocked("waiting on HITL".into()));
        assert!(body.contains("Blocked"));
        assert!(body.contains("waiting on HITL"));
    }

    #[test]
    fn render_status_card_failed_includes_sanitized_reason() {
        let body =
            render_status_card("abc", &StatusCardState::Failed("hook rejected".into()));
        assert!(body.contains("Failed"));
        assert!(body.contains("hook rejected"));
    }

    #[test]
    fn sanitize_status_reason_strips_backticks_and_control_chars() {
        // Backticks would break the audit-id code-span on the card. Newlines
        // would inflate the card vertically. Both must be stripped/folded.
        let s = sanitize_status_reason("oops `code` failed\n\nsecond line");
        assert!(!s.contains('`'));
        assert!(!s.contains('\n'));
        assert!(s.contains("oops"));
        assert!(s.contains("second line"));
    }

    #[test]
    fn sanitize_status_reason_clamps_long_input() {
        let long = "x".repeat(1000);
        let s = sanitize_status_reason(&long);
        // ≤ 200 chars total (197 + ellipsis).
        assert!(s.chars().count() <= 200, "len={}", s.chars().count());
        assert!(s.ends_with('…'));
    }

    #[test]
    fn sanitize_audit_id_for_display_handles_empty_and_long() {
        assert_eq!(sanitize_audit_id_for_display(""), "unknown");
        assert_eq!(sanitize_audit_id_for_display("   "), "unknown");
        // Backticks would break the code-span — must be stripped.
        assert_eq!(sanitize_audit_id_for_display("a`b`c"), "abc");
        let long = "0123456789abcdef0123456789";
        assert_eq!(sanitize_audit_id_for_display(long).chars().count(), 12);
    }

    #[test]
    fn status_card_payload_disables_all_mention_parsing() {
        // Codex P1 on PR #1278: a sanitized-but-mention-shaped reason
        // (e.g. `@everyone` slipping through whitespace-only sanitization)
        // must NOT generate Discord notifications when the card is
        // posted or edited. The wire body must therefore declare
        // `allowed_mentions.parse = []`.
        let body = status_card_payload("Failed: @everyone something broke");
        assert_eq!(body["content"], "Failed: @everyone something broke");
        assert_eq!(body["allowed_mentions"]["parse"], serde_json::json!([]));
    }

    #[test]
    fn render_status_card_does_not_leak_raw_channel_id_or_path() {
        // Privacy contract: the renderer must never emit channel/message
        // ids or filesystem paths. Verified by construction (the renderer
        // only sees the sanitized audit_id and reason) — this test pins
        // the contract so future edits don't accidentally include them.
        let card = StatusCard::new("audit-9F0AB");
        let body = card.render();
        assert!(!body.contains("channels/"));
        assert!(!body.contains("/home/"));
        assert!(!body.contains("Bot "));
    }

    // -----------------------------------------------------------------------
    // Reaction-driven HITL controls (sera-yeg.7 / sera-q66q)
    //
    // Pure mapping + authorization gate. No live Discord traffic, no policy
    // bypass: accepted intents are explicitly NOT mutated through here —
    // the call site is expected to route them through ApprovalMatrix /
    // GoalRun controls.
    // -----------------------------------------------------------------------

    fn reaction_payload(message_id: &str, user_id: &str, emoji: &str) -> Value {
        serde_json::json!({
            "op": 0,
            "t": "MESSAGE_REACTION_ADD",
            "s": 1,
            "d": {
                "user_id": user_id,
                "channel_id": "ch-control",
                "message_id": message_id,
                "emoji": { "name": emoji, "id": null }
            }
        })
    }

    fn default_bindings() -> Vec<ReactionControlBinding> {
        vec![
            ReactionControlBinding { emoji: "✅".into(), intent: ControlIntent::Approve },
            ReactionControlBinding { emoji: "❌".into(), intent: ControlIntent::Deny },
            ReactionControlBinding { emoji: "🛑".into(), intent: ControlIntent::Cancel },
            ReactionControlBinding { emoji: "🔁".into(), intent: ControlIntent::Retry },
            ReactionControlBinding { emoji: "⏸".into(), intent: ControlIntent::Pause },
            ReactionControlBinding { emoji: "▶".into(), intent: ControlIntent::Resume },
        ]
    }

    #[test]
    fn parse_reaction_add_extracts_fields() {
        let p = reaction_payload("msg-target", "op-1", "✅");
        let r = parse_reaction_add(&p, Some("self-bot")).expect("parse");
        assert_eq!(r.channel_id, "ch-control");
        assert_eq!(r.target_message_id, "msg-target");
        assert_eq!(r.user_id, "op-1");
        assert_eq!(r.emoji, "✅");
    }

    #[test]
    fn parse_reaction_add_self_bot_dropped() {
        // Loop safety: when SERA's own bot adds a 👀/✅/❌ reaction as a UX
        // affordance, the parser must drop the event so it cannot drive
        // its own control flow.
        let p = reaction_payload("msg-target", "self-bot", "✅");
        assert!(parse_reaction_add(&p, Some("self-bot")).is_none());
    }

    #[test]
    fn parse_reaction_add_wrong_event_type() {
        let p = serde_json::json!({
            "op": 0,
            "t": "MESSAGE_CREATE",
            "s": 1,
            "d": { "user_id": "x", "channel_id": "y", "message_id": "z",
                   "emoji": { "name": "✅" } }
        });
        assert!(parse_reaction_add(&p, None).is_none());
    }

    #[test]
    fn parse_reaction_add_missing_emoji_name() {
        // Custom-emoji-only payloads (no unicode `name`) are ignored: the
        // current binding format is unicode-only, so a custom emoji
        // reaction never maps anywhere.
        let p = serde_json::json!({
            "op": 0,
            "t": "MESSAGE_REACTION_ADD",
            "s": 1,
            "d": {
                "user_id": "op-1",
                "channel_id": "c",
                "message_id": "m",
                "emoji": { "id": "12345" }
            }
        });
        assert!(parse_reaction_add(&p, None).is_none());
    }

    #[test]
    fn map_reaction_to_intent_known_emoji() {
        let b = default_bindings();
        assert_eq!(map_reaction_to_intent("✅", &b), Some(ControlIntent::Approve));
        assert_eq!(map_reaction_to_intent("❌", &b), Some(ControlIntent::Deny));
        assert_eq!(map_reaction_to_intent("🛑", &b), Some(ControlIntent::Cancel));
        assert_eq!(map_reaction_to_intent("🔁", &b), Some(ControlIntent::Retry));
        assert_eq!(map_reaction_to_intent("⏸", &b), Some(ControlIntent::Pause));
        assert_eq!(map_reaction_to_intent("▶", &b), Some(ControlIntent::Resume));
    }

    #[test]
    fn map_reaction_to_intent_unknown_emoji_is_unmapped() {
        // Strict default: anything not explicitly bound is `None`, never
        // a "default to Approve" or similar. Operators reacting with
        // hearts, eyes, etc. must NOT produce a control intent.
        let b = default_bindings();
        assert_eq!(map_reaction_to_intent("❤", &b), None);
        assert_eq!(map_reaction_to_intent("👀", &b), None);
        assert_eq!(map_reaction_to_intent("", &b), None);
    }

    #[test]
    fn map_reaction_is_case_and_byte_sensitive() {
        // The mapping is exact-match on the emoji name; an extra
        // variation selector or modifier byte must NOT collapse onto the
        // configured entry. This guards against a misconfigured manifest
        // silently widening the accepted set.
        let b = vec![ReactionControlBinding { emoji: "OK".into(), intent: ControlIntent::Approve }];
        assert_eq!(map_reaction_to_intent("OK", &b), Some(ControlIntent::Approve));
        assert_eq!(map_reaction_to_intent("ok", &b), None);
        assert_eq!(map_reaction_to_intent("OK ", &b), None);
    }

    #[test]
    fn authorize_control_principal_default_deny() {
        // Empty allowlist denies everyone — the safe default for a fresh
        // deployment without an operator roster. Empty user id always
        // denies regardless of allowlist contents.
        assert!(!authorize_control_principal("op-1", &[]));
        assert!(!authorize_control_principal("", &[]));
        assert!(!authorize_control_principal("", &["op-1".into()]));
    }

    #[test]
    fn authorize_control_principal_ignores_empty_entries() {
        // Empty strings in the allowlist must NOT match an empty principal
        // id — guards against a manifest with stray blank lines silently
        // turning into a wildcard match.
        let allow: Vec<String> = vec!["".into(), "op-1".into()];
        assert!(!authorize_control_principal("", &allow));
        assert!(authorize_control_principal("op-1", &allow));
        assert!(!authorize_control_principal("stranger", &allow));
    }

    #[test]
    fn gate_unmapped_reaction_is_silent() {
        // A reaction the manifest doesn't bind must drop to `Unmapped`
        // so the caller can choose not to audit. Hearts and eyes are
        // operator chatter, not control input.
        let r = DiscordReactionAdd {
            channel_id: "c".into(),
            target_message_id: "card-1".into(),
            user_id: "op-1".into(),
            emoji: "❤".into(),
        };
        let surfaces = vec!["card-1".to_string()];
        let allow = vec!["op-1".to_string()];
        assert_eq!(
            gate_control_reaction(&r, &default_bindings(), &surfaces, &allow),
            ControlGateDecision::Unmapped,
        );
    }

    #[test]
    fn gate_unauthorized_principal_is_denied_with_audit_reason() {
        // Mapped reaction + unknown principal → Denied so the caller
        // emits a `discord_control_denied` audit row. The denial reason
        // must distinguish `unauthorized_principal` from
        // `untargeted_surface` for log analysis.
        let r = DiscordReactionAdd {
            channel_id: "c".into(),
            target_message_id: "card-1".into(),
            user_id: "stranger".into(),
            emoji: "✅".into(),
        };
        let surfaces = vec!["card-1".to_string()];
        let allow = vec!["op-1".to_string()];
        let decision = gate_control_reaction(&r, &default_bindings(), &surfaces, &allow);
        assert_eq!(
            decision,
            ControlGateDecision::Denied(ControlDenyReason::UnauthorizedPrincipal),
        );
        if let ControlGateDecision::Denied(reason) = decision {
            assert_eq!(reason.audit_reason(), "unauthorized_principal");
        }
    }

    #[test]
    fn gate_untargeted_surface_is_denied_with_audit_reason() {
        // Mapped reaction on a non-bot-owned message → Denied. This
        // guards against an operator reacting to a peer-bot or human
        // post with ✅ and unintentionally driving a SERA control.
        let r = DiscordReactionAdd {
            channel_id: "c".into(),
            target_message_id: "peer-post".into(),
            user_id: "op-1".into(),
            emoji: "✅".into(),
        };
        let surfaces = vec!["card-1".to_string()]; // peer-post not in here
        let allow = vec!["op-1".to_string()];
        let decision = gate_control_reaction(&r, &default_bindings(), &surfaces, &allow);
        assert_eq!(
            decision,
            ControlGateDecision::Denied(ControlDenyReason::UntargetedSurface),
        );
        if let ControlGateDecision::Denied(reason) = decision {
            assert_eq!(reason.audit_reason(), "untargeted_surface");
        }
    }

    #[test]
    fn gate_authorized_operator_on_bot_surface_accepts_intent() {
        // Happy path: configured emoji + bot-owned message + operator
        // on allowlist → Accepted with the mapped intent. The caller is
        // expected to route through ApprovalMatrix / GoalRun controls;
        // this gate does NOT mutate any session state.
        let r = DiscordReactionAdd {
            channel_id: "c".into(),
            target_message_id: "card-1".into(),
            user_id: "op-1".into(),
            emoji: "✅".into(),
        };
        let surfaces = vec!["card-1".to_string()];
        let allow = vec!["op-1".to_string()];
        let decision = gate_control_reaction(&r, &default_bindings(), &surfaces, &allow);
        assert_eq!(decision, ControlGateDecision::Accepted(ControlIntent::Approve));
        if let ControlGateDecision::Accepted(intent) = decision {
            assert_eq!(intent.audit_tag(), "approve");
        }
    }

    #[test]
    fn gate_empty_surface_list_denies_untargeted() {
        // Fresh-deployment safety: with no tracked bot surfaces, every
        // mapped reaction lands on `UntargetedSurface` so the operator
        // sees an audit row instead of a silent drop.
        let r = DiscordReactionAdd {
            channel_id: "c".into(),
            target_message_id: "anything".into(),
            user_id: "op-1".into(),
            emoji: "✅".into(),
        };
        let allow = vec!["op-1".to_string()];
        let decision = gate_control_reaction(&r, &default_bindings(), &[], &allow);
        assert_eq!(
            decision,
            ControlGateDecision::Denied(ControlDenyReason::UntargetedSurface),
        );
    }

    #[test]
    fn gate_targeted_surface_evaluated_before_principal() {
        // Surface check runs before principal check so the denial reason
        // for "stranger reacts to peer message" reports the more useful
        // `untargeted_surface` rather than masking it as a principal
        // problem. Documents the gate order so a future refactor can't
        // silently flip the priority.
        let r = DiscordReactionAdd {
            channel_id: "c".into(),
            target_message_id: "peer-post".into(),
            user_id: "stranger".into(),
            emoji: "✅".into(),
        };
        let surfaces = vec!["card-1".to_string()];
        let allow = vec!["op-1".to_string()];
        let decision = gate_control_reaction(&r, &default_bindings(), &surfaces, &allow);
        assert_eq!(
            decision,
            ControlGateDecision::Denied(ControlDenyReason::UntargetedSurface),
        );
    }

    #[test]
    fn control_intent_audit_tags_are_distinct_and_stable() {
        // Stable strings are part of the audit contract — log analysis
        // and downstream filters must be able to pin them.
        assert_eq!(ControlIntent::Approve.audit_tag(), "approve");
        assert_eq!(ControlIntent::Deny.audit_tag(), "deny");
        assert_eq!(ControlIntent::Cancel.audit_tag(), "cancel");
        assert_eq!(ControlIntent::Retry.audit_tag(), "retry");
        assert_eq!(ControlIntent::Pause.audit_tag(), "pause");
        assert_eq!(ControlIntent::Resume.audit_tag(), "resume");
    }

    #[test]
    fn control_deny_reason_audit_tags_are_distinct() {
        assert_eq!(
            ControlDenyReason::UnauthorizedPrincipal.audit_reason(),
            "unauthorized_principal",
        );
        assert_eq!(
            ControlDenyReason::UntargetedSurface.audit_reason(),
            "untargeted_surface",
        );
    }

    #[test]
    fn end_to_end_parse_then_gate_accepts_harmless_approve() {
        // Wire the parser + gate together on a realistic payload to
        // pin the contract a caller will exercise: operator reacts ✅
        // to the bot's status card, the gate emits Accept(Approve),
        // ready to be routed through ApprovalMatrix.
        let p = reaction_payload("card-9F0AB", "op-trusted", "✅");
        let parsed = parse_reaction_add(&p, Some("self-bot")).expect("parse");
        let bindings = default_bindings();
        let surfaces = vec!["card-9F0AB".to_string()];
        let allow = vec!["op-trusted".to_string()];
        assert_eq!(
            gate_control_reaction(&parsed, &bindings, &surfaces, &allow),
            ControlGateDecision::Accepted(ControlIntent::Approve),
        );
    }

    #[test]
    fn end_to_end_parse_then_gate_denies_unauthorized_with_audit() {
        // Same surface, same emoji, but a stranger's user id — the gate
        // must reject and surface `unauthorized_principal` so the caller
        // writes a `discord_control_denied` audit row. This is the
        // acceptance evidence for "unauthorized controls denied/audited"
        // from sera-yeg.7.
        let p = reaction_payload("card-9F0AB", "intruder", "✅");
        let parsed = parse_reaction_add(&p, Some("self-bot")).expect("parse");
        let bindings = default_bindings();
        let surfaces = vec!["card-9F0AB".to_string()];
        let allow = vec!["op-trusted".to_string()];
        let decision = gate_control_reaction(&parsed, &bindings, &surfaces, &allow);
        match decision {
            ControlGateDecision::Denied(ControlDenyReason::UnauthorizedPrincipal) => {}
            other => panic!("expected denied/unauthorized, got {other:?}"),
        }
    }

    // -----------------------------------------------------------------------
    // Outbound chunking (sera-yeg.8)
    // -----------------------------------------------------------------------

    #[test]
    fn chunk_short_reply_passes_through_as_single_message() {
        // Replies under the 2000-char limit must not be touched — no `[1/1]`
        // suffix, no extra messages. This is the common happy path.
        let plan = chunk_for_discord("hello operator");
        assert_eq!(plan.audit, ChunkAudit::SingleMessage);
        assert_eq!(plan.parts, vec!["hello operator".to_string()]);
    }

    #[test]
    fn chunk_empty_reply_emits_single_empty_part() {
        // The caller may still want to POST nothing rather than skip; we
        // preserve the empty single-message shape and tag it as such.
        let plan = chunk_for_discord("   \n  \t\n");
        assert_eq!(plan.audit, ChunkAudit::SingleMessage);
        assert_eq!(plan.parts.len(), 1);
        assert!(plan.parts[0].is_empty());
    }

    #[test]
    fn chunk_reply_exactly_at_limit_is_single_message() {
        // Boundary case: the entire reply fits in one Discord message and
        // must not be needlessly split.
        let body = "a".repeat(DISCORD_CONTENT_LIMIT);
        let plan = chunk_for_discord(&body);
        assert_eq!(plan.audit, ChunkAudit::SingleMessage);
        assert_eq!(plan.parts.len(), 1);
        assert_eq!(plan.parts[0].chars().count(), DISCORD_CONTENT_LIMIT);
    }

    #[test]
    fn chunk_long_reply_splits_at_paragraph_boundary() {
        // Two ~budget-sized paragraphs separated by a blank line: the
        // chunker must cut on the paragraph break, not inside a word.
        let p1 = "a".repeat(DISCORD_CHUNK_BUDGET - 50);
        let p2 = "b".repeat(DISCORD_CHUNK_BUDGET - 50);
        let body = format!("{p1}\n\n{p2}");
        let plan = chunk_for_discord(&body);
        assert_eq!(plan.audit, ChunkAudit::Chunked { parts: 2 });
        assert_eq!(plan.parts.len(), 2);
        // First chunk holds paragraph 1 (no remnants of paragraph 2).
        assert!(plan.parts[0].starts_with(&p1));
        assert!(!plan.parts[0].contains('b'));
        // Each part stays under the Discord hard limit, suffix included.
        for part in &plan.parts {
            assert!(
                part.chars().count() <= DISCORD_CONTENT_LIMIT,
                "chunk over Discord limit: {} chars",
                part.chars().count()
            );
            assert!(part.contains("[1/2]") || part.contains("[2/2]"));
        }
    }

    #[test]
    fn chunk_runaway_reply_truncates_visibly_and_audits() {
        // A reply large enough to need >MAX_CHUNKS pieces must surface a
        // visible ⚠ notice as its final part and audit as Truncated with
        // the count of dropped chunks. This is the acceptance evidence
        // for the "unsupported case is visible and audited" requirement
        // of sera-yeg.8.
        let huge =
            "x".repeat(DISCORD_CHUNK_BUDGET * (DISCORD_MAX_CHUNKS + 3));
        let plan = chunk_for_discord(&huge);
        match &plan.audit {
            ChunkAudit::Truncated {
                kept,
                dropped_chunks,
            } => {
                assert_eq!(*kept, DISCORD_MAX_CHUNKS);
                assert!(*dropped_chunks >= 3);
            }
            other => panic!("expected Truncated audit, got {other:?}"),
        }
        // One ⚠ notice tail message in addition to the kept chunks.
        assert_eq!(plan.parts.len(), DISCORD_MAX_CHUNKS + 1);
        let tail = plan.parts.last().expect("at least one part");
        assert!(
            tail.contains("⚠ output truncated"),
            "truncation notice missing: {tail}"
        );
        assert!(
            tail.contains(&format!("[{n}/{n}]", n = DISCORD_MAX_CHUNKS + 1)),
            "truncation notice missing tail index suffix: {tail}"
        );
        for (i, part) in plan.parts.iter().enumerate() {
            assert!(
                part.chars().count() <= DISCORD_CONTENT_LIMIT,
                "chunk {i} over Discord limit: {} chars",
                part.chars().count()
            );
        }
    }

    #[test]
    fn chunk_preserves_leading_indentation_after_paragraph_cut() {
        // Markdown / code-block indentation on a tail chunk must survive
        // the split — content after a paragraph boundary keeps its
        // leading whitespace so indented blocks render identically to
        // the single-message path. Pins the fix for Codex P2 on PR #1281
        // (previous splitter trim-stripped both ends of every chunk and
        // dropped intentional indentation on the next part).
        let p1 = "a".repeat(DISCORD_CHUNK_BUDGET - 20);
        let p2 = "    indented start of block\n    second indented line";
        let body = format!("{p1}\n\n{p2}");
        let plan = chunk_for_discord(&body);
        assert_eq!(plan.audit, ChunkAudit::Chunked { parts: 2 });
        assert_eq!(plan.parts.len(), 2);
        // Tail chunk keeps its 4-space indent.
        assert!(
            plan.parts[1].starts_with("    indented start of block"),
            "leading indentation stripped from tail chunk: {:?}",
            plan.parts[1]
        );
        assert!(
            plan.parts[1].contains("    second indented line"),
            "interior indentation stripped from tail chunk: {:?}",
            plan.parts[1]
        );
    }

    #[test]
    fn chunk_no_soft_boundary_falls_back_to_hard_cut() {
        // A single contiguous block with no whitespace must still be
        // delivered — the chunker hard-cuts at the byte budget rather
        // than emitting an oversize chunk.
        let body = "y".repeat(DISCORD_CHUNK_BUDGET + 100);
        let plan = chunk_for_discord(&body);
        match &plan.audit {
            ChunkAudit::Chunked { parts } => assert_eq!(*parts, 2),
            other => panic!("expected Chunked, got {other:?}"),
        }
        for part in &plan.parts {
            assert!(part.chars().count() <= DISCORD_CONTENT_LIMIT);
        }
    }

    // -----------------------------------------------------------------------
    // Reliability hardening — sera-yeg.9 slice
    //  * sera-bib9: OnceLock semantics for bot_user_id
    //  * sera-dxmv: deterministic exponential backoff + jitter
    //  * sera-h1iq: fuzzy/malformed payload coverage + content clamp
    // -----------------------------------------------------------------------

    /// sera-dxmv #1+#3: deterministic backoff schedule must match the
    /// truncated-exponential 1s base / 30s cap curve and the published unit
    /// test must demonstrate the sequence for 5 consecutive failures.
    #[test]
    fn connect_backoff_secs_schedule_for_5_failures() {
        // Discord guidance: truncated exponential, 1s base, 30s cap.
        // attempt 0 is treated as "before the first failure" → still 1s so
        // the caller doesn't have to special-case the off-by-one.
        assert_eq!(connect_backoff_secs(0), 1);
        assert_eq!(connect_backoff_secs(1), 1);
        assert_eq!(connect_backoff_secs(2), 2);
        assert_eq!(connect_backoff_secs(3), 4);
        assert_eq!(connect_backoff_secs(4), 8);
        assert_eq!(connect_backoff_secs(5), 16);
        // attempt 6 doubles to 32 which is clamped to the 30s cap.
        assert_eq!(connect_backoff_secs(6), 30);
        assert_eq!(connect_backoff_secs(7), 30);
        // Cap holds for arbitrarily large failure counts.
        assert_eq!(connect_backoff_secs(50), 30);
        assert_eq!(connect_backoff_secs(u32::MAX), 30);
    }

    /// sera-dxmv #3: with jitter disabled (deterministic mode) the schedule
    /// must equal the published exponential curve exactly.
    #[test]
    fn apply_backoff_jitter_disabled_matches_base() {
        let mut rng = rand::thread_rng();
        for attempt in 1..=6 {
            let base = connect_backoff_secs(attempt);
            let observed = apply_backoff_jitter(base, 0, &mut rng);
            assert_eq!(
                observed, base,
                "deterministic mode (jitter_pct=0) must yield base for attempt {attempt}",
            );
        }
    }

    /// sera-dxmv #1: ±25% jitter must keep the sleep within Discord's
    /// guidance — between 0.75x and 1.25x of the base, clamped to the
    /// `[1, 60]` second envelope.
    #[test]
    fn apply_backoff_jitter_stays_within_envelope() {
        let mut rng = rand::thread_rng();
        for attempt in 1..=6 {
            let base = connect_backoff_secs(attempt);
            // Run many iterations so we exercise the rng range, not just a
            // single draw.
            for _ in 0..200 {
                let observed = apply_backoff_jitter(base, 25, &mut rng);
                // Lower envelope: 75% of base, clamped at 1 second.
                let lower = ((base as f64) * 0.75).floor().max(1.0) as u64;
                // Upper envelope: 125% of base, clamped at 60 seconds. We
                // round-up to allow for f64.round() ties.
                let upper = (((base as f64) * 1.25).round() as u64).min(60);
                assert!(
                    observed >= lower && observed <= upper,
                    "attempt {attempt}: jittered {observed}s outside [{lower}, {upper}] (base {base}s)",
                );
            }
        }
    }

    /// sera-bib9: `bot_user_id` is single-write — the first READY wins; a
    /// later READY for a different user id is ignored (it can't happen with
    /// a real Discord gateway since the bot's own user id is immutable, but
    /// the invariant is now structural via `OnceLock`).
    #[tokio::test]
    async fn bot_user_id_is_single_write() {
        let (conn, _rx) = test_connector();
        let ready_a = serde_json::json!({
            "op": 0, "t": "READY", "s": 1,
            "d": {
                "session_id": "sess-a",
                "user": { "id": "bot-a", "username": "sera" }
            }
        });
        conn.handle_payload(&ready_a).await;
        assert_eq!(conn.bot_user_id().as_deref(), Some("bot-a"));

        // A second READY (e.g. after a re-IDENTIFY) is a no-op for the
        // OnceLock — the original id stands.
        let ready_b = serde_json::json!({
            "op": 0, "t": "READY", "s": 2,
            "d": {
                "session_id": "sess-b",
                "user": { "id": "bot-b", "username": "sera" }
            }
        });
        conn.handle_payload(&ready_b).await;
        assert_eq!(conn.bot_user_id().as_deref(), Some("bot-a"));
    }

    /// sera-dxmv: the connect-failure counter resets when a connection has
    /// observed READY. Pure observation on the `observed_ready` flag — we
    /// can't drive `run()` end-to-end without a real socket, but we can
    /// assert the flag semantics that `run()` keys off of.
    #[tokio::test]
    async fn observed_ready_flag_set_on_ready_dispatch() {
        let (conn, _rx) = test_connector();
        assert!(!conn.observed_ready.load(Ordering::Relaxed));

        let ready = serde_json::json!({
            "op": 0, "t": "READY", "s": 1,
            "d": {
                "session_id": "s",
                "user": { "id": "b", "username": "n" }
            }
        });
        conn.handle_payload(&ready).await;
        assert!(conn.observed_ready.load(Ordering::Relaxed));

        // GUILD_CREATE alone (no READY) does not set the flag — reset and
        // verify nothing else flips it.
        conn.observed_ready.store(false, Ordering::Relaxed);
        let guild = serde_json::json!({ "op": 0, "t": "GUILD_CREATE", "s": 2, "d": {} });
        conn.handle_payload(&guild).await;
        assert!(!conn.observed_ready.load(Ordering::Relaxed));
    }

    /// sera-dxmv repair (PR #1282 P2 review): a RESUMED dispatch is just as
    /// healthy as a fresh READY — the resume succeeded and the connection
    /// is good. Without this signal, reconnect cycles that recover via
    /// RESUME would never clear the generic-close counter, so a later
    /// transport drop would keep paying escalating backoff toward the 30s
    /// cap on an otherwise-recovered session.
    #[tokio::test]
    async fn observed_ready_flag_set_on_resumed_dispatch() {
        let (conn, _rx) = test_connector();
        assert!(!conn.observed_ready.load(Ordering::Relaxed));

        let resumed = serde_json::json!({
            "op": 0, "t": "RESUMED", "s": 5, "d": {}
        });
        conn.handle_payload(&resumed).await;
        assert!(
            conn.observed_ready.load(Ordering::Relaxed),
            "RESUMED dispatch must mark the connection healthy for backoff reset",
        );
    }

    // ---- sera-h1iq fuzz coverage ------------------------------------------

    /// MESSAGE_CREATE without a `content` field must silently return `None`
    /// — never panic. The runtime never sees the message; it cannot
    /// route something that has no body.
    #[test]
    fn parse_message_create_missing_content_returns_none() {
        let p = serde_json::json!({
            "op": 0, "t": "MESSAGE_CREATE", "s": 1,
            "d": {
                "id": "msg",
                "channel_id": "chan",
                "author": { "id": "u", "username": "n" }
            }
        });
        assert!(parse_message_create(&p, None).is_none());
    }

    /// MESSAGE_CREATE without an `author` field must silently return `None`.
    /// Without an author we cannot decide bot-policy, so the safest action
    /// is to drop the payload.
    #[test]
    fn parse_message_create_missing_author_returns_none() {
        let p = serde_json::json!({
            "op": 0, "t": "MESSAGE_CREATE", "s": 1,
            "d": {
                "id": "msg",
                "channel_id": "chan",
                "content": "hi"
            }
        });
        assert!(parse_message_create(&p, None).is_none());
    }

    /// MESSAGE_CREATE where `author.id` is a number instead of a string —
    /// Discord docs say IDs are stringified snowflakes, but a misbehaving
    /// region (or an MITM) could deliver the wrong type. Must drop, not
    /// panic.
    #[test]
    fn parse_message_create_wrong_author_id_type_returns_none() {
        let p = serde_json::json!({
            "op": 0, "t": "MESSAGE_CREATE", "s": 1,
            "d": {
                "id": "msg",
                "channel_id": "chan",
                "content": "hi",
                "author": { "id": 12345, "username": "n" }
            }
        });
        assert!(parse_message_create(&p, None).is_none());
    }

    /// `handle_payload` with `op` as a string instead of a u64 must not
    /// panic — the parser uses `unwrap_or(u64::MAX)` so the path lands in
    /// the unknown-opcode branch and is silently dropped.
    #[tokio::test]
    async fn handle_payload_op_wrong_type_returns_none() {
        let (conn, _rx) = test_connector();
        let p = serde_json::json!({ "op": "zero", "d": null });
        assert_eq!(conn.handle_payload(&p).await, None);
    }

    /// `handle_payload` with completely missing `op` must not panic — same
    /// guarantee: silently drop.
    #[tokio::test]
    async fn handle_payload_no_op_field_returns_none() {
        let (conn, _rx) = test_connector();
        let p = serde_json::json!({ "d": null });
        assert_eq!(conn.handle_payload(&p).await, None);
    }

    /// A handful of unhandled dispatch events (TYPING_START, PRESENCE_UPDATE,
    /// CHANNEL_PINS_UPDATE) must NOT panic and must NOT dispatch a
    /// DiscordMessage. They just update the sequence and exit silently.
    #[tokio::test]
    async fn handle_payload_unhandled_dispatch_events_are_silent() {
        let (conn, mut rx) = test_connector();
        for event in [
            "TYPING_START",
            "PRESENCE_UPDATE",
            "CHANNEL_PINS_UPDATE",
            "GUILD_MEMBER_UPDATE",
        ] {
            let p = serde_json::json!({ "op": 0, "t": event, "s": 1, "d": {} });
            assert_eq!(conn.handle_payload(&p).await, None);
        }
        // Nothing dispatched.
        assert!(rx.try_recv().is_err());
    }

    /// 1MB inbound MESSAGE_CREATE content: the parser must not panic, must
    /// truncate at the configured byte budget, and the resulting
    /// `raw_content` must be valid UTF-8.
    #[test]
    fn parse_message_create_oversized_content_is_clamped() {
        let huge = "x".repeat(1024 * 1024);
        let p = serde_json::json!({
            "op": 0, "t": "MESSAGE_CREATE", "s": 1,
            "d": {
                "id": "m",
                "channel_id": "c",
                "content": huge,
                "author": { "id": "u", "username": "n" }
            }
        });
        let msg = parse_message_create(&p, None).expect("parse must not drop");
        assert!(
            msg.raw_content.len() <= MAX_MESSAGE_CONTENT_BYTES,
            "raw_content kept {} bytes, budget {}",
            msg.raw_content.len(),
            MAX_MESSAGE_CONTENT_BYTES,
        );
        // UTF-8 validity follows from `String`, but make the assertion
        // explicit so a future refactor that returns `Vec<u8>` doesn't
        // silently regress.
        assert!(std::str::from_utf8(msg.raw_content.as_bytes()).is_ok());
    }

    /// `clamp_message_content` must never split a multi-byte UTF-8
    /// codepoint — a payload that overflows the budget exactly at a
    /// 4-byte emoji boundary must still produce a valid UTF-8 string.
    #[test]
    fn clamp_message_content_respects_char_boundary() {
        // 4-byte emoji: U+1F4A9 (💩). Build a string of one-byte 'a'
        // characters padded so the cut would land mid-emoji if the
        // function didn't honor `is_char_boundary`.
        let mut body = "a".repeat(MAX_MESSAGE_CONTENT_BYTES - 2);
        body.push('💩'); // 4 bytes — straddles the budget.
        assert!(body.len() > MAX_MESSAGE_CONTENT_BYTES);
        let (clamped, truncated) = clamp_message_content(&body);
        assert!(truncated);
        assert!(clamped.len() <= MAX_MESSAGE_CONTENT_BYTES);
        // String invariant proves there is no mid-codepoint split. Explicit
        // UTF-8 check just to make the intent obvious.
        assert!(std::str::from_utf8(clamped.as_bytes()).is_ok());
    }

    /// `clamp_message_content` must be a no-op for content that already
    /// fits — `truncated=false` and the string is returned untouched.
    #[test]
    fn clamp_message_content_passthrough_under_budget() {
        let body = "hello world";
        let (clamped, truncated) = clamp_message_content(body);
        assert!(!truncated);
        assert_eq!(clamped, body);
    }

    /// `parse_heartbeat_interval` must reject obviously bogus payloads
    /// without panicking: missing `d`, non-numeric heartbeat_interval,
    /// negative numbers (serde_json parses these as i64 → `as_u64()` is
    /// None).
    #[test]
    fn parse_heartbeat_interval_fuzz_rejects_bogus() {
        // Missing `d` entirely
        assert!(parse_heartbeat_interval(&serde_json::json!({ "op": 10 })).is_none());
        // Non-numeric heartbeat_interval
        assert!(
            parse_heartbeat_interval(&serde_json::json!({
                "op": 10, "d": { "heartbeat_interval": "fast" }
            }))
            .is_none(),
        );
        // Negative value
        assert!(
            parse_heartbeat_interval(&serde_json::json!({
                "op": 10, "d": { "heartbeat_interval": -1 }
            }))
            .is_none(),
        );
        // Wrong opcode (not Hello)
        assert!(
            parse_heartbeat_interval(&serde_json::json!({
                "op": 0, "d": { "heartbeat_interval": 41250 }
            }))
            .is_none(),
        );
    }

    /// `parse_ready_session` must reject malformed READY payloads without
    /// panicking — non-string session_id, missing `d`, etc.
    #[test]
    fn parse_ready_session_fuzz_rejects_bogus() {
        // session_id present but not a string
        assert!(
            parse_ready_session(&serde_json::json!({
                "op": 0, "t": "READY", "s": 1,
                "d": { "session_id": 999 }
            }))
            .is_none(),
        );
        // d completely missing
        assert!(
            parse_ready_session(&serde_json::json!({
                "op": 0, "t": "READY", "s": 1
            }))
            .is_none(),
        );
        // truncated payload (`{}`)
        assert!(parse_ready_session(&serde_json::json!({})).is_none());
    }

    /// Hand-rolled property-style smoke: a small grab-bag of malformed
    /// JSON values — none of them should panic the parser. Confirms the
    /// `Option`-returning contract holds across the documented threat
    /// model (sera-h1iq module-level note).
    #[test]
    fn parse_message_create_never_panics_on_random_shapes() {
        let bogus = vec![
            serde_json::json!(null),
            serde_json::json!({}),
            serde_json::json!({ "op": 0 }),
            serde_json::json!({ "op": 0, "t": "MESSAGE_CREATE" }),
            serde_json::json!({ "op": 0, "t": "MESSAGE_CREATE", "d": null }),
            serde_json::json!({ "op": 0, "t": "MESSAGE_CREATE", "d": [] }),
            serde_json::json!({ "op": 0, "t": "MESSAGE_CREATE", "d": "string" }),
            serde_json::json!({ "op": 0, "t": "MESSAGE_CREATE", "d": { "deeply": { "nested": { "missing": true } } } }),
            // Nested array under content
            serde_json::json!({
                "op": 0, "t": "MESSAGE_CREATE", "d": {
                    "id": "m", "channel_id": "c", "content": ["arr"],
                    "author": { "id": "u", "username": "n" }
                }
            }),
            // Empty content is technically valid (Discord sends embeds-only
            // messages with content=""). Parser must succeed but the result
            // has empty content/raw_content.
            serde_json::json!({
                "op": 0, "t": "MESSAGE_CREATE", "d": {
                    "id": "m", "channel_id": "c", "content": "",
                    "author": { "id": "u", "username": "n" }
                }
            }),
        ];
        for (i, p) in bogus.iter().enumerate() {
            // The function must terminate (no panic) on every shape — the
            // result Option<T> is allowed to be either Some or None
            // depending on whether enough fields parsed.
            let _ = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                parse_message_create(p, None)
            }))
            .unwrap_or_else(|_| panic!("parse_message_create panicked on bogus shape {i}: {p}"));
        }
    }
}

//! Ingress correlator — maps an [`InboundMessage`] back to the
//! [`OutboundEnvelope`] that opened the gate.
//!
//! Implements the B1 → B2 → B3 ladder:
//!
//! - **B1 — RFC 5322 headers**: the most trustworthy signal; inspect
//!   `In-Reply-To` and the `References` chain.
//! - **B2 — SERA nonce footer**: clients that strip threading headers still
//!   retain body content, including the `[SERA:nonce=...]` footer SERA
//!   appended to the outbound body.
//! - **B3 — Drop**: the reply matches nothing we have outstanding; log-warn
//!   once per (sender, subject) and return [`CorrelationOutcome::Dropped`].
//!
//! The correlator is independent of the scheduler: correlation results are
//! pushed into an [`crate::lookup::InMemoryMailLookup`] via its `notify`
//! method; the scheduler pulls at its own cadence via
//! [`sera_workflow::MailLookup::thread_event`].

use std::collections::HashMap;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use async_trait::async_trait;
use regex::Regex;
use serde::{Deserialize, Serialize};

use sera_workflow::task::{MailEvent, MailThreadId};

use crate::envelope::{GateId, IssuanceHook, OutboundEnvelope};
use crate::error::MailCorrelationError;
use crate::inbound::InboundMessage;

/// Tier at which a correlation resolved. Exposed so callers can log which
/// signal path succeeded and prioritise fixing client behaviour (e.g. if
/// nearly all resolutions are B2, some upstream client is stripping headers).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CorrelationTier {
    /// Matched via `In-Reply-To` or `References` headers (preferred).
    B1Headers,
    /// Matched via a `[SERA:nonce=...]` footer in the body.
    B2BodyNonce,
    /// Reserved for future use (per-recipient reply-to token mailbox).
    B2ReplyToToken,
}

/// Reason a correlation was dropped at tier B3.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DropReason {
    /// Nothing matched — no outstanding envelope keyed by header or nonce.
    NoMatch,
    /// Heuristic-based spoof indicator (reserved; the MVS correlator does not
    /// currently populate this — threat model says content-authenticity is
    /// handled by the gate's handler layer). Present in the taxonomy so
    /// future correlators can plug in DKIM / SPF / DMARC checks.
    Spoof,
    /// Parse produced an [`InboundMessage`] but key fields were unusable
    /// (e.g. invalid UTF-8 in body). Propagated mainly for observability.
    MalformedHeaders,
}

/// Outcome of a correlate call.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum CorrelationOutcome {
    /// The reply was matched to a pending gate.
    Resolved {
        gate_id: GateId,
        thread_id: MailThreadId,
        tier: CorrelationTier,
    },
    /// The reply could not be correlated.
    Dropped { reason: DropReason },
}

/// Storage for outstanding envelopes.
///
/// Abstracted as a trait so persistent backings (SQLite, Postgres, etc.) can
/// plug in later without touching the correlator itself. The default
/// [`InMemoryEnvelopeIndex`] is sufficient for single-process deployments.
pub trait EnvelopeIndex: Send + Sync {
    /// Record an outstanding envelope. Idempotent on `thread_id` — calling
    /// twice with the same thread-id overwrites the previous entry.
    fn register(&self, env: OutboundEnvelope) -> Result<(), MailCorrelationError>;

    /// Lookup by thread-id (RFC 5322 Message-ID). Returns `None` if absent or
    /// expired.
    fn by_thread_id(
        &self,
        id: &MailThreadId,
    ) -> Result<Option<OutboundEnvelope>, MailCorrelationError>;

    /// Lookup by SERA-issued nonce. Returns `None` if absent or expired.
    fn by_nonce(&self, nonce: &str) -> Result<Option<OutboundEnvelope>, MailCorrelationError>;

    /// Remove an envelope once the gate has resolved (optional — the default
    /// impl may choose to retain for a short grace period to tolerate
    /// duplicate replies).
    fn forget(&self, id: &MailThreadId) -> Result<(), MailCorrelationError>;
}

/// Default in-memory [`EnvelopeIndex`].
///
/// Keys envelopes by both `thread_id` and `nonce` so B1 / B2 lookups are
/// `O(1)`. Applies a TTL to silently expire outstanding gates: envelopes
/// older than `ttl` are treated as absent. TTL defaults to 7 days which is
/// sufficient for interactive gates; long-running batch workflows should use
/// a persistent index.
#[derive(Debug)]
pub struct InMemoryEnvelopeIndex {
    inner: Mutex<IndexInner>,
    ttl: Duration,
}

#[derive(Debug, Default)]
struct IndexInner {
    by_thread: HashMap<String, Entry>,
    by_nonce: HashMap<String, String>,
}

#[derive(Debug, Clone)]
struct Entry {
    env: OutboundEnvelope,
    inserted_at: Instant,
}

impl Default for InMemoryEnvelopeIndex {
    fn default() -> Self {
        Self::new(Duration::from_secs(60 * 60 * 24 * 7))
    }
}

impl InMemoryEnvelopeIndex {
    /// Construct with a custom TTL.
    pub fn new(ttl: Duration) -> Self {
        Self { inner: Mutex::new(IndexInner::default()), ttl }
    }

    /// Current number of outstanding envelopes (non-expired). Useful for
    /// metrics / assertions.
    pub fn len(&self) -> usize {
        let inner = self.inner.lock().expect("envelope index mutex poisoned");
        inner
            .by_thread
            .values()
            .filter(|e| e.inserted_at.elapsed() < self.ttl)
            .count()
    }

    /// Returns `true` iff there are no outstanding envelopes.
    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }
}

impl EnvelopeIndex for InMemoryEnvelopeIndex {
    fn register(&self, env: OutboundEnvelope) -> Result<(), MailCorrelationError> {
        let mut inner = self.inner.lock().map_err(|_| MailCorrelationError::IndexPoisoned)?;
        let tid = env.thread_id.as_str().to_string();
        // If this thread was already registered (re-issuance on retry, etc.)
        // drop the previous nonce binding so the old nonce no longer resolves.
        if let Some(prev) = inner.by_thread.get(&tid) {
            let prev_nonce = prev.env.nonce.clone();
            inner.by_nonce.remove(&prev_nonce);
        }
        let nonce = env.nonce.clone();
        inner.by_nonce.insert(nonce, tid.clone());
        inner
            .by_thread
            .insert(tid, Entry { env, inserted_at: Instant::now() });
        Ok(())
    }

    fn by_thread_id(
        &self,
        id: &MailThreadId,
    ) -> Result<Option<OutboundEnvelope>, MailCorrelationError> {
        let inner = self.inner.lock().map_err(|_| MailCorrelationError::IndexPoisoned)?;
        Ok(inner.by_thread.get(id.as_str()).and_then(|e| {
            if e.inserted_at.elapsed() < self.ttl {
                Some(e.env.clone())
            } else {
                None
            }
        }))
    }

    fn by_nonce(&self, nonce: &str) -> Result<Option<OutboundEnvelope>, MailCorrelationError> {
        let inner = self.inner.lock().map_err(|_| MailCorrelationError::IndexPoisoned)?;
        let tid = match inner.by_nonce.get(nonce) {
            Some(t) => t.clone(),
            None => return Ok(None),
        };
        Ok(inner.by_thread.get(&tid).and_then(|e| {
            if e.inserted_at.elapsed() < self.ttl {
                Some(e.env.clone())
            } else {
                None
            }
        }))
    }

    fn forget(&self, id: &MailThreadId) -> Result<(), MailCorrelationError> {
        let mut inner = self.inner.lock().map_err(|_| MailCorrelationError::IndexPoisoned)?;
        if let Some(entry) = inner.by_thread.remove(id.as_str()) {
            inner.by_nonce.remove(&entry.env.nonce);
        }
        Ok(())
    }
}

/// Correlator trait — one call resolves (or drops) one inbound message.
#[async_trait]
pub trait MailCorrelator: Send + Sync {
    /// Resolve `msg` against the current outstanding-envelope index.
    async fn correlate(
        &self,
        msg: &InboundMessage,
    ) -> Result<CorrelationOutcome, MailCorrelationError>;
}

/// Default correlator — header-first, nonce-fallback, drop otherwise.
///
/// Wraps an [`EnvelopeIndex`] plus an optional "notify" sink (an
/// `Arc<dyn NotifySink>`) that receives `(gate_id, thread_id, MailEvent)`
/// tuples when correlations resolve. The sink is typically
/// [`crate::lookup::InMemoryMailLookup`].
pub struct HeaderMailCorrelator {
    index: Arc<dyn EnvelopeIndex>,
    notify: Option<Arc<dyn NotifySink>>,
    nonce_re: Regex,
    /// Dropped-sender rate-limit state. Keyed by `(from, subject)`; value is
    /// the last log-warn instant. Warns are emitted at most once per hour per
    /// key to keep the log signal-to-noise ratio manageable.
    drop_rate_limit: Mutex<HashMap<(String, String), Instant>>,
    drop_log_cooldown: Duration,
}

/// Callback invoked when a correlation resolves.
#[async_trait]
pub trait NotifySink: Send + Sync {
    async fn on_resolved(
        &self,
        gate_id: &GateId,
        thread_id: &MailThreadId,
        event: MailEvent,
    ) -> Result<(), MailCorrelationError>;
}

impl HeaderMailCorrelator {
    /// Construct with an in-memory index and no notify sink. Suitable for
    /// tests and for the minimum-viable gateway wiring where the index and
    /// lookup are constructed together and wired externally.
    pub fn new_in_memory() -> Self {
        Self::new(Arc::new(InMemoryEnvelopeIndex::default()), None)
    }

    /// Construct with explicit index + optional notify sink.
    pub fn new(index: Arc<dyn EnvelopeIndex>, notify: Option<Arc<dyn NotifySink>>) -> Self {
        Self {
            index,
            notify,
            // Matches `[SERA:nonce=VALUE]` where VALUE is base64url-safe
            // (alphanumeric + `-_`). The length is bounded to avoid accidental
            // matches on user-pasted nonsense.
            nonce_re: Regex::new(r"\[SERA:nonce=([A-Za-z0-9_\-]{8,128})\]")
                .expect("nonce regex is valid"),
            drop_rate_limit: Mutex::new(HashMap::new()),
            drop_log_cooldown: Duration::from_secs(60 * 60),
        }
    }

    /// Access the underlying envelope index. Useful for tests and for
    /// external code that needs to register outbound envelopes via the same
    /// index the correlator reads.
    pub fn index(&self) -> &Arc<dyn EnvelopeIndex> {
        &self.index
    }

    /// Attach (or replace) the notify sink. Returns `self` for builder-style
    /// wiring.
    pub fn with_notify(mut self, notify: Arc<dyn NotifySink>) -> Self {
        self.notify = Some(notify);
        self
    }

    /// Override the drop-log cooldown (defaults to 1h). Primarily for tests.
    pub fn with_drop_log_cooldown(mut self, cooldown: Duration) -> Self {
        self.drop_log_cooldown = cooldown;
        self
    }

    fn should_log_drop(&self, from: &str, subject: &str) -> bool {
        let mut map = match self.drop_rate_limit.lock() {
            Ok(m) => m,
            Err(_) => return true, // degrade open on poison — we prefer log over silence
        };
        let key = (from.to_string(), subject.to_string());
        let now = Instant::now();
        match map.get(&key) {
            Some(last) if now.duration_since(*last) < self.drop_log_cooldown => false,
            _ => {
                map.insert(key, now);
                true
            }
        }
    }
}

#[async_trait]
impl IssuanceHook for HeaderMailCorrelator {
    async fn on_issued(&self, env: &OutboundEnvelope) -> Result<(), MailCorrelationError> {
        self.index.register(env.clone())
    }
}

#[async_trait]
impl MailCorrelator for HeaderMailCorrelator {
    async fn correlate(
        &self,
        msg: &InboundMessage,
    ) -> Result<CorrelationOutcome, MailCorrelationError> {
        // ── B1: RFC 5322 headers ─────────────────────────────────────────
        // Primary: In-Reply-To. Secondary: walk References (some clients
        // only populate the chain). First hit wins.
        let mut candidates: Vec<&str> = Vec::new();
        if let Some(irt) = msg.in_reply_to.as_deref() {
            candidates.push(irt);
        }
        for r in &msg.references {
            candidates.push(r.as_str());
        }

        for cand in candidates {
            let tid = MailThreadId::new(cand);
            if let Some(env) = self.index.by_thread_id(&tid)? {
                let outcome = CorrelationOutcome::Resolved {
                    gate_id: env.gate_id.clone(),
                    thread_id: env.thread_id.clone(),
                    tier: CorrelationTier::B1Headers,
                };
                self.emit_notify(&env).await?;
                tracing::debug!(
                    tier = "B1",
                    thread_id = %env.thread_id,
                    gate_id = %env.gate_id,
                    "mail gate correlated via headers"
                );
                return Ok(outcome);
            }
        }

        // ── B2: SERA nonce footer ────────────────────────────────────────
        // Pull every `[SERA:nonce=...]` match out of the body and try each
        // against the index. Usually there is exactly one (the footer); the
        // loop tolerates clients that duplicate the footer when quoting.
        for cap in self.nonce_re.captures_iter(&msg.body_text) {
            let nonce = &cap[1];
            if let Some(env) = self.index.by_nonce(nonce)? {
                let outcome = CorrelationOutcome::Resolved {
                    gate_id: env.gate_id.clone(),
                    thread_id: env.thread_id.clone(),
                    tier: CorrelationTier::B2BodyNonce,
                };
                self.emit_notify(&env).await?;
                tracing::debug!(
                    tier = "B2",
                    thread_id = %env.thread_id,
                    gate_id = %env.gate_id,
                    "mail gate correlated via body nonce"
                );
                return Ok(outcome);
            }
        }

        // ── B3: Drop ─────────────────────────────────────────────────────
        if self.should_log_drop(&msg.from, &msg.subject) {
            tracing::warn!(
                from = %msg.from,
                subject = %msg.subject,
                message_id = ?msg.message_id,
                "inbound mail did not correlate to any pending gate — dropping"
            );
        }
        Ok(CorrelationOutcome::Dropped { reason: DropReason::NoMatch })
    }
}

impl HeaderMailCorrelator {
    async fn emit_notify(&self, env: &OutboundEnvelope) -> Result<(), MailCorrelationError> {
        if let Some(sink) = &self.notify {
            sink.on_resolved(&env.gate_id, &env.thread_id, MailEvent::ReplyReceived)
                .await?;
        }
        Ok(())
    }
}

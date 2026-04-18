//! Outbound envelope type and nonce generation.
//!
//! An [`OutboundEnvelope`] is what SERA emits when a Mail gate is opened:
//! it captures the RFC 5322 `Message-ID` used as the thread-id plus a
//! SERA-issued `nonce` written into a body footer for clients that strip
//! headers on reply.

use async_trait::async_trait;
use base64::Engine as _;
use rand::RngCore;
use serde::{Deserialize, Serialize};

use sera_workflow::task::MailThreadId;

use crate::error::MailCorrelationError;

/// Workflow-scoped identifier for a Mail gate instance.
///
/// Opaque string — usually the `WorkflowTaskId` that opened the gate, but
/// implementations may also use synthetic test ids.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
pub struct GateId(pub String);

impl GateId {
    /// Construct a [`GateId`] from anything convertible to [`String`].
    pub fn new(id: impl Into<String>) -> Self {
        Self(id.into())
    }

    /// Borrow the underlying id.
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl std::fmt::Display for GateId {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.0)
    }
}

/// Marker prefix for the SERA nonce footer.
///
/// Clients that preserve body content but strip `In-Reply-To` / `References`
/// headers will still quote the original message, including this footer.
/// The correlator's B2 tier extracts the nonce via a regex anchored on this
/// prefix.
pub const SERA_FOOTER_PREFIX: &str = "[SERA:nonce=";

/// Outbound envelope — what SERA emits when a Mail gate is opened.
///
/// The `thread_id` is the RFC 5322 `Message-ID` SERA generates for the
/// outgoing message, also registered with the
/// [`crate::correlator::EnvelopeIndex`] so inbound replies can be routed
/// back to the originating gate.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct OutboundEnvelope {
    /// Workflow-scoped gate id this envelope originates from.
    pub gate_id: GateId,
    /// RFC 5322 `Message-ID` SERA generates (without angle brackets for
    /// internal use; added back when rendering headers). Also serves as the
    /// [`MailThreadId`] threaded through to the scheduler.
    pub thread_id: MailThreadId,
    /// Base64-encoded 16-byte random nonce. Written into the body footer so
    /// header-stripping clients can still be correlated.
    pub nonce: String,
    /// Recipient addresses.
    pub to: Vec<String>,
    /// Subject line.
    pub subject: String,
    /// Body text. SERA appends a `[SERA:nonce=...]` footer to this body before
    /// transport; this field holds the final body-as-sent.
    pub body: String,
}

impl OutboundEnvelope {
    /// Convenience constructor.
    ///
    /// Appends `[SERA:nonce=<nonce>]` to the body if not already present.
    pub fn new(
        gate_id: GateId,
        thread_id: MailThreadId,
        nonce: String,
        to: Vec<String>,
        subject: String,
        body: String,
    ) -> Self {
        let body_with_footer = if body.contains(SERA_FOOTER_PREFIX) {
            body
        } else {
            format!("{body}\n\n{SERA_FOOTER_PREFIX}{nonce}]")
        };
        Self { gate_id, thread_id, nonce, to, subject, body: body_with_footer }
    }
}

/// Generate a fresh 16-byte random nonce, base64-encoded (URL-safe, no pad).
///
/// 128 bits of entropy is sufficient for collision-resistance across any
/// realistic number of outstanding gates; the nonce space does not need to be
/// human-readable.
pub fn generate_nonce() -> String {
    let mut bytes = [0u8; 16];
    rand::thread_rng().fill_bytes(&mut bytes);
    base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(bytes)
}

/// Hook invoked when an outbound envelope is issued.
///
/// Implementations typically register the envelope with an
/// [`crate::correlator::EnvelopeIndex`] so later inbound replies can be
/// matched back. Keeping this as a trait (rather than a concrete type) means
/// transport crates can wire persistent backings without touching the
/// correlator.
#[async_trait]
pub trait IssuanceHook: Send + Sync {
    /// Record that `env` has been issued. Should be idempotent — callers may
    /// retry on transient transport errors.
    async fn on_issued(&self, env: &OutboundEnvelope) -> Result<(), MailCorrelationError>;
}

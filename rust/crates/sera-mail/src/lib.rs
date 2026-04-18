//! `sera-mail` — Mail gate ingress correlator for SERA workflows.
//!
//! # Overview
//!
//! The `sera-workflow` crate defines [`sera_workflow::MailLookup`] and
//! [`sera_workflow::MailEvent`] for scheduler-side gating of `AwaitType::Mail`
//! tasks, but does not describe *how* inbound replies get matched back to the
//! pending gate. `sera-mail` fills that gap:
//!
//! - [`OutboundEnvelope`] records what SERA emits when a Mail gate is opened
//!   (thread-id == RFC 5322 `Message-ID`, plus a SERA-issued `nonce`).
//! - [`InboundMessage`] + [`parse_raw_message`] parse a raw MIME blob into the
//!   fields relevant for correlation.
//! - [`MailCorrelator`] (trait) + [`HeaderMailCorrelator`] (impl) implement
//!   the **B1 → B2 → B3 ladder**:
//!   - **B1 — RFC 5322 headers**: match on `In-Reply-To` / `References` chain.
//!   - **B2 — SERA nonce footer**: clients that strip headers still retain a
//!     `[SERA:nonce=...]` tag in the body; match on that.
//!   - **B3 — Drop**: nothing matches; log-warn-once per (sender, subject).
//! - [`InMemoryMailLookup`] bridges correlator output back to the scheduler
//!   by implementing [`sera_workflow::MailLookup`].
//!
//! # Threat model
//!
//! Option A (sender+subject pattern matching) is **intentionally not
//! implemented** — it is spoofable. The correlator's job is only to route an
//! inbound reply to the right gate instance; authenticity / content validation
//! lives one layer up (in the handler that wakes up on the gate event).
//!
//! See `.omc/wiki/mail-gate-correlation.md` for the full design note.

pub mod correlator;
pub mod envelope;
pub mod error;
pub mod inbound;
pub mod lookup;

pub use correlator::{
    CorrelationOutcome, CorrelationTier, DropReason, EnvelopeIndex, HeaderMailCorrelator,
    InMemoryEnvelopeIndex, MailCorrelator,
};
pub use envelope::{generate_nonce, GateId, IssuanceHook, OutboundEnvelope, SERA_FOOTER_PREFIX};
pub use error::MailCorrelationError;
pub use inbound::{parse_raw_message, InboundMessage};
pub use lookup::InMemoryMailLookup;

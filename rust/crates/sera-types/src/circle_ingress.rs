//! Resident Circle ingress request/record types — Task C ingress seam.
//!
//! This module defines the typed *ingress* shape for accepting a
//! `to:circle:<name>` request as a resident runtime target. It deliberately
//! reuses [`crate::circle_channel::CircleChannelAddress`] for the address
//! and [`crate::circle_channel::CircleChannelEvent`] /
//! [`crate::circle_channel::CircleChannelRole`] for the channel/event
//! vocabulary. *No new parser is introduced here.* The runtime-side seam
//! that consumes this type lives in `sera-runtime::circle_ingress`.
//!
//! ## Scope
//!
//! This module is the contract surface that the resident Circle ingress
//! funnel speaks. It is intentionally small: a request wrapper (what a
//! caller hands in) and a receipt wrapper (what the runtime returns).
//! Anything that would couple to LLM calls, DB I/O, scheduling, or external
//! connectors is explicitly out of scope and lives downstream.
//!
//! ## Lineage
//!
//! Session lineage is conveyed through the `parent_session_key` field on
//! the request (carried verbatim into the returned `TurnContext`). This
//! matches the existing convention in `crate::runtime::TurnContext` used
//! by the gateway/runtime NDJSON bridge, so observers do not need a
//! parallel lineage channel.
//!
//! Spec note: see `docs/public/decisions/2026-06-30-circle-team-channel-topology.md`
//! §4 for the documented Task C ingress seam and its boundaries.

use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::circle_channel::{
    CircleAddressParseError, CircleChannelAddress, CircleChannelEvent, CircleChannelRole,
    CircleMemberId,
};
use crate::runtime::TurnContext;

/// Maximum number of channel events produced by a single ingress accept.
///
/// Used as a safety bound to keep the resident ingress path in-process: an
/// accept call returns at most this many [`CircleChannelEvent`] entries in
/// its [`CircleIngressRecord::channel_events`] list.
pub const CIRCLE_INGRESS_DEFAULT_EVENT_LIMIT: usize = 32;

/// Identity of the caller submitting the ingress request.
///
/// Kept tiny on purpose — the runtime already has its own caller identity
/// surfaces (e.g. `agent_id`, principal scopes), and Task C only needs an
/// opaque but stable per-call id plus the Circle `member_id` for channel
/// events.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CircleIngressCaller {
    /// Member id that will be claimed on the channel (e.g. `"alice"`).
    pub member_id: CircleMemberId,
    /// Optional agent id of the caller; when present it is recorded on
    /// every produced [`CircleChannelEvent`] as `event.member` would be.
    /// This is metadata only — actual member attribution goes through
    /// `member_id`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub agent_id: Option<String>,
    /// Role claim to attach to the channel event envelope.
    pub role: CircleChannelRole,
}

impl CircleIngressCaller {
    /// Default label used in receipts when no caller summary is provided.
    pub fn default_label(&self) -> &'static str {
        self.role.default_label()
    }
}

/// Resident ingress request — what a caller hands to the ingress funnel.
///
/// Construction should go through [`CircleIngressRequest::new`] so that
/// the address is parsed exactly once via [`CircleChannelAddress::parse`]
/// and the optional `parent_session_key` is preserved.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CircleIngressRequest {
    /// Canonical `to:circle:<name>` address (or parser-supported shorthand).
    pub to: String,
    /// Caller identity (member + role + optional agent id).
    pub caller: CircleIngressCaller,
    /// Short human-readable summary for the post event and the activity log.
    pub summary: String,
    /// Optional parent session key for lineage preservation. When `Some`,
    /// it is propagated verbatim onto the returned [`TurnContext`]; when
    /// `None` the resident ingress path treats the new run as a root.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub parent_session_key: Option<String>,
    /// Optional event id override. When unset, the ingress path assigns
    /// monotonic ids `1..=n` over the bounded produced event list.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub event_id_base: Option<u64>,
}

impl CircleIngressRequest {
    /// Construct a request with the required fields; helper for callers that
    /// do not need a parent_session_key or event id override.
    pub fn new(
        to: impl Into<String>,
        member_id: impl Into<String>,
        role: CircleChannelRole,
        summary: impl Into<String>,
    ) -> Self {
        Self {
            to: to.into(),
            caller: CircleIngressCaller {
                member_id: CircleMemberId::from(member_id.into()),
                agent_id: None,
                role,
            },
            summary: summary.into(),
            parent_session_key: None,
            event_id_base: None,
        }
    }

    /// Attach a parent_session_key (lineage).
    pub fn with_parent_session_key(mut self, parent: impl Into<String>) -> Self {
        self.parent_session_key = Some(parent.into());
        self
    }

    /// Attach an event id base.
    pub fn with_event_id_base(mut self, base: u64) -> Self {
        self.event_id_base = Some(base);
        self
    }

    /// Attach an agent id to the caller identity.
    pub fn with_agent_id(mut self, agent_id: impl Into<String>) -> Self {
        self.caller.agent_id = Some(agent_id.into());
        self
    }

    /// Parse the address using [`CircleChannelAddress::parse`].
    pub fn address(&self) -> Result<CircleChannelAddress, CircleAddressParseError> {
        CircleChannelAddress::parse(&self.to)
    }
}

/// Resident run record — what the runtime returns from the ingress funnel.
///
/// This is the *bounded* run/session state an ingress accept call produces.
/// It carries the canonical address, the derived [`TurnContext`] that the
/// runtime would route the turn through (with `session_key` set from the
/// Circle address and `parent_session_key` preserved), and the bounded
/// list of channel events written on accept.
///
/// `Debug` is derived; `PartialEq` is intentionally omitted because the
/// embedded [`TurnContext`] only derives `Debug`, `Clone`, `Serialize`,
/// and `Deserialize`. Equality assertions in tests compare the relevant
/// fields individually instead.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CircleIngressRecord {
    /// Unique identifier for this ingress accept call.
    pub request_id: Uuid,
    /// Canonical Circle address the request resolved to.
    pub address: CircleChannelAddress,
    /// Caller identity echoed back (for receipts and tracing).
    pub caller: CircleIngressCaller,
    /// Session/run state carrying Circle identity and lineage.
    pub turn_context: TurnContext,
    /// Channel events produced on accept. Bounded by
    /// [`CIRCLE_INGRESS_DEFAULT_EVENT_LIMIT`].
    pub channel_events: Vec<CircleChannelEvent>,
    /// Wall-clock timestamp at accept-time (assigned by the runtime).
    pub accepted_at: chrono::DateTime<chrono::Utc>,
}

impl CircleIngressRecord {
    /// Build the canonical session_key for a Circle ingress record.
    ///
    /// Format: `session:circle:<circle-name>:<member-id>`. This is a
    /// *naming convention only* — the runtime owns how session keys are
    /// routed. Lineage is preserved through `parent_session_key` on the
    /// [`TurnContext`], not through string-encoding the parent into this
    /// field.
    pub fn session_key_for(address: &CircleChannelAddress, caller: &CircleIngressCaller) -> String {
        format!("session:circle:{}:{}", address.name, caller.member_id)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::circle_channel::CircleChannelRole;

    #[test]
    fn request_new_defaults_to_no_lineage_and_no_agent_id() {
        let req = CircleIngressRequest::new(
            "to:circle:sera-nqh3",
            "alice",
            CircleChannelRole::Lead,
            "kick off",
        );
        assert_eq!(req.to, "to:circle:sera-nqh3");
        assert_eq!(req.caller.member_id.as_str(), "alice");
        assert_eq!(req.caller.role, CircleChannelRole::Lead);
        assert_eq!(req.summary, "kick off");
        assert!(req.parent_session_key.is_none());
        assert!(req.caller.agent_id.is_none());
        assert!(req.event_id_base.is_none());
    }

    #[test]
    fn request_builder_chains_attach_fields() {
        let req = CircleIngressRequest::new(
            "sera-nqh3",
            "bob",
            CircleChannelRole::Worker,
            "submit proposal",
        )
        .with_parent_session_key("session:parent-1")
        .with_event_id_base(7)
        .with_agent_id("agent-007");
        assert_eq!(req.parent_session_key.as_deref(), Some("session:parent-1"));
        assert_eq!(req.event_id_base, Some(7));
        assert_eq!(req.caller.agent_id.as_deref(), Some("agent-007"));
    }

    #[test]
    fn request_address_uses_canonical_parse() {
        let req = CircleIngressRequest::new(
            "circle:sera-nqh3",
            "alice",
            CircleChannelRole::Referee,
            "verdict",
        );
        let addr = req.address().unwrap();
        assert_eq!(addr.name, "sera-nqh3");
        assert_eq!(addr.to_string(), "to:circle:sera-nqh3");
    }

    #[test]
    fn request_address_rejects_garbage() {
        let req = CircleIngressRequest::new(
            "to:agent:bob",
            "alice",
            CircleChannelRole::Lead,
            "x",
        );
        assert!(req.address().is_err());
    }

    #[test]
    fn session_key_for_is_deterministic_for_same_inputs() {
        let addr = CircleChannelAddress::parse("to:circle:sera-nqh3").unwrap();
        let caller = CircleIngressCaller {
            member_id: CircleMemberId::from("alice"),
            agent_id: Some("agent-007".to_string()),
            role: CircleChannelRole::Lead,
        };
        let a = CircleIngressRecord::session_key_for(&addr, &caller);
        let b = CircleIngressRecord::session_key_for(&addr, &caller);
        assert_eq!(a, b);
        assert_eq!(a, "session:circle:sera-nqh3:alice");
    }

    #[test]
    fn caller_default_label_reflects_role() {
        let caller = CircleIngressCaller {
            member_id: CircleMemberId::from("r"),
            agent_id: None,
            role: CircleChannelRole::Referee,
        };
        assert_eq!(caller.default_label(), "Referee/Integrator");
    }

    #[test]
    fn receipt_round_trips_via_json() {
        let addr = CircleChannelAddress::parse("to:circle:sera-nqh3").unwrap();
        let caller = CircleIngressCaller {
            member_id: CircleMemberId::from("alice"),
            agent_id: Some("agent-007".to_string()),
            role: CircleChannelRole::Lead,
        };
        let tc = TurnContext {
            event_id: "evt-1".to_string(),
            agent_id: "agent-007".to_string(),
            session_key: "session:circle:sera-nqh3:alice".to_string(),
            messages: vec![],
            available_tools: vec![],
            metadata: std::collections::HashMap::new(),
            change_artifact: None,
            parent_session_key: Some("session:parent".to_string()),
            tool_use_behavior: crate::tool::ToolUseBehavior::default(),
        };
        let rec = CircleIngressRecord {
            request_id: Uuid::nil(),
            address: addr,
            caller,
            turn_context: tc,
            channel_events: vec![],
            accepted_at: "2026-07-02T09:00:00Z".parse().unwrap(),
        };
        let json = serde_json::to_string(&rec).unwrap();
        let back: CircleIngressRecord = serde_json::from_str(&json).unwrap();
        assert_eq!(back.address, rec.address);
        assert_eq!(back.turn_context.parent_session_key.as_deref(), Some("session:parent"));
        assert_eq!(back.caller.role, CircleChannelRole::Lead);
    }
}

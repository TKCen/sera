//! Circle channel-addressed types — `docs/public/decisions/2026-06-30-circle-team-channel-topology.md`.
//!
//! A Circle is a resident team-channel addressed as `to:circle:<name>` (e.g.
//! `to:circle:sera-nqh3`). This module holds the address parser/display types
//! and a thin event envelope used by the offline `sera circle run --to`
//! proof slice to assemble a `CollaborationProofBundle` from a deterministic
//! sequence of channel events.
//!
//! These types intentionally depend only on `serde` / `chrono` and avoid
//! coupling to runtime concerns (no LLM calls, no DB I/O) so the address
//! parser and event envelope are usable from `sera-cli`, `sera-types`
//! integration tests, and future transport layers.
//!
//! Bead: `sera-nqh3` Common-goal Circle demo — prove collaboration envelope
//! with receipts/lineage/referee (P1 OPEN).
//!
//! Spec note: members and referees are identifiers only here; the runtime
//! maps them to actors via the existing `CircleMember` registry surfaces.

use std::fmt;

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

/// Sentinel prefix that identifies a Circle channel address inside the
/// generic `to:` addressing envelope.
pub const CIRCLE_CHANNEL_KIND: &str = "circle";

/// Well-formed parse error for `CircleChannelAddress::parse`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CircleAddressParseError {
    /// Address was empty or missing the `kind:` prefix.
    MissingKind,
    /// Kind was not `"circle"`.
    UnsupportedKind { found: String },
    /// Circle name was empty or contained characters outside `[A-Za-z0-9._-]`.
    InvalidName { found: String, reason: &'static str },
}

impl fmt::Display for CircleAddressParseError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            CircleAddressParseError::MissingKind => {
                write!(
                    f,
                    "address must be of the form `to:circle:<name>` (missing kind)"
                )
            }
            CircleAddressParseError::UnsupportedKind { found } => {
                write!(f, "unsupported channel kind {found:?}: expected `circle`")
            }
            CircleAddressParseError::InvalidName { found, reason } => {
                write!(f, "invalid Circle name {found:?}: {reason}")
            }
        }
    }
}

impl std::error::Error for CircleAddressParseError {}

/// Canonical `to:circle:<name>` addressing handle.
///
/// Display renders the canonical `to:circle:<name>` form. `parse` accepts the
/// canonical form and the bare `<name>` form (auto-prefixed with
/// `to:circle:`). Any other `kind:` prefix is rejected.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(try_from = "String", into = "String")]
pub struct CircleChannelAddress {
    /// Channel kind — always [`CIRCLE_CHANNEL_KIND`] for this type.
    #[serde(default = "default_kind")]
    pub kind: String,
    /// Circle name — the addressable identifier of the resident team-channel.
    pub name: String,
}

#[allow(dead_code)]
fn default_kind() -> String {
    CIRCLE_CHANNEL_KIND.to_string()
}

impl CircleChannelAddress {
    /// Construct an address; returns `Err` if the name is empty or contains
    /// characters outside `[A-Za-z0-9._-]`.
    pub fn new(name: impl Into<String>) -> Result<Self, CircleAddressParseError> {
        let name = name.into();
        Self::validate_name(&name)?;
        Ok(Self {
            kind: CIRCLE_CHANNEL_KIND.to_string(),
            name,
        })
    }

    /// Parse a string address. Accepts:
    /// - `to:circle:<name>` (canonical)
    /// - `circle:<name>` (without the `to:` prefix)
    /// - `<name>` alone (auto-prefixed with `to:circle:`).
    pub fn parse(input: &str) -> Result<Self, CircleAddressParseError> {
        let trimmed = input.trim();
        if trimmed.is_empty() {
            return Err(CircleAddressParseError::MissingKind);
        }
        let had_to_prefix = trimmed.starts_with("to:");
        let without_to = trimmed.strip_prefix("to:").unwrap_or(trimmed);
        if had_to_prefix && !without_to.contains(':') {
            if without_to == CIRCLE_CHANNEL_KIND {
                return Err(CircleAddressParseError::InvalidName {
                    found: String::new(),
                    reason: "name must not be empty",
                });
            }
            return Err(CircleAddressParseError::UnsupportedKind {
                found: without_to.to_string(),
            });
        }
        // split on the first ':' to peel off the kind
        let (kind, rest) = match without_to.split_once(':') {
            Some(pair) => pair,
            None => {
                // bare name — accept and auto-prefix
                let name = without_to;
                Self::validate_name(name)?;
                return Ok(Self {
                    kind: CIRCLE_CHANNEL_KIND.to_string(),
                    name: name.to_string(),
                });
            }
        };
        if kind != CIRCLE_CHANNEL_KIND {
            return Err(CircleAddressParseError::UnsupportedKind {
                found: kind.to_string(),
            });
        }
        if rest.is_empty() {
            return Err(CircleAddressParseError::InvalidName {
                found: rest.to_string(),
                reason: "name must not be empty",
            });
        }
        Self::validate_name(rest)?;
        Ok(Self {
            kind: CIRCLE_CHANNEL_KIND.to_string(),
            name: rest.to_string(),
        })
    }

    fn validate_name(name: &str) -> Result<(), CircleAddressParseError> {
        if name.is_empty() {
            return Err(CircleAddressParseError::InvalidName {
                found: name.to_string(),
                reason: "name must not be empty",
            });
        }
        if name != name.trim() {
            return Err(CircleAddressParseError::InvalidName {
                found: name.to_string(),
                reason: "name must not have leading or trailing whitespace",
            });
        }
        if name.len() > 128 {
            return Err(CircleAddressParseError::InvalidName {
                found: name.to_string(),
                reason: "name must be at most 128 chars",
            });
        }
        for ch in name.chars() {
            let ok = ch.is_ascii_alphanumeric() || matches!(ch, '.' | '_' | '-');
            if !ok {
                return Err(CircleAddressParseError::InvalidName {
                    found: name.to_string(),
                    reason: "name must match [A-Za-z0-9._-]+",
                });
            }
        }
        Ok(())
    }
}

impl fmt::Display for CircleChannelAddress {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "to:{}:{}", self.kind, self.name)
    }
}

impl TryFrom<String> for CircleChannelAddress {
    type Error = CircleAddressParseError;
    fn try_from(value: String) -> Result<Self, Self::Error> {
        Self::parse(&value)
    }
}

impl From<CircleChannelAddress> for String {
    fn from(addr: CircleChannelAddress) -> String {
        addr.to_string()
    }
}

/// Stable identifier of a member inside a Circle channel.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct CircleMemberId(pub String);

impl CircleMemberId {
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl fmt::Display for CircleMemberId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

impl From<&str> for CircleMemberId {
    fn from(value: &str) -> Self {
        Self(value.to_string())
    }
}

impl From<String> for CircleMemberId {
    fn from(value: String) -> Self {
        Self(value)
    }
}

/// Role annotation inside a Circle channel.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum CircleChannelRole {
    /// Lead participant — drives the objective, posts proposals, owns the run id.
    Lead,
    /// Worker participant — produces proposal payloads.
    Worker,
    /// Critic participant — emits objections that surface as `LineageEdge`s.
    Critic,
    /// Referee participant — produces the final verdict.
    Referee,
}

impl CircleChannelRole {
    /// Default label used in `ProofBundleMember` when no explicit label is
    /// provided.
    pub fn default_label(self) -> &'static str {
        match self {
            CircleChannelRole::Lead => "Lead",
            CircleChannelRole::Worker => "Worker",
            CircleChannelRole::Critic => "Critic",
            CircleChannelRole::Referee => "Referee/Integrator",
        }
    }
}

/// Kind discriminator for [`CircleChannelEvent`].
///
/// Members of a Circle channel address one another with one of these event
/// kinds. The list is intentionally tiny for the first offline slice; new
/// kinds can be added without breaking round-trip serialization.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum CircleChannelEventKind {
    /// A member claims a role inside the Circle (lead, worker, critic, referee).
    ClaimRole {
        member: CircleMemberId,
        role: CircleChannelRole,
    },
    /// A member posts a payload contribution (proposal, objection, verdict body).
    Post {
        member: CircleMemberId,
        artifact_type: String,
        summary: String,
    },
    /// An objective-binding receipt proofing that the contribution was emitted
    /// by the channel and is reproducible.
    Receipt {
        member: CircleMemberId,
        action: String,
        tokens: Option<u64>,
    },
    /// A directed lineage edge in the proposal-DAG (criticises, resolves, ...).
    Lineage {
        member: CircleMemberId,
        from_entry_id: u64,
        to_entry_id: u64,
        relation: String,
    },
    /// A referee verdict, optionally accompanied by rationale.
    Verdict {
        member: CircleMemberId,
        verdict: String,
        rationale: String,
    },
}

/// Single channel-resident event.
///
/// Events are time-stamped and ordered. The offline `sera circle run --to`
/// pipeline produces a `Vec<CircleChannelEvent>` deterministically from CLI
/// args and uses it to assemble a `CollaborationProofBundle` with one entry
/// per `Post`/`Verdict` event.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct CircleChannelEvent {
    /// Monotonic 1-indexed event id (assigned by the producer).
    pub event_id: u64,
    /// Channel the event lives on.
    pub address: CircleChannelAddress,
    /// Wall-clock timestamp at event-emission time.
    pub timestamp: DateTime<Utc>,
    /// Event kind discriminator.
    pub body: CircleChannelEventKind,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_canonical_to_circle_form() {
        let addr = CircleChannelAddress::parse("to:circle:sera-nqh3").unwrap();
        assert_eq!(addr.name, "sera-nqh3");
        assert_eq!(addr.kind, "circle");
        assert_eq!(addr.to_string(), "to:circle:sera-nqh3");
    }

    #[test]
    fn parse_circle_kind_without_to_prefix() {
        let addr = CircleChannelAddress::parse("circle:engineering").unwrap();
        assert_eq!(addr.name, "engineering");
    }

    #[test]
    fn parse_bare_name_auto_prefixes_to_circle() {
        let addr = CircleChannelAddress::parse("ops").unwrap();
        assert_eq!(addr.kind, "circle");
        assert_eq!(addr.name, "ops");
        assert_eq!(addr.to_string(), "to:circle:ops");
    }

    #[test]
    fn parse_rejects_unsupported_kind() {
        let err = CircleChannelAddress::parse("to:agent:foo").unwrap_err();
        assert!(matches!(
            err,
            CircleAddressParseError::UnsupportedKind { .. }
        ));
    }

    #[test]
    fn parse_rejects_empty_name() {
        let err = CircleChannelAddress::parse("to:circle:").unwrap_err();
        assert!(matches!(err, CircleAddressParseError::InvalidName { .. }));
    }

    #[test]
    fn parse_rejects_path_separator_in_name() {
        let err = CircleChannelAddress::parse("to:circle:../escape").unwrap_err();
        assert!(matches!(err, CircleAddressParseError::InvalidName { .. }));
    }

    #[test]
    fn parse_rejects_whitespace_in_name() {
        let err = CircleChannelAddress::parse("to:circle:sera nqh3").unwrap_err();
        assert!(matches!(err, CircleAddressParseError::InvalidName { .. }));
    }

    #[test]
    fn parse_rejects_blank_input() {
        let err = CircleChannelAddress::parse("   ").unwrap_err();
        assert_eq!(err, CircleAddressParseError::MissingKind);
    }

    #[test]
    fn serde_round_trip_via_string_codec() {
        let addr = CircleChannelAddress::parse("to:circle:sera-nqh3").unwrap();
        let json = serde_json::to_string(&addr).unwrap();
        // The codec is `into = "String"` so JSON is the canonical form.
        assert_eq!(json, "\"to:circle:sera-nqh3\"");
        let back: CircleChannelAddress = serde_json::from_str(&json).unwrap();
        assert_eq!(back, addr);
    }

    #[test]
    fn try_from_string_rejects_garbage() {
        let err = <CircleChannelAddress as TryFrom<String>>::try_from("to:agent:bob".to_string())
            .unwrap_err();
        assert!(matches!(
            err,
            CircleAddressParseError::UnsupportedKind { .. }
        ));
    }

    #[test]
    fn channel_event_round_trips_through_json() {
        let addr = CircleChannelAddress::parse("to:circle:hello").unwrap();
        let event = CircleChannelEvent {
            event_id: 1,
            address: addr,
            timestamp: "2026-07-02T09:00:00Z".parse().unwrap(),
            body: CircleChannelEventKind::Post {
                member: CircleMemberId::from("alice"),
                artifact_type: "proposal".to_string(),
                summary: "hello".to_string(),
            },
        };
        let json = serde_json::to_string(&event).unwrap();
        let back: CircleChannelEvent = serde_json::from_str(&json).unwrap();
        assert_eq!(back, event);
    }

    #[test]
    fn channel_event_verdict_kind_serializes_with_snake_tag() {
        let addr = CircleChannelAddress::parse("to:circle:hello").unwrap();
        let event = CircleChannelEvent {
            event_id: 2,
            address: addr,
            timestamp: "2026-07-02T09:00:01Z".parse().unwrap(),
            body: CircleChannelEventKind::Verdict {
                member: CircleMemberId::from("ref"),
                verdict: "Approved".to_string(),
                rationale: "looks good".to_string(),
            },
        };
        let value: serde_json::Value = serde_json::to_value(&event).unwrap();
        assert_eq!(value["body"]["kind"], "verdict");
        assert_eq!(value["body"]["verdict"], "Approved");
    }

    #[test]
    fn role_default_labels_are_distinct() {
        assert_eq!(CircleChannelRole::Lead.default_label(), "Lead");
        assert_eq!(CircleChannelRole::Worker.default_label(), "Worker");
        assert_eq!(CircleChannelRole::Critic.default_label(), "Critic");
        assert_eq!(
            CircleChannelRole::Referee.default_label(),
            "Referee/Integrator"
        );
    }
}

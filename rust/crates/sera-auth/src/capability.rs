//! Canonical CapabilityToken — the single definition for the workspace.
//!
//! This module replaces the previous split between
//! `sera_auth::capability::CapabilityToken` (narrowing-semantic, never used
//! outside its own tests) and `sera_types::evolution::CapabilityToken` (the
//! signed wire-token used by the evolve pipeline and gateway). They are now
//! one struct: the wire-serialised form stays byte-identical (the shape the
//! gateway signs today), with the narrowing / `has` / `consume_proposal`
//! helpers promoted from the old sera-auth type.
//!
//! A CapabilityToken is both:
//! - a **signed** token carried on the wire — `signature: [u8; 64]` is a
//!   gateway-side HMAC-SHA-512 produced by
//!   [`sera_gateway::evolve_token::EvolveTokenSigner`]; and
//! - a **narrowable** token at the auth layer — `narrow` produces a subset
//!   scope view, and `has` / `consume_proposal` enforce the budgets at the
//!   policy gate.
//!
//! # Stability
//!
//! The field layout matches what live tokens in the wild expect: `id`,
//! `scopes` (sorted for canonical-bytes stability at signing time),
//! `expires_at`, `max_proposals`, `signature`. Adding new fields would break
//! the MAC canonicalisation in `EvolveTokenSigner::canonical_bytes` — do not
//! extend this struct without a coordinated signer update.

use std::collections::HashSet;
use std::fmt;

use serde::{Deserialize, Deserializer, Serialize, Serializer};

use sera_types::evolution::BlastRadius;

mod bytes64 {
    use super::*;

    pub fn serialize<S: Serializer>(bytes: &[u8; 64], s: S) -> Result<S::Ok, S::Error> {
        s.serialize_bytes(bytes)
    }

    pub fn deserialize<'de, D: Deserializer<'de>>(d: D) -> Result<[u8; 64], D::Error> {
        struct Visitor;
        impl<'de> serde::de::Visitor<'de> for Visitor {
            type Value = [u8; 64];
            fn expecting(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
                f.write_str("64 bytes")
            }
            fn visit_bytes<E: serde::de::Error>(self, v: &[u8]) -> Result<[u8; 64], E> {
                v.try_into().map_err(|_| E::invalid_length(v.len(), &self))
            }
            fn visit_seq<A: serde::de::SeqAccess<'de>>(
                self,
                mut seq: A,
            ) -> Result<[u8; 64], A::Error> {
                let mut arr = [0u8; 64];
                for (i, slot) in arr.iter_mut().enumerate() {
                    *slot = seq
                        .next_element()?
                        .ok_or_else(|| serde::de::Error::invalid_length(i, &self))?;
                }
                Ok(arr)
            }
        }
        d.deserialize_bytes(Visitor)
    }
}

/// A bounded, narrowable, signed capability token.
///
/// Tokens are issued by [`CapabilityTokenIssuer`] and signed by the
/// gateway's [`sera_gateway::evolve_token::EvolveTokenSigner`] (HMAC-SHA-512
/// over the canonical serialisation). The issuer and signer are intentionally
/// orthogonal: the issuer constructs the token *value*; the signer installs
/// the MAC on the serialised form.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CapabilityToken {
    /// Token identifier — doubles as the issuer-identity anchor (see the
    /// gateway's `propose` identity cross-check).
    pub id: String,
    /// Blast-radius scopes granted to this token. The set is narrowable
    /// via [`CapabilityToken::narrow`]; widening attempts are rejected.
    pub scopes: HashSet<BlastRadius>,
    /// When this token expires (Unix-wall-clock, UTC).
    pub expires_at: chrono::DateTime<chrono::Utc>,
    /// Maximum number of proposals this token may authorise. Zero means the
    /// token cannot propose — not unlimited — matching the existing gateway
    /// `ProposalUsageTracker` contract.
    pub max_proposals: u32,
    /// HMAC-SHA-512 over the canonical serialisation. All-zero for unsigned
    /// tokens (issuance before signing); verification rejects the all-zero
    /// signature.
    #[serde(with = "bytes64")]
    pub signature: [u8; 64],
}

/// Errors that can occur when using or narrowing a CapabilityToken.
#[derive(Debug, thiserror::Error, PartialEq, Eq)]
pub enum CapabilityTokenError {
    #[error("scope missing: {0:?}")]
    ScopeMissing(BlastRadius),
    #[error("widening attempt denied")]
    WideningAttempt,
    #[error("token expired")]
    Expired,
    #[error("proposal limit exhausted: limit={limit}")]
    ProposalLimitExhausted { limit: u32 },
}

impl CapabilityToken {
    /// Narrow this token to a smaller set of scopes.
    ///
    /// Every requested scope must already be in `self.scopes`; any scope not
    /// already held results in [`CapabilityTokenError::WideningAttempt`].
    ///
    /// Returns a new token with the narrowed scope and a fresh signature slot
    /// (all-zero — callers must re-sign before the gateway will accept it).
    /// The original is unchanged.
    pub fn narrow(
        &self,
        scopes: HashSet<BlastRadius>,
    ) -> Result<CapabilityToken, CapabilityTokenError> {
        for scope in &scopes {
            if !self.scopes.contains(scope) {
                return Err(CapabilityTokenError::WideningAttempt);
            }
        }

        Ok(CapabilityToken {
            id: self.id.clone(),
            scopes,
            expires_at: self.expires_at,
            max_proposals: self.max_proposals,
            // Narrowing invalidates the MAC — caller must re-sign.
            signature: [0u8; 64],
        })
    }

    /// Check whether this token holds the given scope.
    pub fn has(&self, scope: BlastRadius) -> bool {
        self.scopes.contains(&scope)
    }

    /// Check whether this token is currently expired against `chrono::Utc::now`.
    pub fn is_expired(&self) -> bool {
        chrono::Utc::now() > self.expires_at
    }

    /// Record `used` proposals against this token's budget and return whether
    /// one more is permitted.
    ///
    /// `Ok(())` when `used < max_proposals`; otherwise
    /// [`CapabilityTokenError::ProposalLimitExhausted`]. This helper is pure:
    /// it does not mutate the token (the count lives in the
    /// [`sera_db::proposal_usage::ProposalUsageStore`]), so callers feed in
    /// the currently-consumed count.
    pub fn consume_proposal(&self, used: u32) -> Result<(), CapabilityTokenError> {
        if used >= self.max_proposals {
            return Err(CapabilityTokenError::ProposalLimitExhausted {
                limit: self.max_proposals,
            });
        }
        Ok(())
    }
}

// ── Issuer ────────────────────────────────────────────────────────────────

/// Constructs unsigned [`CapabilityToken`]s from a scope set and expiry
/// policy. Signing is orthogonal and lives in
/// [`sera_gateway::evolve_token::EvolveTokenSigner`]: the issuer produces the
/// value, the signer installs the MAC.
///
/// The default implementation is [`DefaultCapabilityTokenIssuer`].
pub trait CapabilityTokenIssuer: Send + Sync {
    /// Issue an unsigned token with the given identity anchor, scopes,
    /// proposal budget, and TTL. `id` is stored on the token and used by the
    /// gateway as the issuer-identity anchor during the propose identity
    /// cross-check.
    fn issue(
        &self,
        id: String,
        scopes: HashSet<BlastRadius>,
        max_proposals: u32,
        ttl: std::time::Duration,
    ) -> CapabilityToken;
}

/// Default issuer — stamps `expires_at = now + ttl` and leaves the signature
/// zeroed for the gateway signer to fill in.
#[derive(Debug, Default, Clone, Copy)]
pub struct DefaultCapabilityTokenIssuer;

impl DefaultCapabilityTokenIssuer {
    /// Construct a fresh issuer. Has no internal state — the unit struct is
    /// returned purely for trait-object ergonomics.
    pub fn new() -> Self {
        Self
    }
}

impl CapabilityTokenIssuer for DefaultCapabilityTokenIssuer {
    fn issue(
        &self,
        id: String,
        scopes: HashSet<BlastRadius>,
        max_proposals: u32,
        ttl: std::time::Duration,
    ) -> CapabilityToken {
        let ttl_chrono = chrono::Duration::from_std(ttl).unwrap_or(chrono::Duration::zero());
        CapabilityToken {
            id,
            scopes,
            expires_at: chrono::Utc::now() + ttl_chrono,
            max_proposals,
            signature: [0u8; 64],
        }
    }
}

// ── ChangeProposer ────────────────────────────────────────────────────────

/// The principal proposing a change artifact, together with the capability
/// token that authorises the proposal.
///
/// Moved to `sera-auth` alongside [`CapabilityToken`] so the two types live
/// in the same crate — avoids forcing `sera-types` to depend on `sera-auth`.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ChangeProposer {
    pub principal_id: String,
    pub capability_token: CapabilityToken,
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Duration;

    fn make_token(scopes: impl IntoIterator<Item = BlastRadius>) -> CapabilityToken {
        CapabilityToken {
            id: "tok-test".to_string(),
            scopes: scopes.into_iter().collect(),
            expires_at: chrono::Utc::now() + chrono::Duration::hours(1),
            max_proposals: 10,
            signature: [0u8; 64],
        }
    }

    #[test]
    fn narrow_subset_succeeds() {
        let token = make_token([BlastRadius::AgentMemory, BlastRadius::SingleHookConfig]);
        let narrowed = token
            .narrow([BlastRadius::AgentMemory].into_iter().collect())
            .expect("narrow should succeed");
        assert!(narrowed.has(BlastRadius::AgentMemory));
        assert!(!narrowed.has(BlastRadius::SingleHookConfig));
        // Narrowing must reset the signature so the gateway re-signs.
        assert_eq!(narrowed.signature, [0u8; 64]);
    }

    #[test]
    fn narrow_widening_denied() {
        let token = make_token([BlastRadius::AgentMemory]);
        let result = token.narrow(
            [BlastRadius::AgentMemory, BlastRadius::SingleHookConfig]
                .into_iter()
                .collect(),
        );
        assert_eq!(result.unwrap_err(), CapabilityTokenError::WideningAttempt);
    }

    #[test]
    fn has_returns_correct_results() {
        let token = make_token([BlastRadius::AgentMemory]);
        assert!(token.has(BlastRadius::AgentMemory));
        assert!(!token.has(BlastRadius::GatewayCore));
    }

    #[test]
    fn consume_proposal_respects_budget() {
        let token = make_token([BlastRadius::AgentMemory]);
        // max_proposals = 10, used = 9 → still permitted.
        assert!(token.consume_proposal(9).is_ok());
        // used = 10 → exhausted.
        let err = token.consume_proposal(10).unwrap_err();
        assert_eq!(
            err,
            CapabilityTokenError::ProposalLimitExhausted { limit: 10 }
        );
    }

    #[test]
    fn is_expired_after_expiry() {
        let mut token = make_token([BlastRadius::AgentMemory]);
        token.expires_at = chrono::Utc::now() - chrono::Duration::seconds(1);
        assert!(token.is_expired());
    }

    // ── Issuer tests ──────────────────────────────────────────────────────

    #[test]
    fn issuer_stamps_expires_at_from_ttl() {
        let issuer = DefaultCapabilityTokenIssuer::new();
        let before = chrono::Utc::now();
        let token = issuer.issue(
            "agent-1".to_string(),
            [BlastRadius::AgentMemory].into_iter().collect(),
            5,
            Duration::from_secs(60),
        );
        let after = chrono::Utc::now();
        assert_eq!(token.id, "agent-1");
        assert!(token.has(BlastRadius::AgentMemory));
        assert_eq!(token.max_proposals, 5);
        // expires_at must be between (before + 60s) and (after + 60s).
        let lower = before + chrono::Duration::seconds(59);
        let upper = after + chrono::Duration::seconds(61);
        assert!(
            token.expires_at >= lower && token.expires_at <= upper,
            "expires_at {} out of expected window [{}, {}]",
            token.expires_at,
            lower,
            upper
        );
        // Issuer leaves the signature zeroed for the gateway signer.
        assert_eq!(token.signature, [0u8; 64]);
    }

    #[test]
    fn issuer_preserves_all_scopes() {
        let issuer = DefaultCapabilityTokenIssuer::new();
        let scopes: HashSet<BlastRadius> = [
            BlastRadius::AgentMemory,
            BlastRadius::SingleHookConfig,
            BlastRadius::GatewayCore,
        ]
        .into_iter()
        .collect();
        let token = issuer.issue(
            "multi".to_string(),
            scopes.clone(),
            3,
            Duration::from_secs(30),
        );
        assert_eq!(token.scopes, scopes);
    }

    #[test]
    fn issuer_is_dyn_compatible() {
        // The trait must be usable behind `dyn` so call sites can swap
        // implementations without generics (e.g. a test mock that yields a
        // fixed id).
        let issuer: Box<dyn CapabilityTokenIssuer> =
            Box::new(DefaultCapabilityTokenIssuer::new());
        let token = issuer.issue(
            "dyn-call".to_string(),
            [BlastRadius::AgentMemory].into_iter().collect(),
            1,
            Duration::from_secs(10),
        );
        assert_eq!(token.id, "dyn-call");
        assert_eq!(token.max_proposals, 1);
    }

    // ── Wire-serde parity with the old sera-types shape ──────────────────

    #[test]
    fn wire_serde_roundtrip_preserves_all_fields() {
        let token = CapabilityToken {
            id: "wire-test".to_string(),
            scopes: [BlastRadius::AgentMemory, BlastRadius::GlobalConfig]
                .into_iter()
                .collect(),
            expires_at: chrono::Utc::now(),
            max_proposals: 7,
            signature: [0xABu8; 64],
        };

        let json = serde_json::to_string(&token).expect("serialize");
        let back: CapabilityToken = serde_json::from_str(&json).expect("deserialize");
        assert_eq!(back.id, token.id);
        assert_eq!(back.scopes, token.scopes);
        assert_eq!(back.max_proposals, token.max_proposals);
        assert_eq!(back.signature, token.signature);
    }
}

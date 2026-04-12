//! CapabilityToken narrowing — Phase 0 (P0-7).
//!
//! A CapabilityToken represents a bounded set of capabilities granted to an
//! agent. Tokens can only be narrowed (scoped down), never widened. This
//! enforces the principle of least privilege at the auth layer.

use std::collections::HashSet;

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use sera_types::evolution::{AgentCapability, BlastRadius};

/// A narrowable capability token for an agent.
///
/// Issued by the auth layer and carried through the request context. Agents
/// may further narrow their own token before delegating to sub-agents, but
/// cannot widen beyond what was originally granted.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CapabilityToken {
    /// Unique identifier for this token issuance.
    pub token_id: uuid::Uuid,
    /// The agent this token was issued to.
    pub agent_id: String,
    /// Capabilities granted to this token.
    pub capabilities: HashSet<AgentCapability>,
    /// Optional blast-radius restriction (limits scope of change proposals).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub blast_radius: Option<BlastRadius>,
    /// Number of change proposals consumed against this token.
    pub proposals_consumed: u32,
    /// Maximum number of change proposals allowed (None = unlimited).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub max_proposals: Option<u32>,
    /// Whether a revocation check must be performed before use.
    pub revocation_check_required: bool,
    /// When this token was issued.
    pub issued_at: DateTime<Utc>,
    /// When this token expires (None = no expiry).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub expires_at: Option<DateTime<Utc>>,
}

/// Errors that can occur when using or narrowing a CapabilityToken.
#[derive(Debug, thiserror::Error)]
pub enum CapabilityTokenError {
    #[error("capability missing: {0:?}")]
    CapabilityMissing(AgentCapability),
    #[error("widening attempt denied")]
    WideningAttempt,
    #[error("token expired")]
    Expired,
    #[error("proposal limit exhausted: limit={limit}, consumed={consumed}")]
    ProposalLimitExhausted { limit: u32, consumed: u32 },
}

impl CapabilityToken {
    /// Narrow this token to a smaller set of capabilities and/or blast radius.
    ///
    /// The requested capabilities must be a subset of the current token's
    /// capabilities. Requesting any capability not already present results in
    /// [`CapabilityTokenError::WideningAttempt`].
    ///
    /// Returns a new token with the narrowed scope; the original is unchanged.
    pub fn narrow(
        &self,
        capabilities: HashSet<AgentCapability>,
        blast_radius: Option<BlastRadius>,
    ) -> Result<CapabilityToken, CapabilityTokenError> {
        // Every requested capability must already be in self.capabilities.
        for cap in &capabilities {
            if !self.capabilities.contains(cap) {
                return Err(CapabilityTokenError::WideningAttempt);
            }
        }

        Ok(CapabilityToken {
            token_id: uuid::Uuid::new_v4(),
            agent_id: self.agent_id.clone(),
            capabilities,
            blast_radius,
            proposals_consumed: 0,
            max_proposals: self.max_proposals,
            revocation_check_required: self.revocation_check_required,
            issued_at: Utc::now(),
            expires_at: self.expires_at,
        })
    }

    /// Check whether this token has the given capability.
    pub fn has(&self, cap: AgentCapability) -> bool {
        self.capabilities.contains(&cap)
    }

    /// Record one proposal consumed against this token's budget.
    ///
    /// Returns [`CapabilityTokenError::ProposalLimitExhausted`] if the limit
    /// would be exceeded.
    pub fn consume_proposal(&mut self) -> Result<(), CapabilityTokenError> {
        if let Some(limit) = self.max_proposals
            && self.proposals_consumed >= limit
        {
            return Err(CapabilityTokenError::ProposalLimitExhausted {
                limit,
                consumed: self.proposals_consumed,
            });
        }
        self.proposals_consumed += 1;
        Ok(())
    }

    /// Check whether this token is currently expired.
    pub fn is_expired(&self) -> bool {
        self.expires_at
            .map(|exp| Utc::now() > exp)
            .unwrap_or(false)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn make_token(caps: impl IntoIterator<Item = AgentCapability>) -> CapabilityToken {
        CapabilityToken {
            token_id: uuid::Uuid::new_v4(),
            agent_id: "agent-test".to_string(),
            capabilities: caps.into_iter().collect(),
            blast_radius: None,
            proposals_consumed: 0,
            max_proposals: None,
            revocation_check_required: false,
            issued_at: Utc::now(),
            expires_at: None,
        }
    }

    #[test]
    fn narrow_subset_succeeds() {
        let token = make_token([AgentCapability::MetaChange, AgentCapability::CodeChange]);
        let narrowed = token
            .narrow(
                [AgentCapability::CodeChange].into_iter().collect(),
                None,
            )
            .expect("narrow should succeed");
        assert!(narrowed.has(AgentCapability::CodeChange));
        assert!(!narrowed.has(AgentCapability::MetaChange));
    }

    #[test]
    fn narrow_widening_denied() {
        let token = make_token([AgentCapability::CodeChange]);
        let result = token.narrow(
            [AgentCapability::MetaChange, AgentCapability::CodeChange]
                .into_iter()
                .collect(),
            None,
        );
        assert!(matches!(result, Err(CapabilityTokenError::WideningAttempt)));
    }

    #[test]
    fn has_returns_correct_results() {
        let token = make_token([AgentCapability::CodeChange]);
        assert!(token.has(AgentCapability::CodeChange));
        assert!(!token.has(AgentCapability::MetaChange));
    }

    #[test]
    fn proposal_limit_enforced() {
        let mut token = make_token([AgentCapability::CodeChange]);
        token.max_proposals = Some(2);
        assert!(token.consume_proposal().is_ok());
        assert!(token.consume_proposal().is_ok());
        let err = token.consume_proposal().unwrap_err();
        assert!(matches!(
            err,
            CapabilityTokenError::ProposalLimitExhausted { limit: 2, consumed: 2 }
        ));
    }
}

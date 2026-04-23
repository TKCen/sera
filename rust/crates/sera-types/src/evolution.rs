use serde::{Deserialize, Serialize};
use std::fmt;

/// Content-addressed identity for change artifacts (SHA-256).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct ChangeArtifactId {
    pub hash: [u8; 32],
}

impl fmt::Display for ChangeArtifactId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        for byte in &self.hash {
            write!(f, "{byte:02x}")?;
        }
        Ok(())
    }
}

#[non_exhaustive]
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum BlastRadius {
    AgentMemory,
    AgentPersonaMutable,
    AgentSkill,
    AgentExperiencePool,
    SingleHookConfig,
    SingleToolPolicy,
    SingleConnector,
    SingleCircleConfig,
    AgentManifest,
    TierPolicy,
    HookChainStructure,
    ApprovalPolicy,
    SecretProvider,
    GlobalConfig,
    RuntimeCrate,
    GatewayCore,
    ProtocolSchema,
    DbMigration,
    ConstitutionalRuleSet,
    KillSwitchProtocol,
    AuditLogBackend,
    SelfEvolutionPipeline,
}

// CapabilityToken moved to `sera-auth::capability` — the canonical
// definition lives there so narrowing + issuance + signing can share a type
// without `sera-types` depending on `sera-auth` (dep-graph: types is a leaf).

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ConstitutionalRule {
    pub id: String,
    pub description: String,
    pub enforcement_point: ConstitutionalEnforcementPoint,
    pub content_hash: [u8; 32],
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ConstitutionalEnforcementPoint {
    PreProposal,
    PreApproval,
    PreApplication,
    PostApplication,
}

#[non_exhaustive]
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum EvolutionTier {
    AgentImprovement,
    ConfigEvolution,
    CodeEvolution,
}

// ChangeProposer moved to `sera-auth::capability` — it carries a
// CapabilityToken field, so it lives next to the token definition. Keeps
// sera-types free of an inverted dependency on sera-auth.

// AgentCapability moved to `capability.rs` — colocated with CapabilityToken
// and CapabilityPolicy, where it belongs semantically.

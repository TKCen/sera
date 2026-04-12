use serde::{Deserialize, Deserializer, Serialize, Serializer};
use std::collections::HashSet;
use std::fmt;

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
            fn visit_seq<A: serde::de::SeqAccess<'de>>(self, mut seq: A) -> Result<[u8; 64], A::Error> {
                let mut arr = [0u8; 64];
                for (i, slot) in arr.iter_mut().enumerate() {
                    *slot = seq.next_element()?.ok_or_else(|| serde::de::Error::invalid_length(i, &self))?;
                }
                Ok(arr)
            }
        }
        d.deserialize_bytes(Visitor)
    }
}

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

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CapabilityToken {
    pub id: String,
    pub scopes: HashSet<BlastRadius>,
    pub expires_at: chrono::DateTime<chrono::Utc>,
    pub max_proposals: u32,
    #[serde(with = "bytes64")]
    pub signature: [u8; 64],
}

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

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ChangeProposer {
    pub principal_id: String,
    pub capability_token: CapabilityToken,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AgentCapability {
    MetaChange,
    CodeChange,
    MetaApprover,
    ConfigRead,
    ConfigPropose,
}

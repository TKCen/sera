use sera_types::evolution::*;
use std::collections::HashSet;

#[test]
fn blast_radius_has_22_variants() {
    #[deny(unreachable_patterns)]
    fn variant_name(v: &BlastRadius) -> &'static str {
        match v {
            BlastRadius::AgentMemory => "AgentMemory",
            BlastRadius::AgentPersonaMutable => "AgentPersonaMutable",
            BlastRadius::AgentSkill => "AgentSkill",
            BlastRadius::AgentExperiencePool => "AgentExperiencePool",
            BlastRadius::SingleHookConfig => "SingleHookConfig",
            BlastRadius::SingleToolPolicy => "SingleToolPolicy",
            BlastRadius::SingleConnector => "SingleConnector",
            BlastRadius::SingleCircleConfig => "SingleCircleConfig",
            BlastRadius::AgentManifest => "AgentManifest",
            BlastRadius::TierPolicy => "TierPolicy",
            BlastRadius::HookChainStructure => "HookChainStructure",
            BlastRadius::ApprovalPolicy => "ApprovalPolicy",
            BlastRadius::SecretProvider => "SecretProvider",
            BlastRadius::GlobalConfig => "GlobalConfig",
            BlastRadius::RuntimeCrate => "RuntimeCrate",
            BlastRadius::GatewayCore => "GatewayCore",
            BlastRadius::ProtocolSchema => "ProtocolSchema",
            BlastRadius::DbMigration => "DbMigration",
            BlastRadius::ConstitutionalRuleSet => "ConstitutionalRuleSet",
            BlastRadius::KillSwitchProtocol => "KillSwitchProtocol",
            BlastRadius::AuditLogBackend => "AuditLogBackend",
            BlastRadius::SelfEvolutionPipeline => "SelfEvolutionPipeline",
            _ => "unknown",
        }
    }

    let variants = vec![
        BlastRadius::AgentMemory,
        BlastRadius::AgentPersonaMutable,
        BlastRadius::AgentSkill,
        BlastRadius::AgentExperiencePool,
        BlastRadius::SingleHookConfig,
        BlastRadius::SingleToolPolicy,
        BlastRadius::SingleConnector,
        BlastRadius::SingleCircleConfig,
        BlastRadius::AgentManifest,
        BlastRadius::TierPolicy,
        BlastRadius::HookChainStructure,
        BlastRadius::ApprovalPolicy,
        BlastRadius::SecretProvider,
        BlastRadius::GlobalConfig,
        BlastRadius::RuntimeCrate,
        BlastRadius::GatewayCore,
        BlastRadius::ProtocolSchema,
        BlastRadius::DbMigration,
        BlastRadius::ConstitutionalRuleSet,
        BlastRadius::KillSwitchProtocol,
        BlastRadius::AuditLogBackend,
        BlastRadius::SelfEvolutionPipeline,
    ];

    let names: Vec<&str> = variants.iter().map(variant_name).collect();
    assert_eq!(names.len(), 22);

    let unique: HashSet<&&str> = names.iter().collect();
    assert_eq!(unique.len(), 22, "all variant names must be distinct");
}

#[test]
fn blast_radius_serde_roundtrip_all_variants() {
    let variants = vec![
        BlastRadius::AgentMemory,
        BlastRadius::AgentPersonaMutable,
        BlastRadius::AgentSkill,
        BlastRadius::AgentExperiencePool,
        BlastRadius::SingleHookConfig,
        BlastRadius::SingleToolPolicy,
        BlastRadius::SingleConnector,
        BlastRadius::SingleCircleConfig,
        BlastRadius::AgentManifest,
        BlastRadius::TierPolicy,
        BlastRadius::HookChainStructure,
        BlastRadius::ApprovalPolicy,
        BlastRadius::SecretProvider,
        BlastRadius::GlobalConfig,
        BlastRadius::RuntimeCrate,
        BlastRadius::GatewayCore,
        BlastRadius::ProtocolSchema,
        BlastRadius::DbMigration,
        BlastRadius::ConstitutionalRuleSet,
        BlastRadius::KillSwitchProtocol,
        BlastRadius::AuditLogBackend,
        BlastRadius::SelfEvolutionPipeline,
    ];

    for variant in &variants {
        let json = serde_json::to_string(variant).unwrap();
        let decoded: BlastRadius = serde_json::from_str(&json).unwrap();
        assert_eq!(variant, &decoded);
    }
}

#[test]
fn change_artifact_id_display_is_hex() {
    let id = ChangeArtifactId { hash: [0u8; 32] };
    let s = id.to_string();
    assert_eq!(s.len(), 64);
    assert!(s.chars().all(|c| c == '0'));
}

// capability_token_scope_is_set removed — CapabilityToken moved to
// sera-auth::capability. See sera-auth tests for its coverage.

#[test]
fn evolution_tier_non_exhaustive_serde() {
    let variants = vec![
        EvolutionTier::AgentImprovement,
        EvolutionTier::ConfigEvolution,
        EvolutionTier::CodeEvolution,
    ];

    for variant in &variants {
        let json = serde_json::to_string(variant).unwrap();
        let decoded: EvolutionTier = serde_json::from_str(&json).unwrap();
        assert_eq!(variant, &decoded);
    }
}

#[test]
fn agent_capability_all_variants_serde() {
    let variants = vec![
        (AgentCapability::MetaChange, "meta_change"),
        (AgentCapability::CodeChange, "code_change"),
        (AgentCapability::MetaApprover, "meta_approver"),
        (AgentCapability::ConfigRead, "config_read"),
        (AgentCapability::ConfigPropose, "config_propose"),
    ];

    for (variant, expected_snake) in &variants {
        let json = serde_json::to_string(variant).unwrap();
        assert_eq!(json, format!("\"{}\"", expected_snake));
        let decoded: AgentCapability = serde_json::from_str(&json).unwrap();
        assert_eq!(variant, &decoded);
    }
}

#[test]
fn constitutional_rule_serde_roundtrip() {
    let enforcement_points = vec![
        ConstitutionalEnforcementPoint::PreProposal,
        ConstitutionalEnforcementPoint::PreApproval,
        ConstitutionalEnforcementPoint::PreApplication,
        ConstitutionalEnforcementPoint::PostApplication,
    ];

    for point in enforcement_points {
        let rule = ConstitutionalRule {
            id: "rule-001".to_string(),
            description: "No self-modifying kill switches".to_string(),
            enforcement_point: point,
            content_hash: [0xabu8; 32],
        };

        let json = serde_json::to_string(&rule).unwrap();
        let decoded: ConstitutionalRule = serde_json::from_str(&json).unwrap();

        assert_eq!(decoded.id, rule.id);
        assert_eq!(decoded.description, rule.description);
        assert_eq!(decoded.enforcement_point, rule.enforcement_point);
        assert_eq!(decoded.content_hash, rule.content_hash);
    }
}

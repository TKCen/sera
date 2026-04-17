//! Integration tests for the self-patching scaffold.

use sera_skills::self_patch::{
    DefaultSelfPatchValidator, FsSelfPatchApplier, InMemorySelfPatchApplier, PatchError,
    PatchKind, PatchPayload, SelfPatchApplier, SelfPatchValidator, SkillPack, SkillPatch,
    MAX_SKILL_MD_BYTES,
};

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

fn make_patch(
    skill_id: &str,
    base_version: &str,
    kind: PatchKind,
    payload: PatchPayload,
) -> SkillPatch {
    SkillPatch {
        skill_id: skill_id.to_string(),
        base_version: base_version.to_string(),
        patch_kind: kind,
        payload,
    }
}

fn base_pack(version: &str) -> SkillPack {
    SkillPack::new("code-review", version)
}

const VALID_SKILL_MD: &str = "---\nname: code-review\nversion: 1.0.0\n---\n\nBody text.\n";

// ---------------------------------------------------------------------------
// 1. Version mismatch -> VersionMismatch error
// ---------------------------------------------------------------------------

#[test]
fn version_mismatch_is_rejected() {
    let validator = DefaultSelfPatchValidator;
    let current = base_pack("1.0.0");
    let patch = make_patch(
        "code-review",
        "0.9.0", // wrong
        PatchKind::UpdateSkillMd,
        PatchPayload::SkillMd {
            new_body: VALID_SKILL_MD.to_string(),
        },
    );
    let err = validator.validate(&patch, &current).unwrap_err();
    assert!(
        matches!(err, PatchError::VersionMismatch { .. }),
        "expected VersionMismatch, got {err:?}"
    );
}

#[test]
fn version_mismatch_error_message_contains_both_versions() {
    let validator = DefaultSelfPatchValidator;
    let current = base_pack("2.3.1");
    let patch = make_patch(
        "code-review",
        "1.0.0",
        PatchKind::UpdateSkillMd,
        PatchPayload::SkillMd {
            new_body: VALID_SKILL_MD.to_string(),
        },
    );
    let err = validator.validate(&patch, &current).unwrap_err();
    let msg = err.to_string();
    assert!(msg.contains("1.0.0"), "expected base_version in message: {msg}");
    assert!(msg.contains("2.3.1"), "expected actual version in message: {msg}");
}

// ---------------------------------------------------------------------------
// 2. Valid SkillMd patch applies cleanly to in-memory applier
// ---------------------------------------------------------------------------

#[test]
fn valid_skill_md_patch_applies_in_memory() {
    let validator = DefaultSelfPatchValidator;
    let current = base_pack("1.0.0");
    let new_body = VALID_SKILL_MD.to_string();
    let patch = make_patch(
        "code-review",
        "1.0.0",
        PatchKind::UpdateSkillMd,
        PatchPayload::SkillMd {
            new_body: new_body.clone(),
        },
    );

    let validated = validator.validate(&patch, &current).unwrap();
    assert_eq!(validated.payload_bytes, new_body.len());
    assert!(validated.diff_summary.contains("UpdateSkillMd"));

    let applier = InMemorySelfPatchApplier::new(current);
    let updated = applier.apply(validated).unwrap();
    assert_eq!(updated.skill_md, new_body);
}

// ---------------------------------------------------------------------------
// 3. AddKnowledgeBlock with duplicate filename rejected
// ---------------------------------------------------------------------------

#[test]
fn duplicate_knowledge_filename_rejected() {
    let validator = DefaultSelfPatchValidator;
    let mut current = base_pack("1.0.0");
    current
        .knowledge
        .insert("intro.md".to_string(), "existing content".to_string());

    let patch = make_patch(
        "code-review",
        "1.0.0",
        PatchKind::AddKnowledgeBlock,
        PatchPayload::Knowledge {
            filename: "intro.md".to_string(), // duplicate
            body: "new content".to_string(),
        },
    );
    let err = validator.validate(&patch, &current).unwrap_err();
    assert!(
        matches!(err, PatchError::DuplicateKnowledge(ref f) if f == "intro.md"),
        "expected DuplicateKnowledge(intro.md), got {err:?}"
    );
}

#[test]
fn new_knowledge_block_applies_in_memory() {
    let validator = DefaultSelfPatchValidator;
    let current = base_pack("1.0.0");
    let patch = make_patch(
        "code-review",
        "1.0.0",
        PatchKind::AddKnowledgeBlock,
        PatchPayload::Knowledge {
            filename: "tips.md".to_string(),
            body: "# Tips\n\nBe careful.\n".to_string(),
        },
    );
    let validated = validator.validate(&patch, &current).unwrap();
    let applier = InMemorySelfPatchApplier::new(current);
    let updated = applier.apply(validated).unwrap();
    assert!(updated.knowledge.contains_key("tips.md"));
    assert!(updated.knowledge["tips.md"].contains("Be careful"));
}

// ---------------------------------------------------------------------------
// 4. SkillMd over 64 KB rejected
// ---------------------------------------------------------------------------

#[test]
fn skill_md_over_64kb_rejected() {
    let validator = DefaultSelfPatchValidator;
    let current = base_pack("1.0.0");

    // Build a body just over the limit.
    let big_body = format!(
        "---\nname: code-review\nversion: 1.0.0\n---\n\n{}",
        "x".repeat(MAX_SKILL_MD_BYTES + 1)
    );
    let patch = make_patch(
        "code-review",
        "1.0.0",
        PatchKind::UpdateSkillMd,
        PatchPayload::SkillMd { new_body: big_body },
    );
    let err = validator.validate(&patch, &current).unwrap_err();
    assert!(
        matches!(err, PatchError::SizeExceeded(n) if n > MAX_SKILL_MD_BYTES),
        "expected SizeExceeded, got {err:?}"
    );
}

#[test]
fn skill_md_exactly_64kb_is_accepted() {
    let validator = DefaultSelfPatchValidator;
    let current = base_pack("1.0.0");

    // Construct a body that is exactly MAX_SKILL_MD_BYTES.
    let header = "---\nname: code-review\nversion: 1.0.0\n---\n\n";
    let padding = "x".repeat(MAX_SKILL_MD_BYTES - header.len());
    let body = format!("{header}{padding}");
    assert_eq!(body.len(), MAX_SKILL_MD_BYTES);

    let patch = make_patch(
        "code-review",
        "1.0.0",
        PatchKind::UpdateSkillMd,
        PatchPayload::SkillMd { new_body: body },
    );
    assert!(validator.validate(&patch, &current).is_ok());
}

// ---------------------------------------------------------------------------
// 5. Invalid YAML frontmatter is rejected
// ---------------------------------------------------------------------------

#[test]
fn missing_frontmatter_fence_rejected() {
    let validator = DefaultSelfPatchValidator;
    let current = base_pack("1.0.0");
    let patch = make_patch(
        "code-review",
        "1.0.0",
        PatchKind::UpdateSkillMd,
        PatchPayload::SkillMd {
            new_body: "# No frontmatter here\n\nJust markdown.\n".to_string(),
        },
    );
    let err = validator.validate(&patch, &current).unwrap_err();
    assert!(
        matches!(err, PatchError::SyntaxInvalid(_)),
        "expected SyntaxInvalid, got {err:?}"
    );
}

#[test]
fn malformed_yaml_frontmatter_rejected() {
    let validator = DefaultSelfPatchValidator;
    let current = base_pack("1.0.0");
    // Opening fence present but YAML is broken (unclosed mapping).
    let patch = make_patch(
        "code-review",
        "1.0.0",
        PatchKind::UpdateSkillMd,
        PatchPayload::SkillMd {
            new_body: "---\nname: :\n  bad: [unclosed\n---\n\nbody\n".to_string(),
        },
    );
    let err = validator.validate(&patch, &current).unwrap_err();
    assert!(
        matches!(err, PatchError::SyntaxInvalid(_)),
        "expected SyntaxInvalid for bad YAML, got {err:?}"
    );
}

// ---------------------------------------------------------------------------
// 6. Metadata patch applies
// ---------------------------------------------------------------------------

#[test]
fn metadata_patch_applies_in_memory() {
    let validator = DefaultSelfPatchValidator;
    let current = base_pack("1.0.0");
    let patch = make_patch(
        "code-review",
        "1.0.0",
        PatchKind::UpdateMetadata,
        PatchPayload::Metadata {
            field: "description".to_string(),
            value: "Updated description".to_string(),
        },
    );
    let validated = validator.validate(&patch, &current).unwrap();
    let applier = InMemorySelfPatchApplier::new(current);
    let updated = applier.apply(validated).unwrap();
    assert_eq!(updated.metadata["description"], "Updated description");
}

// ---------------------------------------------------------------------------
// 7. FsSelfPatchApplier writes through a tempdir
// ---------------------------------------------------------------------------

#[test]
fn fs_applier_writes_skill_md_atomically() {
    let root = tempfile::tempdir().unwrap();
    // Pre-populate an existing skill directory with an old SKILL.md.
    let skill_dir = root.path().join("code-review");
    std::fs::create_dir_all(&skill_dir).unwrap();
    std::fs::write(skill_dir.join("SKILL.md"), "---\nname: code-review\nversion: 0.9.0\n---\n\nOld body.\n").unwrap();

    let validator = DefaultSelfPatchValidator;
    // Treat current version as 0.9.0 so the patch aligns.
    let current = base_pack("0.9.0");
    let new_body = "---\nname: code-review\nversion: 0.9.0\n---\n\nNew body.\n".to_string();
    let patch = make_patch(
        "code-review",
        "0.9.0",
        PatchKind::UpdateSkillMd,
        PatchPayload::SkillMd {
            new_body: new_body.clone(),
        },
    );
    let validated = validator.validate(&patch, &current).unwrap();

    let applier = FsSelfPatchApplier::new(root.path());
    let updated = applier.apply(validated).unwrap();

    // In-memory snapshot reflects new body.
    assert_eq!(updated.skill_md, new_body);
    // File on disk reflects new body.
    let on_disk = std::fs::read_to_string(skill_dir.join("SKILL.md")).unwrap();
    assert_eq!(on_disk, new_body);
}

#[test]
fn fs_applier_adds_knowledge_block() {
    let root = tempfile::tempdir().unwrap();
    let skill_dir = root.path().join("code-review");
    std::fs::create_dir_all(&skill_dir).unwrap();

    let validator = DefaultSelfPatchValidator;
    let current = base_pack("1.0.0");
    let patch = make_patch(
        "code-review",
        "1.0.0",
        PatchKind::AddKnowledgeBlock,
        PatchPayload::Knowledge {
            filename: "context.md".to_string(),
            body: "# Context\n\nImportant.\n".to_string(),
        },
    );
    let validated = validator.validate(&patch, &current).unwrap();

    let applier = FsSelfPatchApplier::new(root.path());
    let updated = applier.apply(validated).unwrap();

    // In-memory snapshot contains the new knowledge file.
    assert!(updated.knowledge.contains_key("context.md"));
    // File written to knowledge/ subdirectory.
    let on_disk = std::fs::read_to_string(skill_dir.join("knowledge").join("context.md")).unwrap();
    assert!(on_disk.contains("Important"));
}

#[test]
fn fs_applier_creates_skill_dir_when_absent() {
    let root = tempfile::tempdir().unwrap();
    // skill directory does NOT exist yet.

    let validator = DefaultSelfPatchValidator;
    let current = base_pack("1.0.0");
    let new_body = VALID_SKILL_MD.to_string();
    let patch = make_patch(
        "new-skill",
        "1.0.0",
        PatchKind::UpdateSkillMd,
        PatchPayload::SkillMd {
            new_body: new_body.clone(),
        },
    );
    let validated = validator.validate(&patch, &current).unwrap();

    let applier = FsSelfPatchApplier::new(root.path());
    let updated = applier.apply(validated).unwrap();
    assert_eq!(updated.skill_md, new_body);
    assert!(root.path().join("new-skill").join("SKILL.md").exists());
}

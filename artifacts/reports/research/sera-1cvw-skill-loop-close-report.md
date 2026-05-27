# sera-1cvw — Close Tier 1 Self-Improvement Skill Loop

## Summary

Closes the first end-to-end self-improvement loop: an agent can patch a skill via `skill-manage`, and a subsequent runtime turn auto-matches the patched skill via `TriggerDispatcher` with durable activity provenance and a governed apply boundary.

## Changes

### sera-skills crate

| File | Change |
|------|--------|
| `src/knowledge_activity_log.rs` | `save_to_path()` / `load_from_path()` — atomic JSON persistence + reload, with rolling-window truncation on load. 5 new tests. |
| `src/patch_policy.rs` | **New module.** `SkillPatchPolicy` trait + `Tier1SkillPatchPolicy` (allowed-root + resolved skill-path + body-budget) + `AllowAllPolicy` (test/open sandbox). 7 new tests including symlink-escape rejection. |
| `src/lib.rs` | Export `patch_policy` module + re-export key types. |

### sera-runtime crate

| File | Change |
|------|--------|
| `src/tools/skill_management.rs` | `SkillManagementContext`: optional `log_path` + `patch_policy` fields, `with_log_persistence()`, `with_patch_policy()`, `persist_log()`. `patch_skill()`: Tier 1 policy gate before validation using the resolved skill path, closing single-file symlink escape. Richer provenance metadata (`skill_name`, `patch_kind`, `diff_summary`, `root`, `body_bytes`, `after_hash`). `skill_management_context_from_env()`: wires `Tier1SkillPatchPolicy` + log persistence by default. 8 new tests. |
| `src/skill_dispatch.rs` | `prepare_turn_context()` — integration seam combining `on_turn()` + `active_context_injections()` for turn-loop callers. |

## Acceptance Criteria Verification

| AC | Status | Evidence |
|----|--------|----------|
| 1. `skill-manage patch` validates/applies atomically | PASS | All 49 skill_management tests pass including version-mismatch, name-mismatch, path-traversal rejection tests |
| 2. Turn auto-match via TriggerDispatcher | PASS | `fresh_engine_loads_patched_skill_and_fires_trigger` test: patch adds "welcome" trigger, fresh engine loads from disk and fires on "welcome aboard" |
| 3. KnowledgeActivityLog persists to disk | PASS | `activity_log_persists_to_disk_and_survives_restart` test: session 1 creates + persists, session 2 loads + verifies entry |
| 4. Fresh runtime loads patched skill | PASS | `fresh_engine_loads_patched_skill_and_fires_trigger` test: new `SkillDispatchEngine` loads via `load_dir()`, proves patched content is on disk |
| 5. Tier 1 policy rejects out-of-bounds patches | PASS | `patch_rejected_by_policy_outside_root`, `patch_rejected_by_policy_over_budget`, and `patch_rejected_when_single_file_skill_symlink_escapes_allowed_root` tests |
| 6. Patch provenance audit-visible | PASS | `activity_log_provenance_has_required_fields` test: verifies skill_name, action, patch_kind, diff_summary, root, body_bytes, after_hash |
| 7. Tests pass + cargo check | PASS | See below |
| 8. PR opened against main | PASS | #1288 |

## Tests/Checks Run

| Command | Result |
|---------|--------|
| `cargo test --lib -p sera-skills` | **256 passed** (was 244; +12 new) |
| `cargo test --lib -p sera-runtime -- skill_management` | **50 passed** (was 42; +8 new) |
| `cargo test --lib -p sera-runtime -- skill_dispatch` | **9 passed** |
| `cargo test -p sera-skills --test self_patch` | **13 passed** |
| `cargo check -p sera-runtime -p sera-skills` | Clean, 0 errors, 0 warnings |
| `cargo clippy --workspace -- -D warnings` | Clean after CI repair |

## PR URL

https://github.com/TKCen/sera/pull/1288

## PR Review / CI Repair

After PR #1288 opened, CI and automated review found two blockers:

1. `validate-rust` failed clippy on `std::io::Error::new(ErrorKind::Other, e)`; fixed with `std::io::Error::other`.
2. Codex identified a P1 symlink escape in the Tier 1 policy gate: a single-file skill inside an allowed root could be a symlink to a target outside the root. Fixed by passing the actual skill path into `SkillPatchPolicy`, canonicalizing the allowed root/root/path, requiring the resolved target to stay inside an allowed root, and adding unit + runtime regression tests.

Repair evidence:

- `cargo test --lib -p sera-skills patch_policy -- --nocapture`: 7 passed.
- `cargo test --lib -p sera-runtime -- skill_management::tests::patch_rejected_when_single_file_skill_symlink_escapes_allowed_root skill_management::tests::patch_rejected_by_policy_outside_root --nocapture`: 2 passed.
- `cargo clippy --workspace -- -D warnings`: clean.
- `cargo test --lib -p sera-runtime -- skill_management`: 50 passed.
- `cargo test --lib -p sera-skills`: 256 passed.
- `cargo test -p sera-skills --test self_patch`: 13 passed.
- `cargo check -p sera-runtime -p sera-skills`: clean.

## PR Review / CI Repair 2

A second Codex review on `46e8196b` found that long-running gateway processes still used an in-memory skill registry loaded at startup. This left patched triggers invisible until restart. The repair adds:

- `SkillDispatchEngine` source-dir tracking and `reload_registered_dirs()`;
- `prepare_turn_context()` as the refreshed async turn-loop seam, plus `prepare_loaded_turn_context()` as the explicit no-refresh fallback;
- gateway `execute_turn` wiring to call the refreshed `prepare_turn_context()` before injecting skill context, with warn-and-fallback behavior if reload fails;
- regression tests proving patched markdown triggers are visible through the default turn-context seam without process restart and active skills survive reload;
- corrected markdown parser semantics: frontmatter `triggers` now stay as keyword triggers on the `SkillDefinition` while `SkillConfig::trigger` remains `Manual`, avoiding accidental fire-on-any-turn behavior.

Repair evidence:

- `cargo test --lib -p sera-runtime -- skill_dispatch -- --nocapture`: 11 passed, including `prepare_turn_context_sees_patched_triggers_without_restart`.
- `cargo test --lib -p sera-runtime -- skill_management::tests::fresh_engine_loads_patched_skill_and_fires_trigger -- --nocapture`: 1 passed.
- `cargo test --lib -p sera-skills markdown::tests::parses_valid_frontmatter_and_body -- --nocapture`: 1 passed.
- `cargo test --lib -p sera-skills trigger_dispatcher -- --nocapture`: 18 passed.
- `cargo check -p sera-runtime -p sera-gateway -p sera-skills`: clean.
- `cargo clippy -p sera-runtime -p sera-gateway -p sera-skills -- -D warnings`: clean.

## Remaining Gaps / Follow-up Beads

1. **DefaultRuntime turn-loop wiring**: gateway `execute_turn` now refreshes and prepares skill context before the think step; standalone `DefaultRuntime` integration remains a follow-up if/when it owns skill dispatch directly.
2. **Full sera-meta PolicyEngine integration**: The narrow `Tier1SkillPatchPolicy` is a focused interface; wiring the full `PolicyEngine` with `ChangeArtifact` + `EvolutionResult` pipeline is a follow-up.
3. **OCSF audit event emission**: Provenance is captured in `KnowledgeActivityLog` entries; emitting structured OCSF events alongside is a follow-up.

## Git Status

```
## omc/sera-1cvw-skill-loop-close...origin/omc/sera-1cvw-skill-loop-close
```

Clean — all changes committed and pushed.

## Done Marker

SERA_1CVW_SKILL_LOOP_CLOSE_DONE https://github.com/TKCen/sera/pull/1288

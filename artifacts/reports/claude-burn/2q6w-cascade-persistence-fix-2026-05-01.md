# 2q6w Cascade Persistence Fix Handoff

Date: 2026-05-01
Branch: `fix/sera-2q6w-cascade-persistence`

## Summary

Fixed the cascade revocation persistence bug by separating chain metadata from revocation state in `capability_token_revocations`.

- `record()` now registers unrevoked chain metadata.
- `is_revoked()` now checks the explicit `revoked` marker.
- `revoke_cascade()` now marks every visited descendant row revoked on InMemory, SQLite, and Postgres.
- Postgres cascade uses a recursive CTE plus `INSERT ... SELECT ... ON CONFLICT DO NOTHING` and an in-transaction mark step, preserving tenant scope, CYCLE protection, and the full-depth `<=` cap.

## Tests Added

- InMemory and SQLite regressions assert `root -> child -> grandchild` descendants are unrevoked before `revoke_cascade(root)` and revoked after it.
- Auth smoke asserts a cascaded root revocation rejects a grandchild by the grandchild `instance_id`.
- Existing lookup failure coverage confirms store errors map to `EvolveTokenError::RevocationLookupFailed`, not `Revoked`.
- Postgres SQL-shape tests cover descendant `INSERT ... SELECT`, idempotent `ON CONFLICT DO NOTHING`, marking existing metadata rows revoked, and `d.depth <= max-depth` behavior.

## Verification

- `RUSTC_WRAPPER= cargo test -p sera-db --lib capability_revocation`
- `RUSTC_WRAPPER= cargo test -p sera-auth --lib revocation`
- `RUSTC_WRAPPER= cargo clippy -p sera-auth -p sera-db --all-targets -- -D warnings`

All passed.

## Notes

- The requested assessment file `artifacts/reports/claude-burn/overall-assessment-2026-05-01.md` was not present in this worktree when the fix started.
- I did not wire `verify_revocable` into gateway admission because there is no obvious current gateway `CapabilityToken` validation path in `rust/crates/sera-gateway/src`; the only live auth middleware path found is JWT/API-key verification.

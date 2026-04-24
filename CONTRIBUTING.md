# Contributing to SERA

Thanks for your interest. SERA is a Rust-first agent platform, still in active development toward Phase 1 completion.

## Before you start

1. Read `README.md` for the project overview and `docs/WHY-SERA.md` for the architectural commitments.
2. Read `CLAUDE.md` — four "Working Principles" govern every change (Think Before Coding, Simplicity First, Surgical Changes, Goal-Driven Execution). These apply to humans as well as LLM contributors.
3. Read `CODE_OF_CONDUCT.md`. It applies to human and automated contributors alike.
4. Check `bd ready` (beads issue tracker) for available work that isn't blocked.

## Good first issues

New contributors: look for beads filtered by type and priority before picking something bigger.

```bash
bd ready --type question             # open questions & discussion-flagged items
bd ready --priority 4                # lowest-priority, usually low-risk
bd list --label good-first-issue     # if the label exists in the current graph
```

A good first bead typically: touches one crate, has a clear acceptance criterion, and doesn't require changing cross-crate traits. If you're unsure whether something qualifies, comment on the bead (`bd comment <id> -m "..."`) and ask.

## Development loop

```bash
cd rust
cargo check --workspace           # fast incremental validation
cargo test --workspace            # full suite
cargo clippy --workspace -- -D warnings
```

A green `cargo check --workspace` is the minimum bar before opening a PR. Feature-matrix coverage lives at `scripts/check-feature-matrix.sh` and runs the three configurations (`default`, `--no-default-features`, `--features enterprise`) — run it before touching workspace deps.

## Crate ownership & reading order

SERA's workspace has explicit crate boundaries; changes inside certain crates require you to read the local guide first. If you're touching one of these paths, read the matching file **before** writing code:

| Crate or module | Read first |
| --- | --- |
| `rust/` (any change) | `rust/CLAUDE.md` |
| `rust/crates/sera-runtime/**` | `rust/crates/sera-runtime/CLAUDE.md` |
| `rust/crates/sera-session/src/state.rs` | `docs/plan/ARCHITECTURE-2.0.md` §3 (data flow) and §4 (traits) |
| `rust/crates/sera-hooks/**` | `docs/plan/specs/SPEC-hooks.md` and `rust/crates/sera-types/src/hook.rs` (`HookPoint` enum) |
| `rust/crates/sera-meta/**` (constitutional, policy, evolution) | `docs/plan/ARCHITECTURE-2.0.md` and `docs/plan/specs/SPEC-meta.md` if present |
| `rust/crates/sera-memory/**` | `docs/plugins/memory.md` |
| `rust/crates/sera-workflow/**` | the `AwaitType` variants in `sera-types` plus `ready.rs` |
| `rust/crates/sera-oci/**` and `sera-tools/**` | `docs/plan/specs/SPEC-runtime.md` §tool dispatch |
| `legacy/**` | **don't** — `legacy/` is frozen pre-Rust history kept for reference only |

If you add a new crate-level guide, link it from `rust/CLAUDE.md` and this table.

## Commit style

- One concern per commit. Explain the *why* in the message, not the *what* — the diff shows the what.
- Reference specs or phase-plan anchors where relevant (e.g. `(P0-6)`, `(SPEC-runtime §4)`).
- Reference the bead when applicable: include `(sera-xxxx)` in the subject line.
- Sign-offs and `Co-Authored-By` trailers are welcome when pairing with an LLM.

### Sign-off / DCO

**No DCO sign-off is required.** SERA is MIT-licensed (`LICENSE`) and all code contributions are accepted under that license by the act of opening a PR. `Signed-off-by:` trailers are allowed but not enforced by CI.

## Pull requests

- Target `main`. Keep PRs focused — one feature or one fix per PR.
- **Title format:** `<type>(<scope>): <subject> (sera-xxxx)` where type is one of `feat`, `fix`, `docs`, `refactor`, `test`, `chore`, `ci`. Example: `feat(sera-runtime): add streaming tool-call parser (sera-1234)`.
- **PR description must include:**
  - A bead reference (`Closes sera-xxxx` or `Refs sera-xxxx`) if the work tracks a bead.
  - The spec section or architectural decision the change follows, if applicable.
  - A **Test plan** checklist:
    ```
    ## Test plan
    - [ ] `cargo check --workspace` passes
    - [ ] `cargo test -p <crate>` passes for affected crates
    - [ ] `cargo clippy --workspace -- -D warnings` is clean
    - [ ] (if applicable) `scripts/check-feature-matrix.sh` passes
    - [ ] Manual verification: <what you did>
    ```
- CI (`validate-rust`, `clippy`, `codeql`, `e2e-smoke`) must be green before merge.
- Admin-merging docs-only PRs without waiting on `e2e-smoke` is acceptable.

## Issue tracker

This project uses [beads (`bd`)](https://github.com/steveyegge/beads) — do not use GitHub Issues for task tracking. Beads is auto-synced via Dolt and survives across contributors.

```bash
bd ready              # show unblocked issues
bd show <id>          # details
bd update <id> --claim
bd close <id>
```

File new work as beads (`bd create ...`) before writing code. One-line bead references in PR descriptions (e.g. `Closes sera-xxxx`) keep the graph coherent.

## Code of conduct

See `CODE_OF_CONDUCT.md`. In short: be kind, be honest, surface disagreements early, keep the humor clean. Treat automated contributors with the same professional courtesy you'd extend to a new human collaborator, and treat their output with the same scrutiny.

## Questions

Open a GitHub discussion or file a bead with `type=question`.

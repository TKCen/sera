# Contributing to SERA

Thanks for your interest. SERA is a Rust-first agent platform, still in active development toward Phase 1 completion.

## Before you start

1. Read `README.md` for the project overview.
2. Read `CLAUDE.md` — four "Working Principles" govern every change (Think Before Coding, Simplicity First, Surgical Changes, Goal-Driven Execution). These apply to humans as well as LLM contributors.
3. Check `bd ready` (beads issue tracker) for available work that isn't blocked.

## Development loop

```bash
cd rust
cargo check --workspace           # fast incremental validation
cargo test --workspace            # full suite
cargo clippy --workspace -- -D warnings
```

A green `cargo check --workspace` is the minimum bar before opening a PR. Feature-matrix coverage lives at `scripts/check-feature-matrix.sh` and runs the three configurations (`default`, `--no-default-features`, `--features enterprise`) — run it before touching workspace deps.

## Commit style

- One concern per commit. Explain the *why* in the message, not the *what* — the diff shows the what.
- Reference specs or phase-plan anchors where relevant (e.g. `(P0-6)`, `(SPEC-runtime §4)`).
- Sign-offs and `Co-Authored-By` trailers are welcome when pairing with an LLM.

## Pull requests

- Target `main`. Keep PRs focused — one feature or one fix per PR.
- The PR description should name the spec section or bead being addressed and list the tests that verify it.
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

Be kind, be honest, surface disagreements early, keep the humor clean. We reserve the right to remove contributors who aren't operating in good faith.

## Questions

Open a GitHub discussion or file a bead with `type=question`.

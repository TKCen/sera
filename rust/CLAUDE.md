# rust/ — SERA Rust Workspace

## Overview

Cargo workspace containing all Rust crates for the SERA migration. See `MIGRATION-PLAN.md` for the full phase-by-phase plan and `docs/RUST-MIGRATION-PLAN.md` for architectural decisions.

## Toolchain

- **Rust:** 1.94+ (edition 2024)
- **Cargo:** workspace at `rust/Cargo.toml`
- **LSP:** rust-analyzer (installed via `rustup component add rust-analyzer`)

## Commands

All commands run from `rust/` directory. On Windows, use absolute paths since `cd` doesn't persist.

```bash
# Primary validation loop — run after every change (~1-3s incremental)
cargo check --workspace

# Full test suite
cargo test --workspace

# Clippy lints (treat warnings as errors)
cargo clippy --workspace -- -D warnings

# Build release binaries
cargo build --release

# Run a specific crate's tests
cargo test -p sera-domain
cargo test -p sera-db

# Check a single crate (faster)
cargo check -p sera-core
```

## Crate Map

| Crate             | Type | Purpose                                                 |
| ----------------- | ---- | ------------------------------------------------------- |
| `sera-domain`     | lib  | Shared types, enums, IDs (leaf crate, no internal deps) |
| `sera-config`     | lib  | Environment/file config loading                         |
| `sera-db`         | lib  | PostgreSQL via sqlx, migrations, repositories           |
| `sera-auth`       | lib  | API keys, JWT, OIDC, axum middleware                    |
| `sera-events`     | lib  | Audit trail, Centrifugo pub/sub, lifecycle events       |
| `sera-docker`     | lib  | Container lifecycle via bollard                         |
| `sera-hooks`      | lib  | In-process hook registry + chain executor               |
| `sera-hitl`       | lib  | HITL approval routing, escalation chains                |
| `sera-workflow`   | lib  | Workflow engine, dreaming config, cron scheduling       |
| `sera-gateway`    | bin  | Main API server + SQ/EQ gateway (axum)                  |
| `sera-runtime`    | bin  | Agent worker binary — runs inside containers            |
| `sera-tui`        | bin  | Terminal UI (ratatui) — replaces Go TUI                 |
| `sera-testing`    | lib  | Test utilities, fixtures, golden tests                  |
| `sera-byoh-agent` | bin  | BYOH agent reference implementation                     |

## Dependency Graph

```
sera-domain (leaf)
  └─ sera-config
  └─ sera-db ← sera-auth
  └─ sera-events ← sera-docker
  └─ sera-core (all above)
  └─ sera-runtime (domain + config only)
  └─ sera-tui (domain + reqwest only)
```

## Development Workflow

1. **Edit code** — rust-analyzer provides real-time diagnostics
2. **`cargo check --workspace`** — fast incremental validation (no codegen)
3. **`cargo test -p <crate>`** — run tests for the crate you changed
4. **`cargo clippy --workspace`** — lint check before committing

## Build Performance (Windows)

- Use `lld-link` for faster link times: add to `.cargo/config.toml`:
  ```toml
  [target.x86_64-pc-windows-msvc]
  linker = "lld-link"
  ```
- `cargo check` skips codegen — always prefer it over `cargo build` during dev
- Incremental compilation is on by default in dev profile
- First build downloads + compiles all deps (~30s); subsequent checks are ~1-3s

## Contract Tests

For verifying Rust↔TypeScript compatibility:

- Golden YAML manifests in `contracts/manifests/`
- Route response comparisons in `contracts/routes.json`
- Run: `cargo test -p sera-testing`

## Integration Tests

Require `DATABASE_URL` pointing to a PostgreSQL instance with the SERA schema:

```bash
DATABASE_URL=postgres://sera:sera@localhost:5432/sera cargo test --workspace --features integration
```

## Learnings

- **sqlx compile-time checks need `DATABASE_URL`**: Set it in `.env` or as env var. Without it, `sqlx::query!` macros won't compile. Use `sqlx::query()` (runtime) during early development if no DB is available.
- **bollard on Windows uses named pipes**: Docker client connects via `//./pipe/docker_engine` on Windows, not `/var/run/docker.sock`. The bollard crate handles this automatically.
- **serde_yaml 0.9 is deprecated but stable**: The crate works fine; the maintainer recommends alternatives for new projects, but for SERA's manifest parsing it's adequate.
- **Use `tls-rustls` not `tls-native-tls` for sqlx and reqwest**: WSL2 and minimal Docker images lack `libssl-dev`. Using `rustls-tls` (pure Rust TLS) avoids the system OpenSSL dependency. Set `default-features = false` on reqwest to prevent it pulling in `native-tls`.
- **MVS crate mapping**: The MVS review plan's 8 crates map to the existing workspace: sera-types→sera-domain, sera-config→sera-config (manifest_loader module), sera-errors→distributed thiserror, sera-db→sera-db (sqlite module), sera-memory→sera-domain::memory, sera-tools→sera-runtime::tools::mvs_tools, sera-models→sera-runtime::llm_client, sera-gateway→sera-core (discord module + bin/sera.rs).
- **SQLite via rusqlite (not sqlx)**: MVS uses rusqlite for SQLite — simpler for embedded use. sqlx remains for PostgreSQL in the enterprise path. Both coexist in sera-db.
- **Workspace Cargo.toml had duplicate sera-runtime member**: Fixed — was listed twice causing no error but was incorrect.
- **sera-runtime is bin-only**: To reuse reasoning loop and tools from sera-core's MVS binary, sera-runtime needs a `[lib]` section or the logic must be inlined. Currently bin-only.
- **K8s-style config lives in sera-config::manifest_loader**: Single-file YAML with --- separators. Secret resolution via SERA_SECRET_* env vars. Types in sera-domain::config_manifest.
- **thiserror v2 auto-detects `source` fields**: Any field named `source` in a thiserror enum is treated as `#[source]`, requiring `std::error::Error`. Use `reason` instead for plain String error context.
- **Edition 2024 let-chains**: Collapsible if statements should use `if cond && let Ok(x) = expr { ... }` syntax. Clippy enforces this with `-D warnings`.
- **async-trait for Hook trait**: The in-process Hook trait uses `async_trait` crate. When WASM lands, the WasmHookAdapter will implement the same trait.

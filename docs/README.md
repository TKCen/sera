# SERA Documentation

This directory contains all SERA documentation, split into two subtrees:

## `docs/public/`

Public-facing documentation that is published to the SERA docs site (built with Zensical).

| Path | Contents |
| ---- | -------- |
| `public/index.md` | Landing page |
| `public/architecture/` | Architecture docs, design decisions, TUI specs |
| `public/adr/` | Architecture Decision Records (ADR-0001 … ADR-000N) |
| `public/specs/` | Component specifications (SPEC-*.md) |
| `public/crate-docs/` | Per-crate reference documentation |
| `public/decisions/` | Time-stamped design decision records |
| `public/plugins/` | Plugin authoring guides |
| `public/research/` | Published research notes |
| `public/RUST-MIGRATION-PLAN.md` | Active Rust migration plan |

The docs site is built from `docs/public/` using Zensical (`zensical.toml` at repo root).
CI triggers on `docs/public/**` and `zensical.toml` changes only.

## `docs/internal/`

Working notes, session artifacts, and in-progress planning documents. Versioned in git but **not published** to the public docs site.

| Path | Contents |
| ---- | -------- |
| `internal/sessions/` | Session reports and wave summaries |
| `internal/audits/` | Implementation audits, spec gap inventories, test gap reports |
| `internal/investigations/` | Incident and routing investigations |
| `internal/plans/` | Handoff notes, HANDOFF.md, PHASE-0-PLAN.md, architecture addenda |

## Contributing

To add a new doc:
- Public content (user-facing, architecture, guides) → `docs/public/`
- Working notes, session artifacts, internal tracking → `docs/internal/`

See `zensical.toml` at the repo root for the site navigation configuration.

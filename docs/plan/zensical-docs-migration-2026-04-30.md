# SERA → Zensical Docs Migration Roadmap

**Lane:** claude-burn / docs-zensical
**Date:** 2026-04-30
**Author:** Claude Code (OMC, sonnet/opus)
**Bead:** docs-zensical (parent epic to be filed at closeout — see "Beads to file" section)

---

## 0. TL;DR

The repo currently has a **partially-wired, broken** docs publishing pipeline:

- `legacy/mkdocs.yml` (Material for MkDocs) — points at `tkcen.github.io/sera/`, an old org URL. Not deployed.
- `.github/workflows/docs.yml` — runs `mkdocs build` at the repo root, but no top-level `mkdocs.yml` exists. **The workflow fails on every `docs/**` change today.**
- `docs/` is non-trivial (6 directories, ~30 markdown files including `RUST-MIGRATION-PLAN.md`, ADRs, specs, plan/, plugins/, research/), but `docs/index.md` is a 276-byte placeholder.

The directive "SERA docs should move to https://zensical.org/" has two plausible interpretations:

1. **Framework migration** (most likely): switch the docs SSG from MkDocs Material to **Zensical** (the announced successor to Material for MkDocs, hosted at zensical.org). Domain for the SERA docs site itself is then a separate decision (e.g. `docs.sera.dev`, `sera.zensical.app` if Zensical offers hosting, or a custom subdomain).
2. **Host on zensical.org**: literally publish at `https://zensical.org/sera/` or similar. Implausible — `zensical.org` is the framework's project site, not a hosting service for arbitrary projects.

**Recommended interpretation: (1)**, plus pick a target domain in the kickoff bead. This roadmap assumes (1).

---

## 1. Current state inventory

### 1.1 Repo artifacts

| Path | Status | Notes |
| --- | --- | --- |
| `legacy/mkdocs.yml` | Stale | Material for MkDocs config; references retired `TKCen/sera` repo URL and `tkcen.github.io/sera/` site URL. |
| `.github/workflows/docs.yml` | **Broken** | Runs `mkdocs build` at repo root; expects `./mkdocs.yml` which does not exist. Triggers on `docs/**` and `mkdocs.yml`. Deploys to GitHub Pages. |
| `docs/index.md` | Placeholder | 7 lines, points at `plan/plan.md` and `RUST-MIGRATION-PLAN.md`. |
| `docs/RUST-MIGRATION-PLAN.md` | Live, 29.6 KB | Authoritative Rust migration plan. |
| `docs/plan/` | Live, ~30 docs | `ARCHITECTURE-2.0.md` (32 KB), `IMPL-AUDIT.md` (61.7 KB), `PHASE-0-PLAN.md` (154 KB), addenda, session reports, specs, decisions, crate-docs. |
| `docs/adr/` | Live, 4 ADRs | `ADR-0001` … `ADR-0004`. |
| `docs/plugins/` | Live | Plugin reference docs. |
| `docs/research/` | Live | Research notes. |
| `docs/{competitive-analysis,sera-eval-design,sera-eval-first-run,sera-stack-audit,signal-system-design,skill-format}.md` | Live | Mixed: design notes, evals, comparative analysis. |
| `docs/openapi.yaml` (per `CLAUDE.md`) | Live | API spec. (Not visible in `ls docs/` because `find -maxdepth 2` shows it; verified via the table in `CLAUDE.md`.) |
| `README.md` (root) | Live, 26.7 KB | Architecture diagram inline; **does not link to any deployed docs site**. |
| `CNAME`, `.pages` | Absent | No GitHub Pages custom-domain config in repo. |
| `docs.sera*`, `tkcen.github.io`, `sera.dev` mentions in `docs/`/`README.md` | None | Nothing else hard-codes a docs URL. |

### 1.2 What this means

- **Public docs are not currently published anywhere.** The deploy job exists but cannot succeed.
- **The corpus is migration-ready in shape**: it's all Markdown, organized by domain (plan/specs/adr/research), with one root index. MkDocs Material → Zensical should be a near-drop-in for the source files; nav/config rewrites are the work.
- **Audience is mixed.** `docs/` mixes (a) public-facing architecture/README-style content with (b) internal session reports, gap audits, and migration tracking artifacts. Some of these (`SESSION-25-REPORT.md`, `IMPL-AUDIT.md`, `discord-routing-investigation*.md`) are working notes, not public docs. **The migration must split public vs. internal.**

---

## 2. Target structure

Goal: a published SERA docs site, built with **Zensical**, hosted at a domain owned by the SERA project (default assumption: **`docs.sera.dev`** — confirm with maintainer; this is a placeholder until the bead resolves).

### 2.1 Site IA (proposed)

```
SERA Docs
├── /                          → "What is SERA" + quickstart entry points
├── /quickstart/               → 5-minute local run
├── /architecture/             → cleaned-up subset of docs/plan/ARCHITECTURE-2.0.md
│   ├── overview
│   ├── gateway (MES)
│   ├── runtime (embedded library)
│   ├── memory ladder
│   ├── hooks & lifecycle
│   └── sandboxing
├── /reference/
│   ├── api/                   → from docs/openapi.yaml (rendered)
│   ├── crates/                → from docs/plan/crate-docs/
│   └── cli/                   → sera CLI surface
├── /guides/
│   ├── manifests
│   ├── memory backends
│   ├── plugins                → from docs/plugins/
│   └── operations
├── /decisions/                → from docs/adr/ (ADR-0001 … ADR-N, public ADRs only)
├── /roadmap/                  → trimmed RUST-MIGRATION-PLAN.md + current status
└── /research/                 → from docs/research/ (curated subset only)
```

**Excluded from public site (kept in repo as `docs/internal/` or moved to `artifacts/`):**

- `SESSION-*-REPORT.md`
- `IMPL-AUDIT.md`, `SPEC-GAPS-INVENTORY.md`, `gateway-stubs-classification.md`
- `discord-routing-investigation*.md`
- `code-introspection-audit-2026-04-16.md`
- `mvs-review-plan.md`, `CLAWHIP-LANES-PLAN.md`
- `HANDOFF.md`, `IMPLEMENTATION-TRACKER.md`
- ARCHITECTURE-ADDENDUM-* (until consolidated into the main architecture doc)

These are working artifacts; publishing them gives outsiders a misleadingly chaotic view of the project state.

### 2.2 Source layout

Two viable layouts:

**A. In-repo `docs/` stays the source of truth (recommended):**

```
docs/
├── public/            # Zensical reads from here
│   ├── index.md
│   ├── architecture/
│   ├── reference/
│   ├── guides/
│   ├── decisions/
│   ├── roadmap/
│   └── research/
├── internal/          # working notes, kept versioned, not published
│   ├── sessions/
│   ├── audits/
│   └── investigations/
└── (zensical config at repo root: zensical.yml)
```

**B. Separate `site/` directory** (as MkDocs builds today): keep `docs/` as raw notes, copy/curate into `site/` for publishing. More overhead.

**Choose (A).** It matches what the existing `docs.yml` workflow expects (single source dir) and minimizes the second-source-of-truth problem.

---

## 3. DNS & publishing assumptions

These are **assumptions** — the kickoff bead must confirm with the project owner before the first PR that touches DNS.

| Assumption | Default | Confirm with |
| --- | --- | --- |
| Publishing target domain | `docs.sera.dev` | Project owner (currently unowned/unverified) |
| Domain registrar / DNS provider | Unknown — confirm | Project owner |
| Hosting | GitHub Pages (matches existing `docs.yml`) — Zensical may also offer first-party hosting; check at kickoff | Project owner + Zensical maturity |
| TLS | Provider-managed (GH Pages auto, or Zensical hosting) | n/a |
| Redirects from old URL | `tkcen.github.io/sera/*` was never live (deploy is broken). No live URL to redirect today. | n/a |
| Repo URL inside config | `https://github.com/<owner>/sera` — confirm whether `TKCen` is the canonical owner or migrated | Project owner |
| Edit URI | `edit/main/docs/public/` | n/a |
| Search | Built-in Zensical search (Material-MkDocs heritage) | n/a |
| Analytics | Opt-out by default | Project owner |

---

## 4. First five PRs (sequenced)

The migration is **five small, reversible PRs**. Each one is independently mergeable and leaves the repo in a working state.

### PR 1 — **This PR.** Roadmap + beads + decision capture

**Scope:**

- `artifacts/reports/claude-burn/docs-zensical-migration-2026-04-30.md` (this file).
- Beads filed for the epic and the next four PRs.

**No source/config changes.** Safe.

**Tests:** none required.

### PR 2 — Triage `docs/` into `public/` and `internal/`

**Scope:**

- `git mv` only. Move the audit/session/investigation files listed in §2.1 under `docs/internal/`. Move the public-eligible files under `docs/public/`.
- Add `docs/README.md` describing the split.
- Update any cross-doc relative links broken by the move (mostly internal `[…](./plan/…)` references — done by `rg -l 'docs/plan/'` + `sed`).

**Tests:** `rg -l '\]\(\.\.?/' docs/` for broken links; manual spot-check on the largest files (`PHASE-0-PLAN.md`, `IMPL-AUDIT.md`).

**Risk:** low. Pure file moves. The broken `docs.yml` workflow stays broken (and is fixed in PR 4).

### PR 3 — Add Zensical config skeleton (no deploy yet)

**Scope:**

- `zensical.yml` at repo root (or whatever the framework's filename convention is at kickoff time — Zensical's docs are the source of truth; this report assumes MkDocs-style config).
- Site name, repo URL, navigation, theme. Mirrors the trimmed `legacy/mkdocs.yml` but points `docs_dir: docs/public`.
- `docs/public/index.md` — proper landing page (replaces the 276-byte placeholder).
- **Do not yet enable the deploy workflow.** Local-build-only.

**Tests:** `pip install zensical && zensical build` (or whatever the CLI is) locally; verify the output `site/` tree.

**Risk:** low. New file; no deploy path touched.

### PR 4 — Replace broken `docs.yml` workflow with Zensical build & GH Pages deploy

**Scope:**

- Rewrite `.github/workflows/docs.yml` to install Zensical and build from `docs/public/`.
- Add `CNAME` file with the chosen domain (default: `docs.sera.dev`).
- Wire up the GitHub Pages environment + `pages.write` permission (already in current workflow — keep).
- Update `paths:` filter to `docs/public/**` and `zensical.yml` so internal docs changes don't trigger deploy.

**Tests:**

- CI: workflow run on the PR branch (use `workflow_dispatch` to validate without merging).
- Manual: confirm `docs.sera.dev` (or chosen domain) DNS A/CNAME resolves to GH Pages before merging — coordinate with project owner.

**Risk:** medium. First time the deploy is real. Roll back by reverting the workflow.

### PR 5 — README + repo metadata

**Scope:**

- Add a "Documentation" section to `README.md` linking to the new docs site.
- Update `legacy/mkdocs.yml` repo URL to whatever the new canonical owner is, OR delete `legacy/mkdocs.yml` if it's no longer reference-worthy (file a small bead first; deleting legacy assets needs a check).
- Add a `docs/CONTRIBUTING.md` describing the public-vs-internal split and the publishing pipeline.

**Tests:** none beyond CI.

**Risk:** very low.

---

## 5. Open questions (must resolve before PR 4)

1. **Target domain.** `docs.sera.dev` is a guess. Owner confirms or picks alternative.
2. **DNS control.** Who owns the registrar account? Is the project owner ready to add a CNAME?
3. **Zensical maturity at the time of PR 3.** As of 2026-04-30, Zensical is a relatively new framework; check its CLI is stable (`zensical build` produces deterministic output, supports the Material-MkDocs extensions we use today: admonitions, mermaid, pymdownx.*).
4. **`legacy/mkdocs.yml` retirement.** Keep as historical reference, or delete? (Recommend delete in PR 5; the new `zensical.yml` supersedes it cleanly.)
5. **Public vs. internal cutoff.** §2.1 lists the obvious internal files; project owner reviews and approves.
6. **Owner of the `TKCen/sera` GitHub URL.** Is that still the canonical repo, or did it migrate? `legacy/mkdocs.yml` still says `TKCen/sera`. PR 5 fixes whichever way it lands.

---

## 6. Risks & mitigations

| Risk | Mitigation |
| --- | --- |
| Zensical config format changes between announcement and 1.0 | Don't enable deploy in PR 3. Pin Zensical version. Treat `zensical.yml` as draft until PR 4. |
| Internal docs accidentally published | PR 2 establishes `public/` vs `internal/` split. PR 4's `docs_dir: docs/public` enforces it at build time. CI lints (later bead) can `grep -L "internal-only"` etc. |
| Cross-doc links break during PR 2 move | Run `rg '\]\(' docs/` before and after; diff. Fix with `sed`/manual review. |
| DNS lag after CNAME change | Schedule PR 4 deploy during a low-traffic window; have owner pre-stage the CNAME at the registrar 24 hours before merge. |
| GH Pages can't host the chosen domain (e.g. apex domain limitations) | Use a subdomain (`docs.*`), not apex. Already the default. |
| Existing `docs.yml` workflow burns CI budget on every docs change today | Already broken. PR 4 fixes it; until then, the failure is silent (just red checks on `docs/**` PRs). Optional: add a `docs/internal/` ignore in the trigger filter as a 1-line follow-up bead. |

---

## 7. Beads to file

The kickoff bead (parent epic) plus follow-ups for PRs 2–5. Naming follows the SERA convention: native `task` type, labels `issue,<severity>,docs`. Severities below are placeholder priorities — the kickoff bead can re-tier them.

| Bead | Type | Severity | Title |
| --- | --- | --- | --- |
| Epic (parent) | `task` | P2 | docs(epic): migrate SERA docs to Zensical and publish at custom domain |
| PR 2 | `task` | P2 | docs: split docs/ into public/ and internal/ subtrees |
| PR 3 | `task` | P2 | docs: add Zensical config skeleton (no deploy) |
| PR 4 | `task` | P2 | docs(ci): replace broken docs.yml with Zensical build + GH Pages deploy |
| PR 5 | `task` | P3 | docs: link new docs site from README and retire legacy/mkdocs.yml |
| Side-quest | `bug` | P3 | ci: docs.yml fails on every docs/** change (no top-level mkdocs.yml) — fix or disable until PR 4 lands |

These will be filed in the bead store at closeout (subject to bd lock contention; will retry with backoff and note in the lane finding if filing fails).

---

## 8. Out of scope for this lane

- **Docs content rewrites.** The triage in PR 2 is `git mv` only. Rewriting `ARCHITECTURE-2.0.md` for a public audience is its own multi-PR effort.
- **Internationalization.** Not requested.
- **Versioned docs (mike-style).** Single `latest` branch is fine for now; revisit when SERA hits 1.0.
- **Search analytics, comments, contribution UI.** Defaults only.

---

## 9. Final answer to the directive

> "SERA docs should move to https://zensical.org/"

**Smallest path:** five small PRs as sequenced above. PR 1 is this report and the bead set. PRs 2–4 land the migration. PR 5 closes the loop with README + cleanup. No DNS or deploy changes happen until PR 4, which is gated on the project owner picking a domain and pre-staging the CNAME.

**This PR is a docs-only, deploy-neutral preparatory PR.** It changes nothing about the build, the runtime, or any user-facing surface. Reviewing it should take under five minutes.

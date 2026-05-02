# SERA 2.0 Stack Audit — `sera-skills` & Semantic Memory

**Audit date:** 2026-04-19 · **Branch:** `ulw/2026-04-18-rust-parity-push`
**Scope:** what the code actually does today vs. what the docs promise.

> There is **no `sera-memory` crate**. Semantic memory lives in `sera-types` (trait)
> + `sera-db::{sqlite_memory_store, pgvector_store}` (implementations)
> + `sera-testing::semantic_memory` (fake). Workspace manifest at `rust/Cargo.toml:24–26`
> and `rust/CLAUDE.md:62`. This audit treats those four files as "sera-memory."

---

## Part 1 — `sera-skills`

### 1.1 Does it learn from use?

**No.** There is no closed-loop learning / refinement path inside the crate or anywhere it is consumed.

What looks like a learning surface and isn't:

- **`knowledge_activity_log.rs`** — a chronological ring buffer of `KnowledgeOp`
  (`Store | Update | Delete | Synthesize | Lint`). It is a pure data structure:
  `append()` (`rust/crates/sera-skills/src/knowledge_activity_log.rs:266`), `recent()`
  (:276), `query()` (:282). Nothing writes `Synthesize` or `Lint` entries from a
  real-world code path — the type is `Serialize/Deserialize` only, and `grep`
  for callers outside the crate returns nothing.
- **`knowledge_lint.rs` / `knowledge_schema.rs`** — validators that can run
  *against* knowledge pages, but they are not triggered by any scheduler,
  workflow, or hook.
- **`self_patch.rs`** — see §1.3. Pipeline exists, zero production callers.

No telemetry is collected on skill use. `ParsedSkillMarkdown` carries a
`body_raw` field but nothing downstream records which skills fired, what their
outputs looked like, or whether they succeeded. There is no counter, no
"effective/ineffective" flag, no ranking update.

**Verdict:** The crate is a *content-addressable skill-loading library*, not a
learning system.

### 1.2 How are skills defined, loaded, triggered?

**Defined** — three coexisting formats:

1. `SKILL.md` single-file markdown with YAML frontmatter (`md_loader.rs:58–74`).
   Required: `name`, `description`. Optional: `inputs`, `tier` (default `1`,
   `md_loader.rs:48`). Unknown keys are warn-logged and dropped (`md_loader.rs:29`).
2. Strict AgentSkills format parsed by `markdown.rs` (keyword `triggers: [review, audit]`
   list — `markdown.rs:399`, example frontmatter).
3. Legacy two-file JSON + YAML pack (`loader.rs:166` `FileSystemSkillPack` —
   `.json` definition + `.yaml` config). Kept alive via
   `SkillLoader::with_legacy_fallback` (`loader.rs:49–54`).

**Loaded** — three entry points:

- `SkillLoader` (`loader.rs:32`) walks a priority-ordered list of filesystem
  roots, **first-path-wins** on name collision (`loader.rs:58–87`, covered by
  `multi_path_first_wins_on_collision` at :283).
- `SkillResolver` (`resolver.rs:30`) fans out across `Vec<Arc<dyn SkillSource>>`.
  Stock sources: `FileSystemSource`, `PluginSource`, `RegistrySource` (OCI-pull)
  registered via `SkillResolverBuilder` (`resolver.rs:126–145`). `resolve_batch`
  tries sources in order and falls through on `SkillNotFound | Unavailable |
  Unsupported` (`resolver.rs:65–71`).
- `SkillLockFile` (`lockfile.rs`) pins resolved source + version for
  reproducible loads.

**Triggered** — trigger modeling is there; runtime wiring is **not**:

- `SkillTrigger` enum in `sera-types/src/skill.rs:164–171` has three variants:
  `Manual`, `Event(String)`, `Always`.
- Markdown parser derives trigger from frontmatter: `Always` if a `triggers:`
  list exists, else `Manual` (`markdown.rs:360–364`).
- **No dispatcher reads this enum.** Global grep confirms:
  - `sera-runtime` — `grep SkillPack|skill_trigger|execute_skill` → zero hits.
  - `sera-gateway` — `grep skill|Skill` in `bin/sera.rs` → zero hits.
  - `sera-hooks` — does not depend on `sera-skills`.

So: skills are parseable, loadable, and searchable (`SkillResolver::search`,
`resolver.rs:85–105`), but **nothing in the agent loop looks them up, matches
triggers, or injects their bodies into a prompt**.

### 1.3 Self-patch / self-improve mechanism

Scaffold-only. `self_patch.rs` implements:

- `SkillPatch { skill_id, base_version, patch_kind, payload }` —
  three patch kinds: `UpdateSkillMd`, `AddKnowledgeBlock`, `UpdateMetadata`
  (`self_patch.rs:49–56`).
- `DefaultSelfPatchValidator` — version alignment
  (`self_patch.rs:173–178`), YAML frontmatter syntax check (`:230–250`),
  64 KB budget (`MAX_SKILL_MD_BYTES` at `:27`, enforced at `:186–188`),
  duplicate knowledge filename check (`:198–200`).
- `FsSelfPatchApplier` (`:302–381`) — temp-copy-then-atomic-rename write
  strategy (`:336–348`). The "atomic rename" comment at `:343–346`
  acknowledges a TOCTOU window: *"no concurrent writers assumed."*
- `InMemorySelfPatchApplier` (`:269–287`) for tests.

**Callers of this API (workspace-wide):**
```
rust/crates/sera-skills/src/self_patch.rs      — the implementation
rust/crates/sera-skills/tests/self_patch.rs    — the tests
rust/crates/sera-skills/src/lib.rs             — the module re-export
```
That's it. No gateway route, no runtime hook, no scheduler, no HITL approval
wiring. The module doc (`self_patch.rs:1–19`) calls itself a "scaffold" and
explicitly notes *"No agent-tool wiring is included here — that is a follow-up
layer."*

`sera-meta` (advertised in `rust/CLAUDE.md:63` as "Self-evolution: 3-tier
policy, shadow sessions, rules") has zero imports of `sera_skills::self_patch`
(grep in `sera-meta/` → no matches). The two self-modification systems are
logically separate and never meet.

### 1.4 Gaps vs. Hermes Agent's skill system

I don't have Hermes' code in this workspace (no `docs/competitive-analysis.md`
content surfaced from the search beyond the untracked file reference), so
this is based on the generally-known Hermes design:

| Capability | Hermes | sera-skills |
| --- | --- | --- |
| Skill authoring format | YAML/Python class | `SKILL.md` + 2 legacy formats |
| Discovery | Registry + local | FS + Plugin + OCI + lockfile |
| Trigger matching | Keyword + LLM-routed | Enum defined, **no dispatcher** |
| Effectiveness tracking | Per-skill success/failure counters | None |
| Self-patch / refinement | Driven by scoring feedback | Validator+applier, **no caller** |
| Skill composition | Chained via planner | None — skills are isolated blobs |
| Runtime injection | Auto-inject matched skills into prompt | Not wired |
| Versioning | Semver + migration | Semver + lockfile (good) |

**The critical gap is the same one everywhere:** sera-skills is a library that
no production code path consumes. Loading, searching, and patching primitives
exist; the agent loop that would close the loop (match trigger → inject body →
observe outcome → score → patch) does not.

---

## Part 2 — Semantic memory (sera-db + sera-types)

### 2.1 The `SemanticMemoryStore` trait

Defined at `rust/crates/sera-types/src/semantic_memory.rs:417–548`.

Required methods: `put`, `query`, `delete`, `evict`, `stats`.
Defaulted methods: `promote`, `touch`, `maintenance`, `query_hierarchical`
(:520, :534, :545, :439).

Error policy is explicit and enforced: `// Backends MUST fail loudly [...]
There are no silent fallbacks or default vectors` (`semantic_memory.rs:22–28`).
Tested in `sqlite_memory_store.rs:1340–1379` (`embedding_service_propagates_errors_on_put`).

Multi-tenant isolation: contract requires filtering on `agent_id` first
(`semantic_memory.rs:13–15`). SQLite backend enforces it via explicit
`WHERE e.agent_id = ?` in every probe (`sqlite_memory_store.rs:490–507`,
`575–591`). Test at `sqlite_memory_store.rs:1214–1256`.

### 2.2 Implementations

Three, all real:

| Backend | Crate / file | Storage | Recall |
| --- | --- | --- | --- |
| `SqliteMemoryStore` | `sera-db/src/sqlite_memory_store.rs:152` | `rusqlite` + FTS5 + optional `sqlite-vec` | BM25, vector, or RRF fusion |
| `PgVectorStore` | `sera-db/src/pgvector_store.rs:94` | `sqlx` → Postgres + `vector` ext | Cosine only (`1 - (embedding <=> $1)`, :408) |
| `InMemorySemanticStore` | `sera-testing/src/semantic_memory.rs:21` | `HashMap<MemoryId, SemanticEntry>` | Linear cosine scan |

### 2.3 Tier ladder — is it real?

The **three tiers are real *as alternative backends*, not as an automatic ladder.**
The README contract (`README.md:65`) advertises plugin tier; the code does not
have any tier migration, promotion across tiers, or hot-swap logic.

**Tier 1 — SQLite/FTS5 (keyword-only) and Tier 1.5 — SQLite/FTS5 + sqlite-vec:**
Real and well-built.
- Schema created idempotently at construction (`sqlite_memory_store.rs:236–277`),
  FTS5 self-contained (NOT contentless, deliberately — `:31`) so UPDATE/DELETE
  don't hit the "cannot DELETE from contentless fts5 table" footgun.
- Extension autoload behind a `std::sync::Once` (`:107–135`).
- If sqlite-vec is missing: logs once, `vec_available = false`, BM25 keeps
  working (`:207–213`). Degraded mode is tested (`:1409–1446`).
- RRF fusion (`RRF_K = 60` at `:94`, `fuse_rrf` at `:621–655`) when both signals
  are present. BM25-only fallback at `:769–773`.

**Tier 2 — Postgres + pgvector:**
Real. `initialize()` at `pgvector_store.rs:148–232` issues `CREATE EXTENSION
IF NOT EXISTS vector`, creates the table at configured dimensions
(`DEFAULT_SEMANTIC_DIMENSIONS = 1536` at `:90`), builds an `ivfflat` index.
`extension_available()` (:134) lets callers probe before committing.
Additive column migration for GH#140 scope columns at `:182–196`.
One rough edge: `query()` at `:374–383` **requires** a pre-computed
`query_embedding` and will not embed on read; SQLite does both.

**Tier 3 — plugin:**
**Aspirational.** Evidence:
- `docs/plugins/memory.md:1–25` documents the contract and names user-targets
  (mem0 HTTP, Hindsight, HTTP RAG, Milvus/Weaviate/Pinecone, Neo4j).
- `sera-plugins` has a `PluginCapability::MemoryBackend` enum variant
  (`rust/crates/sera-plugins/src/types.rs:11`) — but no code anywhere
  constructs or dispatches a `SemanticMemoryStore` through the plugin
  registry. Global grep for `MemoryBackend` capability use in non-test
  code returns only the variant definition and its `Display` impl.
- `rust/docs/plan/HYBRID-RETRIEVAL.md:103` sketches a `HybridRetrieval`
  struct with a `Box<dyn MemoryBackend>` — a *planning doc*, not code.

In short: the trait is stable, two backends plug into it, and a third tier is
advertised but not buildable today.

### 2.4 Tier migration path

**Not implemented. Not even sketched in code.** There is:

- no "promote from SQLite to pgvector" pipeline
- no shared migration format between the two schemas (SQLite serializes `tier`
  as JSON TEXT at `sqlite_memory_store.rs:244`; pgvector stores it as JSONB at
  `pgvector_store.rs:164`)
- no dual-write adapter that would let a single deployment span both while
  cutting over
- no automatic eviction-then-reinsert across backends

The gateway binary (`rust/crates/sera-gateway/src/bin/sera.rs:1979–1988`)
makes this explicit in a TODO comment: *"SemanticMemoryStore (Tier-2 recall)
backend selection. sera-vzce left this as a TODO so the MVS boot path stays
minimal"* — the binary picks **one** store at startup from config and that's
it.

What *is* real is the `promote(id)` method (`semantic_memory.rs:520` trait
defaulted; `pgvector_store.rs:533–546` + `sqlite_memory_store.rs:963–979`
SQL-backed overrides). That's **row-level** promotion (a row survives
eviction), not **tier-level** migration.

### 2.5 What's real vs. stub — summary

| Feature | Status | Evidence |
| --- | --- | --- |
| `SemanticMemoryStore` trait | Real | `sera-types/src/semantic_memory.rs:417` |
| SQLite FTS5 keyword recall | Real | `sqlite_memory_store.rs:474–536` |
| sqlite-vec vector recall | Real, feature-gated | `sqlite_memory_store.rs:109–135`, `555–619` |
| RRF hybrid fusion | Real | `sqlite_memory_store.rs:621–655` |
| pgvector cosine recall | Real | `pgvector_store.rs:374–463` |
| Scope hierarchy (GH#140) | Real in trait + pgvector; defaulted impl in SQLite | `semantic_memory.rs:439–499`, `pgvector_store.rs:391–431` |
| `query_hierarchical` damping walk | Real (trait default impl) | `semantic_memory.rs:444–499` |
| Multi-tenant agent_id isolation | Real + tested | `sqlite_memory_store.rs:1214–1256` |
| `evict` (TTL + cap, promoted-exempt) | Real | both backends; tests at `sqlite_memory_store.rs:1270–1326` |
| `promote` (row-level pin) | Real | SQL overrides in both |
| `maintenance` (FTS optimize / REINDEX) | Real | `sqlite_memory_store.rs:1049–1061`, `pgvector_store.rs:562–570` |
| Plugin-tier `SemanticMemoryStore` | **Stub** — doc contract only | `docs/plugins/memory.md` vs. zero callers of `PluginCapability::MemoryBackend` |
| Tier migration (SQLite ↔ pgvector) | **Not implemented** | `sera-gateway/src/bin/sera.rs:1979–1988` TODO |
| Auto-tier-selection at boot | **Partial** — operator picks via config; no capability-probe-and-fallback | same TODO as above |

---

## Bottom line

- **`sera-skills`** is a solid content-management layer with a
  trigger type, self-patch pipeline, and multi-source resolver — none of which
  are wired into the running agent. Calling it a "skill system" is aspirational;
  today it's a loader.
- **Semantic memory** is the healthier of the two: the trait contract is
  honest, two real backends implement it with proper isolation and honest
  failure modes, and the hybrid-recall path in the SQLite backend is genuinely
  thoughtful. The plugin tier is doc-only and the cross-tier migration path
  does not exist.

**If a user reads the README and expects "SQLite tier ladders up to pgvector
ladders up to plugins,"** the first two rungs are real (as swappable choices),
the third rung is a promise, and there is no ladder — just three independent
backends.

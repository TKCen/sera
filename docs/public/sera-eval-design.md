# SERA 2.0 Evaluation Harness — Design

> Status: design + stub PR (`feat/sera-eval-design`)
> Owner: sera-eval crate (`rust/crates/sera-eval`)
> Related: `docs/plan/ARCHITECTURE-2.0.md`, `rust/CLAUDE.md`

## 1. Hypothesis and success criteria

### 1.1 Core hypothesis

> **H0 (null):** A local `qwen/qwen3.6-35b-a3b` served through LM Studio, when driven
> by the full SERA harness (context engine + skills + memory + tools + hooks),
> performs **no better** than the same model served raw on agent-style tasks.
>
> **H1 (alternative):** The SERA harness lifts `qwen3.6-35b-a3b` to **match or
> beat** GPT-4-class frontier models (GPT-4o, Claude Sonnet 4.6) on the same
> tasks, on at least one of the primary metrics, at a statistically meaningful
> margin.

We reject H0 in favour of H1 when, across a stable task suite:

| # | Criterion | Threshold |
|---|-----------|-----------|
| S1 | **Task success rate** of `qwen3.6-35b-a3b + full SERA harness` is ≥ the median success rate of `{gpt-4o, claude-sonnet-4-6}` measured raw (no SERA harness) on the same suite. | ≥ parity |
| S2 | The lift between `qwen3.6-35b-a3b raw` and `qwen3.6-35b-a3b + full SERA` is statistically significant. | McNemar paired test, p < 0.05, N ≥ 50 tasks |
| S3 | Context efficiency (solved tasks per 1k tokens) for `qwen3.6-35b-a3b + full SERA` is within 1.5× of the best frontier baseline. | tokens/success ratio ≤ 1.5× best |
| S4 | P50 end-to-end latency for `qwen3.6-35b-a3b + full SERA` stays under 2× the fastest raw frontier baseline on tasks solved by both. | P50 latency ratio ≤ 2.0× |

S1 is the **primary** success criterion. S2–S4 are gating guardrails: if lift is
not statistically meaningful (S2), or the harness pays for quality with a
runaway context/latency bill (S3/S4), we cannot claim the win.

### 1.2 Non-goals

- Beating every frontier model on every suite. We target the **median** of
  `{gpt-4o, claude-sonnet-4-6}`, not the best of either.
- Training or fine-tuning. The harness evaluates the *inference-time* stack
  only. Any fine-tune is out of scope for this epic.
- Human preference evals (LMSYS-style). We measure objective task success,
  not style/voice.

## 2. Metric definitions

Each `TaskResult` records the following metrics. A `MetricSet` is the full
numeric vector recorded for one (task × harness × model) triple.

| Metric | Symbol | Definition | Unit |
|--------|--------|------------|------|
| Task success rate | `success_rate` | Fraction of tasks whose `verdict == Pass`. Averaged over the suite. | % |
| Tool-use correctness | `tool_correctness` | Of all tool calls made, the fraction that (a) parsed as valid JSON against the tool schema, (b) did not raise an avoidable error (permission denied, 4xx), and (c) were *required* by the task. Invalid tool calls, redundant repeats, and hallucinated tool names all count against. | % |
| Context efficiency | `tokens_per_success` | `sum(prompt_tokens + completion_tokens) / count(Pass)`. Lower is better. Reported only when `count(Pass) > 0`; suites with zero passes log `null`. | tokens |
| Turn count | `turns_to_solution` | Number of assistant turns from first user message to final `Pass`/`Fail` verdict. Turns after `Fail` do not count. | int |
| Wall-clock latency | `latency_ms_p50`, `latency_ms_p95` | Time from task start to terminal verdict. Reported as per-suite P50 and P95. Frontier API timeouts count toward latency. | ms |
| Memory retrieval P@k | `memory_precision_at_k` | For tasks with a **gold memory segment ID set** `G`, let `R` be the top-k retrieved segments the harness injected into context. `P@k = \|R ∩ G\| / k`. Reported with k ∈ {1, 3, 5}. Only computed on the `+memory` and `+full` isolation slices. | fraction |
| Cost (optional) | `cost_usd` | Frontier API billed cost. Local (LM Studio) models report `0.0`. | USD |

**Aggregation rules.** Suite-level numbers are reported as `mean ± stderr` over
tasks. `tokens_per_success` is a ratio, so we also report the bootstrap 95% CI
to avoid divide-by-small-N artefacts.

**Determinism.** Every task runs with `temperature=0` and a fixed seed per
task. Non-deterministic frontier models (OpenAI, Anthropic) still vary run to
run — we record `n_samples` per task (default 1, configurable up to 3) and
report majority-vote verdict to cap the variance.

## 3. Benchmark suites

The harness supports three suite families behind the `BenchmarkSuite` trait:

### 3.1 SWE-Bench Lite

- Subset of SWE-Bench focused on smaller, self-contained patches.
- Task = a GitHub issue + repo snapshot; success = generated patch passes the
  project's test suite.
- Each task pins a container image (included in the task YAML).
- Runs inside a sandboxed Docker container via `sera-tools::sandbox` — keeps
  the agent's filesystem access confined to the repo snapshot.
- External source: `princeton-nlp/SWE-bench_Lite` (HuggingFace). The adapter
  downloads + caches under `$SERA_EVAL_CACHE/swe-bench-lite/`. Download is
  opt-in via `sera eval suites pull swe-bench-lite`.

### 3.2 TAU-bench

- Tool-use / multi-turn conversation benchmark (Sierra Research, 2024).
- Task = a customer-service scenario with a tool schema and a gold trajectory.
- Success = final database state matches the expected diff.
- Covers **retail** and **airline** domains; both are supported, retail is
  the default.
- External source: `sierra-research/tau-bench` (GitHub). Adapter shells out to
  the reference harness for grading to avoid re-implementing the grader.

### 3.3 SERA internal corpus

- Curated tasks from the beads issue tracker + hand-written scenarios that
  exercise SERA-specific surfaces (memory recall across sessions, skill
  invocation, HITL approval, egress allowlist, sandbox boundaries).
- Task definitions live in `rust/crates/sera-eval/tasks/*.yaml` and ship
  with the crate.
- The sample 5–10 tasks in this PR are all internal — no external downloads
  required to run `cargo test -p sera-eval`.

### 3.4 Future suites (out of scope for this PR)

- GAIA (general-assistant), AgentBench, WebArena, OSWorld. The `BenchmarkSuite`
  trait is intentionally model-agnostic so any of these can be added later
  without schema changes.

## 4. Isolation matrix

The experiment grid is:

```
models     = { qwen3.6-35b-a3b, gpt-4o, claude-sonnet-4-6 }
harnesses  = { raw, +context, +skills, +memory, +full }
suites     = { swe-bench-lite, tau-bench-retail, sera-internal }
```

Each of the `3 × 5 × 3 = 45` cells produces one `MetricSet` per task. Not every
cell is equally interesting — priority order:

| Priority | Cells | Reason |
|----------|-------|--------|
| P0 | `qwen3.6-35b-a3b × {raw, +full}` across all suites | The headline A/B — SERA harness lift on the target model. Needed for S1/S2. |
| P1 | `{gpt-4o, claude-sonnet-4-6} × raw` across all suites | Frontier baseline we need to beat. Needed for S1. |
| P2 | `qwen3.6-35b-a3b × {+context, +skills, +memory}` | Ablation — which component contributes which fraction of the lift. Needed to inform follow-up work. |
| P3 | Frontier models × `+full` | Confirms the harness isn't actively *hurting* frontier models (a silent failure mode — if it is, the design is wrong). |

"Raw" means: OpenAI-style `chat/completions` with only the user prompt, no
system prompt beyond `"You are a helpful assistant."`, no tool definitions
beyond what the task itself specifies. This is the "what if you just asked
the model" baseline.

`+context` adds the ContextEngine (condensers + transcript compression).
`+skills` adds skill-pack autoloading. `+memory` adds SQLite-FTS5 memory +
optional pgvector. `+full` is everything + hooks + HITL wiring.

## 5. Task definition format

Tasks use YAML frontmatter + markdown body, mirroring the SWE-Bench style and
our `sera-skills` markdown format. Parsing is done by a small adapter in
`sera-eval::task_def` (stub in this PR; real loader lands when we wire the
runner).

```markdown
---
id: sera-internal-0001
title: Recall user's preferred programming language across sessions
suite: sera-internal
version: 1
tags: [memory, cross-session]
setup:
  memory_seed:
    - role: user
      content: "I prefer Rust for systems programming."
  skills: []
  sandbox_tier: 1
input:
  prompt: "What language should I use for the new CLI you're helping me with?"
expected:
  # Either a literal string match, a regex, or a grading rubric reference.
  assertions:
    - kind: contains_any
      values: ["Rust", "rust"]
    - kind: not_contains
      values: ["Python", "Go", "TypeScript"]
gold_memory_segment_ids:
  - preference.language.systems
budget:
  max_turns: 3
  max_tokens: 2000
  max_wall_seconds: 60
---

## Rationale

Exercises Tier-1 (SQLite FTS5) memory recall. The model must retrieve the
seeded preference and apply it to a question in a new session. A model that
ignores memory will guess a generic language and fail the assertions.
```

**Assertion kinds (v1):**

| Kind | Semantics |
|------|-----------|
| `contains_any` | Final response contains at least one of `values`. |
| `contains_all` | Final response contains all of `values`. |
| `not_contains` | Final response contains none of `values`. |
| `regex` | `values[0]` matches the final response. |
| `tool_called` | A tool with `name == values[0]` was invoked. |
| `file_written` | Sandbox has a file at `values[0]` matching `values[1]` (optional regex). |
| `patch_applies` | SWE-Bench-style: the generated patch applies and the project tests pass. |
| `external_grader` | Shells out to an external grader (TAU-bench); `values[0]` is the grader command. |

The `external_grader` escape hatch is how we integrate TAU-bench without
re-implementing its grader.

## 6. Results storage schema

Results land in a dedicated SQLite database, `sera-eval.db`, alongside the
other SQLite artefacts the project emits. The schema mirrors the
`sera-db::sqlite` conventions (TEXT ids, created_at TEXT in RFC3339, JSON
blobs for bag-of-metrics).

```sql
CREATE TABLE IF NOT EXISTS eval_runs (
  id                TEXT PRIMARY KEY,          -- run_<ulid>
  suite             TEXT NOT NULL,             -- swe-bench-lite | tau-bench-retail | sera-internal | ...
  model             TEXT NOT NULL,             -- qwen/qwen3.6-35b-a3b | gpt-4o | ...
  harness           TEXT NOT NULL,             -- raw | +context | +skills | +memory | +full
  harness_config    TEXT NOT NULL,             -- JSON: exact config used (condenser set, tier cap, ...)
  started_at        TEXT NOT NULL,
  finished_at       TEXT,                      -- null => run in progress
  git_sha           TEXT NOT NULL,
  host              TEXT NOT NULL,             -- hostname, for multi-machine sharding
  notes             TEXT
);
CREATE INDEX IF NOT EXISTS idx_eval_runs_suite ON eval_runs(suite);
CREATE INDEX IF NOT EXISTS idx_eval_runs_model ON eval_runs(model);

CREATE TABLE IF NOT EXISTS eval_tasks (
  id                TEXT PRIMARY KEY,          -- stable task id from the YAML
  suite             TEXT NOT NULL,
  version           INTEGER NOT NULL,
  title             TEXT NOT NULL,
  definition_json   TEXT NOT NULL,             -- parsed task YAML as JSON for reproducibility
  UNIQUE(suite, id, version)
);

CREATE TABLE IF NOT EXISTS eval_task_results (
  id                TEXT PRIMARY KEY,          -- result_<ulid>
  run_id            TEXT NOT NULL REFERENCES eval_runs(id) ON DELETE CASCADE,
  task_id           TEXT NOT NULL,
  verdict           TEXT NOT NULL,             -- pass | fail | error | skipped
  turns             INTEGER NOT NULL,
  prompt_tokens     INTEGER NOT NULL,
  completion_tokens INTEGER NOT NULL,
  latency_ms        INTEGER NOT NULL,
  tool_calls_total  INTEGER NOT NULL,
  tool_calls_valid  INTEGER NOT NULL,
  memory_hits       INTEGER,                   -- nullable: only set when gold segments are defined
  memory_k          INTEGER,                   --           the k used for P@k
  memory_gold       INTEGER,                   --           |G|
  metrics_json      TEXT NOT NULL,             -- full MetricSet as JSON
  transcript_json   TEXT NOT NULL,             -- full turn-by-turn trace
  error_message     TEXT,
  created_at        TEXT NOT NULL
);
CREATE INDEX IF NOT EXISTS idx_eval_task_results_run ON eval_task_results(run_id);
CREATE INDEX IF NOT EXISTS idx_eval_task_results_task ON eval_task_results(task_id);
```

**Why SQLite, not JSON files.** We want cross-run comparisons ("how did
`qwen3.6-35b-a3b + +full` drift between `main@abc123` and `main@def456`?")
without loading a directory of files. SQLite is already the default SERA
local store, so there is zero new infra.

**Why a separate db.** `sera.db` holds operational state (sessions, jobs,
memory) and must stay small and hot. Eval runs grow without bound and benefit
from being archived / rsynced separately.

## 7. CLI surface

The eval runner lives behind `sera eval` (a subcommand added to the existing
`sera-cli` crate in a later PR; this PR only stubs the types). Targeted
surface:

```
sera eval run <suite> --model <model> [--harness <config>] [--tasks <glob>]
                       [--n-samples <int>] [--out <eval.db>] [--run-id <id>]
                       [--report <format>] [--fail-on regression]

sera eval list
sera eval show <run-id>
sera eval report <run-id> [--format md|json|csv] [--compare <run-id>]
sera eval suites pull <suite>
```

**Key flags:**

- `--suite`: `swe-bench-lite`, `tau-bench-retail`, `sera-internal`, or a
  filesystem path to a suite directory.
- `--model`: OpenAI-compat model id, e.g. `qwen/qwen3.6-35b-a3b`,
  `gpt-4o`, `claude-sonnet-4-6`. Provider routing comes from `sera-models`.
- `--harness`: one of `raw | +context | +skills | +memory | +full`, or a
  path to a YAML harness config for custom ablations.
- `--tasks`: glob over task ids so you can run a single scenario.
- `--fail-on regression`: exits non-zero if a task that passed in the
  compared-against run now fails. Intended for CI gating on P0 suites.

**Env knobs:**

- `SERA_EVAL_DB` — path to results SQLite (default `./sera-eval.db`).
- `SERA_EVAL_CACHE` — path for downloaded suites (default
  `$XDG_CACHE_HOME/sera-eval`).
- `SERA_EVAL_LLM_BASE_URL` — OpenAI-compat endpoint. Defaults to
  `http://localhost:1234/v1` (LM Studio).

## 8. Reporting format

Two outputs per run: a machine-readable JSON blob and a human-readable
Markdown table. Both are derived from the same query against `eval_task_results`
so they never drift.

### 8.1 JSON schema (v1)

```json
{
  "run_id": "run_01H...",
  "git_sha": "cc3eaac5",
  "suite": "sera-internal",
  "model": "qwen/qwen3.6-35b-a3b",
  "harness": "+full",
  "n_tasks": 10,
  "metrics": {
    "success_rate": 0.80,
    "tool_correctness": 0.92,
    "tokens_per_success": 1450,
    "turns_to_solution_p50": 3,
    "latency_ms_p50": 4200,
    "latency_ms_p95": 11800,
    "memory_precision_at_3": 0.73,
    "cost_usd": 0.00
  },
  "by_task": [
    { "task_id": "sera-internal-0001", "verdict": "pass", "turns": 2, "metrics": { "...": "..." } }
  ]
}
```

### 8.2 Markdown format

```markdown
## Run `run_01H...` — `qwen/qwen3.6-35b-a3b` @ `+full` on `sera-internal`

| Metric                | Value  |
|-----------------------|--------|
| Success rate          | 80.0%  |
| Tool correctness      | 92.0%  |
| Tokens / success      | 1,450  |
| Turns to solution P50 | 3      |
| Latency P50 / P95     | 4.2s / 11.8s |
| Memory P@3            | 0.73   |

### Tasks
| Id | Verdict | Turns | Notes |
|----|---------|-------|-------|
| sera-internal-0001 | pass | 2 | memory hit |
| sera-internal-0002 | fail | 3 | skipped tool call |
```

A **`report --compare <run-id>`** variant renders a diff table showing
deltas per metric and per task. This is what CI surfaces on a harness
change.

## 9. Running locally against LM Studio

1. **Install LM Studio** and load `qwen/qwen3.6-35b-a3b` (or any
   OpenAI-compat quantisation of it). Enable the local server (default
   `http://localhost:1234/v1`). From inside a Docker Desktop / WSL2
   container, the host address is `http://host.docker.internal:1234/v1`
   (see `rust/CLAUDE.md` learnings).

2. **Point the eval harness at it:**
   ```bash
   export SERA_EVAL_LLM_BASE_URL=http://localhost:1234/v1
   export OPENAI_API_KEY=lm-studio   # placeholder — LM Studio ignores auth
   ```

3. **Run the internal suite (no external downloads):**
   ```bash
   cargo run -p sera-cli -- eval run sera-internal \
     --model qwen/qwen3.6-35b-a3b \
     --harness +full
   ```

4. **Inspect results:**
   ```bash
   cargo run -p sera-cli -- eval report <run-id> --format md
   sqlite3 sera-eval.db 'select verdict, count(*) from eval_task_results group by verdict;'
   ```

5. **Compare against a frontier baseline** (requires an OpenAI or
   Anthropic key in env):
   ```bash
   cargo run -p sera-cli -- eval run sera-internal --model gpt-4o --harness raw
   cargo run -p sera-cli -- eval report <qwen-run-id> --compare <gpt4o-run-id>
   ```

### 9.1 What this PR ships vs. what later PRs add

This PR (design + stub) intentionally lands only the **scaffolding**:

- `rust/crates/sera-eval` crate with the trait, types, SQLite schema, and
  5–10 sample internal task YAMLs.
- `cargo check --workspace` + `cargo test -p sera-eval` pass.
- No runner wiring, no `sera eval` subcommand yet — those land in follow-up
  PRs so the design can be reviewed in isolation.

Follow-ups (filed after this PR merges):

1. Wire `BenchmarkSuite` loaders for `sera-internal` (reads the bundled YAML).
2. Implement the core `EvalRunner` using `sera-models::ModelProvider` and
   the five harness profiles.
3. Add the `sera eval` clap subcommand to `sera-cli`.
4. Add SWE-Bench Lite and TAU-bench adapters behind `--features external-suites`.
5. CI job on a schedule to run P0 cells and gate harness regressions.

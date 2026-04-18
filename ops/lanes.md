# Sera 2.0 — Lane Assignment Rules

Use this to decide which tool/agent lane to route a task to. When in doubt, default to OMC and escalate only if it stalls.

---

## Route to OMC (Claude Code, Max 20x) when:

- Implementing a well-scoped bead with clear spec coverage
- Writing tests
- Scaffolding new crates
- Normal implementation where the approach is clear
- Anything P2/P3 priority

**OMC is the default lane.** Most work should go here.

---

## Route to OMX (Codex, free tier) when:

- Deep diagnosis of a failing test or broken integration
- Architecture decision with real trade-offs
- Precision review of a high-risk surface (auth, tool dispatch, memory)
- The OMC lane produced something that needs a second opinion
- P0/P1 bugs with unclear root cause

**OMX is rate-limited (free tier). Reserve it.** Do not use for routine implementation.

---

## Route to Gemini when:

- Need to read multiple large spec files simultaneously
- Cross-referencing specs against implementation
- Intake for a new epic — understand the full scope before decomposing
- Any task needing > 100K tokens of context

**Gemini has 1000 req/day.** Use for large-context intake and read-heavy tasks, not implementation.

---

## Escalate to Sebastian when:

- Stop condition is ambiguous
- Scope changed materially mid-task
- Two lanes disagree on approach
- Anything touching auth, security boundaries, or the public API contract
- OMX and OMC both stall

---

## Priority → Lane mapping

| Priority | Default lane | Override condition |
|---|---|---|
| P0 | OMX | Root cause unclear |
| P1 | OMX | If approach is clear, use OMC |
| P2 | OMC | — |
| P3 | OMC | — |

---

## Decision flowchart

```
Is the task > 100K tokens of context reading?
  YES → Gemini
  NO ↓

Is the approach clear and spec-covered?
  YES → OMC
  NO ↓

Is it a P0/P1 with unclear root cause, arch decision, or high-risk surface?
  YES → OMX (sparingly)
  NO → OMC (default)

Is stop condition ambiguous or scope changed?
  YES → Escalate to Sebastian
```

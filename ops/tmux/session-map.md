# Sera 2.0 — tmux Session Map

## Lanes

| Lane     | Tool                     | Role                                                                 |
|----------|--------------------------|----------------------------------------------------------------------|
| `hermes` | Coordinator (this agent) | Reads beads, routes work, reads handoffs, drives the build loop      |
| `omc`    | Claude Code + OMC Max    | Planning / scaffolding / normal implementation (20x context)         |
| `omx`    | Codex + OMX free tier    | Deep diagnosis / architecture / precision — use sparingly            |
| `gemini` | Gemini CLI               | Large context reads, intake, cross-referencing specs (1000 req/day)  |
| `review` | Any                      | Validation / review lane                                             |

## Session Naming

```
<lane>-sera-<bead-id>-<slug>
```

**Examples:**
- `omc-sera-yadp-discord-connector`
- `omx-sera-dispatch-gateway-runtime`
- `gemini-sera-spec-review`
- `review-sera-yadp-handoff-check`

## Operating Rules

1. **One session = one responsibility.** Do not expand scope inside a running session. If scope changes, stop, write a handoff, and start a new session.

2. **Sessions write files, not chat.** All output goes to `artifacts/handoffs/<session-name>.md` or `artifacts/reports/`. Never rely on terminal scroll for state.

3. **Always resume from handoff.** When reattaching or continuing a lane's work, read the most recent handoff file before issuing any commands.

4. **OMC rewrites its plan if scope changes.** If a bead's requirements shift mid-task, OMC must update the task file and write a new handoff before continuing — do not silently absorb scope creep.

5. **OMX is free tier — use sparingly.** Only route there for P0/P1 issues, architecture decisions with real trade-offs, or precision review of high-risk surfaces.

6. **Gemini for large context.** Any task needing > 100K tokens of simultaneous context goes to Gemini before decomposition.

7. **Hermes is the only coordinator.** Lanes do not route work to other lanes. Lanes write a handoff and stop. Hermes reads it and decides what's next.

8. **Escalate stop-condition ambiguity immediately.** If a lane cannot determine whether its stop condition is met, it writes a handoff flagging the ambiguity and stops. Hermes escalates to Sebastian if needed.

## Active Sessions

> Update this table as sessions open and close.

| Session Name | Lane | Bead | Status | Handoff |
|---|---|---|---|---|
| _(none yet)_ | | | | |

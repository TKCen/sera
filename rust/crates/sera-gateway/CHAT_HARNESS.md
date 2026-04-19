# /api/chat harness inventory

This document inventories what is — and is not — wired into the SERA
gateway's `POST /api/chat` handler today. It is intentionally short and
maintained alongside the binary (`src/bin/sera.rs`). Pair this with
`docs/sera-eval-first-run.md` when deciding what an eval is allowed to
exercise.

## What IS wired into /api/chat today

- **Authentication**. `Authorization: Bearer <key>` is validated in
  `validate_api_key()` before any handler work. In autonomous mode (no
  key configured) all requests are admitted.
- **Lane queue admission**. Each call enqueues a `DomainEvent::api_message`
  into the shared lane queue so concurrent turns on the same session
  short-circuit with `429 Too Many Requests`. Lane slots are released on
  every exit path.
- **Transcript persistence**. The inbound user message is appended to the
  session transcript, and `persist_tool_events` + an assistant row are
  written after the turn completes.
- **Runtime harness dispatch**. The pre-connected `StdioHarness`
  (sera-runtime) executes the turn. The gateway builds the outgoing
  `messages` vec and never touches LLM or tool I/O directly.
- **Skill dispatch context injection** (NEW). `SkillDispatchEngine` is
  loaded at boot from `$SERA_SKILLS_DIR` (default `./skills`).
  `execute_turn` calls `on_turn(user_message)` and prepends every active
  `context_injection` as a `role: system` message after the persona
  anchor and before transcript replay.
- **Memory replay** (NEW). The Tier-2 `SemanticMemoryStore` built in
  boot is now threaded into `execute_turn` and queried text-only (FTS5
  when the backend is `SqliteMemoryStore`) for the top-3 hits matching
  the current user message. Hits are injected as a single
  `role: system` "Relevant memories:" message. Any recall failure is
  logged and skipped — never fails the turn.
- **HITL pattern gate** (NEW). Before harness dispatch, the inbound
  message is scanned case-insensitively for flagged substrings
  (`rm -rf`, `sudo `, `drop table`, `git push --force`,
  `docker system prune`). A hit releases the lane and returns `403`
  with body `{"error": "hitl_approval_required", "reason": ..., "message": ...}`.
- **Hook chain registration**. `OnApprovalRequest` and related hook
  points are registered in `HookRegistry` at boot and discoverable via
  `/api/hooks`, but the `/api/chat` path itself does not yet run those
  chains — see gaps below.
- **SSE streaming mode**. `stream: true` returns an
  `Sse` response streaming the reply word-by-word, plus a terminal
  `done` event carrying usage.

## FIXMEs / gaps (not wired today)

- **Full ApprovalRouter wiring**. The HITL gate is pattern-based only.
  The `sera-hitl::ApprovalRouter` path — including the
  `approval_token` round-trip, audit-row linkage, and structured
  approval-required envelope — is not yet threaded through
  `chat_handler`. Tracked as a follow-up.
- **Sandbox tier enforcement**. Tier policy files in
  `sandbox-boundaries/` are loaded by config but `/api/chat` does not
  yet consult them per-tool-call; enforcement lives in the runtime
  harness today and is not surfaced as a pre-dispatch check.
- **Egress allowlist enforcement**. Network allow/denylists are read
  from `lists/` at boot but `/api/chat` does not apply them to tool
  invocations at the gateway layer.
- **Token accounting from provider usage**. `UsageInfo` returned from
  `execute_turn` is currently zeroed — the harness does not yet
  bubble up provider-reported prompt/completion token counts.
- **Embedder-backed recall**. Recall runs text-only (FTS5/BM25). The
  hybrid BM25 + vector + RRF path requires an `Arc<dyn EmbeddingService>`
  threaded into `SqliteMemoryStore::open(path, Some(embedder))` at
  boot, which is not yet wired.
- **Role-aware memory seeding**. Memory injection is a single
  `role: system` blob. Per-role seeding (tool/assistant memories
  replayed as their original roles) is deferred.
- **`OnApprovalRequest` / `OnToolCall` hook chains on the chat path**.
  The registry carries them, but `chat_handler` does not yet invoke
  `run_hook_point` the way the Discord `process_message` loop does.

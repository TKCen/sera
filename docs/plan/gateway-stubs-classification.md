# Gateway Stubs Classification — sera-3npy

Audit of every handler in `rust/crates/sera-gateway/src/routes/stubs.rs`
plus the two inline stubs in `embedding.rs` and the no-op methods in
`services/mcp_server_manager.rs`. Target split: MVS-CRITICAL (must ship
for SERA 2.0 MVS), MVS-OPTIONAL (nice-to-have in MVS, not blocking),
POST-MVS (deferred until after MVS cut).

Note: the original bead title mentions "session_persist.rs" — in the
current tree that file is `transcript_persist.rs`, and the TODO about
git2 integration has since been removed. The session_persist P0 item is
no longer present as a git2-coupled stub; transcripts persist via the
existing `SessionPersistenceBackend`. The real stubs live in
`routes/stubs.rs`, `routes/embedding.rs` and `services/mcp_server_manager.rs`.

## Classification table

| Path | Method | Current behavior | Class | Rationale | Owning bead |
|------|--------|------------------|-------|-----------|-------------|
| `/api/pipelines` | POST | `501 not_impl` placeholder | POST-MVS | Pipeline engine is a post-MVS feature (sera-workflow extension). | sera-pipelines |
| `/api/pipelines/:id` | GET | `501 not_impl` placeholder | POST-MVS | Same as above. | sera-pipelines |
| `/api/chat` | POST | `501 not_impl` placeholder | MVS-OPTIONAL | Real impl lives in `routes::chat`; this stub is an older alias. Keep 501 to avoid accidental regression. | sera-chat |
| `/v1/chat/completions` | POST | `501 not_impl` placeholder | MVS-OPTIONAL | Real impl in `routes::openai_compat`; stub here is unused alias. | sera-openai |
| `/api/embedding/config` (stub) | GET | Returns fake JSON `{provider, model, status: "stub"}` | DEAD | Superseded by real `routes::embedding::get_config`. Delete stub callsite — route not registered from here. | — |
| `/api/embedding/status` (stub) | GET | Returns `{status: "unavailable"}` | DEAD | Superseded by real `routes::embedding::get_status`. | — |
| `/api/embedding/embed` | POST | Returns **zero-vector** 1536-dim | MVS-CRITICAL (reclassify to 503) | Silent fake-success is dangerous. Degrade to `503 Service Unavailable` + `tracing::warn!`. Provider impl is post-MVS. | sera-embedding |
| `/api/embedding/batch` | POST | Returns **zero-vectors** | MVS-CRITICAL (reclassify to 503) | Same as above. | sera-embedding |
| `/api/knowledge/circles/:id/history` | GET | Returns `[]` empty array | MVS-OPTIONAL | Circle knowledge history is post-MVS. Change to 501. | sera-knowledge |
| `/api/agents/:id/logs` | GET | Returns `[]` empty array | **MVS-CRITICAL (SHIPPED)** | Trivially backed by audit_trail filtered by actor_id. Implemented. | sera-3npy |
| `/api/agents/:id/subagents` | GET | Returns `[]` empty array | **MVS-CRITICAL (SHIPPED)** | `agent_instances.parent_instance_id` column already present. Implemented. | sera-3npy |
| `/api/agents/pending-updates` | GET | Already DB-backed (JOIN instances/templates) | MVS-CRITICAL (DONE) | Already implemented — no action. | — |
| `/api/tools` | GET | DB-backed — built-ins + skills | MVS-CRITICAL (DONE) | Already implemented. | — |
| `/v1/tools/catalog` | GET | DB-backed — built-ins + skills | MVS-CRITICAL (DONE) | Already implemented. | — |
| `/api/templates` | GET | DB-backed via AgentRepository | MVS-CRITICAL (DONE) | Already implemented. | — |
| `/api/schedules/:id` | GET | DB-backed via schedules table | MVS-CRITICAL (DONE) | Already implemented. | — |
| `/api/schedules/runs` | GET | DB-backed via task_queue | MVS-CRITICAL (DONE) | Already implemented. | — |
| `/api/memory/overview` | GET | DB-backed aggregate query | MVS-CRITICAL (DONE) | Already implemented. | — |
| `/api/memory/:agentId/core` | GET | DB-backed via MemoryRepository | MVS-CRITICAL (DONE) | Already implemented. | — |
| `/api/memory/:agentId/core/:name` | PUT | DB-backed UPDATE | MVS-CRITICAL (DONE) | Already implemented. | — |
| `/api/memory/:agentId/blocks` | GET | DB-backed (delegates to core) | MVS-CRITICAL (DONE) | Already implemented. | — |
| `/api/memory/:agentId/blocks/:id` | DELETE | DB-backed via MemoryRepository | MVS-CRITICAL (DONE) | Already implemented. | — |
| `/api/memory/recent` | GET | DB-backed ORDER BY updated_at | MVS-CRITICAL (DONE) | Already implemented. | — |
| `/api/memory/explorer-graph` | GET | Returns `{nodes: [], edges: []}` | POST-MVS | Graph visualization is post-MVS. Change to 501. | sera-memory-graph |
| `/api/audit/verify` | GET | DB-backed chain verification | MVS-CRITICAL (DONE) | Already implemented. | — |
| `/api/providers/dynamic` | GET | Reads in-memory provider state | MVS-CRITICAL (DONE) | Already implemented. | — |
| `/api/providers/dynamic/statuses` | GET | Reads in-memory provider state | MVS-CRITICAL (DONE) | Already implemented. | — |
| `/api/providers/templates` | GET | Returns `{templates: []}` | POST-MVS | Provider template library is post-MVS. Change to 501. | sera-providers-templates |
| `/api/providers/default-model` | GET | Returns `{defaultModel: null}` | MVS-OPTIONAL | Single-default-model config is a UX convenience. Change to 501. | sera-defaults |
| `/api/providers/default-model` | PUT | Returns `{success: true}` (fake!) | MVS-OPTIONAL | Fake-success is dangerous. Change to 501. | sera-defaults |
| `/api/agents/:id/grants` | GET | DB-backed via capability_grants | MVS-CRITICAL (DONE) | Already implemented. | — |
| `/api/agents/:id/context-debug` | GET | Returns empty skeleton JSON | POST-MVS | Debug UX, not MVS-blocking. Change to 501. | sera-debug |
| `/api/agents/:id/system-prompt` | GET | Returns `{prompt: ""}` | MVS-OPTIONAL | System prompt is built from manifest+templates in runtime — not stored on instance. Defer to runtime integration. Change to 501. | sera-runtime |
| `/api/agents/:id/health-check` | GET | DB-backed heartbeat check | MVS-CRITICAL (DONE) | Already implemented. | — |
| `/api/agents/:id/sessions/:sid/commands` | GET | DB-backed via task_queue (note: uses `session_id` column — may be missing) | MVS-OPTIONAL | Implemented but unverified. Leave as-is; file separate bead. | sera-sessions-cmds |
| `/api/agents/:id/template-diff` | GET | Returns `{hasChanges: false}` skeleton | **MVS-CRITICAL (SHIPPED)** | Simple compare of instance.updated_at vs template.updated_at. Implemented. | sera-3npy |
| `/api/agents/instances/:id/tools` | GET | Reads `resolved_capabilities.tools` | MVS-CRITICAL (DONE) | Already implemented. | — |
| `/api/knowledge/:agent_id` (stub in embedding.rs) | GET | Returns `{content: ""}` | MVS-OPTIONAL | Knowledge store is a post-MVS feature in current planning. Change to 501. | sera-knowledge |
| `/api/knowledge/:agent_id` | POST | Echoes body back with timestamp (fake!) | MVS-OPTIONAL | Fake-success is dangerous. Change to 501. | sera-knowledge |
| `/api/knowledge/:agent_id/history` | GET | Returns `{versions: []}` | MVS-OPTIONAL | Same. Change to 501. | sera-knowledge |
| `/api/knowledge/:agent_id/diff` | GET | Returns `{diff: ""}` | MVS-OPTIONAL | Same. Change to 501. | sera-knowledge |
| `services::McpServerManager::call_tool` | — | Returns echo `{tool, args, status: "success"}` | POST-MVS | Acknowledged stub. Out of scope for this bead; existing tracing warning sufficient. | sera-mcp |
| `services::McpServerManager::check_health` | — | No-op | POST-MVS | Acknowledged stub. Out of scope for this bead. | sera-mcp |

## Summary

- **MVS-CRITICAL already shipped (DB-backed):** 16 endpoints
- **MVS-CRITICAL newly shipped in sera-3npy:** 3 (`agent_logs`, `agent_subagents`, `agent_template_diff`)
- **MVS-CRITICAL demoted to `503` (embedding):** 2 (`embed_text`, `embed_batch`)
- **MVS-OPTIONAL (now `501`):** 9
- **POST-MVS (now `501`):** 5
- **DEAD (dead-code `embedding_config`/`embedding_status` stubs in stubs.rs):** 2 (unregistered — real impls in embedding.rs)

## Behavior change contract

All handlers that previously returned fake-200 JSON payloads now return
either:

- `501 Not Implemented` with body `{"error": "not_implemented", "planned": "<short scope>", "bead": "sera-..."}`, or
- `503 Service Unavailable` (embedding) with body `{"error": "service_unavailable", "planned": "...", "bead": "..."}`

Router signatures are preserved — no routes were removed.

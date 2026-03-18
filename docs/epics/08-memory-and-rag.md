# Epic 08: Memory & RAG

## Overview

Agents need persistent, semantically searchable memory to maintain context across sessions, share knowledge within circles, and build up expertise over time. SERA's memory model has three explicit scopes: personal (file-based, per agent), circle (git-backed, per circle), and global (system circle, git-backed). All embeddings are generated locally. Qdrant is the search index; git repos are the source of truth for shared knowledge. The `knowledge-store` and `knowledge-query` tools support all three scopes with explicit scope parameters.

## Context

- See `docs/ARCHITECTURE.md` → Memory & RAG
- Three memory scopes: **personal** (files), **circle** (git repo per circle), **global** (system circle git repo)
- Qdrant namespaces: `personal:{agentId}`, `circle:{circleId}`, `global`
- Circle and global knowledge: git-backed; agents commit with their identity; merging to `main` triggers Qdrant re-indexing
- Global knowledge = the system circle's knowledge base; all agents have read access automatically
- Embeddings: `@xenova/transformers` running locally — 1536-dim vectors; no data leaves the network
- Rate limiting: 10 writes per minute per agent
- Context assembly searches all accessible scopes before each LLM call

## Dependencies

- Epic 01 (Infrastructure) — PostgreSQL + pgvector, Qdrant running
- Epic 02 (Agent Manifest) — `memory` block in manifest
- Epic 04 (LLM Proxy) — context assembly runs before forwarding to LiteLLM

---

## Stories

### Story 8.1: Memory block format and file store

**As** sera-core
**I want** a file-based memory block store with a documented format
**So that** memory is human-readable, inspectable, and version-controllable

**Acceptance Criteria:**
- [ ] Memory blocks are `.md` files with YAML front-matter
- [ ] Front-matter fields: `id` (UUID), `agentId`, `type` (`fact|context|memory|insight|reference|observation|decision`), `timestamp` (ISO8601), `tags` (list), `importance` (1-5, default 3)
- [ ] Body is free-form Markdown
- [ ] `MemoryBlockStore.write(block)` writes to `{memoryRoot}/{agentId}/{type}/{timestamp}-{id}.md`
- [ ] `MemoryBlockStore.read(id)` reads and parses a block by ID
- [ ] `MemoryBlockStore.list(agentId, filters?)` lists blocks with optional type/tag/date filters
- [ ] `MemoryBlockStore.delete(id)` removes a block file
- [ ] Directory structure created automatically if absent
- [ ] Invalid front-matter on read: log warning, skip block, do not crash

---

### Story 8.2: Embedding service (local)

**As** sera-core
**I want** to generate vector embeddings locally for memory blocks
**So that** semantic search works without sending data to an external API

**Acceptance Criteria:**
- [ ] `EmbeddingService.embed(text)` returns a 1536-dimension float array
- [ ] Uses `@xenova/transformers` with a locally cached model (e.g. `all-MiniLM-L6-v2` or similar)
- [ ] Model downloaded and cached at startup — not downloaded per-request
- [ ] Model path configurable via `EMBEDDING_MODEL_PATH` env var
- [ ] `embed()` is async and non-blocking; multiple concurrent calls processed efficiently
- [ ] Embedding generation time logged at DEBUG level; warn if > 500ms
- [ ] Fallback: if local model unavailable, log error and disable RAG (agents still function without memory retrieval)

**Technical Notes:**
- Model choice affects quality vs speed trade-off — document the default choice and how to swap it
- The 1536-dim size matches OpenAI's `text-embedding-ada-002` so the schema is compatible if operators switch to API embeddings later

---

### Story 8.3: Qdrant vector store integration

**As** sera-core
**I want** to index and search memory blocks in Qdrant with explicit scope namespacing
**So that** agents can retrieve semantically relevant memories efficiently across personal, circle, and global scopes

**Acceptance Criteria:**
- [ ] `VectorService.upsert(blockId, namespace, vector, metadata)` stores vector in Qdrant
- [ ] Namespace format: `personal:{agentId}`, `circle:{circleId}`, `global`
- [ ] Qdrant collection per namespace — created automatically if absent
- [ ] `VectorService.search(namespaces[], queryVector, topK, filters?)` searches across multiple namespaces in one call; results tagged with source namespace
- [ ] Cosine distance metric used
- [ ] `VectorService.delete(blockId, namespace)` removes a vector
- [ ] `VectorService.rebuildNamespace(namespace, sourcePath)` re-indexes all blocks in a directory into a namespace — used after git merge to main
- [ ] Qdrant connection retried with exponential backoff on startup
- [ ] `GET /api/memory/:agentId/stats` returns: block count, vector count per scope, collection sizes

---

### Story 8.4: Context assembly (RAG before LLM call)

**As** sera-core
**I want** to retrieve relevant memory blocks from all accessible scopes before each LLM proxy call and inject them into the system prompt
**So that** agents have contextual grounding from personal, circle, and global knowledge without manually querying

**Acceptance Criteria:**
- [ ] `ContextAssembler.assemble(agentId, circleIds[], currentMessage)` called in the LLM proxy before forwarding
- [ ] Builds the set of accessible Qdrant namespaces: `personal:{agentId}` always included; `circle:{id}` for each of the agent's circles; `global` always included if the agent has global read access (default: yes)
- [ ] Embeds `currentMessage` → searches all accessible namespaces in one `VectorService.search()` call → retrieves top-K blocks across all scopes (default K=8: 4 personal, 2 circle, 2 global weighting)
- [ ] Retrieved blocks appended to system prompt as `<memory>` blocks annotated with scope and author: `<block id="..." type="..." scope="circle" author="Coder-1" timestamp="...">content</block>`
- [ ] Total memory context capped at configurable token limit (default: 4000 tokens) — lowest-score blocks dropped first, maintaining scope diversity
- [ ] Context assembly adds < 200ms to LLM call latency (fast path: skip if agent has no memory and no circle/global knowledge exists)
- [ ] External content (tool results, fetched data) wrapped in untrusted delimiters before injection — see Prompt Injection section in ARCHITECTURE.md
- [ ] Assembly skipped if agent manifest has no `memory` configuration
- [ ] Retrieved block IDs and source scopes logged at DEBUG for observability

---

### Story 8.5: knowledge-store tool

**As an** agent
**I want** to store knowledge to personal, circle, or global memory during task execution
**So that** I can build up persistent knowledge that is appropriately shared or kept private

**Acceptance Criteria:**
- [ ] `knowledge-store` tool accepts: `content` (string), `type` (memory block type), `scope` (`'personal' | 'circle' | 'global'`, default `'personal'`), `tags?` (list), `title?` (string), `importance?` (1-5)
- [ ] `scope: 'personal'` — writes to agent's personal file store; no capability check needed
- [ ] `scope: 'circle'` — writes to the agent's primary circle's git-backed knowledge repo via `KnowledgeGitService`; requires `knowledgeWrite: circle` in resolved capabilities and agent must be a circle member; if agent is in multiple circles, requires an additional `circleId` parameter
- [ ] `scope: 'global'` — writes to the system circle's git-backed knowledge repo; requires `knowledgeWrite: global` in resolved capabilities
- [ ] Rate limited: 10 writes per minute per agent across all scopes
- [ ] Personal scope: file written → embedding generated → upserted to `personal:{agentId}` Qdrant namespace
- [ ] Circle/global scope: `KnowledgeGitService` writes file to agent's knowledge branch → commits with agent identity → embeds and indexes into agent branch namespace → if `merge-without-approval`: auto-merges to main and re-indexes main namespace
- [ ] Operation recorded in audit trail with scope
- [ ] Returns: `{ id, scope, success: true, pendingMerge?: boolean }` — `pendingMerge: true` if circle/global write requires operator merge approval

---

### Story 8.6: knowledge-query tool

**As an** agent
**I want** to query memory across personal, circle, and global scopes
**So that** I can recall past context and benefit from shared knowledge during reasoning

**Acceptance Criteria:**
- [ ] `knowledge-query` tool accepts: `query` (string), `scopes?` (array of `'personal' | 'circle' | 'global'`, default: all accessible), `topK?` (default 10, max 30), `filter?` (`{ type?, tags?, since?, author? }`)
- [ ] Default scope set: `personal` always, `circle` if agent has circle membership, `global` always
- [ ] Embeds query → `VectorService.search(namespaces, queryVector, topK, filter)` → retrieves top-K blocks across all requested scopes
- [ ] Results annotated with source: `[{ id, type, content, tags, relevanceScore, timestamp, scope, author, committedAt? }]`
- [ ] `scope: ['circle']` with no circle membership → returns `{ results: [], error: 'not_a_circle_member' }`
- [ ] `scope: ['global']` searches `global` namespace (main branch only — pending-merge knowledge not included)
- [ ] Empty results returned as an empty array, not an error
- [ ] Query latency target: < 300ms including embedding generation

---

### Story 8.8: Git-backed circle knowledge service

**As** sera-core
**I want** a `KnowledgeGitService` that manages git-backed knowledge repositories for circles
**So that** shared knowledge has versioning, attribution, conflict resolution, and provenance

**Acceptance Criteria:**
- [ ] `KnowledgeGitService.initCircleRepo(circleId)` initialises a bare git repo at `{KNOWLEDGE_BASE_PATH}/circles/{circleId}/`
- [ ] System circle repo initialised at `{KNOWLEDGE_BASE_PATH}/system/` on first start
- [ ] `KnowledgeGitService.write(circleId, agentInstanceId, agentName, block)` writes a knowledge block file, commits to the agent's branch (`knowledge/agent-{instanceId}`) with committer: `{agentName} <sera-agent-{agentId}@{instanceId}>`
- [ ] `KnowledgeGitService.mergeToMain(circleId, agentInstanceId, approvedBy?)` merges the agent's branch to `main`; triggers `VectorService.rebuildNamespace('circle:{circleId}', mainBranchPath)`
- [ ] Auto-merge: if agent has `knowledgeWrite: merge-without-approval` capability, merge is automatic after write; no approval record needed
- [ ] Approval-required merge: creates a `knowledge_merge_requests` record; `POST /api/knowledge/circles/:id/merge-requests/:requestId/approve` triggers the merge
- [ ] `KnowledgeGitService.diff(circleId, agentInstanceId)` returns the diff between the agent's branch and main — used in the merge approval UI
- [ ] `KnowledgeGitService.log(circleId, filePath?)` returns git log entries: `{ commitHash, authorName, authorAgentId, timestamp, message }`
- [ ] `GET /api/knowledge/circles/:id/history` returns git log for the circle knowledge repo
- [ ] `GET /api/knowledge/circles/:id/merge-requests` lists pending merge requests — operator role required to approve
- [ ] On agent instance teardown: agent's knowledge branch is retained (history preserved) but no new commits accepted
- [ ] Conflict on merge: merge request transitions to `conflict` state; operator can resolve via UI (accept ours, accept theirs, or LLM-assisted merge via `POST /api/knowledge/circles/:id/merge-requests/:id/resolve`)

---

### Story 8.7: Memory compaction and archival

**As an** operator
**I want** old or low-importance memory blocks archived automatically
**So that** the active memory collection stays focused and retrieval quality doesn't degrade over time

**Acceptance Criteria:**
- [ ] Background job runs daily: identifies blocks older than `MEMORY_ARCHIVE_AFTER_DAYS` (default: 30) with `importance <= 2`
- [ ] Archived blocks: moved to `memory/{agentId}/archive/` directory, removed from Qdrant index
- [ ] Archived blocks still readable via `GET /api/memory/:agentId/blocks/:id` but not returned in semantic search
- [ ] `POST /api/memory/:agentId/compact` triggers manual compaction
- [ ] Compaction summary logged: blocks archived, vectors removed, space reclaimed
- [ ] Compaction never deletes blocks — only moves to archive (human can recover)

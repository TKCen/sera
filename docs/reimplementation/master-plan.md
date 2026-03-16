# 🎓 Master Implementation Plan: The SERA Reimagination

This master plan consolidates the architectural vision, feature goals, and adaptation points from **OpenFang**, **crewAI**, **Letta**, **Serena**, and others into a single, executable roadmap for **SERA** (Sandboxed Extensible Reasoning Agent).

## 🎯 Core Objectives
1.  **Safety First**: Absolute isolation via "Sandholed Reality" (Docker-native).
2.  **Stateful Intelligence**: Advanced memory blocks that learn and self-improve (Letta-style).
3.  **Collaborative Orchestration**: Multi-agent "Crews" and event-driven "Flows" (crewAI-style).
4.  **Semantic Precision**: Symbol-level code understanding and surgical editing (Serena-style).
5.  **Extensible Ecosystem**: First-class MCP support and 40+ messaging adapters.

---

## 📅 Roadmap: Phased Execution

### Phase 1: The Core Pulse (Infrastructure)
**Focus**: Communication, Event Streaming, and Basic Agent Lifecycle.
- [ ] **Orchestrator V2**: Implement the `ProcessManager` supporting Sequential, Parallel, and Hierarchical (Manager-Worker) execution patterns.
- [ ] **Centrifugo Event Hub**: Finalize the "Thought Streaming" schema for real-time UI updates.
- [ ] **Adapter Registry**: Port OpenFang's adapter architecture to TypeScript, enabling support for Telegram, Discord, and WhatsApp Web.
- [ ] **Agent YAML Standard**: Define a declarative `AGENT.yaml` format for versioned agent definitions.

### Phase 2: The Sandholed Reality (Security)
**Focus**: Achieving Tier-1 Isolation and Crytographic Integrity.
- [ ] **Ephemeral Docker Runner**: Implement the service to spawn containerized tool execution with cgroup-limited CPU/Memory.
- [ ] **Capability-Based RBAC**: Build the gating logic to enforce Ed25519-signed agent permissions.
- [ ] **Dynamic DevEnv**: Implement "Smart Dockerfiles" that allow agents to self-extend their sandbox with required dependencies (herm-style).
- [ ] **Merkle Audit Logs**: Migrate PostgreSQL schemas to support a cryptographically linked action chain.

### Phase 3: The Stateful Mind (Memory & Context)
**Focus**: Memory Tiering and Semantic Code Understanding.
- [ ] **Memory Blocks**: Transition from simple "Working Memory" to structured "Blocks" (Human, Persona, Core, Archive) as seen in Letta.
- [ ] **LSP + Serena Tools**: Integrate Serena's symbol-level tools (`find_symbol`, `search_references`) into the Sera LSP service.
- [ ] **Qdrant Vector Pipeline**: Implement active ingestion of codebase and research findings into semantic storage.
- [ ] **Auto-Compaction**: Background "Reflector" agent that summarizes interactions into long-term archival memory.

### Phase 4: Autonomous Hands (Capability)
**Focus**: Reimplementing OpenFang "Hands" as modern SERA Workers.
- [ ] **Researcher & Collector**: High-autonomy workers for OSINT and deep literature review.
- [ ] **Browser & Twitter**: Playwright-based automation with mandatory "High-Value" approval gates.
- [ ] **Flow-Based Pipelines**: Implement complex, multi-stage tasks (like CLIP's video pipeline) using event-driven logic.

---

## 🛠️ Technology Stack (The Decoupled Stack)

| Component | Technology | Role |
| :--- | :--- | :--- |
| **Backend** | Node.js 20 + TypeScript | Core Reasoning & Orchestration |
| **Transport** | Centrifugo | Real-time "Thought & Terminal" Streaming |
| **Storage** | PostgreSQL + pgvector | Metadata & Audit Trail |
| **Vector DB** | Qdrant | Semantic Code & Knowledge Recall |
| **Sandbox** | Docker | Ephemeral, Tiered Execution Environments |
| **Inter-Agent** | MCP | Model Context Protocol for Tool Sharing |
| **Frontend** | Next.js 16 + Tailwind v4 | The "AIU Aurora Cyber" Holographic Interface |

---

## ✅ Success Metrics & Verification

### 1. The Security Barrier
- **Test**: Attempt a "Sandbox Escape" (host filesystem access) from a Tier-1 Worker.
- **Goal**: 100% failure rate for escapes.

### 2. The Thought Latency
- **Test**: Measure time from Agent "Thought" to Centrifugo "UI Render".
- **Goal**: < 50ms (Zero-latency feel).

### 3. Semantic Accuracy
- **Test**: Run `find_symbol` on a complex 100k+ LOC codebase.
- **Goal**: > 95% precision compared to IDE search.

### 4. Memory Persistence
- **Test**: Restart a "Persona" agent and verify it remembers "Human" details from multiple sessions ago.
- **Goal**: 100% recall of core "Memory Blocks".

# 🗺️ Implementation Plan: OpenFang ➡️ SERA

This plan details the phased approach to reimplementing OpenFang's core capabilities within the modernized SERA architecture.

## 📅 Roadmap

### Phase 1: Foundation & Adapters (The Pipes)
**Goal**: Establish the communication infrastructure and basic agent lifecycle.
- [ ] **Adapter Registry**: Implement the logic to handle multi-channel communication (Telegram, Discord, Slack) following the OpenFang adapter model.
- [ ] **Worker Pattern Expansion**: Update `WorkerAgent` to support "Phase-based" execution (e.g., CLIP's 8-phase pipeline).
- [ ] **Centrifugo Integration**: Standardize the event schema for thought streaming and tool output.

### Phase 2: Sandholed Execution (The Shield)
**Goal**: Implement the tier-1 isolation system for secure tool execution.
- [ ] **Docker Task Runner**: Create a service that spawns ephemeral Docker containers with pre-determined resource limits (OpenFang's "Dual-Metered" equivalent).
- [ ] **Capability-based RBAC**: Implement the gating logic that checks an agent's manifest before allowing tool execution.
- [ ] **Audit Trail (Merkle)**: Implement the cryptographic logging system in PostgreSQL to ensure action integrity.

### Phase 3: Hand Reimplementation (The Claws)
**Goal**: Port OpenFang's "Hands" as SERA `WorkerAgents`.
- [ ] **Researcher Hand**: Multi-source context gathering with CRAAP criteria scoring.
- [ ] **Collector Hand**: OSINT monitoring and knowledge graph construction in Qdrant.
- [ ] **Browser Hand**: Playwright-based automation with mandatory approval gates for "high-value" actions.
- [ ] **Twitter/X Hand**: Content scheduling and engagement automation.

### Phase 4: Intelligence & Memory (The Soul)
**Goal**: Enhance reasoning and long-term recall.
- [ ] **Vector Ingestion Pipeline**: Auto-ingest codebase and external research into Qdrant.
- [ ] **Archival Sync**: Automated "thought-to-markdown" archival system for long-term memory.
- [ ] **LSP Integration**: Cross-language code navigation for the "Coder" worker.

---

## 🛠️ Step-by-Step Implementation Guide

### 1. The Adapter Layer
Create a new directory `sera/core/src/adapters` to house the 40+ channel adapters.
```typescript
// Proposed BaseAdapter.ts
export abstract class BaseAdapter {
  abstract connect(): Promise<void>;
  abstract sendMessage(to: string, text: string): Promise<void>;
  abstract onMessage(callback: (msg: any) => void): void;
}
```

### 2. The Task Sandbox
Implement the `SandboxRunner` in `sera/core/src/lib/sandbox.ts`.
```typescript
// Proposed sandbox tool execution
async runToolInSandbox(toolCommand: string, tier: number) {
  const container = await docker.createContainer({
    Image: 'sera-worker-base',
    Cmd: toolCommand.split(' '),
    HostConfig: {
      Memory: getMemoryLimit(tier),
      NanoCpus: getCpuLimit(tier),
      NetworkMode: tier === 1 ? 'none' : 'bridge',
    }
  });
}
```

### 3. The Merkle Audit Trail
Update `sera/core/src/lib/database.ts` to include an `audit_logs` table with `parent_hash` and `hash` columns.

---

## ✅ Verification Strategy

### Automated Testing
- **Unit Tests**: Test each adapter's formatting logic.
- **Integration Tests**: Verify Docker container spawning and resource limiting.
- **Security Audit**: Automated check for SSRF protection and "taint" propagation.

### Manual Verification
1.  **Hand Activation**: Use `sera-cli` (to be developed) to activate a Hand and watch the thoughts stream in the UI.
2.  **Sandbox Breach Test**: Attempt to access the host file system from a Tier-1 tool execution.

# SERA Documentation

> **SERA** — Sandboxed Extensible Reasoning Agent — a Docker-native multi-agent AI orchestration platform.

## Quick Links

| Audience | Document |
|----------|----------|
| **New developer** | [Developer Quickstart](QUICKSTART.md) |
| **Operator** | [Deployment Guide](DEPLOYMENT.md) |
| **Architect** | [Architecture Overview](ARCHITECTURE.md) |
| **Contributor** | [Testing Guide](TESTING.md) |

## Documentation Map

### Architecture & Design

| Document | Description | Status |
|----------|-------------|--------|
| [ARCHITECTURE.md](ARCHITECTURE.md) | Canonical system architecture — all components, data models, design decisions | Current |
| [IMPLEMENTATION-ORDER.md](IMPLEMENTATION-ORDER.md) | Epic dependency graph and build sequence | Current |
| [SKILL-ECOSYSTEM.md](SKILL-ECOSYSTEM.md) | Skills vs tools taxonomy, provider design | Current |
| [openapi.yaml](openapi.yaml) | REST API specification (~190 endpoints) | Current |

### Architecture Decision Records (ADRs)

| ADR | Status | Title |
|-----|--------|-------|
| [ADR-001](adrs/001-tool-execution-architecture.md) | Proposed | Tool Execution Architecture |
| [ADR-002](adrs/002-implementation-gap-analysis.md) | Accepted | Implementation Gap Analysis |
| [ADR-003](adrs/003-scope-validation-framework.md) | Proposed | Scope Validation Framework |
| [ADR-004](adrs/004-permission-grant-persistence.md) | Proposed | Permission Grant Persistence |
| [ADR-005](adrs/005-core-modularity-and-component-interfaces.md) | Proposed | Core Modularity and Component Interfaces |
| [ADR-006](adrs/006-web-and-agent-runtime-modularity.md) | Proposed | Web Frontend and Agent-Runtime Modularity |
| [ADR-007](adrs/007-duplication-and-consolidation.md) | Proposed | Duplication Inventory and Consolidation Plan |

See [adrs/README.md](adrs/README.md) for the ADR process.

### Epic Specifications

Epics define the acceptance criteria for each feature area. Implementation status is tracked inline.

| Phase | Epic | Status |
|-------|------|--------|
| **1: MVP** | [01: Infrastructure Foundation](epics/01-infrastructure-foundation.md) | ✅ Complete |
| | [02: Agent Manifest & Registry](epics/02-agent-manifest-and-registry.md) | ✅ Complete |
| | [03: Docker Sandbox & Lifecycle](epics/03-docker-sandbox-and-lifecycle.md) | ✅ 95% |
| | [04: LLM Proxy & Governance](epics/04-llm-proxy-and-governance.md) | ✅ Complete |
| **2: Usable** | [05: Agent Runtime](epics/05-agent-runtime.md) | ✅ 90% |
| | [06: Skill Library](epics/06-skill-library.md) | ✅ Complete |
| | [07: MCP Tool Registry](epics/07-mcp-tool-registry.md) | ⚠️ 70% — [ADR-001](adrs/001-tool-execution-architecture.md) |
| | [08: Memory & RAG](epics/08-memory-and-rag.md) | ✅ 85% |
| | [09: Real-Time Messaging](epics/09-real-time-messaging.md) | ✅ Complete |
| | [10: Circles & Coordination](epics/10-circles-and-coordination.md) | ✅ 85% |
| | [11: Scheduling & Audit](epics/11-scheduling-and-audit.md) | ✅ Complete |
| | [12: sera-web Foundation](epics/12-sera-web-foundation.md) | ✅ 95% |
| | [13: sera-web Agent UX](epics/13-sera-web-agent-ux.md) | ⚠️ 80% |
| | [14: sera-web Observability](epics/14-sera-web-observability.md) | ⚠️ 70% |
| | [20: Egress Proxy](epics/20-egress-proxy.md) | ⚠️ 60% |
| **3: Ecosystem** | [15: Plugin SDK](epics/15-plugin-sdk-and-ecosystem.md) | ❌ 20% |
| | [16: Authentication & Secrets](epics/16-authentication-and-secrets.md) | ✅ 85% |
| | [17: Agent Identity & Delegation](epics/17-agent-identity-and-delegation.md) | ⚠️ 70% — [ADR-003](adrs/003-scope-validation-framework.md) |
| | [18: Integration Channels](epics/18-integration-channels.md) | ⚠️ 65% |
| **4: Consolidation** | [19: Memory Consolidation](epics/19-memory-system-consolidation.md) | ❌ Deferred |
| | [21: ACP / IDE Bridge](epics/21-acp-ide-bridge.md) | ❌ Deferred |
| | [22: Canvas / A2UI](epics/22-canvas-a2ui.md) | ❌ Deferred |
| | [23: Voice Interface](epics/23-voice-interface.md) | ❌ Deferred |
| | [24: A2A Federation](epics/24-a2a-federation.md) | ❌ Deferred |

### Subsystem Guides

| Document | Description |
|----------|-------------|
| [mcp/FORMAT.md](mcp/FORMAT.md) | MCP server manifest format |
| [messaging/CHANNELS.md](messaging/CHANNELS.md) | Centrifugo channel naming conventions |
| [messaging/FEDERATION.md](messaging/FEDERATION.md) | A2A federation protocol (Phase 4, planned) |
| [channel-validation-cases.md](channel-validation-cases.md) | Channel integration test cases |

### Reference & Research

| Document | Description |
|----------|-------------|
| [OPENCLAW-ANALYSIS.md](OPENCLAW-ANALYSIS.md) | OpenClaw competitive analysis |
| [REFERENCE-ANALYSIS.md](REFERENCE-ANALYSIS.md) | Landscape review of 14+ AI agent platforms |
| [research/browser-agent-capabilities.md](research/browser-agent-capabilities.md) | Browser automation research |

### Operations

| Document | Description |
|----------|-------------|
| [MIGRATIONS.md](MIGRATIONS.md) | Database migration guide |
| [jules-recurring-tasks.md](jules-recurring-tasks.md) | Automated maintenance task definitions |

### Archived

Legacy documents from earlier planning phases. Preserved for historical context but no longer canonical:

| Document | Reason |
|----------|--------|
| [_archive/reimplementation/](/_archive/reimplementation/) | Pre-v1 migration plan, superseded by current epics |
| [_archive/v2-distributed-architecture/](/_archive/v2-distributed-architecture/) | Speculative distributed v2 vision, not in current roadmap |

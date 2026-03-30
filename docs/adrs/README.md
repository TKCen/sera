# Architecture Decision Records (ADRs)

ADRs document significant architecture decisions — the current state, canonical design, deviations with reasoning, and the recommended path forward.

## Index

| ADR | Status | Title |
|-----|--------|-------|
| [001](001-tool-execution-architecture.md) | Proposed | Tool Execution Architecture |
| [002](002-implementation-gap-analysis.md) | Accepted | Implementation Gap Analysis vs Canonical Epics |
| [003](003-scope-validation-framework.md) | Proposed | Scope Validation Framework |
| [004](004-permission-grant-persistence.md) | Proposed | Permission Grant Persistence |
| [005](005-core-modularity-and-component-interfaces.md) | Proposed | Core Modularity and Component Interfaces |
| [006](006-web-and-agent-runtime-modularity.md) | Proposed | Web Frontend and Agent-Runtime Modularity |

## Process

1. Before implementing a feature that touches core infrastructure, check the relevant epic
2. If the implementation differs from the spec, document why in an ADR
3. ADRs are reviewed and approved before implementation begins
4. Status: `Proposed` → `Accepted` → `Implemented` (or `Superseded` / `Deprecated`)

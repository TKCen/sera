# 05 Implementation Roadmap

The transition to SERA v2 is a phased migration designed to maintain system stability while upgrading the core foundations.

## Phase 1: The Security Gateway
**Goal**: Centralize identity and LLM access in `sera-core`.

1.  **OpenAI Proxy**: Implement a `/v1/llm` proxy in `sera-core` that handles API key injection.
2.  **JWT Identity**: Implement a service to sign and verify Identity Tokens for worker containers.
3.  **Token Metering**: Add the infrastructure to track and limit token usage (Epic 14).
4.  **Verification**: Confirm that a simple HTTP client can use the proxy with a Core-issued JWT.

## Phase 2: Distributed Reasoning Actor
**Goal**: Move the agent logic *into* the container.

1.  **Worker Runtime**: Develop a minimal Node/Python process that acts as the "reasoning actor."
2.  **Lifecycle Handshake**: Update the Orchestrator to monitor the container's heartbeat instead of running the loop.
3.  **Local Tool Integration**: Update built-in skills to run natively (no `docker exec`).
4.  **Verification**: Create an agent that successfully thinks and Acts entirely within its isolated sandbox.

## Phase 3: Memory & State Maturity
**Goal**: Implement high-density, structured memory blocks.

1.  **Memory Block Store**: Implement the Markdown + YAML storage for Human/Persona/Core/Archive blocks.
2.  **The Reflector**: Build the background agent that summarizes and compacts the Active Context.
3.  **Archival Search**: Connect Qdrant collections to the new memory block types.
4.  **Verification**: Verify that an agent "remembers" a fact across 10+ sessions after multiple compacting cycles.

## Phase 4: Full Circle Federation
**Goal**: Multi-instance collaboration.

1.  **Circle Bridge Service**: Implement mTLS-secured Centrifugo bridging.
2.  **Constitution Engine**: Automate the injection of `project-context.md` into all circle agents.
3.  **Verification**: Pass a message from an agent on Instance A to an agent on Instance B.

---

## Success Checkpoints
- [ ] 100% of LLM calls flow through the Gateway.
- [ ] 0 commands executed via `docker exec` (replaced by native runtime).
- [ ] 100ms average latency for thought streaming.
- [ ] Memory persistence verified across container restarts.

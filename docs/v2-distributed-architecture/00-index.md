# SERA v2: Distributed Agent Architecture

Welcome to the future of the **Sandboxed Extensible Reasoning Agent**. 

SERA v2 moves from a centralized "Manager-Worker" model to a decentralized **"Distributed Actor"** model. This shift maximizes security, enables polyglot agent development, and creates a truly resilient autonomous substrate.

## 🗺️ Documentation Map

1.  **[Architecture & Topology](01-architecture-topology.md)**
    *   The "Distributed Actor" paradigm.
    *   Roles of Core Gateway vs. Agent Actor.
    *   Communication mesh via Centrifugo.

2.  **[Security & The Gateway](02-security-and-gateway.md)**
    *   LLM Proxying and API Key Vaulting.
    *   Identity Management via JWT.
    *   Capability-based Gating (RBAC).

3.  **[Memory & Workspace](03-memory-and-workspace.md)**
    *   Structured Memory Blocks (Human, Persona, Core, Archive).
    *   Automated compaction via the Reflector.
    *   Pluggable Storage Providers.

4.  **[Circles & Federation](04-circles-and-federation.md)**
    *   Agent grouping and the "Circle Constitution".
    *   Cross-instance messaging (Federation).
    *   Party Mode orchestrated discussions.

5.  **[Implementation Roadmap](05-implementation-roadmap.md)**
    *   Phase-by-phase transition strategy.
    *   Milestones and verification checkpoints.

---

## 🚀 The Vision
> "Every agent is a sovereign, isolated entity. Every interaction is a secure, audited event."

By decoupling the reasoning loop from the infrastructure, SERA becomes more than a tool—it becomes a **Distributed AI Operating System** capable of running anywhere, from a single homelab server to a federated cloud mesh.

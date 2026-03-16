# 🏗️ Architecture: OpenFang ➡️ SERA

This document outlines the architectural shift from OpenFang's monolithic Rust design to SERA's decoupled, sandboxed, and event-driven ecosystem.

## 🌉 Architectural Mapping

| Feature | OpenFang (Legacy) | SERA (Modernized) | Rationale |
| :--- | :--- | :--- | :--- |
| **Language** | Rust | TypeScript (Node.js 20) | Flexibility, ecosystem depth (MCP/LSP Support). |
| **Concurrency** | Rust Threads / Tokio | Node.js Worker Threads / Docker | Improved isolation and resource management. |
| **Sandboxing** | WASM Dual-Metered | **Sandholed Reality** (Docker) | Native system capability with tier-1 isolation. |
| **Orchestration** | Monolithic Kernel | **Decoupled Pulse** (Orchestrator) | Independent scaling of Mind, Pulse, and Interface. |
| **Memory** | SQLite + Vector | Postgres + Qdrant + Markdown | Robustness and standard homelab integration. |
| **Communication** | REST / WS / SSE | **Centrifugo** (Streaming WS) | Ultra-low latency "Thought Streaming". |
| **UI Aesthetics** | Basic Dashboard | **AIU Aurora Cyber** | Cohesive with `radio-player` blueprint. |

---

## 🎨 The "Aurora Cyber" Interface
The SERA interface is designed as an extension of the **AIU Aurora** brand identity found in the `radio-player` project.

- **Primary Palette**: AIU Cyan (`#00E5FF`) and AIU Green (`#00FF00`) gradients.
- **Surface**: Aurora Black Deep (`#020402`) for maximum "OLED" contrast.
- **Visual Language**: High-fidelity glassmorphism, backdrop-blur (12-20px), and subtle "Matrix Glow" on interactive components.
- **Thought Streaming**: Real-time reasoning logs use the "AIU Cyan" spectrum to signify tech and innovation.

In the SERA reimplementation, OpenFang's functionalities are redistributed into modular services:

### 1. The Orchestrator (The Mind)
The `PrimaryAgent` acts as the central coordinator, delegating tasks to specialized `WorkerAgents`. 
- **OpenFang "Hands"** are reimplemented as specialized `WorkerAgents` with tailored system prompts and toolsets.
- **Dynamic Skill Injection** is handled via Sera's LSP-aware ingestion system.

### 2. The Sandholed Execution (The Reality)
The core architecture leverages a direct mount to the Docker socket to spawn **ephemeral environments**.
- **Security Tiers**: Actions are tiered (1-Read, 2-Write, 3-Execute, 4-Destructive) and executed in containers with varying resource limits and network access.
- **Audit Trail**: Every action is recorded in a PostgreSQL-backed Merkle hash-chain, mirroring OpenFang's integrity system.

### 3. The Decoupled Pulse (The Flow)
By using `Centrifugo`, the agent's internal state is streamed in real-time.
- **Thought Channels**: Users can subscribe to specific agent thought streams.
- **Terminal Multiplexing**: Real-time terminal output from sandboxed containers.

---

## 🛡️ Security Layers Redesign

OpenFang's 16 security layers are mapped as follows:

| OpenFang System | SERA Implementation |
| :--- | :--- |
| WASM Dual-Metered | **Docker Cgroup Limits + Seccomp Profiles** |
| Merkle Audit Trail | **Postgres Merkle-Hashed Event Store** |
| Taint Tracking | **Metadata labeling in Agent Context** |
| Capability Gates | **Container capabilities (drop ALL, add specific)** |
| Loop Guard | **Orchestrator-level cycle detection** |
| Secret Zeroization | **Encapsulated Env Variable Management** |

---

## 🚦 Future-Proofing
The architecture is designed to be **MCP-native** (Model Context Protocol). Any tool implemented for SERA can be exposed as an MCP server, allowing for cross-agent collaboration and easy expansion.

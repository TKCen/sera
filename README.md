# 💠 SERA: Sandboxed Extensible Reasoning Agent

<p align="center">
  <img src="https://raw.githubusercontent.com/devicons/devicon/master/icons/robot/robot-original.svg" width="120" alt="SERA Logo">
  <h2 align="center">The AI Resident for the Modern Homelab</h2>
</p>

<p align="center">
  <a href="https://github.com/TKCen/sera/actions"><img src="https://img.shields.io/badge/Build-Passing-brightgreen?style=for-the-badge&logo=docker" alt="Build Status"></a>
  <a href="https://github.com/TKCen/sera/issues"><img src="https://img.shields.io/badge/Roadmap-Active-blue?style=for-the-badge&logo=github" alt="Issues"></a>
  <a href="#"><img src="https://img.shields.io/badge/Aesthetic-Holographic_Glitch-crimson?style=for-the-badge" alt="Design"></a>
</p>

---

## 🚀 The Vision: Beyond the Chatbot

**SERA** is not just an interface; it's a **Permanent Resident** of your digital ecosystem. Inspired by the autonomy of **OpenClaw** and the modularity of **OpenFang**, SERA is designed to be a proactive, state-aware collaborator that lives where your code lives.

### Why SERA?
1.  **Ownership**: Local-first storage, local-first reasoning. Your data never leaves your network.
2.  **Safety**: A "Sandholed Reality" where every execution is tiered and isolated within Docker containers.
3.  **Presence**: Real-time "Thought Streaming" makes the agent's work-in-progress visible and interactive.
4.  **Aesthetic**: Reclaiming the "digital cool" with a high-fidelity **Holographic Glitch** UI.

---

## 🧠 The Architecture: "The Decoupled Pulse"

SERA is built on a high-performance, event-driven architecture designed for zero-latency interaction:

| Component | Technology | Role |
| :--- | :--- | :--- |
| **Foundation** | Docker Compose | Multi-container orchestration & Homelab networking. |
| **The Mind** | Node.js 20 (TS) | Core Reasoning, LSP Coordination, & Sandbox Management. |
| **The Pulse** | Centrifugo | Ultra-low latency WebSocket streaming for thoughts & terminal. |
| **The Interface** | Next.js 16 (Tailwind v4) | The "Aurora Cyber" dashboard with glitch-aware UI. |
| **The Memory** | PostgreSQL / Vector | Persistent metadata and semantic codebase knowledge. |

### 🛠️ Strategic Integration
*   **Homepage**: Native discovery via labels (AI Agents / Infrastructure).
*   **Nginx Proxy Manager**: Pre-configured WS-capable reverse proxying.
*   **Uptime Kuma**: Integrated `/api/health` monitoring for 100% reliability.

---

## 🛡️ Core Pillars

### 1. Sandholed Execution
Unlike agents that run on your host, SERA's core holds a direct mount to the Docker socket. It spawns **ephemeral, isolated environments** for every shell command or file edit, ensuring your host remains pristine.

### 2. Semantic Mastery (LSP & RAG)
SERA doesn't just "grep". It integrates with **Language Server Protocol (LSP)** for graph-aware code navigation and **Vector Databases** for semantic recall that understands the "why" behind your code, not just the "what".

### 3. Real-Time Symbiosis
By leveraging **Centrifugo**, SERA streams its internal reasoning state (Thoughts) directly to the dashboard. You can watch the agent narrow down a bug in real-time, just as if you were looking over a colleague's shoulder.

---

## 📅 Roadmap: Build the Future

We are currently in **Phase 1: Foundation**. Upcoming work items include:

*   **Phase 2: Knowledge & Context**
    *   [ ] Vector Database Ingestion (Qdrant/pgvector).
    *   [ ] LSP-Native Indexing for Python/TS/Rust.
    *   [ ] Archival Markdown-based long-term memory.
*   **Phase 3: Deep Autonomy**
    *   [ ] MCP-native tool expansion.
    *   [ ] Multi-agent "Swarm" mode for complex refactors.

---

## 🚦 Getting Started

### Prerequisites
- Docker & Docker Compose
- `agent_net` Docker network: `docker network create agent_net`
- LLM Provider (LM Studio, Ollama, OpenAI, or Anthropic)

### Quick Start
1.  **Clone the repository:**
    ```bash
    git clone https://github.com/TKCen/sera.git
    cd sera
    ```
2.  **Initialize environment:**
    ```bash
    cp .env.example .env
    # Edit .env to add your API keys or adjust URLs
    ```
3.  **Launch the stack:**
    ```bash
    docker compose up -d
    ```

### Access Points
- **UI**: [http://localhost:3000](http://localhost:3000)
- **Core API**: [http://localhost:3001](http://localhost:3001)
- **LiteLLM**: [http://localhost:4000](http://localhost:4000)

---

## 🛠️ Development

### Local Development Loop
To run with hot-reloading for `sera-core` and `sera-web`:
```bash
docker compose -f docker-compose.yaml -f docker-compose.dev.yaml up
```

### Database Migrations
Migrations run automatically on `sera-core` startup. To manage migrations manually:
```bash
cd core
npm run migrate -- up
npm run migrate -- create my_migration_name
```

### Running Tests
```bash
cd core
npm test
```

---

<p align="center">
  <em>"SERA: Your agent. Your network. Your reality."</em>
</p>

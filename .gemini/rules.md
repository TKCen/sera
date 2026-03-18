# Antigravity Rules: Developing Project SERA

These rules dictate how AI assistants should approach *developing* the SERA platform itself.

## 🏗️ Architecture & Navigation
- **Decoupled Monorepo**: SERA is split into multiple independent services:
  - `core/`: Node.js 20 (TS) backend, containing the reasoning logic, Sandbox tools, and Agent Runtime.
  - `web/`: Next.js 16 (Tailwind v4) frontend UI ("Aurora Cyber" aesthetic).
  - `memory/`: Vector DB / PostgreSQL configurations.
  - `centrifugo/`: Real-time streaming service configurations.
- **Context Boundaries**: When working on a feature, ensure you are in the correct service directory before running npm commands (e.g., `cd core` before modifying core dependencies or running tests).

## 🛠️ Development & Testing Workflow
- **Small Increments**: Work in small, verifiable increments. Do not rewrite large swaths of undocumented code in one go.
- **Testing**: We use **Vitest** for testing (unit and integration tests). Run tests within the respective service directory (e.g., `npm run test` inside `core/`). Be mindful of target environments and module transformations (e.g., top-level await compatibility).
- **Docker Infrastructure**: The platform runs as a multi-container stack via Docker Compose.
  - Ensure the Docker environment remains healthy.
  - If you change a Dockerfile or major system dependencies, remind the user to rebuild the containers (`docker compose build` / `docker compose up -d`).

## 🎨 Frontend (Web) Conventions
- **Framework**: Next.js 16 with Tailwind CSS v4.
- **Aesthetic**: Maintain the "Holographic Glitch" / "Aurora Cyber" design language. Interfaces should feel glitch-aware, high-fidelity, and real-time.
- **Real-Time Data**: UI interacts heavily with Centrifugo WebSockets for real-time thought streaming. Ensure WebSocket connections and data handlers are robust.

## 🧠 Core Backend Conventions
- **TypeScript Strictness**: Maintain strong typing. Avoid `any` where possible.
- **Containerization Logic**: The core creates ephemeral Docker containers for agent execution. Operations touching `SandboxManager` or container orchestration must be highly scrutinized for security and state hygiene.

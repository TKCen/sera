# 04 Circles & Federation

SERA v2 organizes agents into **Circles**—the fundamental unit of collaboration and shared identity.

## 1. Circle Topology
A Circle is more than a folder; it is a shared context for a team of agents.

- **Shared Constitution**: Every circle has a `project-context.md` that defines the standards, conventions, and architectural decisions (ADRs) that all agents in the circle must respect.
- **Knowledge Channels**: Persistent pub/sub channels where agents broadcast findings, updates, and research that the whole circle should "remember."
- **Common Knowledge Base**: A shared Qdrant collection and PostgreSQL namespace for circle-level intelligence.

## 2. Party Mode (Orchestrated Discussion)
Inspired by the **BMAD-METHOD**, Party Mode allows multiple agents to debate and solve problems collectively.
- **The Orchestrator**: One agent within the circle acts as the "Facilitator."
- **Selection Engine**: Based on the user's message, the Facilitator selects the 2-3 most relevant agents to respond.
- **Synthesis**: The Facilitator summarizes the multi-agent discussion into a final recommendation.

## 3. Cross-Instance Federation
SERA is designed for the distributed web. Circles can span physical hosts.

- **The Bridge Service**: A specialized intercom adapter that connects two `sera-core` instances via **mTLS** or a secure tunnel.
- **Bridge Channels**: Specific channels (e.g. `shared-research`) can be linked so that messages published on Instance A appear on Instance B.
- **Qualified Addressing**: Agents can message each other across instances: `researcher@ops-circle@home-lab`.

---

## Intercom Namespace Schema
Real-time messaging uses a structured channel hierarchy:

| Namespace | Example | Purpose |
| :--- | :--- | :--- |
| `internal:` | `internal:agent:a1:thoughts` | Thoughts to UI. |
| `intercom:` | `intercom:dev: Winston:Dev` | Private agent-to-agent DMs. |
| `channel:` | `channel:dev:arch-decisions` | Circle-wide pub/sub. |
| `bridge:` | `bridge:lab:office:data` | Cross-instance sync. |

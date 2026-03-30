# 03 Memory & Workspace

In SERA v2, memory is a **Structured Block Substrate** that facilitates long-term learning and surgical context management.

## 1. Letta-style Memory Blocks
Memory is partitioned into distinct blocks, each with a specific purpose:

- **Human Persona**: What the agent knows about the user (preferences, style, past requests).
- **Agent Persona**: The agent's own personality, role, and self-defined mission.
- **Core Context**: Essential project-level facts or current task focus.
- **Archive**: Historical session logs and tool outputs.

### Storage Format
Memory is stored as **Markdown files with YAML frontmatter**.
- **Human-Readable**: Parents can browse memory via any markdown editor (like Obsidian).
- **Graph-Linkable**: Using `[[Wikilinks]]` to create semi-structured connections between facts.

## 2. The Reflector (Auto-Compaction)
The **Reflector** is a background process that solves the "Context Overflow" problem.
1.  **Monitors**: Observes the size of the Active Context.
2.  **Summarizes**: When context exceeds a threshold, it uses a secondary LLM to summarize "Archival" blocks.
3.  **Compacts**: Replaces raw chat history with high-density semantic summaries.

## 3. Storage Providers
Workspace storage is abstracted to allow local or remote deployments.

| Provider | Mechanism | Best For |
| :--- | :--- | :--- |
| **local** | Bind Mounts | Local development, lowest latency. |
| **docker-volume** | Named Volumes | Standard containerized portability. |
| **s3 (Future)** | Object Storage | Large-scale datasets or cloud federation. |

---

## Workspace Lifecycle
1.  **Provision**: Core creates the directory/volume per agent ID.
2.  **Mount**: Core attaches the volume to the Agent Actor container.
3.  **Permissions**: Permissions (RO/RW) are enforced based on the Security Tier.
4.  **Persistent**: Workspaces outlive the container, allowing for session resumption.

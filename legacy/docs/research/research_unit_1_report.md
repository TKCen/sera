# Technical Audit Report: 'core' Workspace

## 1. Major Modules and Responsibilities

The `core` workspace serves as the central orchestration layer for the SERA platform. Below are the primary modules identified:

| Module | Responsibility |
| :--- | :--- |
| **`agents/`** | The heart of the system. Contains the `Orchestrator`, `AgentFactory`, and `BaseAgent`. Manages agent lifecycles (spawn, stop, restart), template loading, and subagent recursion management. |
| **`mcp/`** | Implements the Model Context Protocol (MCP) integration. Handles MCP server registration (both host-side and containerized), client connections, and bridging external MCP tools into the `SkillRegistry`. |
| **`sandbox/`** | Manages Docker-based execution environments. Responsible for container lifecycle (`SandboxManager`), network security via egress ACLs (`EgressAclManager`), and workspace management. |
| **`skills/`** | The capability engine. Contains the `SkillRegistry` and a library of built-in skills (e.g., `file-read`, `web-search`). It also handles the ingestion of tools from MCP servers. |
| **`llm/`** | Manplements LLM provider abstraction, routing, and context management. Includes `LlmRouter`, `ProviderRegistry`, and `ContextCompintionService`. |
| **`intercom/`** | The communication backbone. Facilitates pub/sub messaging via Centrifugo (`IntercomService`) and manages outbound notification channels (Discord, Telegram, etc.). |
| **`memory/`** | Handles long-term and short-term knowledge storage. Includes `MemoryManager`, `KnowledgeGitService` for versioned knowledge, and vector database integration (Qdrant). |
| **`auth/`** | Manages identity and access control. Implements API key providers, OIDC support, and session management via `AuthService`. |
| **`tools/`** | Provides the execution engine (`ToolExecutor`) that runs skills and MCP tools within the context of an agent's capabilities. |

## 2. Task Execution Flow (Receipt to Completion)

The flow of a task (e.g., via `orchestrator.executeTask(description)`) follows this path:

1.  **Entry Point:** A request enters through an API route (e.g., `/api/chat` or `/v1/openai-compat`).
2.  **Orchestration Initiation:** The `Orchestdrator` identifies the primary agent and passes the task description to the `ProcessManager`.
3.  **Process Planning:** The `ProcessManager` determines the execution strategy (e.g., `Sequential`, `Parallel`, or `Hierarchical`) based on the task complexity.
4.  **Agent Execution Loop:**
    *   The agent receives the prompt and uses the `LlmRouter` to interact with an LLM provider.
    *   If the LLM decides a tool is needed, the agent calls the `ToolExecutor`.
5.  **Tool/Skill Invocation:**
    *   The `ToolExecutor` checks the agent's `CapabilityResolver` results to ensure the tool is permitted.
    *   It then executes the corresponding `Skill` (built-in) or `MCP Tool` (external).
6.  **Sandbox Execution:** If the tool requires a sandbox (e.g., `shell-exec`), the `SandboxManager` ensures a container is running and routes the command to it.
7.  **Result Processing & Feedback:** The tool output is returned to the agent, which incorporates it into its context. This continues until the LLMM provides a final answer or a recursion/timeout limit is reached.
8.  **Completion:** The final result is returned via the original API response and published to `Intercom` for real-time updates to connected clients.

## 3. MCP Tool Discovery and Invocation

MCP (Model Context Protocol) integration is handled through a registry-based pattern:

*   **Discovery:**
    *   The `MCPRegistry` uses `chokidar` to watch the `mcp-servers/` directory for `.mcp.yaml`, `.mcp.json`, or `.mcp.yml` manifest files.
    *   When a new manifest is detected, `MCPServerManager` spawns the specified server (often in a Docker container).
    *   The registry then connects to the server via an `MCPClient`.
*   **Bridging to Skills:**
    *   A critical feature is the "bridge" between MCP and the internal `SkillRegistry`. Upon registration, the `MCPRegistry` triggers a hook that calls `skillRegistry.bridgeMCPToolsForServer(name, mcpRegistry)`.
    *   This dynamically converts external MCP tool definitions into internal SERA skills, prefixed with the server name (e.g., `server-name/tool-id`).
*   **Invocation:**
    *   When an agent attempts to use a tool, the `ToolExecutor` looks up the skill in the `SkillRegistry`.
    *   If it's an MCP-bridged tool, the executor routes the call through the `MCPClient` back to the original MCP server.

## 4. Architectural Bottlenecks and Complexities

### Identified Bottlenements:
*   **Docker Event Latency:** The system relies heavily on Docker events for lifecycle management (e.g., `die`, `oom`). High-frequency container churn could lead to race conditions between the `SandboxManager` and the `Orchestrator's` internal state.
*   **Sequential Resource Loading:** The startup sequence in `index.ts` is highly order-dependent (DB $\to$ Orchestrator $\to$ Agents $\to$ Skills). While necessary for stability, it increases cold-start time as the number of agents and skills grows.
*   **Context Compaction Overhead:** As agent conversations grow, the `ContextCompactionService` must run frequently to stay within LLM context windows. This adds computational overhead to every LLM interaction.

### Identified Complexities:
*   **Capability Resolution Logic:** The `CapabilityResolver` must reconcile complex hierarchies (Template $\to$ Instance Overrides $\to$ Circle Inheritance $\to$ MCP Bridges). Debugging why an agent *cannot* use a specific tool requires tracing through multiple layers of permission logic.
*   **Hybrid Registry Management:** Managing both in-process "built-in" skills and out-of-process "MCP" tools creates two distinct execution paths that must be unified by the `ToolExecutor`.
*   **State Synchronization:** Maintaining consistency between the PostgreSQL database (the source of truth for agent instances) and the in-memory `Orchestrator` state (especially during crashes/restarts) is a significant complexity, addressed via the `reconcileTasks` logic.

Project Specification: SERA (Sandboxed Extensible Reasoning Agent)

Context for AI Agent: We are building SERA (Sandboxed Extensible Reasoning Agent), a local, open-source coding agent alternative to tools like OpenClaw or Devin. The system must run entirely locally, prioritize security via Docker-based sandboxing, and possess deep semantic understanding of the codebase. Crucially, SERA is designed to be highly extensible—capable of developing its own tools, orchestrating support agents, and evolving a persistent personality over time within a multi-container homelab environment.

Use the following open-source projects and concepts as architectural references when designing our system's components.

1. Core Execution & Sandboxing (Docker-Native)

Goal: Ensure the agent cannot harm the host system. All execution, file modifications, and package installations must occur within ephemeral Docker containers or microVMs.

Herm (aduermael/herm)

Focus Area: Containerized-by-default execution.

Study For: How it dynamically writes Dockerfiles based on project dependencies and executes every command inside the container.

Docker Agent (docker/docker-agent / cagent)

Focus Area: Official declarative AI runtime by Docker.

Study For: Using YAML configurations to define agent behaviors, tool permissions, and native integration with the Model Context Protocol (MCP).

NanoClaw

Focus Area: Security-first MicroVM/Docker wrapping.

Study For: Secure isolation strategies, handling persistent memory across container restarts, and secure scheduled task execution.

2. Cognitive Loop & Reasoning (The "AI Software Engineer")

Goal: The agent's ability to take a high-level prompt, formulate a multi-step plan, browse documentation, and iteratively write/test code.

OpenHands (formerly OpenDevin)

Focus Area: Mature, heavyweight agent architecture.

Study For: The sandbox execution environment (how the LLM interfaces with the containerized terminal via standard streams) and their browser-interaction modules for reading documentation.

Devika

Focus Area: Planning algorithms and local LLM routing.

Study For: Simpler, early-stage "planning and reasoning" algorithms. Good reference for abstracting the LLM provider to easily plug in local models via Ollama or vLLM.

3. Extensibility & CLI Workflows

Goal: Making the agent feel like a native developer tool that can easily integrate external capabilities.

Goose (by Block)

Focus Area: Extensibility and Model Context Protocol (MCP).

Study For: Phenomenal implementation of MCP servers. Look at how Goose cleanly separates core agent reasoning from the isolated tools it uses to interact with APIs, filesystems, and external services. This is the foundation for SERA writing its own tools.

OpenCode (anomalyco/opencode)

Focus Area: Decoupled Client/Server Architecture.

Study For: Running the heavy orchestration engine inside a container while driving it seamlessly via a Terminal User Interface (TUI) or separate client.

4. Semantic Code Understanding (LSP Integration)

Goal: Prevent token waste and hallucinations. The agent must understand the codebase semantically rather than relying on dumb string matching (grep).

Serena (oraios/serena)

Focus Area: Language Server Protocol (LSP) integration.

Study For: How it hooks into LSP to provide the agent with true IDE-like capabilities (e.g., find_referencing_symbols, go_to_definition). Integrating this prevents the agent from blindly reading entire files when searching for specific function implementations.

5. Autonomous Self-Extension, Memory, & Personality

Goal: The agent must be capable of self-improvement, writing its own MCP tools, spawning specialized support agents, and maintaining a long-term, evolving personality using structured local storage.

Letta (formerly MemGPT)

Focus Area: Infinite context and persistent personality.

Study For: How it manages a tiered memory system (working context vs. archival storage) to allow an agent to "remember" user preferences, past mistakes, and develop a consistent persona across sessions without blowing up the context window.

Obsidian & Vector Memory (Homelab RAG Stack)

Focus Area: Human-readable knowledge graphs and semantic search.

Study For: Storing the agent's long-term memory, personality state, and Architectural Decision Records (ADRs) as plain Markdown files in an Obsidian vault format. Pair this with a local Vector DB (e.g., Qdrant, ChromaDB, or pgvector) running as a separate container in your homelab to provide Retrieval-Augmented Generation (RAG) over the agent's past experiences and your codebase.

AutoGen / CrewAI

Focus Area: Multi-agent orchestration.

Study For: Patterns for allowing the primary "SERA" agent to spawn sub-agents (e.g., a dedicated "QA Tester Agent" or "Research Agent") to delegate tasks and synthesize their findings back into the main reasoning loop.

Recommended Architectural Stack

When drafting our initial design document, combine these paradigms for the multi-container homelab deployment:

Sandbox: Use Herm's approach for dynamic, containerized execution environments.

Tooling & Self-Extension: Implement Goose's approach to the Model Context Protocol (MCP) to standardize tool execution, allowing SERA to dynamically register new tools it writes for itself.

Code Reading: Integrate Serena's LSP methods so the agent has semantic awareness of the code graph.

Homelab Infrastructure & Memory: Deploy as a multi-container stack (Agent Core, Execution Sandbox, Vector DB). Use a Letta/MemGPT-style tiered memory where archival storage is saved as Obsidian-compatible Markdown files and embedded into the local Vector DB for semantic retrieval.
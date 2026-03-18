# SERA Core API Reference

This document provides a comprehensive catalog of all REST API endpoints available in the SERA Core service. It serves as a reference for interacting with agents, circles, memory, skills, and the sandbox environment.

## Base URL

By default, the SERA Core service listens on port 3001. All API endpoints are prefixed with `/api`.

- Local Development: `http://localhost:3001/api`
- Docker Deployment: `http://sera-core:3001/api`

---

## 1. Agents

Endpoints for managing agent configurations and manifests.

### List All Agents
- **Method**: `GET`
- **Path**: `/agents/`
- **Description**: Returns a list of all loaded agents.
- **Response** (200): `Array<AgentManifest>`

### Get Agent Details
- **Method**: `GET`
- **Path**: `/agents/:name`
- **Description**: Returns detailed information for a specific agent.
- **Response** (200): `AgentInfo`
- **Error** (404): `{ error: "Agent not found" }`

### Get Agent Manifest (Raw YAML)
- **Method**: `GET`
- **Path**: `/agents/:name/manifest/raw`
- **Description**: Retrieves the raw YAML manifest file for the agent.
- **Response** (200): `text/yaml`
- **Error** (404): `{ error: "Manifest file not found" }`

### Update Agent Manifest
- **Method**: `PUT`
- **Path**: `/agents/:name/manifest`
- **Description**: Updates the agent's manifest (or creates it) and triggers a live reload.
- **Request Body**: `AgentManifest` (JSON object)
- **Response** (200): `{ success: true, ...reloadResult }`
- **Errors**:
  - (400): Request body missing or invalid metadata.
  - (500): Internal server error writing manifest.

### Test Agent Persona
- **Method**: `POST`
- **Path**: `/agents/test-chat`
- **Description**: Simulates a chat session with a non-persisted agent manifest.
- **Request Body**: `{ manifest: AgentManifest, message: string, history?: Array<ChatMessage> }`
- **Response** (200): `{ reply: string, thought: string }`

### Reload All Agents
- **Method**: `POST`
- **Path**: `/agents/reload`
- **Description**: Forces a full reload of all agent manifests from disk.
- **Response** (200): `{ success: true, ...reloadResult }`

---

## 2. Circles

Endpoints for managing agent groups (circles) and collaborative environments.

### List All Circles
- **Method**: `GET`
- **Path**: `/circles/`
- **Description**: Returns summaries of all registered circles.
- **Response** (200): `Array<CircleSummary>`

### Get Circle Details
- **Method**: `GET`
- **Path**: `/circles/:name`
- **Description**: Retrieves details and project context for a specific circle.
- **Response** (200): `CircleDetails`
- **Error** (404): `{ error: "Circle not found" }`

### Create New Circle
- **Method**: `POST`
- **Path**: `/circles/`
- **Description**: Creates a new circle and saves its manifest.
- **Request Body**: `CircleManifest` (JSON object)
- **Response** (201): `{ success: true, name: "circle-name" }`
- **Errors**:
  - (400): Missing required fields.
  - (409): Circle already exists.

### Update Circle Manifest
- **Method**: `PUT`
- **Path**: `/circles/:name`
- **Description**: Updates an existing circle's configuration.
- **Request Body**: `CircleManifest`
- **Response** (200): `{ success: true }`
- **Error** (404): `{ error: "Circle not found" }`

### Delete Circle
- **Method**: `DELETE`
- **Path**: `/circles/:name`
- **Description**: Removes a circle manifest from disk.
- **Response** (200): `{ success: true }`
- **Error** (404): `{ error: "Circle not found on disk" }`

### Update Project Context
- **Method**: `PUT`
- **Path**: `/circles/:name/context`
- **Description**: Updates the project context markdown file for a circle.
- **Request Body**: `{ content: string }`
- **Response** (200): `{ success: true }`

---

## 3. Party Mode (Within Circles)

Endpoints for interactive, multi-agent conversational sessions.

### Create Party Session
- **Method**: `POST`
- **Path**: `/circles/:circleId/party`
- **Description**: Initializes a new party session for the specified circle.
- **Response** (201): `PartySessionInfo`

### Send Message to Party Session
- **Method**: `POST`
- **Path**: `/circles/:circleId/party/:sessionId`
- **Description**: Sends a message to the active party session.
- **Request Body**: `{ message: string }`
- **Response** (200): `{ sessionId: string, responses: Array<Response>, active: boolean }`

### End Party Session
- **Method**: `DELETE`
- **Path**: `/circles/:circleId/party/:sessionId`
- **Description**: Terminates a party session.
- **Response** (200): `{ success: true }`

### List Party Sessions
- **Method**: `GET`
- **Path**: `/circles/:circleId/party`
- **Description**: Lists active sessions for a circle.
- **Response** (200): `Array<PartySessionInfo>`

---

## 4. Chat & Execution

### Chat Interaction
- **Method**: `POST`
- **Path**: `/chat`
- **Description**: Sends a message to the primary agent and returns its response.
- **Request Body**: `{ message: string, conversationId?: string }`
- **Response** (200): `{ conversationId: string, reply: string, thought: string }`

### Execute Task (Orchestrator)
- **Method**: `POST`
- **Path**: `/execute`
- **Description**: Instructs the orchestrator to execute a task based on a prompt.
- **Request Body**: `{ prompt: string }`
- **Response** (200): `{ result: any }`

---

## 5. Memory System

Endpoints for interacting with short-term and long-term memory structures.

### Get All Memory Blocks
- **Method**: `GET`
- **Path**: `/memory/blocks`
- **Description**: Retrieves all specialized memory blocks.
- **Response** (200): `Array<MemoryBlock>`

### Get Memory Block by Type
- **Method**: `GET`
- **Path**: `/memory/blocks/:type`
- **Description**: Retrieves a specific memory block type.
- **Response** (200): `MemoryBlock`

### Add Memory Block Entry
- **Method**: `POST`
- **Path**: `/memory/blocks/:type`
- **Description**: Adds an entry to a specific memory block.
- **Request Body**: `{ title: string, content: string, refs?: Array<string>, tags?: Array<string>, source?: string }`
- **Response** (201): `MemoryEntry`

### Get Memory Entry
- **Method**: `GET`
- **Path**: `/memory/entries/:id`
- **Description**: Retrieves a specific memory entry by ID.
- **Response** (200): `MemoryEntry`

### Update Memory Entry
- **Method**: `PUT`
- **Path**: `/memory/entries/:id`
- **Description**: Updates the content of a memory entry.
- **Request Body**: `{ content: string }`
- **Response** (200): `MemoryEntry`

### Delete Memory Entry
- **Method**: `DELETE`
- **Path**: `/memory/entries/:id`
- **Description**: Deletes a memory entry.
- **Response** (200): `{ success: true }`

### Add Reference Link
- **Method**: `POST`
- **Path**: `/memory/entries/:id/refs`
- **Description**: Links one memory entry to another.
- **Request Body**: `{ targetId: string }`
- **Response** (200): `{ success: true }`

### Remove Reference Link
- **Method**: `DELETE`
- **Path**: `/memory/entries/:id/refs/:targetId`
- **Description**: Removes a link between memory entries.
- **Response** (200): `{ success: true }`

### Get Memory Graph
- **Method**: `GET`
- **Path**: `/memory/graph`
- **Description**: Returns the memory structure as a node-link graph.
- **Response** (200): `MemoryGraph`

### Search Memory
- **Method**: `GET`
- **Path**: `/memory/search`
- **Description**: Performs a semantic search across memory entries.
- **Query Parameters**: `query` (string, required), `limit` (number, optional)
- **Response** (200): `Array<SearchResult>`

---

## 6. Vector Search & Ingestion

### Trigger Ingestion
- **Method**: `POST`
- **Path**: `/ingest`
- **Description**: Starts an asynchronous codebase ingestion process.
- **Response** (200): `{ message: "Ingestion started" }`

### Vector Query
- **Method**: `POST`
- **Path**: `/query`
- **Description**: Queries the vector database for relevant code or documents.
- **Request Body**: `{ query: string, limit?: number }`
- **Response** (200): `{ results: Array<any> }`

---

## 7. Skills & Tools

### List All Skills
- **Method**: `GET`
- **Path**: `/skills/`
- **Description**: Lists all registered skills (both built-in and MCP-bridged) and which agents use them.
- **Response** (200): `Array<SkillInfo>`

### Update Allowed Tools (Informational)
- **Method**: `PUT`
- **Path**: `/skills/agents/:name/tools`
- **Description**: Validates tool additions for an agent (must use agent manifest PUT to persist).
- **Request Body**: `{ allowed: Array<string> }`
- **Response** (200): `{ success: true, message: string, currentAllowed: Array<string>, requested: Array<string> }`

---

## 8. Sandbox Manager

Endpoints for running isolated containers. All requests require an `agentName` to enforce RBAC.

### List Sandbox Containers
- **Method**: `GET`
- **Path**: `/sandbox/`
- **Description**: Lists active containers.
- **Query Parameters**: `agentName` (string, optional)
- **Response** (200): `Array<ContainerInfo>`

### Spawn Container
- **Method**: `POST`
- **Path**: `/sandbox/spawn`
- **Description**: Spawns a new container for persistent tasks or agents.
- **Request Body**: `{ agentName: string, type: string, image: string, command?: string, env?: object, workDir?: string, subagentRole?: string, task?: string }`
- **Response** (201): `ContainerResult`
- **Errors**: (403) Policy Violation

### Execute Command in Container
- **Method**: `POST`
- **Path**: `/sandbox/exec`
- **Description**: Runs a command in an already running container.
- **Request Body**: `{ agentName: string, containerId: string, command: string }`
- **Response** (200): `ExecResult`

### Remove Container
- **Method**: `DELETE`
- **Path**: `/sandbox/:id`
- **Description**: Stops and removes a container.
- **Query Parameters**: `agentName` (string, required)
- **Response** (200): `{ success: true }`

### Get Container Logs
- **Method**: `GET`
- **Path**: `/sandbox/:id/logs`
- **Description**: Retrieves logs from a container.
- **Query Parameters**: `tail` (number, optional)
- **Response** (200): `{ logs: string }`

### Run Ephemeral Tool Container
- **Method**: `POST`
- **Path**: `/sandbox/tool`
- **Description**: Runs a short-lived container to execute a single tool.
- **Request Body**: `{ agentName: string, toolName: string, command: string, image?: string, timeoutSeconds?: number }`
- **Response** (200): `ToolRunResult`

### Spawn Subagent Container
- **Method**: `POST`
- **Path**: `/sandbox/subagent`
- **Description**: Spawns a specialized subagent container.
- **Request Body**: `{ agentName: string, subagentRole: string, task: string, image?: string }`
- **Response** (201): `SubagentRunResult`

---

## 9. Intercom System

Message bus and event routing between agents.

### Publish Message
- **Method**: `POST`
- **Path**: `/intercom/publish`
- **Description**: Broadcasts a message to a specific channel.
- **Request Body**: `{ agent: string, channel: string, type: string, payload: object }`
- **Response** (200): `{ success: true, message: MessageObject }`

### Direct Message
- **Method**: `POST`
- **Path**: `/intercom/dm`
- **Description**: Sends a message directly from one agent to another.
- **Request Body**: `{ from: string, to: string, payload: object }`
- **Response** (200): `{ success: true, message: MessageObject }`

### Get Channel History
- **Method**: `GET`
- **Path**: `/intercom/history`
- **Description**: Retrieves recent messages from a channel.
- **Query Parameters**: `channel` (string, required), `limit` (number, optional)
- **Response** (200): `{ channel: string, messages: Array<MessageObject> }`

### List Agent Channels
- **Method**: `GET`
- **Path**: `/intercom/channels`
- **Description**: Lists all channels an agent is subscribed to or allowed to access.
- **Query Parameters**: `agent` (string, required)
- **Response** (200): `Array<string>`

---

## 10. Language Server Protocol (LSP)

IDE-like code intelligence features powered by LSP.

### Get Definition
- **Method**: `POST`
- **Path**: `/lsp/definition`
- **Description**: Finds the definition for a symbol at a specific file location.
- **Request Body**: `{ filePath: string, line: number, character: number }`
- **Response** (200): `{ definition: any }`

### Get References
- **Method**: `POST`
- **Path**: `/lsp/references`
- **Description**: Finds all references for a symbol at a specific file location.
- **Request Body**: `{ filePath: string, line: number, character: number }`
- **Response** (200): `{ references: any }`

### Get Document Symbols
- **Method**: `POST`
- **Path**: `/lsp/symbols`
- **Description**: Returns all symbols found within a file.
- **Request Body**: `{ filePath: string }`
- **Response** (200): `{ symbols: any }`

---

## 11. System Configuration & Providers

### Health Check
- **Method**: `GET`
- **Path**: `/health`
- **Description**: Service health check.
- **Response** (200): `{ status: "ok", service: "sera-core", timestamp: "..." }`

### Get LLM Config
- **Method**: `GET`
- **Path**: `/config/llm`
- **Description**: Returns current legacy LLM configuration.
- **Response** (200): `LLMConfig`

### Update LLM Config
- **Method**: `POST`
- **Path**: `/config/llm`
- **Description**: Updates the legacy LLM configuration and reinitializes the provider.
- **Request Body**: `LLMConfig`
- **Response** (200): `{ success: true }`

### Test LLM Config
- **Method**: `POST`
- **Path**: `/config/llm/test`
- **Description**: Tests the current LLM connection.
- **Response** (200): `{ success: boolean, model: string, response?: string, error?: string }`

### Get Providers Catalog
- **Method**: `GET`
- **Path**: `/providers`
- **Description**: Lists all available provider configurations (LM Studio, OpenAI, etc.).
- **Response** (200): `{ activeProvider: string, providers: Array<ProviderConfig> }`

### Update Provider Config
- **Method**: `PUT`
- **Path**: `/providers/:id`
- **Description**: Updates settings for a specific provider.
- **Request Body**: `{ baseUrl: string, apiKey: string, model: string }`
- **Response** (200): `{ success: true }`

### Test Specific Provider
- **Method**: `POST`
- **Path**: `/providers/:id/test`
- **Description**: Validates a specific provider connection before making it active.
- **Response** (200): `{ success: boolean, provider: string, response?: string, error?: string }`

### Set Active Provider
- **Method**: `POST`
- **Path**: `/providers/active`
- **Description**: Sets a specific provider as the globally active LLM provider.
- **Request Body**: `{ providerId: string }`
- **Response** (200): `{ success: true, activeProvider: string }`

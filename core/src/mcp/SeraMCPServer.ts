import { Server } from '@modelcontextprotocol/sdk/server/index.js';
import { CallToolRequestSchema, ListToolsRequestSchema } from '@modelcontextprotocol/sdk/types.js';
import { v4 as uuidv4 } from 'uuid';
import type { Orchestrator } from '../agents/Orchestrator.js';
import { CircleService } from '../circles/CircleService.js';
import { pool } from '../lib/database.js';
import { AuditService } from '../audit/AuditService.js';
import { ActingContextBuilder, type DelegationScope } from '../identity/acting-context.js';
import type { ProcessTask } from '../agents/process/types.js';

/**
 * SeraMCPServer — an embedded MCP server that exposes platform management tools.
 */
export class SeraMCPServer {
  public readonly server: Server;
  private circleService = CircleService.getInstance();

  constructor(private orchestrator: Orchestrator) {
    this.server = new Server(
      {
        name: 'sera-core',
        version: '1.0.0',
      },
      {
        capabilities: {
          tools: {},
        },
      }
    );

    this.setupHandlers();
  }

  public getToolDefinitions() {
    return [
      {
        name: 'list_agents',
        description: 'List all active agents and their status.',
        inputSchema: { type: 'object', properties: {} },
      },
      {
        name: 'restart_agent',
        description: 'Restart a specific agent by ID.',
        inputSchema: {
          type: 'object',
          properties: {
            agentId: { type: 'string' },
          },
          required: ['agentId'],
        },
      },
      // ── Circle Management (Story 10.1) ──────────────────────────────────
      {
        name: 'circles.create',
        description: 'Create a new circle.',
        inputSchema: {
          type: 'object',
          properties: {
            name: { type: 'string', description: "Slug name (e.g. 'security-council')" },
            displayName: { type: 'string' },
            description: { type: 'string' },
            constitution: { type: 'string', description: 'Markdown constitution' },
          },
          required: ['name', 'displayName'],
        },
      },
      {
        name: 'circles.list',
        description: 'List all circles.',
        inputSchema: { type: 'object', properties: {} },
      },
      {
        name: 'circles.add_member',
        description: 'Add an agent instance to a circle.',
        inputSchema: {
          type: 'object',
          properties: {
            circleId: { type: 'string' },
            agentId: { type: 'string' },
          },
          required: ['circleId', 'agentId'],
        },
      },
      // ── Coordination (Story 10.3) ───────────────────────────────────────
      {
        name: 'orchestration.sequential',
        description: 'Run tasks in sequence across agents.',
        inputSchema: {
          type: 'object',
          properties: {
            tasks: {
              type: 'array',
              items: {
                type: 'object',
                properties: {
                  id: { type: 'string' },
                  description: { type: 'string' },
                  assignedAgent: { type: 'string' },
                },
                required: ['id', 'description'],
              },
            },
          },
          required: ['tasks'],
        },
      },
      {
        name: 'orchestration.parallel',
        description: 'Run multiple tasks in parallel across agents.',
        inputSchema: {
          type: 'object',
          properties: {
            tasks: {
              type: 'array',
              items: {
                type: 'object',
                properties: {
                  id: { type: 'string' },
                  description: { type: 'string' },
                  assignedAgent: { type: 'string' },
                },
                required: ['id', 'description'],
              },
            },
          },
          required: ['tasks'],
        },
      },
      {
        name: 'orchestration.hierarchical',
        description: 'Run tasks with a manager agent overseeing and validating results.',
        inputSchema: {
          type: 'object',
          properties: {
            managerAgent: { type: 'string', description: 'Name of the manager agent' },
            tasks: {
              type: 'array',
              items: {
                type: 'object',
                properties: {
                  id: { type: 'string' },
                  description: { type: 'string' },
                  assignedAgent: { type: 'string' },
                },
                required: ['id', 'description'],
              },
            },
          },
          required: ['managerAgent', 'tasks'],
        },
      },
      // ── Party Mode (Story 10.5) ─────────────────────────────────────────
      {
        name: 'circle.broadcast',
        description: 'Broadcast a message to all members of a circle.',
        inputSchema: {
          type: 'object',
          properties: {
            circleId: { type: 'string' },
            payload: { type: 'object' },
          },
          required: ['circleId', 'payload'],
        },
      },
      // ── Chat ─────────────────────────────────────────────────────────────
      {
        name: 'chat',
        description: 'Send a message to a SERA agent and get a response. Returns the agent reply.',
        inputSchema: {
          type: 'object',
          properties: {
            agentName: {
              type: 'string',
              description: 'Name of the agent to chat with (e.g. "sera")',
            },
            message: { type: 'string', description: 'The message to send' },
            sessionId: {
              type: 'string',
              description: 'Optional session ID for continuing a conversation',
            },
          },
          required: ['agentName', 'message'],
        },
      },
      {
        name: 'list_sessions',
        description: 'List chat sessions for an agent.',
        inputSchema: {
          type: 'object',
          properties: {
            agentInstanceId: { type: 'string', description: 'Agent instance ID' },
          },
          required: ['agentInstanceId'],
        },
      },
      {
        name: 'knowledge_query',
        description: "Semantic search across an agent's knowledge. Returns relevant entries.",
        inputSchema: {
          type: 'object',
          properties: {
            agentId: { type: 'string', description: 'Agent instance ID' },
            query: { type: 'string', description: 'Search query text' },
            tags: {
              type: 'array',
              items: { type: 'string' },
              description: 'Optional tag filter',
            },
            topK: { type: 'number', description: 'Max results (default 10)' },
          },
          required: ['agentId', 'query'],
        },
      },
      {
        name: 'knowledge_store',
        description: 'Store a knowledge entry for an agent.',
        inputSchema: {
          type: 'object',
          properties: {
            agentId: { type: 'string', description: 'Agent instance ID' },
            content: { type: 'string', description: 'Content to store' },
            type: {
              type: 'string',
              description:
                'Block type: fact, context, memory, insight, reference, observation, decision',
            },
            title: { type: 'string', description: 'Title for the entry' },
            tags: {
              type: 'array',
              items: { type: 'string' },
              description: 'Tags for the entry',
            },
            importance: { type: 'number', description: 'Importance 1-5 (default 3)' },
          },
          required: ['agentId', 'content', 'type'],
        },
      },
      // ── Extended Agent Management ────────────────────────────────────
      {
        name: 'agent_status',
        description: 'Get detailed status and info for a specific agent by instance ID or name.',
        inputSchema: {
          type: 'object',
          properties: {
            agentId: { type: 'string', description: 'Agent instance ID or name' },
          },
          required: ['agentId'],
        },
      },
      {
        name: 'start_agent',
        description: 'Start an existing agent instance by ID.',
        inputSchema: {
          type: 'object',
          properties: {
            agentId: { type: 'string', description: 'Agent instance ID' },
            task: { type: 'string', description: 'Optional initial task description' },
          },
          required: ['agentId'],
        },
      },
      {
        name: 'stop_agent',
        description: 'Stop a running agent instance by ID.',
        inputSchema: {
          type: 'object',
          properties: {
            agentId: { type: 'string', description: 'Agent instance ID' },
          },
          required: ['agentId'],
        },
      },
      {
        name: 'create_agent',
        description: 'Create a new agent instance from an existing template.',
        inputSchema: {
          type: 'object',
          properties: {
            templateName: { type: 'string', description: 'Template name to create from' },
            name: { type: 'string', description: 'Name for the new instance' },
            circleId: { type: 'string', description: 'Optional circle ID to join' },
            task: { type: 'string', description: 'Optional initial task to start with' },
          },
          required: ['templateName', 'name'],
        },
      },
      {
        name: 'agent_capabilities',
        description: "Query an agent's manifest capabilities before delegating or interacting.",
        inputSchema: {
          type: 'object',
          properties: {
            agentName: { type: 'string', description: 'Agent name or instance ID' },
          },
          required: ['agentName'],
        },
      },
      // ── Chat History ─────────────────────────────────────────────────────
      {
        name: 'chat_history',
        description: 'Retrieve the message history for a specific chat session.',
        inputSchema: {
          type: 'object',
          properties: {
            sessionId: { type: 'string', description: 'Session ID' },
            limit: { type: 'number', description: 'Max messages to return (default 50)' },
          },
          required: ['sessionId'],
        },
      },
      // ── Memory Blocks ────────────────────────────────────────────────────
      {
        name: 'memory_blocks',
        description: "List an agent's memory blocks with optional tag and importance filtering.",
        inputSchema: {
          type: 'object',
          properties: {
            agentId: { type: 'string', description: 'Agent instance ID' },
            type: {
              type: 'string',
              description:
                'Filter by block type: fact, context, memory, insight, reference, observation, decision',
            },
            tags: {
              type: 'array',
              items: { type: 'string' },
              description: 'Filter blocks that have any of these tags',
            },
            minImportance: {
              type: 'number',
              description: 'Filter by minimum importance (1-5)',
            },
            limit: { type: 'number', description: 'Max blocks to return (default 50)' },
          },
          required: ['agentId'],
        },
      },
      // ── A2A Delegation ───────────────────────────────────────────────────
      {
        name: 'delegate_task',
        description:
          'Delegate a task to a Sera agent and get the result. Creates a new session for each call.',
        inputSchema: {
          type: 'object',
          properties: {
            agentName: { type: 'string', description: 'Name of the agent to delegate to' },
            task: { type: 'string', description: 'Task description to send to the agent' },
            context: {
              type: 'string',
              description: 'Optional additional context for the agent',
            },
          },
          required: ['agentName', 'task'],
        },
      },
      // ── Subagent Spawning (Story 10.5 / 10.4 / 17.4) ──────────────────
      {
        name: 'agents.spawn_subagent',
        description:
          'Spawn a subagent to handle a delegated subtask. Only available to agents with permissions.canSpawnSubagents.',
        inputSchema: {
          type: 'object',
          properties: {
            role: {
              type: 'string',
              description: 'Subagent role (must be in manifest subagents.allowed)',
            },
            task: { type: 'string', description: 'Task description for the subagent' },
            circle: {
              type: 'string',
              description: "Circle to join. Pass 'none' to skip circle inheritance.",
            },
            parentAgentId: {
              type: 'string',
              description: "Calling agent's instance ID (required for delegation passthrough)",
            },
            delegations: {
              type: 'array',
              description:
                "Delegation tokens to pass to the subagent. Each token must be owned by the calling agent. Child scope may be narrower but not broader than the parent token's scope.",
              items: {
                type: 'object',
                properties: {
                  delegationTokenId: { type: 'string' },
                  narrowedScope: {
                    type: 'object',
                    properties: {
                      service: { type: 'string' },
                      permissions: { type: 'array', items: { type: 'string' } },
                      resourceConstraints: { type: 'object' },
                    },
                    required: ['service', 'permissions'],
                  },
                },
                required: ['delegationTokenId'],
              },
            },
          },
          required: ['role', 'task'],
        },
      },
    ];
  }

  private setupHandlers() {
    this.server.setRequestHandler(ListToolsRequestSchema, async () => {
      return { tools: this.getToolDefinitions() };
    });

    this.server.setRequestHandler(CallToolRequestSchema, async (request) => {
      const { name, arguments: args } = request.params;
      return this.callTool(name, args);
    });
  }

  public async callTool(name: string, args: unknown) {
    try {
      const toolArgs = (args || {}) as Record<string, unknown>;
      switch (name) {
        case 'list_agents':
          return this.handleListAgents();
        case 'restart_agent':
          return this.handleRestartAgent(toolArgs?.agentId as string);
        case 'circles.create':
          return this.handleCreateCircle(toolArgs);
        case 'circles.list':
          return this.handleListCircles();
        case 'circles.add_member':
          return this.handleAddMember(
            toolArgs['circleId'] as string,
            toolArgs['agentId'] as string
          );
        case 'orchestration.sequential':
          return this.handleSequentialOrchestration(toolArgs['tasks'] as ProcessTask[]);
        case 'orchestration.parallel':
          return this.handleParallelOrchestration(toolArgs['tasks'] as ProcessTask[]);
        case 'orchestration.hierarchical':
          return this.handleHierarchicalOrchestration(
            toolArgs['managerAgent'] as string,
            toolArgs['tasks'] as ProcessTask[]
          );
        case 'circle.broadcast':
          return this.handleCircleBroadcast(toolArgs['circleId'] as string, toolArgs['payload']);
        case 'chat':
          return this.handleChat(
            toolArgs['agentName'] as string,
            toolArgs['message'] as string,
            toolArgs['sessionId'] as string | undefined
          );
        case 'list_sessions':
          return this.handleListSessions(toolArgs['agentInstanceId'] as string);
        case 'knowledge_query':
          return this.handleKnowledgeQuery(
            toolArgs['agentId'] as string,
            toolArgs['query'] as string,
            toolArgs['tags'] as string[] | undefined,
            toolArgs['topK'] as number | undefined
          );
        case 'knowledge_store':
          return this.handleKnowledgeStore(
            toolArgs['agentId'] as string,
            toolArgs['content'] as string,
            toolArgs['type'] as string,
            toolArgs['title'] as string | undefined,
            toolArgs['tags'] as string[] | undefined,
            toolArgs['importance'] as number | undefined
          );
        case 'agent_status':
          return this.handleAgentStatus(toolArgs['agentId'] as string);
        case 'start_agent':
          return this.handleStartAgent(
            toolArgs['agentId'] as string,
            toolArgs['task'] as string | undefined
          );
        case 'stop_agent':
          return this.handleStopAgent(toolArgs['agentId'] as string);
        case 'create_agent':
          return this.handleCreateAgent(
            toolArgs['templateName'] as string,
            toolArgs['name'] as string,
            toolArgs['circleId'] as string | undefined,
            toolArgs['task'] as string | undefined
          );
        case 'agent_capabilities':
          return this.handleAgentCapabilities(toolArgs['agentName'] as string);
        case 'chat_history':
          return this.handleChatHistory(
            toolArgs['sessionId'] as string,
            toolArgs['limit'] as number | undefined
          );
        case 'memory_blocks':
          return this.handleMemoryBlocks(
            toolArgs['agentId'] as string,
            toolArgs['type'] as string | undefined,
            toolArgs['tags'] as string[] | undefined,
            toolArgs['minImportance'] as number | undefined,
            toolArgs['limit'] as number | undefined
          );
        case 'delegate_task':
          return this.handleDelegateTask(
            toolArgs['agentName'] as string,
            toolArgs['task'] as string,
            toolArgs['context'] as string | undefined
          );
        case 'agents.spawn_subagent':
          return this.handleSpawnSubagent(
            toolArgs['role'] as string,
            toolArgs['task'] as string,
            toolArgs['circle'] as string,
            toolArgs['parentAgentId'] as string,
            toolArgs['delegations'] as Array<{
              delegationTokenId: string;
              narrowedScope?: DelegationScope;
            }>
          );
        default:
          throw new Error(`Tool not found: ${name}`);
      }
    } catch (err: unknown) {
      return {
        isError: true,
        content: [{ type: 'text', text: (err as Error).message }],
      };
    }
  }

  private handleListAgents() {
    const agents = this.orchestrator.listAgents().map((a) => ({
      id: a.id,
      name: a.name,
      status: a.status,
      startTime: a.startTime,
    }));
    return {
      content: [{ type: 'text', text: JSON.stringify(agents, null, 2) }],
    };
  }

  private async handleRestartAgent(agentId: string) {
    await this.orchestrator.restartAgent(agentId);
    return {
      content: [{ type: 'text', text: `Agent "${agentId}" restarted successfully.` }],
    };
  }

  private async handleCreateCircle(args: Record<string, unknown>) {
    const circle = await this.circleService.createCircle({
      name: args.name as string,
      displayName: args.displayName as string,
      description: args.description as string,
      constitution: args.constitution as string,
    });
    return {
      content: [{ type: 'text', text: `Circle "${circle.name}" created with ID: ${circle.id}` }],
    };
  }

  private async handleListCircles() {
    const circles = await this.circleService.listCircles();
    return {
      content: [{ type: 'text', text: JSON.stringify(circles, null, 2) }],
    };
  }

  private async handleAddMember(circleId: string, agentId: string) {
    await this.circleService.addMember(circleId, agentId);
    return {
      content: [{ type: 'text', text: `Agent "${agentId}" added to circle "${circleId}"` }],
    };
  }

  private async handleSequentialOrchestration(tasks: ProcessTask[]) {
    const result = await this.orchestrator.executeWithProcess('sequential', tasks);
    return {
      content: [{ type: 'text', text: JSON.stringify(result, null, 2) }],
    };
  }

  private async handleParallelOrchestration(tasks: ProcessTask[]) {
    const result = await this.orchestrator.executeWithProcess('parallel', tasks);
    return {
      content: [{ type: 'text', text: JSON.stringify(result, null, 2) }],
    };
  }

  private async handleHierarchicalOrchestration(managerAgent: string, tasks: ProcessTask[]) {
    const result = await this.orchestrator.executeWithProcess('hierarchical', tasks, managerAgent);
    return {
      content: [{ type: 'text', text: JSON.stringify(result, null, 2) }],
    };
  }

  private async handleCircleBroadcast(circleId: string, payload: unknown) {
    const intercom = this.orchestrator.getIntercom();
    if (!intercom) throw new Error('Intercom service not available');
    await intercom.publish(`circle:${circleId}`, payload);
    return {
      content: [{ type: 'text', text: `Broadcast sent to circle "${circleId}"` }],
    };
  }

  private async handleSpawnSubagent(
    role: string,
    task: string,
    circle?: string,
    parentAgentId?: string,
    delegations?: Array<{ delegationTokenId: string; narrowedScope?: DelegationScope }>
  ) {
    if (!role || !task) throw new Error('role and task are required');

    const { AgentFactory } = await import('../agents/AgentFactory.js');

    // Find a template matching the requested role
    const manifests = this.orchestrator.getAllManifests();
    const template = manifests.find((m) => m.identity.role === role || m.metadata.name === role);
    if (!template) {
      throw new Error(`No agent template found for role "${role}"`);
    }

    // Determine circle for the subagent
    const skipCircle = circle === 'none';
    const circleId = skipCircle ? undefined : (circle ?? template.metadata.circle);

    const instanceName = `subagent-${role}-${uuidv4().slice(0, 8)}`;
    const instance = await AgentFactory.createInstance(
      template.metadata.name,
      instanceName,
      '',
      circleId
    );

    // Story 17.4 — Agent-to-subagent delegation
    const childDelegationIds: string[] = [];
    if (delegations && delegations.length > 0 && parentAgentId) {
      const childInstanceId = instance.id;

      for (const delegation of delegations) {
        const { delegationTokenId, narrowedScope } = delegation;

        // Verify the parent token exists and belongs to the calling agent
        const { rows } = await pool.query(
          `SELECT * FROM delegation_tokens
           WHERE id = $1
             AND (actor_agent_id = $2 OR actor_instance_id::text = $2)
             AND revoked_at IS NULL
             AND (expires_at IS NULL OR expires_at > now())`,
          [delegationTokenId, parentAgentId]
        );

        const parentToken = rows[0];
        if (!parentToken) {
          throw new Error(
            `CapabilityEscalationError: delegation token "${delegationTokenId}" not found or not accessible by agent "${parentAgentId}"`
          );
        }

        const parentScope = parentToken.scope as DelegationScope;
        const childScope: DelegationScope = narrowedScope ?? parentScope;

        // Scope intersection validation — child cannot exceed parent
        const scopeError = ActingContextBuilder.validateScopeNarrowing(parentScope, childScope);
        if (scopeError) {
          throw new Error(`CapabilityEscalationError: ${scopeError}`);
        }

        // Issue child delegation token
        const childId = uuidv4();
        await pool.query(
          `INSERT INTO delegation_tokens
           (id, principal_type, principal_id, principal_name,
            actor_agent_id, actor_instance_id, scope, grant_type,
            credential_secret_name, issued_at, expires_at, parent_delegation_id)
           VALUES ($1,$2,$3,$4,$5,$6,$7,$8,$9,$10,$11,$12)`,
          [
            childId,
            parentToken.principal_type,
            parentToken.principal_id,
            parentToken.principal_name,
            template.metadata.name,
            childInstanceId,
            JSON.stringify(childScope),
            'session',
            parentToken.credential_secret_name,
            new Date(),
            parentToken.expires_at,
            delegationTokenId,
          ]
        );

        childDelegationIds.push(childId);

        // Audit
        await AuditService.getInstance()
          .record({
            actorType: 'agent',
            actorId: parentAgentId,
            actingContext: null,
            eventType: 'delegation.derived',
            payload: {
              parentDelegationId: delegationTokenId,
              childDelegationId: childId,
              childAgentId: childInstanceId,
              narrowedScope: childScope,
            },
          })
          .catch(() => {});
      }
    }

    await this.orchestrator.startInstance(instance.id, undefined, task);

    return {
      content: [
        {
          type: 'text',
          text: JSON.stringify({
            instanceId: instance.id,
            role,
            task,
            childDelegationIds,
          }),
        },
      ],
    };
  }

  // ── Chat & Knowledge handlers ───────────────────────────────────────────

  private async handleChat(agentName: string, message: string, sessionId?: string) {
    if (!agentName || !message) throw new Error('agentName and message are required');

    // Call the internal HTTP API directly
    const port = process.env.PORT ?? '3001';
    const apiKey =
      process.env.SERA_BOOTSTRAP_API_KEY ?? process.env.SERA_API_KEY ?? 'sera_bootstrap_dev_123';
    const body: Record<string, string> = { agentName, message };
    if (sessionId) body.sessionId = sessionId;

    const res = await fetch(`http://localhost:${port}/api/chat`, {
      method: 'POST',
      headers: {
        'Content-Type': 'application/json',
        Authorization: `Bearer ${apiKey}`,
      },
      body: JSON.stringify(body),
      signal: AbortSignal.timeout(120_000),
    });

    if (!res.ok) {
      const err = (await res.json().catch(() => ({ error: `HTTP ${res.status}` }))) as {
        error?: string;
      };
      throw new Error(err.error ?? `Chat request failed with status ${res.status}`);
    }

    const result = (await res.json()) as { sessionId: string; reply: string; thought?: string };
    return {
      content: [
        {
          type: 'text',
          text: JSON.stringify({
            sessionId: result.sessionId,
            reply: result.reply,
          }),
        },
      ],
    };
  }

  private async handleListSessions(agentInstanceId: string) {
    if (!agentInstanceId) throw new Error('agentInstanceId is required');
    const { rows } = await pool.query(
      `SELECT id, title, message_count, created_at, updated_at
       FROM sessions WHERE agent_instance_id = $1
       ORDER BY updated_at DESC LIMIT 20`,
      [agentInstanceId]
    );
    return {
      content: [{ type: 'text', text: JSON.stringify(rows, null, 2) }],
    };
  }

  private async handleKnowledgeQuery(
    agentId: string,
    query: string,
    tags?: string[],
    topK?: number
  ) {
    if (!agentId || !query) throw new Error('agentId and query are required');

    const { EmbeddingService } = await import('../services/embedding.service.js');
    const { VectorService } = await import('../services/vector.service.js');
    const embeddingService = EmbeddingService.getInstance();
    const vectorService = new VectorService('_mcp_search');

    if (!embeddingService.isAvailable()) {
      throw new Error('Embedding service unavailable — RAG disabled');
    }

    const queryVector = await embeddingService.embed(query);
    const filter = tags && tags.length > 0 ? { tags } : undefined;
    const results = await vectorService.search(
      [`personal:${agentId}`],
      queryVector,
      topK ?? 10,
      filter
    );

    const entries = results.map((r) => ({
      id: r.id,
      score: r.score,
      title: r.payload.title,
      type: r.payload.type,
      content: r.payload.content,
      tags: r.payload.tags,
    }));

    return {
      content: [{ type: 'text', text: JSON.stringify(entries, null, 2) }],
    };
  }

  private async handleKnowledgeStore(
    agentId: string,
    content: string,
    type: string,
    title?: string,
    tags?: string[],
    importance?: number
  ) {
    if (!agentId || !content || !type) throw new Error('agentId, content, and type are required');

    const { ScopedMemoryBlockStore } = await import('../memory/blocks/ScopedMemoryBlockStore.js');
    const store = new ScopedMemoryBlockStore(process.env.MEMORY_PATH ?? '/memory');
    const block = await store.write({
      agentId,
      content,
      type: type as
        | 'fact'
        | 'context'
        | 'memory'
        | 'insight'
        | 'reference'
        | 'observation'
        | 'decision',
      ...(title ? { title } : {}),
      ...(tags ? { tags } : {}),
      ...(importance
        ? { importance: Math.max(1, Math.min(5, importance)) as 1 | 2 | 3 | 4 | 5 }
        : {}),
    });

    // Index in vector store if embedding available
    try {
      const { EmbeddingService } = await import('../services/embedding.service.js');
      const { VectorService } = await import('../services/vector.service.js');
      const embeddingService = EmbeddingService.getInstance();
      if (embeddingService.isAvailable()) {
        const vector = await embeddingService.embed(`${block.title}\n${block.content}`);
        const vectorService = new VectorService('_mcp_store');
        const ns = `personal:${agentId}` as const;
        await vectorService.upsert(block.id, ns, vector, {
          agent_id: agentId,
          created_at: block.timestamp,
          tags: block.tags,
          type: block.type,
          title: block.title,
          content: block.content,
          importance: block.importance,
          namespace: ns,
        });
      }
    } catch {
      // Non-fatal — block is stored even if indexing fails
    }

    return {
      content: [
        {
          type: 'text',
          text: `Stored ${type} block "${block.title}" (id: ${block.id})`,
        },
      ],
    };
  }

  // ── Extended Agent Management ─────────────────────────────────────────────

  private handleAgentStatus(agentId: string) {
    if (!agentId) throw new Error('agentId is required');

    // Try running agents first (by ID or name)
    const running = this.orchestrator.getAgent(agentId);
    if (running) {
      return {
        content: [
          {
            type: 'text',
            text: JSON.stringify(
              {
                id: running.agentInstanceId,
                name: running.name,
                status: running.status,
                startTime: running.startTime,
              },
              null,
              2
            ),
          },
        ],
      };
    }

    // Fall back to manifest info (template / stopped instance)
    const info = this.orchestrator.getAgentInfo(agentId);
    if (info) {
      return {
        content: [
          {
            type: 'text',
            text: JSON.stringify(
              { name: info.name, status: 'stopped', manifest: info.manifest.metadata },
              null,
              2
            ),
          },
        ],
      };
    }

    throw new Error(`Agent "${agentId}" not found`);
  }

  private async handleStartAgent(agentId: string, task?: string) {
    if (!agentId) throw new Error('agentId is required');
    await this.orchestrator.startInstance(agentId, undefined, task);
    return {
      content: [{ type: 'text', text: `Agent "${agentId}" started.` }],
    };
  }

  private async handleStopAgent(agentId: string) {
    if (!agentId) throw new Error('agentId is required');
    await this.orchestrator.stopInstance(agentId);
    return {
      content: [{ type: 'text', text: `Agent "${agentId}" stopped.` }],
    };
  }

  private async handleCreateAgent(
    templateName: string,
    name: string,
    circleId?: string,
    task?: string
  ) {
    if (!templateName || !name) throw new Error('templateName and name are required');
    const { AgentFactory } = await import('../agents/AgentFactory.js');
    const instance = await AgentFactory.createInstance(templateName, name, '', circleId);
    if (task) {
      await this.orchestrator.startInstance(instance.id, undefined, task);
    }
    return {
      content: [
        {
          type: 'text',
          text: JSON.stringify(
            { instanceId: instance.id, name: instance.name, templateName },
            null,
            2
          ),
        },
      ],
    };
  }

  private handleAgentCapabilities(agentName: string) {
    if (!agentName) throw new Error('agentName is required');

    const info = this.orchestrator.getAgentInfo(agentName);
    if (!info) throw new Error(`Agent "${agentName}" not found`);

    const { manifest } = info;
    const m = manifest as unknown as Record<string, unknown>;
    const spec = m['spec'] as Record<string, unknown> | undefined;
    const capabilities = spec?.['capabilities'] ?? m['capabilities'];
    const identity = spec?.['identity'] ?? m['identity'];
    const model = spec?.['model'] ?? m['model'];

    return {
      content: [
        {
          type: 'text',
          text: JSON.stringify({ name: agentName, identity, model, capabilities }, null, 2),
        },
      ],
    };
  }

  // ── Chat History ──────────────────────────────────────────────────────────

  private async handleChatHistory(sessionId: string, limit?: number) {
    if (!sessionId) throw new Error('sessionId is required');
    const { rows } = await pool.query(
      `SELECT id, role, content, created_at, metadata
       FROM session_messages
       WHERE session_id = $1
       ORDER BY created_at ASC
       LIMIT $2`,
      [sessionId, limit ?? 50]
    );
    return {
      content: [{ type: 'text', text: JSON.stringify(rows, null, 2) }],
    };
  }

  // ── Memory Blocks ─────────────────────────────────────────────────────────

  private async handleMemoryBlocks(
    agentId: string,
    type?: string,
    tags?: string[],
    minImportance?: number,
    limit?: number
  ) {
    if (!agentId) throw new Error('agentId is required');
    const { ScopedMemoryBlockStore } = await import('../memory/blocks/ScopedMemoryBlockStore.js');
    const store = new ScopedMemoryBlockStore(process.env.MEMORY_PATH ?? '/memory');

    const { KNOWLEDGE_BLOCK_TYPES } = await import('../memory/blocks/scoped-types.js');
    const blocks = await store.list(agentId, {
      ...(type && (KNOWLEDGE_BLOCK_TYPES as readonly string[]).includes(type)
        ? { type: type as (typeof KNOWLEDGE_BLOCK_TYPES)[number] }
        : {}),
      ...(tags && tags.length > 0 ? { tags } : {}),
      ...(minImportance !== undefined ? { minImportance } : {}),
    });

    const results = blocks.slice(0, limit ?? 50).map((b) => ({
      id: b.id,
      type: b.type,
      title: b.title,
      content: b.content,
      tags: b.tags,
      importance: b.importance,
      timestamp: b.timestamp,
    }));

    return {
      content: [{ type: 'text', text: JSON.stringify(results, null, 2) }],
    };
  }

  // ── A2A Delegation ────────────────────────────────────────────────────────

  private async handleDelegateTask(agentName: string, task: string, context?: string) {
    if (!agentName || !task) throw new Error('agentName and task are required');
    const message = context ? `${task}\n\nContext:\n${context}` : task;
    return this.handleChat(agentName, message, undefined);
  }
}

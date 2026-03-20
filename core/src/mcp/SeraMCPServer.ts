import { Server } from "@modelcontextprotocol/sdk/server/index.js";
import { type CallToolRequest, CallToolRequestSchema, ListToolsRequestSchema } from "@modelcontextprotocol/sdk/types.js";
import { v4 as uuidv4 } from "uuid";
import type { Orchestrator } from "../agents/Orchestrator.js";
import { Logger } from "../lib/logger.js";
import { CircleService } from "../circles/CircleService.js";
import { pool } from "../lib/database.js";
import { AuditService } from "../audit/AuditService.js";
import {
  ActingContextBuilder,
  type DelegationScope,
} from "../identity/acting-context.js";

const logger = new Logger('SeraMCPServer');

/**
 * SeraMCPServer — an embedded MCP server that exposes platform management tools.
 */
export class SeraMCPServer {
  public readonly server: Server;
  private circleService = CircleService.getInstance();

  constructor(private orchestrator: Orchestrator) {
    this.server = new Server(
      {
        name: "sera-core",
        version: "1.0.0",
      },
      {
        capabilities: {
          tools: {},
        },
      }
    );

    this.setupHandlers();
  }

  private setupHandlers() {
    this.server.setRequestHandler(ListToolsRequestSchema, async () => {
      return {
        tools: [
          {
            name: "list_agents",
            description: "List all active agents and their status.",
            inputSchema: { type: "object", properties: {} },
          },
          {
            name: "restart_agent",
            description: "Restart a specific agent by ID.",
            inputSchema: {
              type: "object",
              properties: {
                agentId: { type: "string" },
              },
              required: ["agentId"],
            },
          },
          // ── Circle Management (Story 10.1) ──────────────────────────────────
          {
            name: "circles.create",
            description: "Create a new circle.",
            inputSchema: {
              type: "object",
              properties: {
                name: { type: "string", description: "Slug name (e.g. 'security-council')" },
                displayName: { type: "string" },
                description: { type: "string" },
                constitution: { type: "string", description: "Markdown constitution" },
              },
              required: ["name", "displayName"],
            },
          },
          {
            name: "circles.list",
            description: "List all circles.",
            inputSchema: { type: "object", properties: {} },
          },
          {
            name: "circles.add_member",
            description: "Add an agent instance to a circle.",
            inputSchema: {
              type: "object",
              properties: {
                circleId: { type: "string" },
                agentId: { type: "string" },
              },
              required: ["circleId", "agentId"],
            },
          },
          // ── Coordination (Story 10.3) ───────────────────────────────────────
          {
            name: "orchestration.sequential",
            description: "Run tasks in sequence across agents.",
            inputSchema: {
              type: "object",
              properties: {
                tasks: {
                  type: "array",
                  items: {
                    type: "object",
                    properties: {
                      id: { type: "string" },
                      description: { type: "string" },
                      assignedAgent: { type: "string" },
                    },
                    required: ["id", "description"],
                  },
                },
              },
              required: ["tasks"],
            },
          },
          {
            name: "orchestration.parallel",
            description: "Run multiple tasks in parallel across agents.",
            inputSchema: {
              type: "object",
              properties: {
                tasks: {
                  type: "array",
                  items: {
                    type: "object",
                    properties: {
                      id: { type: "string" },
                      description: { type: "string" },
                      assignedAgent: { type: "string" },
                    },
                    required: ["id", "description"],
                  },
                },
              },
              required: ["tasks"],
            },
          },
          {
            name: "orchestration.hierarchical",
            description: "Run tasks with a manager agent overseeing and validating results.",
            inputSchema: {
              type: "object",
              properties: {
                managerAgent: { type: "string", description: "Name of the manager agent" },
                tasks: {
                  type: "array",
                  items: {
                    type: "object",
                    properties: {
                      id: { type: "string" },
                      description: { type: "string" },
                      assignedAgent: { type: "string" },
                    },
                    required: ["id", "description"],
                  },
                },
              },
              required: ["managerAgent", "tasks"],
            },
          },
          // ── Party Mode (Story 10.5) ─────────────────────────────────────────
          {
            name: "circle.broadcast",
            description: "Broadcast a message to all members of a circle.",
            inputSchema: {
              type: "object",
              properties: {
                circleId: { type: "string" },
                payload: { type: "object" },
              },
              required: ["circleId", "payload"],
            },
          },
          // ── Subagent Spawning (Story 10.5 / 10.4 / 17.4) ──────────────────
          {
            name: "agents.spawn_subagent",
            description: "Spawn a subagent to handle a delegated subtask. Only available to agents with permissions.canSpawnSubagents.",
            inputSchema: {
              type: "object",
              properties: {
                role: { type: "string", description: "Subagent role (must be in manifest subagents.allowed)" },
                task: { type: "string", description: "Task description for the subagent" },
                circle: { type: "string", description: "Circle to join. Pass 'none' to skip circle inheritance." },
                parentAgentId: { type: "string", description: "Calling agent's instance ID (required for delegation passthrough)" },
                delegations: {
                  type: "array",
                  description: "Delegation tokens to pass to the subagent. Each token must be owned by the calling agent. Child scope may be narrower but not broader than the parent token's scope.",
                  items: {
                    type: "object",
                    properties: {
                      delegationTokenId: { type: "string" },
                      narrowedScope: {
                        type: "object",
                        properties: {
                          service: { type: "string" },
                          permissions: { type: "array", items: { type: "string" } },
                          resourceConstraints: { type: "object" },
                        },
                        required: ["service", "permissions"],
                      },
                    },
                    required: ["delegationTokenId"],
                  },
                },
              },
              required: ["role", "task"],
            },
          },
        ],
      };
    });

    this.server.setRequestHandler(CallToolRequestSchema, async (request) => {
      const { name, arguments: args } = request.params;
      return this.callTool(name, args);
    });
  }

  public async callTool(name: string, args: any) {
    try {
      switch (name) {
        case "list_agents":
          return this.handleListAgents();
        case "restart_agent":
          return this.handleRestartAgent(args?.agentId as string);
        case "circles.create":
          return this.handleCreateCircle(args);
        case "circles.list":
          return this.handleListCircles();
        case "circles.add_member":
          return this.handleAddMember(args.circleId, args.agentId);
        case "orchestration.sequential":
          return this.handleSequentialOrchestration(args.tasks);
        case "orchestration.parallel":
          return this.handleParallelOrchestration(args.tasks);
        case "orchestration.hierarchical":
          return this.handleHierarchicalOrchestration(args.managerAgent, args.tasks);
        case "circle.broadcast":
          return this.handleCircleBroadcast(args.circleId, args.payload);
        case "agents.spawn_subagent":
          return this.handleSpawnSubagent(args.role, args.task, args.circle, args.parentAgentId, args.delegations);
        default:
          throw new Error(`Tool not found: ${name}`);
      }
    } catch (err: any) {
      return {
        isError: true,
        content: [{ type: "text", text: err.message }],
      };
    }
  }

  private handleListAgents() {
    const agents = this.orchestrator.listAgents().map(a => ({
      id: a.id,
      name: a.name,
      status: a.status,
      startTime: a.startTime,
    }));
    return {
      content: [{ type: "text", text: JSON.stringify(agents, null, 2) }],
    };
  }

  private async handleRestartAgent(agentId: string) {
    await this.orchestrator.restartAgent(agentId);
    return {
      content: [{ type: "text", text: `Agent "${agentId}" restarted successfully.` }],
    };
  }

  private async handleCreateCircle(args: any) {
    const circle = await this.circleService.createCircle(args);
    return {
      content: [{ type: "text", text: `Circle "${circle.name}" created with ID: ${circle.id}` }],
    };
  }

  private async handleListCircles() {
    const circles = await this.circleService.listCircles();
    return {
      content: [{ type: "text", text: JSON.stringify(circles, null, 2) }],
    };
  }

  private async handleAddMember(circleId: string, agentId: string) {
    await this.circleService.addMember(circleId, agentId);
    return {
      content: [{ type: "text", text: `Agent "${agentId}" added to circle "${circleId}"` }],
    };
  }

  private async handleSequentialOrchestration(tasks: any[]) {
    const result = await this.orchestrator.executeWithProcess('sequential', tasks);
    return {
      content: [{ type: "text", text: JSON.stringify(result, null, 2) }],
    };
  }

  private async handleParallelOrchestration(tasks: any[]) {
    const result = await this.orchestrator.executeWithProcess('parallel', tasks);
    return {
      content: [{ type: "text", text: JSON.stringify(result, null, 2) }],
    };
  }

  private async handleHierarchicalOrchestration(managerAgent: string, tasks: any[]) {
    const result = await this.orchestrator.executeWithProcess('hierarchical', tasks, managerAgent);
    return {
      content: [{ type: "text", text: JSON.stringify(result, null, 2) }],
    };
  }

  private async handleCircleBroadcast(circleId: string, payload: any) {
    const intercom = this.orchestrator.getIntercom();
    if (!intercom) throw new Error("Intercom service not available");
    await intercom.publish(`circle:${circleId}`, payload);
    return {
      content: [{ type: "text", text: `Broadcast sent to circle "${circleId}"` }],
    };
  }

  private async handleSpawnSubagent(
    role: string,
    task: string,
    circle?: string,
    parentAgentId?: string,
    delegations?: Array<{ delegationTokenId: string; narrowedScope?: DelegationScope }>,
  ) {
    if (!role || !task) throw new Error("role and task are required");

    const { AgentFactory } = await import("../agents/AgentFactory.js");

    // Find a template matching the requested role
    const manifests = this.orchestrator.getAllManifests();
    const template = manifests.find(
      m => m.identity.role === role || m.metadata.name === role,
    );
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
      circleId,
    );

    // Story 17.4 — Agent-to-subagent delegation
    const childDelegationIds: string[] = [];
    if (delegations && delegations.length > 0 && parentAgentId) {
      const childInstanceId = instance.id;
      const issuedAt = new Date().toISOString();

      for (const delegation of delegations) {
        const { delegationTokenId, narrowedScope } = delegation;

        // Verify the parent token exists and belongs to the calling agent
        const { rows } = await pool.query(
          `SELECT * FROM delegation_tokens
           WHERE id = $1
             AND (actor_agent_id = $2 OR actor_instance_id::text = $2)
             AND revoked_at IS NULL
             AND (expires_at IS NULL OR expires_at > now())`,
          [delegationTokenId, parentAgentId],
        );

        const parentToken = rows[0];
        if (!parentToken) {
          throw new Error(
            `CapabilityEscalationError: delegation token "${delegationTokenId}" not found or not accessible by agent "${parentAgentId}"`,
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
          ],
        );

        childDelegationIds.push(childId);

        // Audit
        await AuditService.getInstance().record({
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
        }).catch(() => {});
      }
    }

    await this.orchestrator.startInstance(instance.id, undefined, task);

    return {
      content: [{
        type: "text",
        text: JSON.stringify({
          instanceId: instance.id,
          role,
          task,
          childDelegationIds,
        }),
      }],
    };
  }
}

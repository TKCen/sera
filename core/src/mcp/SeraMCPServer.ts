import { Server } from "@modelcontextprotocol/sdk/server/index.js";
import { type CallToolRequest, CallToolRequestSchema, ListToolsRequestSchema } from "@modelcontextprotocol/sdk/types.js";
import type { Orchestrator } from "../agents/Orchestrator.js";
import { Logger } from "../lib/logger.js";

const logger = new Logger('SeraMCPServer');

/**
 * SeraMCPServer — an embedded MCP server that exposes platform management tools.
 *
 * Tools:
 *  - list_agents: List all active agents and their status
 *  - restart_agent: Restart an agent by ID
 *
 * These tools are bridged into the SkillRegistry and can be called by agents
 * that have the `seraManagement` capability.
 */
export class SeraMCPServer {
  public readonly server: Server;

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
        ],
      };
    });

    this.server.setRequestHandler(CallToolRequestSchema, async (request) => {
      const { name, arguments: args } = request.params;
      return this.callTool(name, args);
    });
  }

  public async callTool(name: string, args: any) {
    // Note: Capability-based gating is handled at the SkillRegistry level 
    // by the bridged skill handler, which has access to the AgentContext.

    switch (name) {
      case "list_agents":
        return this.handleListAgents();
      case "restart_agent":
        return this.handleRestartAgent(args?.agentId as string);
      default:
        throw new Error(`Tool not found: ${name}`);
    }
  }

  private handleListAgents() {
    const agents = this.orchestrator.listAgents().map(a => ({
      id: a.id,
      name: a.manifest.metadata.name,
      status: a.status,
      startTime: a.startTime,
    }));
    return {
      content: [
        {
          type: "text",
          text: JSON.stringify(agents, null, 2),
        },
      ],
    };
  }

  private async handleRestartAgent(agentId: string) {
    if (!agentId) throw new Error("agentId is required");
    try {
      await this.orchestrator.restartAgent(agentId);
      return {
        content: [
          {
            type: "text",
            text: `Agent "${agentId}" restarted successfully.`,
          },
        ],
      };
    } catch (err: any) {
      return {
        isError: true,
        content: [
          {
            type: "text",
            text: `Failed to restart agent "${agentId}": ${err.message}`,
          },
        ],
      };
    }
  }
}

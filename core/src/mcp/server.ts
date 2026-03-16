import { Server } from "@modelcontextprotocol/sdk/server/index.js";
import { StdioServerTransport } from "@modelcontextprotocol/sdk/server/stdio.js";
import {
  CallToolRequestSchema,
  ListToolsRequestSchema,
} from "@modelcontextprotocol/sdk/types.js";

export class MCPServer {
  private server: Server;

  constructor(name: string, version: string) {
    this.server = new Server(
      {
        name,
        version,
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
            name: "example-tool",
            description: "An example tool",
            inputSchema: {
              type: "object",
              properties: {
                input: { type: "string" },
              },
            },
          },
        ],
      };
    });

    this.server.setRequestHandler(CallToolRequestSchema, async (request) => {
      if (request.params.name === "example-tool") {
        return {
          content: [
            {
              type: "text",
              text: `Example tool called with input: ${request.params.arguments?.input}`,
            },
          ],
        };
      }
      throw new Error("Tool not found");
    });
  }

  async start() {
    const transport = new StdioServerTransport();
    await this.server.connect(transport);
    console.log("MCP Server started");
  }
}

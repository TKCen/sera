import { Client } from "@modelcontextprotocol/sdk/client/index.js";
import { StdioClientTransport } from "@modelcontextprotocol/sdk/client/stdio.js";
import { Logger } from '../lib/logger.js';

const logger = new Logger('MCPClient');

export class MCPClient {
  private client: Client;
  private transport: StdioClientTransport;

  constructor(command: string, args: string[] = []) {
    this.transport = new StdioClientTransport({
      command,
      args,
    });

    this.client = new Client(
      {
        name: "sera-orchestrator",
        version: "1.0.0",
      },
      {
        capabilities: {},
      }
    );
  }

  async connect() {
    await this.client.connect(this.transport);
    logger.info('Connected to MCP server');
  }

  async listTools() {
    return await this.client.listTools();
  }

  async callTool(name: string, arguments_: any) {
    return await this.client.callTool({
      name,
      arguments: arguments_,
    });
  }

  async disconnect() {
    await this.transport.close();
  }
}

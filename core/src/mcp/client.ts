import { Client } from "@modelcontextprotocol/sdk/client/index.js";
import { StdioClientTransport } from "@modelcontextprotocol/sdk/client/stdio.js";
import { SSEClientTransport } from "@modelcontextprotocol/sdk/client/sse.js";
import { Logger } from '../lib/logger.js';

const logger = new Logger('MCPClient');

export type MCPTransportType = 'stdio' | 'http';

export interface MCPClientOptions {
  name: string;
  transport: MCPTransportType;
  command?: string; // for stdio
  args?: string[];  // for stdio
  url?: string;     // for http/sse
}

export class MCPClient {
  private client: Client;
  private transport: StdioClientTransport | SSEClientTransport;

  constructor(options: MCPClientOptions) {
    if (options.transport === 'stdio') {
      if (!options.command) throw new Error('Command required for stdio transport');
      this.transport = new StdioClientTransport({
        command: options.command,
        args: options.args ?? [],
      });
    } else {
      if (!options.url) throw new Error('URL required for http transport');
      this.transport = new SSEClientTransport(new URL(options.url));
    }

    this.client = new Client(
      {
        name: "sera-core",
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

  async callTool(name: string, arguments_: any, meta?: any) {
    return await this.client.callTool({
      name,
      arguments: arguments_,
      _meta: meta,
    });
  }

  async disconnect() {
    await this.transport.close();
  }
}


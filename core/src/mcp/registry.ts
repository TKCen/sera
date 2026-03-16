import { MCPClient } from "./client.js";

export class MCPRegistry {
  private static instance: MCPRegistry;
  private clients: Map<string, MCPClient> = new Map();

  private constructor() {}

  public static getInstance(): MCPRegistry {
    if (!MCPRegistry.instance) {
      MCPRegistry.instance = new MCPRegistry();
    }
    return MCPRegistry.instance;
  }

  async registerClient(name: string, command: string, args: string[] = []) {
    const client = new MCPClient(command, args);
    await client.connect();
    this.clients.set(name, client);
    return client;
  }

  getClient(name: string): MCPClient | undefined {
    return this.clients.get(name);
  }

  async getAllTools() {
    const allTools = [];
    for (const [name, client] of this.clients) {
      const tools = await client.listTools();
      allTools.push({
        serverName: name,
        tools: tools.tools,
      });
    }
    return allTools;
  }
}

import fs from 'node:fs';
import path from 'node:path';
import yaml from 'js-yaml';
import chokidar from 'chokidar';
import { MCPClient, type MCPClientOptions } from "./client.js";
import { Logger } from '../lib/logger.js';
import type { MCPServerManager, MCPServerManifest } from "./MCPServerManager.js";
import type { IntercomService } from "../intercom/IntercomService.js";

const logger = new Logger('MCPRegistry');

export interface MCPServerInfo {
  name: string;
  status: 'connected' | 'disconnected' | 'error';
  toolCount: number;
}

export class MCPRegistry {
  private static instance: MCPRegistry;
  private clients: Map<string, { client: MCPClient, instanceId?: string, manifest?: MCPServerManifest }> = new Map();
  private manager?: MCPServerManager;
  private intercom?: IntercomService;
  private watcher?: chokidar.FSWatcher;
  private onRegisterHooks: ((name: string) => void)[] = [];
  private onUnregisterHooks: ((name: string) => void)[] = [];

  private constructor() {}

  public static getInstance(): MCPRegistry {
    if (!MCPRegistry.instance) {
      MCPRegistry.instance = new MCPRegistry();
    }
    return MCPRegistry.instance;
  }

  public setManager(manager: MCPServerManager) {
    this.manager = manager;
  }

  public setIntercom(intercom: IntercomService) {
    this.intercom = intercom;
  }

  public onRegister(hook: (name: string) => void) {
    this.onRegisterHooks.push(hook);
  }

  public onUnregister(hook: (name: string) => void) {
    this.onUnregisterHooks.push(hook);
  }

  async registerClient(name: string, options: Omit<MCPClientOptions, 'name'>) {
    if (this.clients.has(name)) {
      throw new Error(`MCP server "${name}" already registered`);
    }

    const client = new MCPClient({ ...options, name });
    try {
      await client.connect();
      this.clients.set(name, { client });
      logger.info(`Registered host-side MCP server: ${name}`);
      this.broadcast('registered', name);
      for (const hook of this.onRegisterHooks) hook(name);
      return client;
    } catch (err) {
      logger.error(`Failed to connect to MCP server "${name}":`, err);
      throw err;
    }
  }

  async registerContainerServer(manifest: MCPServerManifest) {
    if (!this.manager) throw new Error('MCPServerManager not initialized in registry');
    const name = manifest.metadata.name;
    
    // If already registered, unregister first (hot-reload)
    if (this.clients.has(name)) {
      await this.unregisterClient(name);
    }

    const { info, clientOptions } = await this.manager.spawnServer(manifest);
    const client = new MCPClient(clientOptions);
    try {
      await client.connect();
      this.clients.set(name, { client, instanceId: info.instanceId, manifest });
      logger.info(`Registered containerized MCP server: ${name}`);
      this.broadcast('registered', name);
      for (const hook of this.onRegisterHooks) hook(name);
      return client;
    } catch (err) {
      logger.error(`Failed to connect to MCP server container "${name}":`, err);
      await this.manager.stopServer(info.instanceId);
      throw err;
    }
  }

  async unregisterClient(name: string) {
    const entry = this.clients.get(name);
    if (entry) {
      await entry.client.disconnect().catch(() => {});
      if (entry.instanceId && this.manager) {
        await this.manager.stopServer(entry.instanceId);
      }
      this.clients.delete(name);
      logger.info(`Unregistered MCP server: ${name}`);
      this.broadcast('unregistered', name);
      for (const hook of this.onUnregisterHooks) hook(name);
      return true;
    }
    return false;
  }

  /**
   * Auto-load manifests from a directory.
   */
  async loadFromDirectory(dir: string) {
    if (!fs.existsSync(dir)) return;
    const files = fs.readdirSync(dir);
    for (const file of files) {
      if (file.endsWith('.mcp.yaml') || file.endsWith('.mcp.yml') || file.endsWith('.mcp.json')) {
        const fullPath = path.join(dir, file);
        await this.loadManifest(fullPath);
      }
    }
  }

  /**
   * Watch a directory for manifest changes.
   */
  watchDirectory(dir: string) {
    if (this.watcher) this.watcher.close();
    if (!fs.existsSync(dir)) fs.mkdirSync(dir, { recursive: true });

    this.watcher = chokidar.watch(dir, { ignoreInitial: true });
    
    this.watcher.on('add', async (filePath) => {
      if (filePath.match(/\.mcp\.(yaml|yml|json)$/)) {
        logger.info(`Detected new MCP manifest: ${path.basename(filePath)}`);
        await this.loadManifest(filePath).catch(err => logger.error(`Failed to load ${filePath}:`, err));
      }
    });

    this.watcher.on('change', async (filePath) => {
      if (filePath.match(/\.mcp\.(yaml|yml|json)$/)) {
        logger.info(`Detected MCP manifest change: ${path.basename(filePath)}`);
        await this.loadManifest(filePath).catch(err => logger.error(`Failed to reload ${filePath}:`, err));
      }
    });

    this.watcher.on('unlink', async (filePath) => {
      if (filePath.match(/\.mcp\.(yaml|yml|json)$/)) {
        const name = path.basename(filePath).split('.')[0];
        if (name) {
          logger.info(`MCP manifest removed: ${name}`);
          await this.unregisterClient(name);
        }
      }
    });
  }

  private async loadManifest(filePath: string) {
    const content = fs.readFileSync(filePath, 'utf8');
    const manifest = filePath.endsWith('.json') 
      ? JSON.parse(content) 
      : yaml.load(content) as MCPServerManifest;
    
    await this.registerContainerServer(manifest);
  }

  private broadcast(action: string, serverName: string) {
    if (this.intercom) {
      this.intercom.publishSystem('mcp_registry_update', {
        action,
        serverName,
        timestamp: new Date().toISOString()
      }).catch(err => logger.warn('Failed to broadcast MCP update:', err));
    }
  }

  getClient(name: string): MCPClient | undefined {
    return this.clients.get(name)?.client;
  }

  /** Expose all registered clients for iteration. */
  getClients(): Map<string, MCPClient> {
    const map = new Map<string, MCPClient>();
    for (const [name, entry] of this.clients) {
      map.set(name, entry.client);
    }
    return map;
  }

  async listServers(): Promise<MCPServerInfo[]> {
    const infos: MCPServerInfo[] = [];
    for (const [name, entry] of this.clients) {
      try {
        const tools = await entry.client.listTools();
        infos.push({
          name,
          status: 'connected',
          toolCount: tools.tools.length,
        });
      } catch (err) {
        infos.push({
          name,
          status: 'error',
          toolCount: 0,
        });
      }
    }
    return infos;
  }

  async getAllTools() {
    const allTools = [];
    for (const [name, entry] of this.clients) {
      try {
        const tools = await entry.client.listTools();
        allTools.push({
          serverName: name,
          tools: tools.tools
        });
      } catch (err) {
        logger.error(`Failed to list tools for server "${name}":`, err);
      }
    }
    return allTools;
  }

  /**
   * Register tools from the embedded sera-core MCP server.
   */
  public async registerSeraCoreTools(seraMcp: any): Promise<void> {
    const name = 'sera-core';
    
    // Create a shim that matches the MCPClient interface.
    // Since this is in-process, we bypass the real transport-based MCPClient
    // and call the server instance (or its wrapper) directly.
    const mockClient = {
      listTools: async () => {
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
          ]
        };
      },
      callTool: async (toolName: string, args: any) => {
        return await seraMcp.callTool(toolName, args);
      },
      disconnect: async () => {},
    } as any;

    this.clients.set(name, {
      client: mockClient,
      instanceId: 'local',
    });

    logger.info(`Registered embedded sera-core MCP tools`);
    this.broadcast('registered', name);
    for (const hook of this.onRegisterHooks) hook(name);
  }
}

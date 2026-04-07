import fs from 'node:fs';
import path from 'node:path';
import yaml from 'js-yaml';
import chokidar from 'chokidar';
import { MCPClient, type MCPClientOptions } from './client.js';
import { Logger } from '../lib/logger.js';
import { MCPServerManager, MCPManifestValidationError } from './MCPServerManager.js';
import type { MCPServerManifest } from './MCPServerManager.js';
import type { IntercomService } from '../intercom/IntercomService.js';

const logger = new Logger('MCPRegistry');

export interface MCPServerInfo {
  name: string;
  status: 'connected' | 'disconnected' | 'error';
  toolCount: number;
}

export class MCPRegistry {
  private static instance: MCPRegistry;
  private clients: Map<
    string,
    { client: MCPClient; instanceId?: string; manifest?: MCPServerManifest }
  > = new Map();
  private manager?: MCPServerManager;
  private intercom?: IntercomService;
  private watcher?: chokidar.FSWatcher;
  private reloadTimers: Map<string, ReturnType<typeof setTimeout>> = new Map();
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
      this.attachReconnectHook(name, client);
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
      this.attachReconnectHook(name, client);
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

  private attachReconnectHook(name: string, client: MCPClient): void {
    client.onReconnect(() => {
      logger.info(`MCP server "${name}" reconnected — broadcasting status update`);
      this.broadcast('reconnected', name);
    });
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

    this.watcher.on('add', (filePath) => {
      if (!filePath.match(/\.mcp\.(yaml|yml|json)$/)) return;

      const existing = this.reloadTimers.get(filePath);
      if (existing) clearTimeout(existing);

      this.reloadTimers.set(
        filePath,
        setTimeout(async () => {
          this.reloadTimers.delete(filePath);
          logger.info(`Detected new MCP manifest: ${path.basename(filePath)}`);
          await this.loadManifest(filePath).catch((err) =>
            logger.error(`Failed to load ${filePath}:`, err)
          );
        }, 300)
      );
    });

    this.watcher.on('change', (filePath) => {
      if (!filePath.match(/\.mcp\.(yaml|yml|json)$/)) return;

      const existing = this.reloadTimers.get(filePath);
      if (existing) clearTimeout(existing);

      this.reloadTimers.set(
        filePath,
        setTimeout(async () => {
          this.reloadTimers.delete(filePath);
          logger.info(`Detected MCP manifest change: ${path.basename(filePath)}`);
          await this.loadManifest(filePath).catch((err) =>
            logger.error(`Failed to reload ${filePath}:`, err)
          );
        }, 300)
      );
    });

    this.watcher.on('unlink', (filePath) => {
      if (!filePath.match(/\.mcp\.(yaml|yml|json)$/)) return;

      const existing = this.reloadTimers.get(filePath);
      if (existing) clearTimeout(existing);

      this.reloadTimers.set(
        filePath,
        setTimeout(async () => {
          this.reloadTimers.delete(filePath);
          const name = path.basename(filePath).split('.')[0];
          if (name) {
            logger.info(`MCP manifest removed: ${name}`);
            await this.unregisterClient(name).catch((err) =>
              logger.error(`Failed to unregister ${name}:`, err)
            );
          }
        }, 300)
      );
    });
  }

  public stopWatching(): void {
    for (const timer of this.reloadTimers.values()) {
      clearTimeout(timer);
    }
    this.reloadTimers.clear();
    if (this.watcher) {
      this.watcher.close();
      delete this.watcher;
    }
  }

  private async loadManifest(filePath: string) {
    const content = fs.readFileSync(filePath, 'utf8');
    const raw = filePath.endsWith('.json') ? (JSON.parse(content) as unknown) : yaml.load(content);

    let manifest: MCPServerManifest;
    try {
      manifest = MCPServerManager.validateManifest(raw, filePath);
    } catch (err) {
      if (err instanceof MCPManifestValidationError) {
        logger.error(`Invalid MCP manifest at ${filePath}: ${err.message}`);
        return;
      }
      throw err;
    }

    await this.registerContainerServer(manifest);
  }

  private broadcast(action: string, serverName: string) {
    if (this.intercom) {
      this.intercom
        .publishSystem('mcp_registry_update', {
          action,
          serverName,
          timestamp: new Date().toISOString(),
        })
        .catch((err: unknown) => logger.warn('Failed to broadcast MCP update:', err));
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
      } catch {
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
          tools: tools.tools,
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
  public async registerSeraCoreTools(
    seraMcp: import('./SeraMCPServer.js').SeraMCPServer
  ): Promise<void> {
    const name = 'sera-core';

    // In-process shim — bypasses transport and delegates directly to the server instance.
    // listTools reads the tool definitions from the server so the list stays in sync.
    const mockClient: Partial<MCPClient> = {
      listTools: async () => {
        return {
          tools:
            seraMcp.getToolDefinitions() as import('@modelcontextprotocol/sdk/types.js').Tool[],
        };
      },
      callTool: async (
        toolName: string,
        args: Record<string, unknown>,
        _meta?: Record<string, unknown>
      ) => {
        const result = await seraMcp.callTool(toolName, args);
        return result as import('@modelcontextprotocol/sdk/types.js').CallToolResult;
      },
      disconnect: async () => {},
    };

    this.clients.set(name, {
      client: mockClient as MCPClient,
      instanceId: 'local',
    });

    logger.info(`Registered embedded sera-core MCP tools`);
    this.broadcast('registered', name);
    for (const hook of this.onRegisterHooks) hook(name);
  }
}

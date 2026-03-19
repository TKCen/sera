import { SandboxManager } from '../sandbox/SandboxManager.js';
import type { SandboxInfo } from '../sandbox/types.js';
import { Logger } from '../lib/logger.js';
import { v4 as uuidv4 } from 'uuid';

const logger = new Logger('MCPServerManager');

export interface MCPServerManifest {
  apiVersion: 'sera/v1';
  kind: 'SkillProvider';
  metadata: {
    name: string;
    description?: string;
  };
  image: string;
  transport: 'stdio' | 'http';
  command?: string; // for stdio execution inside container
  args?: string[];    // for stdio execution inside container
  url?: string;     // for http transport if not same as container name
  network?: {
    allowlist?: string[];
  };
  mounts?: Array<{
    hostPath: string;
    containerPath: string;
    mode?: 'ro' | 'rw';
  }>;
  secrets?: string[];
  healthCheck?: {
    command: string[];
    interval?: string;
    timeout?: string;
    retries?: number;
  };
}

export class MCPServerManager {
  constructor(private readonly sandboxManager: SandboxManager) {}

  /**
   * Spawn an MCP server container from a manifest.
   */
  async spawnServer(manifest: MCPServerManifest): Promise<{ info: SandboxInfo, clientOptions: import('./client.js').MCPClientOptions }> {
    const serverName = manifest.metadata.name;
    const instanceId = `mcp-${serverName}-${uuidv4().substring(0, 8)}`;

    const resolvedCapabilities = {
      network: {
        outbound: manifest.network?.allowlist ?? [],
      },
      linux: {
        readonlyRootfs: true,
      },
      mounts: manifest.mounts, // Now supported by SandboxManager.spawn
    };

    const request = {
      agentName: serverName,
      type: 'mcp-server' as const,
      image: manifest.image,
      lifecycleMode: 'persistent' as const,
    };

    logger.info(`Spawning MCP server container: ${serverName} (${instanceId})`);

    const info = await this.sandboxManager.spawn(
      this.manifestToAgentManifest(manifest),
      request as any,
      resolvedCapabilities,
      instanceId
    );

    // Prepare connection options based on transport
    let clientOptions: import('./client.js').MCPClientOptions;

    if (manifest.transport === 'stdio') {
      const command = manifest.command || 'npm';
      const args = manifest.args || ['start'];
      clientOptions = {
        name: serverName,
        transport: 'stdio',
        command: 'docker',
        args: ['exec', '-i', info.containerId, command, ...args],
      };
    } else {
      // http transport: connect via container name in agent_net
      const port = manifest.url ? new URL(manifest.url).port : '3000';
      const path = manifest.url ? new URL(manifest.url).pathname : '/sse';
      clientOptions = {
        name: serverName,
        transport: 'http',
        url: `http://${info.containerId.substring(0, 12)}:${port}${path}`,
      };
    }


    return { info, clientOptions };
  }


  async stopServer(instanceId: string): Promise<void> {
    await this.sandboxManager.teardown(instanceId);
  }

  /**
   * Mock-conversion of MCPServerManifest to AgentManifest for SandboxManager compatibility.
   * This is a shim until SandboxManager is more general.
   */
  private manifestToAgentManifest(mcp: MCPServerManifest): any {
    return {
      apiVersion: mcp.apiVersion,
      kind: 'Agent', // SandboxManager expects 'Agent' or similar
      metadata: {
        name: mcp.metadata.name,
        displayName: mcp.metadata.name,
        tier: 1, // MCP servers are essentially tier 1 (restricted)
      },
      identity: {
        role: 'MCP Server',
        description: mcp.metadata.description ?? '',
      },
      model: { provider: 'none', name: 'none' },
      workspace: { path: `/tmp/mcp-${mcp.metadata.name}` }, // Placeholder
    };
  }
}

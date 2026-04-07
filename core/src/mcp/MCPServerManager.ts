import { SandboxManager } from '../sandbox/SandboxManager.js';
import type { SandboxInfo } from '../sandbox/types.js';
import { Logger } from '../lib/logger.js';
import { v4 as uuidv4 } from 'uuid';

export class MCPManifestValidationError extends Error {
  constructor(
    message: string,
    public readonly field?: string
  ) {
    super(message);
    this.name = 'MCPManifestValidationError';
  }
}

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
  args?: string[]; // for stdio execution inside container
  url?: string; // for http transport if not same as container name
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
   * Validate a raw parsed YAML/JSON object into a typed MCPServerManifest.
   * Throws MCPManifestValidationError on invalid input.
   */
  static validateManifest(raw: unknown, source?: string): MCPServerManifest {
    const ctx = source ? ` (in ${source})` : '';

    if (!raw || typeof raw !== 'object' || Array.isArray(raw)) {
      throw new MCPManifestValidationError(`Manifest must be a YAML object${ctx}`);
    }

    const obj = raw as Record<string, unknown>;

    if (obj['apiVersion'] !== 'sera/v1') {
      throw new MCPManifestValidationError(
        `"apiVersion" must be "sera/v1", got "${String(obj['apiVersion'])}"${ctx}`,
        'apiVersion'
      );
    }

    if (obj['kind'] !== 'SkillProvider') {
      throw new MCPManifestValidationError(
        `"kind" must be "SkillProvider", got "${String(obj['kind'])}"${ctx}`,
        'kind'
      );
    }

    if (!obj['metadata'] || typeof obj['metadata'] !== 'object' || Array.isArray(obj['metadata'])) {
      throw new MCPManifestValidationError(
        `Missing or invalid "metadata" object${ctx}`,
        'metadata'
      );
    }
    const meta = obj['metadata'] as Record<string, unknown>;
    if (!meta['name'] || typeof meta['name'] !== 'string') {
      throw new MCPManifestValidationError(
        `Missing or invalid "metadata.name" string${ctx}`,
        'metadata.name'
      );
    }
    if (meta['description'] !== undefined && typeof meta['description'] !== 'string') {
      throw new MCPManifestValidationError(
        `"metadata.description" must be a string${ctx}`,
        'metadata.description'
      );
    }

    if (!obj['image'] || typeof obj['image'] !== 'string') {
      throw new MCPManifestValidationError(`Missing or invalid "image" string${ctx}`, 'image');
    }

    if (obj['transport'] !== 'stdio' && obj['transport'] !== 'http') {
      throw new MCPManifestValidationError(
        `"transport" must be "stdio" or "http", got "${String(obj['transport'])}"${ctx}`,
        'transport'
      );
    }

    if (obj['command'] !== undefined && typeof obj['command'] !== 'string') {
      throw new MCPManifestValidationError(`"command" must be a string${ctx}`, 'command');
    }

    if (obj['args'] !== undefined) {
      if (!Array.isArray(obj['args']) || !obj['args'].every((a) => typeof a === 'string')) {
        throw new MCPManifestValidationError(`"args" must be an array of strings${ctx}`, 'args');
      }
    }

    if (obj['url'] !== undefined && typeof obj['url'] !== 'string') {
      throw new MCPManifestValidationError(`"url" must be a string${ctx}`, 'url');
    }

    if (obj['secrets'] !== undefined) {
      if (!Array.isArray(obj['secrets']) || !obj['secrets'].every((s) => typeof s === 'string')) {
        throw new MCPManifestValidationError(
          `"secrets" must be an array of strings${ctx}`,
          'secrets'
        );
      }
    }

    if (obj['network'] !== undefined) {
      if (typeof obj['network'] !== 'object' || Array.isArray(obj['network'])) {
        throw new MCPManifestValidationError(`"network" must be an object${ctx}`, 'network');
      }
      const net = obj['network'] as Record<string, unknown>;
      if (net['allowlist'] !== undefined) {
        if (
          !Array.isArray(net['allowlist']) ||
          !net['allowlist'].every((a) => typeof a === 'string')
        ) {
          throw new MCPManifestValidationError(
            `"network.allowlist" must be an array of strings${ctx}`,
            'network.allowlist'
          );
        }
      }
    }

    if (obj['mounts'] !== undefined) {
      if (!Array.isArray(obj['mounts'])) {
        throw new MCPManifestValidationError(`"mounts" must be an array${ctx}`, 'mounts');
      }
      for (let i = 0; i < obj['mounts'].length; i++) {
        const m = obj['mounts'][i];
        if (!m || typeof m !== 'object' || Array.isArray(m)) {
          throw new MCPManifestValidationError(
            `mounts[${i}] must be an object${ctx}`,
            `mounts[${i}]`
          );
        }
        const mount = m as Record<string, unknown>;
        const mCtx = `${ctx} mounts[${i}]`;
        if (!mount['hostPath'] || typeof mount['hostPath'] !== 'string') {
          throw new MCPManifestValidationError(
            `Missing or invalid "hostPath" string${mCtx}`,
            `mounts[${i}].hostPath`
          );
        }
        if ((mount['hostPath'] as string).includes('..')) {
          throw new MCPManifestValidationError(
            `"hostPath" must not contain path traversal sequences${mCtx}`,
            `mounts[${i}].hostPath`
          );
        }
        if (!mount['containerPath'] || typeof mount['containerPath'] !== 'string') {
          throw new MCPManifestValidationError(
            `Missing or invalid "containerPath" string${mCtx}`,
            `mounts[${i}].containerPath`
          );
        }
        if (mount['mode'] !== undefined && mount['mode'] !== 'ro' && mount['mode'] !== 'rw') {
          throw new MCPManifestValidationError(
            `"mode" must be "ro" or "rw"${mCtx}`,
            `mounts[${i}].mode`
          );
        }
      }
    }

    if (obj['healthCheck'] !== undefined) {
      if (typeof obj['healthCheck'] !== 'object' || Array.isArray(obj['healthCheck'])) {
        throw new MCPManifestValidationError(
          `"healthCheck" must be an object${ctx}`,
          'healthCheck'
        );
      }
      const hc = obj['healthCheck'] as Record<string, unknown>;
      if (!Array.isArray(hc['command']) || !hc['command'].every((c) => typeof c === 'string')) {
        throw new MCPManifestValidationError(
          `"healthCheck.command" must be an array of strings${ctx}`,
          'healthCheck.command'
        );
      }
      if (hc['interval'] !== undefined && typeof hc['interval'] !== 'string') {
        throw new MCPManifestValidationError(
          `"healthCheck.interval" must be a string${ctx}`,
          'healthCheck.interval'
        );
      }
      if (hc['timeout'] !== undefined && typeof hc['timeout'] !== 'string') {
        throw new MCPManifestValidationError(
          `"healthCheck.timeout" must be a string${ctx}`,
          'healthCheck.timeout'
        );
      }
      if (hc['retries'] !== undefined && typeof hc['retries'] !== 'number') {
        throw new MCPManifestValidationError(
          `"healthCheck.retries" must be a number${ctx}`,
          'healthCheck.retries'
        );
      }
    }

    return obj as unknown as MCPServerManifest;
  }

  /**
   * Spawn an MCP server container from a manifest.
   */
  async spawnServer(
    manifest: MCPServerManifest
  ): Promise<{ info: SandboxInfo; clientOptions: import('./client.js').MCPClientOptions }> {
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

    const request: import('../sandbox/types.js').SpawnRequest = {
      agentName: serverName,
      type: 'mcp-server' as const,
      image: manifest.image,
      lifecycleMode: 'persistent' as const,
    };

    logger.info(`Spawning MCP server container: ${serverName} (${instanceId})`);

    const info = await this.sandboxManager.spawn(
      this.manifestToAgentManifest(manifest),
      request,
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
  private manifestToAgentManifest(
    mcp: MCPServerManifest
  ): import('../agents/manifest/types.js').AgentManifest {
    return {
      apiVersion: mcp.apiVersion,
      kind: 'Agent', // SandboxManager expects 'Agent' or similar
      metadata: {
        name: mcp.metadata.name,
        displayName: mcp.metadata.name,
        tier: 1, // MCP servers are essentially tier 1 (restricted)
        icon: 'bot',
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

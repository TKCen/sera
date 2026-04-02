/**
 * StorageProvider — pluggable workspace storage abstraction.
 *
 * Agents reference a storage provider in their AGENT.yaml `workspace.provider`
 * field. The SandboxManager uses the provider to create volume mounts instead
 * of hardcoding bind-mount logic.
 *
 * @see sera/docs/reimplementation/implementation-backlog.md § Epic 8
 */

import type { FilesystemMode } from '../sandbox/index.js';

// ── Mount Result ────────────────────────────────────────────────────────────────

export interface MountResult {
  /** Host path (for bind mounts) or volume name (for Docker volumes) */
  hostPathOrVolume: string;
  /** Whether this is a named volume (`true`) or a bind mount (`false`) */
  isVolume: boolean;
}

// ── StorageProvider Interface ───────────────────────────────────────────────────

export interface StorageProvider {
  /** Unique provider name (e.g. 'local', 'docker-volume') */
  readonly name: string;

  /**
   * Prepare storage for an agent. For local providers this ensures the
   * directory exists; for Docker volume providers this creates the volume.
   */
  mount(agentId: string, workspacePath?: string): Promise<MountResult>;

  /**
   * Tear down storage for an agent (e.g. remove a Docker volume).
   * Local providers may choose to leave the directory intact.
   */
  unmount(agentId: string): Promise<void>;

  /**
   * Return the host path or volume name for an agent's workspace.
   */
  getPath(agentId: string, workspacePath?: string): string;

  /**
   * Build a Docker-compatible bind/volume mount string.
   *
   * @param agentId       The agent requesting storage
   * @param containerPath Target path inside the container (e.g. '/workspace')
   * @param mode          'ro' or 'rw' — determined by the agent's security tier
   * @param workspacePath Optional override from AGENT.yaml workspace.path
   * @returns             A string suitable for Docker `Binds` (e.g. '/host:/container:rw')
   */
  getBindMount(
    agentId: string,
    containerPath: string,
    mode: FilesystemMode,
    workspacePath?: string
  ): string;
}

// ── StorageProviderFactory ──────────────────────────────────────────────────────

export class StorageProviderFactory {
  private providers: Map<string, StorageProvider> = new Map();
  private defaultProvider: string;

  constructor(defaultProvider: string = 'local') {
    this.defaultProvider = defaultProvider;
  }

  /**
   * Register a storage provider.
   */
  register(provider: StorageProvider): void {
    this.providers.set(provider.name, provider);
  }

  /**
   * Get a provider by name. Falls back to the default provider if name
   * is undefined. Throws if the provider is not registered.
   */
  getProvider(name?: string): StorageProvider {
    const providerName = name ?? this.defaultProvider;
    const provider = this.providers.get(providerName);

    if (!provider) {
      throw new Error(
        `Storage provider "${providerName}" is not registered. ` +
          `Available providers: ${[...this.providers.keys()].join(', ') || '(none)'}`
      );
    }

    return provider;
  }

  /**
   * List all registered provider names.
   */
  listProviders(): string[] {
    return [...this.providers.keys()];
  }
}

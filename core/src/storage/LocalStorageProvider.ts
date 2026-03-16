/**
 * LocalStorageProvider — bind-mount storage for agent workspaces.
 *
 * This replicates the original SandboxManager behaviour: the agent's
 * workspace is a host directory that gets bind-mounted into containers.
 * This is the default provider.
 *
 * @see sera/docs/reimplementation/implementation-backlog.md § Epic 8
 */

import type { FilesystemMode } from '../sandbox/types.js';
import type { StorageProvider, MountResult } from './StorageProvider.js';

// ── LocalStorageProvider ────────────────────────────────────────────────────────

export class LocalStorageProvider implements StorageProvider {
  readonly name = 'local';

  /** Base directory for agent workspaces (default: /workspaces) */
  private basePath: string;

  constructor(basePath: string = '/workspaces') {
    this.basePath = basePath;
  }

  async mount(agentId: string, workspacePath?: string): Promise<MountResult> {
    const hostPath = workspacePath ?? `${this.basePath}/${agentId}`;
    return { hostPathOrVolume: hostPath, isVolume: false };
  }

  async unmount(_agentId: string): Promise<void> {
    // Local bind mounts are not torn down — the directory persists.
  }

  getPath(agentId: string, workspacePath?: string): string {
    return workspacePath ?? `${this.basePath}/${agentId}`;
  }

  getBindMount(
    agentId: string,
    containerPath: string,
    mode: FilesystemMode,
    workspacePath?: string,
  ): string {
    const hostPath = this.getPath(agentId, workspacePath);
    return `${hostPath}:${containerPath}:${mode}`;
  }
}

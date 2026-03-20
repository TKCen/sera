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

  /** Base directory for agent workspaces (internal to this container) */
  private internalBasePath: string;
  /** Base directory for agent workspaces (on the host system) */
  private hostBasePath: string;

  constructor(internalBasePath: string = '/workspaces', hostBasePath?: string) {
    this.internalBasePath = internalBasePath;
    this.hostBasePath = hostBasePath ?? internalBasePath;
  }

  async mount(agentId: string, workspacePath?: string): Promise<MountResult> {
    const hostPath = workspacePath ?? `${this.hostBasePath}/${agentId}`;
    return { hostPathOrVolume: hostPath, isVolume: false };
  }

  async unmount(_agentId: string): Promise<void> {
    // Local bind mounts are not torn down — the directory persists.
  }

  getPath(agentId: string, workspacePath?: string): string {
    return workspacePath ?? `${this.internalBasePath}/${agentId}`;
  }

  getHostPath(agentId: string, workspacePath?: string): string {
    if (workspacePath) {
      if (workspacePath.startsWith(this.internalBasePath)) {
        const relative = workspacePath.slice(this.internalBasePath.length).replace(/^[/\\]+/, '');
        return `${this.hostBasePath}/${relative}`;
      }
      return workspacePath;
    }
    return `${this.hostBasePath}/${agentId}`;
  }

  getBindMount(
    agentId: string,
    containerPath: string,
    mode: FilesystemMode,
    workspacePath?: string
  ): string {
    let hostPath = this.getHostPath(agentId, workspacePath);
    // On Windows, Docker Desktop prefers /c/path format for bind mounts to avoid colon confusion
    if (process.platform === 'win32' && /^[a-zA-Z]:/.test(hostPath)) {
      hostPath = `/${hostPath[0]!.toLowerCase()}${hostPath.slice(2).replace(/\\/g, '/')}`;
    }
    return `${hostPath}:${containerPath}:${mode}`;
  }
}

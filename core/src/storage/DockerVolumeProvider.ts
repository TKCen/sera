/**
 * DockerVolumeProvider — named Docker volume storage for agent workspaces.
 *
 * Instead of bind-mounting a host directory, this provider creates and
 * manages named Docker volumes. This is more portable and works across
 * Docker Swarm nodes where bind mounts reference different host paths.
 *
 * Volume naming convention: `sera-ws-{agentId}`
 *
 * @see sera/docs/reimplementation/implementation-backlog.md § Epic 8
 */

import Docker from 'dockerode';
import type { FilesystemMode } from '../sandbox/types.js';
import type { StorageProvider, MountResult } from './StorageProvider.js';

// ── DockerVolumeProvider ────────────────────────────────────────────────────────

export class DockerVolumeProvider implements StorageProvider {
  readonly name = 'docker-volume';

  private docker: Docker;
  private prefix: string;

  constructor(docker?: Docker, prefix: string = 'sera-ws') {
    this.docker = docker ?? new Docker({ socketPath: '/var/run/docker.sock' });
    this.prefix = prefix;
  }

  /**
   * Create a named Docker volume for the agent (idempotent).
   */
  async mount(agentId: string): Promise<MountResult> {
    const volumeName = this.volumeName(agentId);

    await this.docker.createVolume({
      Name: volumeName,
      Labels: {
        'sera.storage': 'true',
        'sera.agent': agentId,
        'sera.provider': 'docker-volume',
      },
    });

    return { hostPathOrVolume: volumeName, isVolume: true };
  }

  /**
   * Remove the Docker volume for an agent.
   */
  async unmount(agentId: string): Promise<void> {
    const volumeName = this.volumeName(agentId);
    try {
      const volume = this.docker.getVolume(volumeName);
      await volume.remove();
    } catch {
      // Volume may not exist — that's fine
    }
  }

  getPath(agentId: string): string {
    return this.volumeName(agentId);
  }

  /**
   * Build a Docker volume mount string.
   * Named volumes use the same `source:target:mode` syntax as bind mounts.
   */
  getBindMount(
    agentId: string,
    containerPath: string,
    mode: FilesystemMode,
  ): string {
    const volumeName = this.volumeName(agentId);
    return `${volumeName}:${containerPath}:${mode}`;
  }

  // ── Helpers ─────────────────────────────────────────────────────────────────

  private volumeName(agentId: string): string {
    return `${this.prefix}-${agentId}`;
  }
}

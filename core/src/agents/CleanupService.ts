import { Logger } from '../lib/logger.js';
import type { AgentRegistry } from "./registry.service.js";
import type { SandboxManager } from '../sandbox/index.js';

const logger = new Logger('CleanupService');

/**
 * CleanupService — handles periodic cleanup of stale containers and ephemeral agent TTLs.
 * Story 3.7
 */
export class CleanupService {
  private registry: AgentRegistry | undefined;
  private sandboxManager: SandboxManager | undefined;
  private cleanupInterval: NodeJS.Timeout | undefined;
  private ephemeralTTLs = new Map<string, { deadline: number; parentId?: string }>();

  constructor() {
    // Story 3.7 — periodic cleanup of stopped/error containers
    this.cleanupInterval = setInterval(() => {
      this.runCleanupJob().catch((err) => logger.error('Cleanup job error:', err));
    }, 60000);
  }

  public setRegistry(registry: AgentRegistry): void {
    this.registry = registry;
  }

  public setSandboxManager(sandboxManager: SandboxManager): void {
    this.sandboxManager = sandboxManager;
  }

  /**
   * Register a TTL for an ephemeral agent instance.
   * The cleanup job will kill the container if the deadline passes.
   */
  public registerEphemeralTTL(instanceId: string, ttlMinutes: number): void {
    this.ephemeralTTLs.set(instanceId, {
      deadline: Date.now() + ttlMinutes * 60_000,
    });
  }

  /**
   * Background cleanup job: remove containers for stopped/error instances
   * older than the retention period.
   * Story 3.7
   */
  public async runCleanupJob(): Promise<void> {
    if (!this.registry || !this.sandboxManager) return;

    const retentionMs = parseInt(process.env.CONTAINER_RETENTION_MS ?? String(60 * 60 * 1000), 10);
    const cutoff = new Date(Date.now() - retentionMs);

    try {
      const stopped = await this.registry.listInstances({ status: 'stopped' });
      const errored = await this.registry.listInstances({ status: 'error' });

      for (const instance of [...stopped, ...errored]) {
        const lastUpdate = instance.updated_at ? new Date(instance.updated_at).getTime() : 0;
        if (lastUpdate < cutoff.getTime()) {
          await this.sandboxManager
            .teardown(instance.id)
            .catch((err) => logger.warn(`Cleanup: failed to teardown ${instance.id}:`, err));
          logger.info(`Cleanup job: removed stale container for instance ${instance.id}`);
        }
      }
    } catch (err) {
      logger.error('Cleanup job error:', err);
    }

    // Enforce ephemeral TTLs
    const now = Date.now();
    for (const [instanceId, { deadline }] of this.ephemeralTTLs) {
      if (now > deadline) {
        logger.warn(`Ephemeral agent ${instanceId} exceeded TTL — killing container`);
        await this.sandboxManager
          .teardown(instanceId)
          .catch((err) => logger.warn(`TTL cleanup failed for ${instanceId}:`, err));
        if (this.registry) {
          await this.registry.updateInstanceStatus(instanceId, 'error').catch(() => {});
        }
        this.ephemeralTTLs.delete(instanceId);
      }
    }
  }

  public stop(): void {
    if (this.cleanupInterval) {
      clearInterval(this.cleanupInterval);
      this.cleanupInterval = undefined;
    }
  }
}

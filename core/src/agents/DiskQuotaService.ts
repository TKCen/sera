import { execSync } from 'child_process';
import { Logger } from '../lib/logger.js';
import type { AgentRegistry } from './registry.service.js';
import type { ResolvedCapabilities } from './manifest/types.js';

const logger = new Logger('DiskQuotaService');

/**
 * DiskQuotaService — handles periodic disk quota checks for agent workspaces.
 * Story 3.12
 */
export class DiskQuotaService {
  private registry: AgentRegistry | undefined;
  private interval: NodeJS.Timeout | undefined;
  private onLifecycleEvent: ((type: string, instanceId: string, agentName: string) => void) | undefined;

  public setRegistry(registry: AgentRegistry): void {
    this.registry = registry;
  }

  public setOnLifecycleEvent(callback: (type: string, instanceId: string, agentName: string) => void): void {
    this.onLifecycleEvent = callback;
  }

  public start(intervalMs: number = 15 * 60 * 1000): void {
    if (this.interval) return;
    this.interval = setInterval(() => {
      this.runCheck().catch((err) => logger.error('Disk quota check error:', err));
    }, intervalMs);
  }

  public stop(): void {
    if (this.interval) {
      clearInterval(this.interval);
      this.interval = undefined;
    }
  }

  public async runCheck(): Promise<void> {
    if (!this.registry) return;

    const running = await this.registry.listInstances({ status: 'running' });
    const throttled = await this.registry.listInstances({ status: 'throttled' });

    for (const instance of [...running, ...throttled]) {
      const caps = instance.resolved_capabilities as ResolvedCapabilities;
      const limitGB: number | undefined = caps?.filesystem?.maxWorkspaceSizeGB;

      const workspacePath = instance.workspace_path;
      if (!workspacePath) continue;

      // Log startup warning if no limit is set and agent has write access (Story 3.12)
      if (!limitGB && caps?.filesystem?.write) {
        logger.warn(
          `Agent ${instance.name} has filesystem.write but no maxWorkspaceSizeGB — no quota enforced`
        );
        continue;
      }

      if (!limitGB) continue;

      let usedGB = 0;
      try {
        const duOutput = execSync(
          `du -s --block-size=1G "${workspacePath}" 2>/dev/null || echo "0"`,
          {
            encoding: 'utf-8',
            shell: '/bin/sh',
          }
        );
        usedGB = parseInt(duOutput.split('\t')[0] ?? '0', 10) || 0;
      } catch {
        // du not available (Windows dev env) — skip
        continue;
      }

      await this.registry.updateWorkspaceUsage(instance.id, usedGB);

      const isEphemeral = instance.lifecycle_mode === 'ephemeral';

      if (usedGB >= limitGB) {
        if (!isEphemeral || instance.status !== 'throttled') {
          await this.registry.updateInstanceStatus(instance.id, 'throttled');
          this.onLifecycleEvent?.('throttled', instance.id, instance.name);
          logger.warn(`Agent ${instance.name} exceeded disk quota: ${usedGB}GB / ${limitGB}GB`);
        }
      } else if ((instance.status as string) === 'throttled') {
        // Usage dropped below limit — restore to running
        await this.registry.updateInstanceStatus(instance.id, 'running');
        this.onLifecycleEvent?.('running', instance.id, instance.name);
        logger.info(`Agent ${instance.name} usage back within quota: ${usedGB}GB / ${limitGB}GB`);
      }
    }
  }
}

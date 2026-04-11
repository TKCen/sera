import { Logger } from '../lib/logger.js';
import type { AgentRegistry } from './registry.service.js';
import type { IntercomService } from '../intercom/IntercomService.js';

const logger = new Logger('HeartbeatService');

// Story 3.6 — agents that miss heartbeats for this long are marked unresponsive
const HEARTBEAT_STALE_MS = parseInt(process.env.HEARTBEAT_STALE_MS ?? '120000', 10);

/**
 * HeartbeatService — tracks agent container heartbeats and detects staleness.
 */
export class HeartbeatService {
  /** Last heartbeat timestamp per agent instance ID */
  private heartbeats: Map<string, Date> = new Map();
  private heartbeatInterval: NodeJS.Timeout | undefined;
  private registry: AgentRegistry | undefined;
  private intercom: IntercomService | undefined;

  constructor() {
    this.heartbeatInterval = setInterval(() => {
      this.checkStaleInstances().catch((err) => logger.error('Heartbeat check error:', err));
    }, 30000);
  }

  public setRegistry(registry: AgentRegistry): void {
    this.registry = registry;
  }

  public setIntercom(intercom: IntercomService): void {
    this.intercom = intercom;
  }

  public async registerHeartbeat(instanceId: string): Promise<void> {
    this.heartbeats.set(instanceId, new Date());
    if (this.registry) {
      await this.registry.updateLastHeartbeat(instanceId);
    }
  }

  public async checkStaleInstances(): Promise<void> {
    const now = new Date();
    for (const [instanceId, lastHeartbeat] of this.heartbeats.entries()) {
      if (now.getTime() - lastHeartbeat.getTime() > HEARTBEAT_STALE_MS) {
        logger.warn(`Agent instance ${instanceId} has missed heartbeats — marking unresponsive`);
        this.heartbeats.delete(instanceId);
        if (this.registry) {
          await this.registry.updateInstanceStatus(instanceId, 'unresponsive');
        }
        this.publishLifecycleEvent('unresponsive', instanceId);
      }
    }
  }

  public getUnhealthyInstances(
    timeoutMs: number = HEARTBEAT_STALE_MS
  ): { instanceId: string; lastSeen: Date }[] {
    const now = new Date();
    const unhealthy: { instanceId: string; lastSeen: Date }[] = [];
    for (const [instanceId, lastHeartbeat] of this.heartbeats.entries()) {
      if (now.getTime() - lastHeartbeat.getTime() > timeoutMs) {
        unhealthy.push({ instanceId, lastSeen: lastHeartbeat });
      }
    }
    return unhealthy;
  }

  public removeHeartbeat(instanceId: string): void {
    this.heartbeats.delete(instanceId);
  }

  public stop(): void {
    if (this.heartbeatInterval) {
      clearInterval(this.heartbeatInterval);
      this.heartbeatInterval = undefined;
    }
  }

  private publishLifecycleEvent(type: string, instanceId: string): void {
    if (!this.intercom) return;
    this.intercom
      .publish('system.agents', {
        type,
        agentId: instanceId,
        timestamp: new Date().toISOString(),
      })
      .catch((err) => logger.error('Failed to publish heartbeat lifecycle event:', err));
  }
}

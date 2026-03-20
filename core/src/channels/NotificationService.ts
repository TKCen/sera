import { v4 as uuidv4 } from 'uuid';
import { PgBoss } from 'pg-boss';
import { pool } from '../lib/database.js';
import { Logger } from '../lib/logger.js';
import { ChannelRouter } from './ChannelRouter.js';
import { ActionTokenService } from './ActionTokenService.js';
import type { Channel, ChannelEvent, ChannelSeverity, RequestType } from './channel.interface.js';
import { WebhookChannel } from './adapters/WebhookChannel.js';
import { EmailChannel } from './adapters/EmailChannel.js';
import { DiscordChannel } from './adapters/DiscordChannel.js';
import { SlackChannel } from './adapters/SlackChannel.js';
import { AuditService } from '../audit/AuditService.js';
import type { PermissionRequestService } from '../sandbox/PermissionRequestService.js';

const logger = new Logger('NotificationService');

const DISPATCH_JOB = 'notification.dispatch';

export interface ChannelRecord {
  id: string;
  name: string;
  type: string;
  config: Record<string, unknown>;
  enabled: boolean;
  createdAt: string;
}

export class NotificationService {
  private static instance: NotificationService;
  private boss: PgBoss | null = null;
  private permissionService: PermissionRequestService | null = null;
  private initialized = false;

  private constructor() {}

  static getInstance(): NotificationService {
    if (!NotificationService.instance) {
      NotificationService.instance = new NotificationService();
    }
    return NotificationService.instance;
  }

  setPermissionService(svc: PermissionRequestService): void {
    this.permissionService = svc;
  }

  async start(databaseUrl: string): Promise<void> {
    if (this.initialized) return;

    this.boss = new PgBoss(databaseUrl);
    this.boss.on('error', (err: unknown) => {
      logger.warn('pg-boss error:', err);
    });
    await this.boss.start();

    await this.boss.work<{ eventJson: string; channelId: string; dispatchId: string }>(
      DISPATCH_JOB,
      async (jobs) => {
        for (const job of jobs) {
          const { eventJson, channelId, dispatchId } = job.data;
          const event: ChannelEvent = JSON.parse(eventJson);
          const channel = ChannelRouter.getInstance().getChannel(channelId);
          if (!channel) {
            logger.warn(`Dispatch job: channel ${channelId} not registered`);
            continue;
          }
          try {
            await channel.send(event);
            await pool
              .query(
                `UPDATE notification_dispatches SET status = 'sent', sent_at = now() WHERE id = $1`,
                [dispatchId]
              )
              .catch(() => {});
          } catch (err: unknown) {
            const msg = err instanceof Error ? err.message : String(err);
            await pool
              .query(`UPDATE notification_dispatches SET last_error = $2 WHERE id = $1`, [
                dispatchId,
                msg,
              ])
              .catch(() => {});
            throw err; // Let pg-boss handle retry
          }
        }
      }
    );

    await this.loadChannels();
    this.initialized = true;
    logger.info('NotificationService started');
  }

  async stop(): Promise<void> {
    await this.boss?.stop();
    this.boss = null;
    this.initialized = false;
  }

  private async loadChannels(): Promise<void> {
    try {
      const { rows } = await pool.query<{
        id: string;
        name: string;
        type: string;
        config: Record<string, unknown>;
        enabled: boolean;
      }>('SELECT id, name, type, config, enabled FROM notification_channels WHERE enabled = true');

      for (const row of rows) {
        try {
          const channel = this.buildAdapter(row.id, row.name, row.type, row.config);
          if (channel) ChannelRouter.getInstance().register(channel);
        } catch (err) {
          logger.warn(`Failed to build adapter for channel ${row.id} (${row.type}):`, err);
        }
      }

      logger.info(`Loaded ${rows.length} notification channels`);
    } catch (err) {
      logger.warn('Failed to load notification channels (table may not exist yet):', err);
    }
  }

  buildAdapter(
    id: string,
    name: string,
    type: string,
    config: Record<string, unknown>
  ): Channel | null {
    switch (type) {
      case 'webhook': {
        const url = config['url'];
        if (!url || typeof url !== 'string') return null;
        return new WebhookChannel(id, name, config);
      }
      case 'email': {
        const host = config['smtpHost'];
        if (!host || typeof host !== 'string') return null;
        return new EmailChannel(id, name, config);
      }
      case 'discord': {
        const webhookUrl = config['webhookUrl'];
        if (!webhookUrl || typeof webhookUrl !== 'string') return null;
        return new DiscordChannel(id, name, config);
      }
      case 'slack': {
        const webhookUrl = config['webhookUrl'];
        if (!webhookUrl || typeof webhookUrl !== 'string') return null;
        return new SlackChannel(id, name, config);
      }
      default:
        logger.warn(`Unknown channel type: ${type}`);
        return null;
    }
  }

  registerChannelInstance(channel: Channel): void {
    ChannelRouter.getInstance().register(channel);
  }

  async createChannel(
    name: string,
    type: string,
    config: Record<string, unknown>
  ): Promise<ChannelRecord> {
    const id = uuidv4();
    const { rows } = await pool.query<{
      id: string;
      name: string;
      type: string;
      config: Record<string, unknown>;
      enabled: boolean;
      created_at: Date;
    }>(
      `INSERT INTO notification_channels (id, name, type, config)
       VALUES ($1, $2, $3, $4)
       RETURNING *`,
      [id, name, type, JSON.stringify(config)]
    );

    const row = rows[0]!;

    const channel = this.buildAdapter(row.id, row.name, row.type, row.config);
    if (channel) ChannelRouter.getInstance().register(channel);

    return {
      id: row.id,
      name: row.name,
      type: row.type,
      config: this.redactConfig(row.type, row.config),
      enabled: row.enabled,
      createdAt: row.created_at.toISOString(),
    };
  }

  async deleteChannel(id: string): Promise<boolean> {
    ChannelRouter.getInstance().unregister(id);
    await pool.query('DELETE FROM notification_routing_rules WHERE $1 = ANY(channel_ids)', [id]);
    const { rowCount } = await pool.query('DELETE FROM notification_channels WHERE id = $1', [id]);
    return (rowCount ?? 0) > 0;
  }

  async listChannels(): Promise<ChannelRecord[]> {
    const { rows } = await pool.query<{
      id: string;
      name: string;
      type: string;
      config: Record<string, unknown>;
      enabled: boolean;
      created_at: Date;
    }>(
      'SELECT id, name, type, config, enabled, created_at FROM notification_channels ORDER BY created_at'
    );

    return rows.map((r) => ({
      id: r.id,
      name: r.name,
      type: r.type,
      config: this.redactConfig(r.type, r.config),
      enabled: r.enabled,
      createdAt: r.created_at.toISOString(),
    }));
  }

  private redactConfig(type: string, config: Record<string, unknown>): Record<string, unknown> {
    const sensitive = new Set([
      'secret',
      'password',
      'smtpPassword',
      'botToken',
      'appToken',
      'signingSecret',
      'token',
    ]);
    const out: Record<string, unknown> = {};
    for (const [k, v] of Object.entries(config)) {
      out[k] = sensitive.has(k) ? '[redacted]' : v;
    }
    return out;
  }

  /**
   * Dispatch a notification event. Called by PermissionRequestService, DelegationService, etc.
   * Non-blocking — failures are logged, never thrown.
   */
  dispatchEvent(
    eventType: string,
    title: string,
    body: string,
    severity: ChannelSeverity,
    metadata: Record<string, unknown>,
    actionable?: {
      requestId: string;
      requestType: RequestType;
    }
  ): void {
    (async () => {
      const event: ChannelEvent = {
        id: uuidv4(),
        eventType,
        title,
        body,
        severity,
        metadata,
        timestamp: new Date().toISOString(),
      };

      if (actionable) {
        const tokens = await ActionTokenService.getInstance().issue(
          actionable.requestId,
          actionable.requestType
        );
        event.actions = {
          requestId: actionable.requestId,
          requestType: actionable.requestType,
          ...tokens,
        };
      }

      ChannelRouter.getInstance().route(event);
    })().catch((err: unknown) => {
      logger.warn('dispatchEvent error:', err);
    });
  }

  /**
   * Execute a HitL decision from an action token.
   * Used by POST /api/notifications/action.
   */
  async executeAction(
    requestId: string,
    action: 'approve' | 'deny',
    requestType: RequestType,
    channelSource: string,
    channelType: string
  ): Promise<void> {
    const decision = action === 'approve' ? 'grant' : 'deny';

    if (requestType === 'permission' && this.permissionService) {
      await this.permissionService.decide(requestId, {
        decision: decision as 'grant' | 'deny',
        grantType: 'session',
      });
    }

    await AuditService.getInstance()
      .record({
        actorType: 'operator',
        actorId: `channel:${channelSource}`,
        actingContext: null,
        eventType: `${requestType}.${decision}ed`,
        payload: {
          requestId,
          requestType,
          actorAuthMethod: 'channel-action-token',
          channel: channelSource,
          channelType,
        },
      })
      .catch(() => {});

    ChannelRouter.getInstance()
      .markStale(requestId)
      .catch(() => {});
  }
}

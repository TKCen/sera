import { v4 as uuidv4 } from 'uuid';
import type { PgBoss } from 'pg-boss';
import { pool } from '../lib/database.js';
import { Logger } from '../lib/logger.js';
import { ChannelRouter } from './ChannelRouter.js';
import { ActionTokenService } from './ActionTokenService.js';
import type { Channel, ChannelEvent, ChannelSeverity, RequestType } from './channel.interface.js';
import { WebhookChannel } from './adapters/WebhookChannel.js';
import { EmailChannel } from './adapters/EmailChannel.js';
import { DiscordChannel } from './adapters/DiscordChannel.js';
import { SlackChannel } from './adapters/SlackChannel.js';
import { DiscordChatAdapter } from './adapters/DiscordChatAdapter.js';
import type { DiscordChatConfig } from './adapters/DiscordChatAdapter.js';
import { AuditService } from '../audit/AuditService.js';
import type { PermissionRequestService } from '../sandbox/PermissionRequestService.js';
import type { Orchestrator } from '../agents/Orchestrator.js';
import type { SessionStore } from '../sessions/SessionStore.js';

const logger = new Logger('NotificationService');

const DISPATCH_JOB = 'notification.dispatch';

export interface ChannelRecord {
  id: string;
  name: string;
  description?: string;
  type: string;
  config: Record<string, unknown>;
  enabled: boolean;
  createdAt: string;
}

export class NotificationService {
  private static instance: NotificationService;
  private boss: PgBoss | null = null;
  private permissionService: PermissionRequestService | null = null;
  private orchestrator: Orchestrator | null = null;
  private sessionStore: SessionStore | null = null;
  private chatAdapters = new Map<string, DiscordChatAdapter>();
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

  setOrchestrator(orchestrator: Orchestrator): void {
    this.orchestrator = orchestrator;
  }

  setSessionStore(sessionStore: SessionStore): void {
    this.sessionStore = sessionStore;
  }

  async start(boss: PgBoss): Promise<void> {
    if (this.initialized) return;

    this.boss = boss;
    await this.boss.createQueue(DISPATCH_JOB);

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
    // PgBoss lifecycle is managed by PgBossService singleton
    this.boss = null;
    this.initialized = false;
  }

  private async loadChannels(): Promise<void> {
    try {
      const { rows } = await pool.query<{
        id: string;
        name: string;
        description?: string;
        type: string;
        config: Record<string, unknown>;
        enabled: boolean;
      }>(
        'SELECT id, name, description, type, config, enabled FROM notification_channels WHERE enabled = true'
      );

      for (const row of rows) {
        try {
          if (row.type === 'discord-chat') {
            this.startChatAdapter(row.id, row.config);
          } else {
            const channel = this.buildAdapter(row.id, row.name, row.type, row.config);
            if (channel) ChannelRouter.getInstance().register(channel);
          }
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
      case 'discord-chat':
        // Handled separately — this is a bidirectional chat adapter, not a notification channel
        return null;
      default:
        logger.warn(`Unknown channel type: ${type}`);
        return null;
    }
  }

  /**
   * Start a DiscordChatAdapter for a discord-chat channel config.
   */
  private startChatAdapter(channelId: string, config: Record<string, unknown>): void {
    if (!this.orchestrator || !this.sessionStore) {
      logger.warn('Cannot start discord-chat adapter: orchestrator/sessionStore not set');
      return;
    }

    const botToken = config['botToken'];
    const targetAgentId = config['targetAgentId'];
    const applicationId = config['applicationId'];
    if (typeof botToken !== 'string' || typeof targetAgentId !== 'string') {
      logger.warn(`Invalid discord-chat config for channel ${channelId}`);
      return;
    }
    if (typeof applicationId !== 'string') {
      logger.warn(
        `discord-chat channel ${channelId}: no applicationId — slash commands will not be registered`
      );
    }

    const chatConfig: DiscordChatConfig = {
      botToken,
      applicationId: typeof applicationId === 'string' ? applicationId : '',
      targetAgentId,
      ...(Array.isArray(config['allowedGuilds'])
        ? { allowedGuilds: config['allowedGuilds'] as string[] }
        : {}),
      ...(Array.isArray(config['allowedUsers'])
        ? { allowedUsers: config['allowedUsers'] as string[] }
        : {}),
      ...(typeof config['allowDMs'] === 'boolean' ? { allowDMs: config['allowDMs'] } : {}),
      ...(typeof config['allowMentions'] === 'boolean'
        ? { allowMentions: config['allowMentions'] }
        : {}),
      ...(typeof config['responsePrefix'] === 'string'
        ? { responsePrefix: config['responsePrefix'] }
        : {}),
    };

    const adapter = new DiscordChatAdapter(
      channelId,
      chatConfig,
      this.orchestrator,
      this.sessionStore
    );
    this.chatAdapters.set(channelId, adapter);
    adapter
      .start()
      .catch((err: unknown) =>
        logger.error(`Failed to start discord-chat adapter ${channelId}:`, err)
      );
  }

  /**
   * Stop a DiscordChatAdapter by channel ID.
   */
  private stopChatAdapter(channelId: string): void {
    const adapter = this.chatAdapters.get(channelId);
    if (adapter) {
      adapter
        .stop()
        .catch((err: unknown) =>
          logger.error(`Failed to stop discord-chat adapter ${channelId}:`, err)
        );
      this.chatAdapters.delete(channelId);
    }
  }

  registerChannelInstance(channel: Channel): void {
    ChannelRouter.getInstance().register(channel);
  }

  async createChannel(
    name: string,
    type: string,
    config: Record<string, unknown>,
    description?: string
  ): Promise<ChannelRecord> {
    const id = uuidv4();
    const { rows } = await pool.query<{
      id: string;
      name: string;
      description?: string;
      type: string;
      config: Record<string, unknown>;
      enabled: boolean;
      created_at: Date;
    }>(
      `INSERT INTO notification_channels (id, name, type, config, description)
       VALUES ($1, $2, $3, $4, $5)
       RETURNING *`,
      [id, name, type, JSON.stringify(config), description]
    );

    const row = rows[0]!;

    if (row.type === 'discord-chat') {
      this.startChatAdapter(row.id, row.config);
    } else {
      const channel = this.buildAdapter(row.id, row.name, row.type, row.config);
      if (channel) ChannelRouter.getInstance().register(channel);
    }

    return {
      id: row.id,
      name: row.name,
      description: row.description,
      type: row.type,
      config: this.redactConfig(row.type, row.config),
      enabled: row.enabled,
      createdAt: row.created_at.toISOString(),
    };
  }

  async updateChannel(
    id: string,
    updates: {
      name?: string;
      description?: string;
      config?: Record<string, unknown>;
      enabled?: boolean;
    }
  ): Promise<ChannelRecord | null> {
    // Fetch existing channel (with full unredacted config)
    const existing = await pool.query<{
      id: string;
      name: string;
      description?: string;
      type: string;
      config: Record<string, unknown>;
      enabled: boolean;
      created_at: Date;
    }>('SELECT * FROM notification_channels WHERE id = $1', [id]);

    const row = existing.rows[0];
    if (!row) return null;

    // Merge config: new values override existing, but unmentioned keys are preserved.
    // Also, if a new value is '[redacted]', keep the existing unredacted value.
    let mergedConfig = row.config;
    if (updates.config !== undefined) {
      mergedConfig = { ...row.config };
      for (const [k, v] of Object.entries(updates.config)) {
        if (v === '[redacted]') {
          // Keep existing
          continue;
        }
        mergedConfig[k] = v;
      }
    }

    const mergedName = updates.name ?? row.name;
    const mergedDescription = updates.description ?? row.description;
    const mergedEnabled = updates.enabled ?? row.enabled;

    const { rows } = await pool.query<{
      id: string;
      name: string;
      description?: string;
      type: string;
      config: Record<string, unknown>;
      enabled: boolean;
      created_at: Date;
    }>(
      `UPDATE notification_channels
         SET name = $2, config = $3, enabled = $4, description = $5
       WHERE id = $1
       RETURNING *`,
      [id, mergedName, JSON.stringify(mergedConfig), mergedEnabled, mergedDescription]
    );

    const updated = rows[0]!;

    // Restart the adapter with the new config
    if (updated.type === 'discord-chat') {
      this.stopChatAdapter(id);
      if (updated.enabled) this.startChatAdapter(id, updated.config);
    } else {
      ChannelRouter.getInstance().unregister(id);
      if (updated.enabled) {
        const channel = this.buildAdapter(updated.id, updated.name, updated.type, updated.config);
        if (channel) ChannelRouter.getInstance().register(channel);
      }
    }

    return {
      id: updated.id,
      name: updated.name,
      description: updated.description,
      type: updated.type,
      config: this.redactConfig(updated.type, updated.config),
      enabled: updated.enabled,
      createdAt: updated.created_at.toISOString(),
    };
  }

  async deleteChannel(id: string): Promise<boolean> {
    ChannelRouter.getInstance().unregister(id);
    this.stopChatAdapter(id);
    await pool.query('DELETE FROM notification_routing_rules WHERE $1 = ANY(channel_ids)', [id]);
    const { rowCount } = await pool.query('DELETE FROM notification_channels WHERE id = $1', [id]);
    return (rowCount ?? 0) > 0;
  }

  async listChannels(): Promise<ChannelRecord[]> {
    const { rows } = await pool.query<{
      id: string;
      name: string;
      description?: string;
      type: string;
      config: Record<string, unknown>;
      enabled: boolean;
      created_at: Date;
    }>(
      'SELECT id, name, description, type, config, enabled, created_at FROM notification_channels ORDER BY created_at'
    );

    return rows.map((r) => ({
      id: r.id,
      name: r.name,
      description: r.description,
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

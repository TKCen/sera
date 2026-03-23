/**
 * Notification & Integration Channel Routes — Epic 18
 *
 * Stories covered:
 *   18.1 — Channel CRUD: GET/POST /api/channels, DELETE /api/channels/:id, POST /api/channels/:id/test
 *   18.2 — Routing rule CRUD: GET/POST/DELETE /api/notifications/routing
 *   18.3 — HitL action endpoint: POST /api/notifications/action (public)
 *   18.5 — Inbound routes: GET/POST /api/channels/routes
 *   User mappings: POST /api/channels/discord/user-mapping, POST /api/channels/slack/user-mapping
 */

import { Router } from 'express';
import { v4 as uuidv4 } from 'uuid';
import crypto from 'node:crypto';
import type { RequestHandler } from 'express';
import { pool } from '../lib/database.js';
import { requireRole } from '../auth/authMiddleware.js';
import { NotificationService } from '../channels/NotificationService.js';
import { ChannelRouter } from '../channels/ChannelRouter.js';
import { ActionTokenService } from '../channels/ActionTokenService.js';
import { Logger } from '../lib/logger.js';
import type { ChannelEvent } from '../channels/channel.interface.js';

const logger = new Logger('NotificationsRouter');

// ── Helpers ──────────────────────────────────────────────────────────────────

type IdParam = { id: string };

// ── Router factory ───────────────────────────────────────────────────────────

export function createNotificationsRouter(): {
  publicRouter: Router;
  protectedRouter: Router;
} {
  const publicRouter = Router();
  const protectedRouter = Router();

  // ══════════════════════════════════════════════════════════════════════════
  // PUBLIC — Action token redemption (Story 18.3)
  // ══════════════════════════════════════════════════════════════════════════

  /**
   * POST /api/notifications/action
   * Query param: ?token=<jwt>
   * No session auth — token is the authorisation.
   */
  publicRouter.post('/action', (async (req, res) => {
    const token =
      (req.query['token'] as string | undefined) ?? (req.body as { token?: string })['token'];

    if (!token) {
      return void res.status(400).json({ error: 'token is required' });
    }

    try {
      const claims = await ActionTokenService.getInstance().verify(token);
      const svc = NotificationService.getInstance();

      await svc.executeAction(
        claims.sub,
        claims.action,
        claims.requestType,
        'channel-action',
        'token'
      );

      res.json({ ok: true, requestId: claims.sub, decision: claims.action });
    } catch (err: unknown) {
      const msg = err instanceof Error ? err.message : String(err);
      if (msg.includes('expired') || msg.includes('JWTExpired')) {
        return void res.status(401).json({ error: 'Token expired' });
      }
      res.status(401).json({ error: 'Invalid token' });
    }
  }) as RequestHandler);

  // ══════════════════════════════════════════════════════════════════════════
  // PROTECTED — Channel management (Story 18.1)
  // ══════════════════════════════════════════════════════════════════════════

  /** POST /api/channels — create a channel */
  protectedRouter.post('/', requireRole(['admin']), (async (req, res) => {
    const { name, type, config } = req.body as {
      name?: string;
      type?: string;
      config?: Record<string, unknown>;
    };

    if (!name || !type || !config) {
      return void res.status(400).json({ error: 'name, type, and config are required' });
    }

    try {
      const channel = await NotificationService.getInstance().createChannel(name, type, config);
      res.status(201).json(channel);
    } catch (err: unknown) {
      logger.error('Create channel error:', err);
      res.status(500).json({ error: 'Failed to create channel' });
    }
  }) as RequestHandler);

  /** GET /api/channels — list channels (config values redacted) */
  protectedRouter.get('/', (async (_req, res) => {
    try {
      const channels = await NotificationService.getInstance().listChannels();
      res.json(channels);
    } catch (err: unknown) {
      logger.error('List channels error:', err);
      res.status(500).json({ error: 'Failed to list channels' });
    }
  }) as RequestHandler);

  /** PATCH /api/channels/:id — update channel name, config, or enabled status */
  protectedRouter.patch('/:id', requireRole(['admin', 'operator']), (async (req, res) => {
    const { id } = req.params as IdParam;
    const { name, config, enabled } = req.body as {
      name?: string;
      config?: Record<string, unknown>;
      enabled?: boolean;
    };

    if (name === undefined && config === undefined && enabled === undefined) {
      return void res
        .status(400)
        .json({ error: 'No update fields provided (name, config, enabled)' });
    }

    try {
      const updates: { name?: string; config?: Record<string, unknown>; enabled?: boolean } = {};
      if (name !== undefined) updates.name = name;
      if (config !== undefined) updates.config = config;
      if (enabled !== undefined) updates.enabled = enabled;
      const updated = await NotificationService.getInstance().updateChannel(id, updates);
      if (!updated) return void res.status(404).json({ error: 'Channel not found' });
      res.json(updated);
    } catch (err: unknown) {
      logger.error('Update channel error:', err);
      res.status(500).json({ error: 'Failed to update channel' });
    }
  }) as RequestHandler);

  /** DELETE /api/channels/:id */
  protectedRouter.delete('/:id', requireRole(['admin']), (async (req, res) => {
    const { id } = req.params as IdParam;
    try {
      const deleted = await NotificationService.getInstance().deleteChannel(id);
      if (!deleted) return void res.status(404).json({ error: 'Channel not found' });
      res.json({ ok: true });
    } catch (err: unknown) {
      logger.error('Delete channel error:', err);
      res.status(500).json({ error: 'Failed to delete channel' });
    }
  }) as RequestHandler);

  /** POST /api/channels/:id/test — send a test event */
  protectedRouter.post('/:id/test', requireRole(['admin', 'operator']), (async (req, res) => {
    const { id } = req.params as IdParam;
    const channel = ChannelRouter.getInstance().getChannel(id);
    if (!channel) return void res.status(404).json({ error: 'Channel not found or not active' });

    const testEvent: ChannelEvent = {
      id: uuidv4(),
      eventType: 'system.test',
      title: 'SERA Test Notification',
      body: 'This is a test notification from SERA.',
      severity: 'info',
      metadata: { source: 'test-button' },
      timestamp: new Date().toISOString(),
    };

    try {
      await channel.send(testEvent);
      res.json({ ok: true });
    } catch (err: unknown) {
      const msg = err instanceof Error ? err.message : String(err);
      res.status(502).json({ ok: false, error: msg });
    }
  }) as RequestHandler);

  /** GET /api/channels/:id/health */
  protectedRouter.get('/:id/health', (async (req, res) => {
    const { id } = req.params as IdParam;
    const channel = ChannelRouter.getInstance().getChannel(id);
    if (!channel) return void res.status(404).json({ error: 'Channel not found or not active' });

    const health = await channel.healthCheck().catch((err: unknown) => ({
      healthy: false,
      error: err instanceof Error ? err.message : String(err),
    }));
    res.json(health);
  }) as RequestHandler);

  // ══════════════════════════════════════════════════════════════════════════
  // PROTECTED — Routing rules (Story 18.2)
  // ══════════════════════════════════════════════════════════════════════════

  /** POST /api/notifications/routing */
  protectedRouter.post('/routing', requireRole(['admin']), (async (req, res) => {
    const {
      eventType,
      channelIds,
      filter,
      minSeverity = 'info',
    } = req.body as {
      eventType?: string;
      channelIds?: string[];
      filter?: Record<string, unknown>;
      minSeverity?: string;
    };

    if (!eventType || !channelIds || channelIds.length === 0) {
      return void res.status(400).json({ error: 'eventType and channelIds are required' });
    }

    const id = uuidv4();
    const { rows } = await pool
      .query<{
        id: string;
        event_type: string;
        channel_ids: string[];
        filter: unknown;
        min_severity: string;
        created_at: Date;
      }>(
        `INSERT INTO notification_routing_rules (id, event_type, channel_ids, filter, min_severity)
         VALUES ($1, $2, $3, $4, $5)
         RETURNING *`,
        [id, eventType, channelIds, filter ? JSON.stringify(filter) : null, minSeverity]
      )
      .catch((err: unknown) => {
        logger.error('Create routing rule error:', err);
        throw err;
      });

    const row = rows[0]!;
    res.status(201).json({
      id: row.id,
      eventType: row.event_type,
      channelIds: row.channel_ids,
      filter: row.filter,
      minSeverity: row.min_severity,
      createdAt: row.created_at.toISOString(),
    });
  }) as RequestHandler);

  /** GET /api/notifications/routing */
  protectedRouter.get('/routing', (async (_req, res) => {
    const { rows } = await pool.query<{
      id: string;
      event_type: string;
      channel_ids: string[];
      filter: unknown;
      min_severity: string;
      created_at: Date;
    }>('SELECT * FROM notification_routing_rules ORDER BY created_at');

    res.json(
      rows.map((r) => ({
        id: r.id,
        eventType: r.event_type,
        channelIds: r.channel_ids,
        filter: r.filter,
        minSeverity: r.min_severity,
        createdAt: r.created_at.toISOString(),
      }))
    );
  }) as RequestHandler);

  /** PATCH /api/notifications/routing/:id */
  protectedRouter.patch('/routing/:id', requireRole(['admin']), (async (req, res) => {
    const { id } = req.params as IdParam;
    const { channelIds, minSeverity, filter } = req.body as {
      channelIds?: string[];
      minSeverity?: string;
      filter?: Record<string, unknown> | null;
    };

    const updates: string[] = [];
    const params: unknown[] = [id];

    if (channelIds !== undefined) {
      params.push(channelIds);
      updates.push(`channel_ids = $${params.length}`);
    }
    if (minSeverity !== undefined) {
      params.push(minSeverity);
      updates.push(`min_severity = $${params.length}`);
    }
    if (filter !== undefined) {
      params.push(filter ? JSON.stringify(filter) : null);
      updates.push(`filter = $${params.length}`);
    }

    if (updates.length === 0) {
      return void res.status(400).json({ error: 'No fields to update' });
    }

    const { rows, rowCount } = await pool.query<{
      id: string;
      event_type: string;
      channel_ids: string[];
      filter: unknown;
      min_severity: string;
      created_at: Date;
    }>(
      `UPDATE notification_routing_rules SET ${updates.join(', ')} WHERE id = $1 RETURNING *`,
      params
    );

    if (!rowCount) return void res.status(404).json({ error: 'Rule not found' });
    const row = rows[0]!;
    res.json({
      id: row.id,
      eventType: row.event_type,
      channelIds: row.channel_ids,
      filter: row.filter,
      minSeverity: row.min_severity,
      createdAt: row.created_at.toISOString(),
    });
  }) as RequestHandler);

  /** DELETE /api/notifications/routing/:id */
  protectedRouter.delete('/routing/:id', requireRole(['admin']), (async (req, res) => {
    const { id } = req.params as IdParam;
    const { rowCount } = await pool.query('DELETE FROM notification_routing_rules WHERE id = $1', [
      id,
    ]);
    if (!rowCount) return void res.status(404).json({ error: 'Rule not found' });
    res.json({ ok: true });
  }) as RequestHandler);

  // ══════════════════════════════════════════════════════════════════════════
  // PROTECTED — Inbound channel routes (Story 18.5)
  // ══════════════════════════════════════════════════════════════════════════

  /** POST /api/channels/routes */
  protectedRouter.post('/routes', requireRole(['admin', 'operator']), (async (req, res) => {
    const { channelId, channelType, platformChannelId, targetAgentId, prefix, taskTemplate } =
      req.body as {
        channelId?: string;
        channelType?: string;
        platformChannelId?: string;
        targetAgentId?: string;
        prefix?: string;
        taskTemplate?: string;
      };

    if (!channelId || !channelType || !platformChannelId || !targetAgentId) {
      return void res
        .status(400)
        .json({ error: 'channelId, channelType, platformChannelId, targetAgentId are required' });
    }

    const id = uuidv4();
    const template = taskTemplate ?? '{{message}}';

    const { rows } = await pool.query<{
      id: string;
      channel_id: string;
      channel_type: string;
      platform_channel_id: string;
      target_agent_id: string;
      prefix: string | null;
      task_template: string;
      created_at: Date;
    }>(
      `INSERT INTO inbound_channel_routes
         (id, channel_id, channel_type, platform_channel_id, target_agent_id, prefix, task_template)
       VALUES ($1, $2, $3, $4, $5, $6, $7)
       RETURNING *`,
      [id, channelId, channelType, platformChannelId, targetAgentId, prefix ?? null, template]
    );

    const row = rows[0]!;
    res.status(201).json({
      id: row.id,
      channelId: row.channel_id,
      channelType: row.channel_type,
      platformChannelId: row.platform_channel_id,
      targetAgentId: row.target_agent_id,
      prefix: row.prefix,
      taskTemplate: row.task_template,
      createdAt: row.created_at.toISOString(),
    });
  }) as RequestHandler);

  /** GET /api/channels/routes */
  protectedRouter.get('/routes', (async (_req, res) => {
    const { rows } = await pool.query<{
      id: string;
      channel_id: string;
      channel_type: string;
      platform_channel_id: string;
      target_agent_id: string;
      prefix: string | null;
      task_template: string;
      created_at: Date;
    }>('SELECT * FROM inbound_channel_routes ORDER BY created_at');

    res.json(
      rows.map((r) => ({
        id: r.id,
        channelId: r.channel_id,
        channelType: r.channel_type,
        platformChannelId: r.platform_channel_id,
        targetAgentId: r.target_agent_id,
        prefix: r.prefix,
        taskTemplate: r.task_template,
        createdAt: r.created_at.toISOString(),
      }))
    );
  }) as RequestHandler);

  // ══════════════════════════════════════════════════════════════════════════
  // PROTECTED — User mappings (Stories 18.4/18.5)
  // ══════════════════════════════════════════════════════════════════════════

  /** POST /api/channels/discord/user-mapping */
  protectedRouter.post('/discord/user-mapping', requireRole(['admin', 'operator']), (async (
    req,
    res
  ) => {
    const { channelId, discordUserId, operatorSub } = req.body as {
      channelId?: string;
      discordUserId?: string;
      operatorSub?: string;
    };

    if (!channelId || !discordUserId || !operatorSub) {
      return void res
        .status(400)
        .json({ error: 'channelId, discordUserId, operatorSub are required' });
    }

    await pool.query(
      `INSERT INTO channel_user_mappings (id, channel_id, channel_type, platform_user_id, operator_sub)
       VALUES ($1, $2, 'discord', $3, $4)
       ON CONFLICT (channel_type, platform_user_id) DO UPDATE SET operator_sub = EXCLUDED.operator_sub`,
      [uuidv4(), channelId, discordUserId, operatorSub]
    );

    res.status(201).json({ ok: true });
  }) as RequestHandler);

  /** POST /api/channels/slack/user-mapping */
  protectedRouter.post('/slack/user-mapping', requireRole(['admin', 'operator']), (async (
    req,
    res
  ) => {
    const { channelId, slackUserId, operatorSub } = req.body as {
      channelId?: string;
      slackUserId?: string;
      operatorSub?: string;
    };

    if (!channelId || !slackUserId || !operatorSub) {
      return void res
        .status(400)
        .json({ error: 'channelId, slackUserId, operatorSub are required' });
    }

    await pool.query(
      `INSERT INTO channel_user_mappings (id, channel_id, channel_type, platform_user_id, operator_sub)
       VALUES ($1, $2, 'slack', $3, $4)
       ON CONFLICT (channel_type, platform_user_id) DO UPDATE SET operator_sub = EXCLUDED.operator_sub`,
      [uuidv4(), channelId, slackUserId, operatorSub]
    );

    res.status(201).json({ ok: true });
  }) as RequestHandler);

  // ══════════════════════════════════════════════════════════════════════════
  // PROTECTED — Slack interactive callback handler (Story 18.5)
  // ══════════════════════════════════════════════════════════════════════════

  /**
   * POST /api/channels/slack/interactions
   * Validates Slack request signature before processing.
   */
  protectedRouter.post('/slack/interactions', (async (req, res) => {
    const channelId = req.query['channelId'] as string | undefined;
    if (!channelId) return void res.status(400).json({ error: 'channelId required' });

    // Validate Slack signature
    const signingSecret = await getSlackSigningSecret(channelId);
    if (signingSecret) {
      const timestamp = req.headers['x-slack-request-timestamp'] as string;
      const slackSig = req.headers['x-slack-signature'] as string;
      const rawBody = (req as unknown as { rawBody?: Buffer }).rawBody;

      if (!rawBody || !timestamp || !slackSig) {
        return void res.status(401).json({ error: 'Missing Slack signature headers' });
      }

      const fiveMinAgo = Math.floor(Date.now() / 1000) - 5 * 60;
      if (parseInt(timestamp, 10) < fiveMinAgo) {
        return void res.status(401).json({ error: 'Request too old' });
      }

      const sigBase = `v0:${timestamp}:${rawBody.toString()}`;
      const expected =
        'v0=' + crypto.createHmac('sha256', signingSecret).update(sigBase).digest('hex');

      if (!crypto.timingSafeEqual(Buffer.from(expected), Buffer.from(slackSig))) {
        return void res.status(401).json({ error: 'Invalid signature' });
      }
    }

    const { SlackChannel } = await import('../channels/adapters/SlackChannel.js');
    const channel = ChannelRouter.getInstance().getChannel(channelId);
    if (channel instanceof SlackChannel) {
      const payload = typeof req.body === 'string' ? JSON.parse(req.body) : (req.body as unknown);
      await channel.handleInteractivePayload(
        payload as Parameters<InstanceType<typeof SlackChannel>['handleInteractivePayload']>[0]
      );
    }

    res.json({ ok: true });
  }) as RequestHandler);

  return { publicRouter, protectedRouter };
}

async function getSlackSigningSecret(channelId: string): Promise<string | null> {
  try {
    const { rows } = await pool.query<{ config: Record<string, unknown> }>(
      `SELECT config FROM notification_channels WHERE id = $1 AND type = 'slack'`,
      [channelId]
    );
    const cfg = rows[0]?.config;
    if (!cfg) return null;
    const secret = cfg['signingSecret'];
    return typeof secret === 'string' ? secret : null;
  } catch {
    return null;
  }
}

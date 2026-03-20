import { v4 as uuidv4 } from 'uuid';
import { pool } from '../lib/database.js';
import { Logger } from '../lib/logger.js';
import type { Channel, ChannelEvent, ChannelSeverity } from './channel.interface.js';

const logger = new Logger('ChannelRouter');

const SEVERITY_LEVEL: Record<ChannelSeverity, number> = {
  info: 0,
  warning: 1,
  critical: 2,
};

interface RoutingRule {
  id: string;
  eventType: string;
  channelIds: string[];
  minSeverity: ChannelSeverity;
  filter: Record<string, unknown> | null;
}

/** Wildcard matching: 'permission.*' matches 'permission.requested', '*' matches everything. */
export function matchesPattern(pattern: string, eventType: string): boolean {
  if (pattern === '*') return true;
  if (pattern === eventType) return true;
  if (pattern.endsWith('.*')) {
    const prefix = pattern.slice(0, -2);
    return eventType === prefix || eventType.startsWith(prefix + '.');
  }
  return false;
}

function meetsMinSeverity(event: ChannelSeverity, min: ChannelSeverity): boolean {
  return SEVERITY_LEVEL[event]! >= SEVERITY_LEVEL[min]!;
}

function matchesFilter(event: ChannelEvent, filter: Record<string, unknown> | null): boolean {
  if (!filter) return true;
  for (const [key, value] of Object.entries(filter)) {
    if (event.metadata[key] !== value) return false;
  }
  return true;
}

export class ChannelRouter {
  private static instance: ChannelRouter;
  private channels = new Map<string, Channel>();

  private constructor() {}

  static getInstance(): ChannelRouter {
    if (!ChannelRouter.instance) {
      ChannelRouter.instance = new ChannelRouter();
    }
    return ChannelRouter.instance;
  }

  register(channel: Channel): void {
    this.channels.set(channel.id, channel);
  }

  unregister(channelId: string): void {
    this.channels.delete(channelId);
  }

  getChannel(channelId: string): Channel | undefined {
    return this.channels.get(channelId);
  }

  getAllChannels(): Channel[] {
    return [...this.channels.values()];
  }

  /**
   * Route an event to all matching channels.
   * Non-blocking: failures are logged, never thrown.
   */
  route(event: ChannelEvent): void {
    this.routeAsync(event).catch((err: unknown) => {
      logger.warn('ChannelRouter.route error:', err);
    });
  }

  async routeAsync(event: ChannelEvent): Promise<void> {
    let rules: RoutingRule[] = [];
    try {
      const { rows } = await pool.query<{
        id: string;
        event_type: string;
        channel_ids: string[];
        min_severity: string;
        filter: Record<string, unknown> | null;
      }>(
        'SELECT id, event_type, channel_ids, min_severity, filter FROM notification_routing_rules'
      );

      rules = rows.map((r) => ({
        id: r.id,
        eventType: r.event_type,
        channelIds: r.channel_ids,
        minSeverity: (r.min_severity ?? 'info') as ChannelSeverity,
        filter: r.filter,
      }));
    } catch (err) {
      logger.warn('Failed to load routing rules — skipping dispatch:', err);
      return;
    }

    const matchedChannelIds = new Set<string>();
    for (const rule of rules) {
      if (!matchesPattern(rule.eventType, event.eventType)) continue;
      if (!meetsMinSeverity(event.severity, rule.minSeverity)) continue;
      if (!matchesFilter(event, rule.filter)) continue;
      for (const cid of rule.channelIds) matchedChannelIds.add(cid);
    }

    for (const channelId of matchedChannelIds) {
      const channel = this.channels.get(channelId);
      if (!channel) {
        logger.warn(`Channel ${channelId} in routing rule not registered — skipping`);
        continue;
      }
      this.dispatchToChannel(channel, event);
    }
  }

  private dispatchToChannel(channel: Channel, event: ChannelEvent): void {
    const dispatchId = uuidv4();
    pool
      .query(
        `INSERT INTO notification_dispatches (id, event_id, channel_id, event_type, status, attempts)
         VALUES ($1, $2, $3, $4, 'pending', 0)`,
        [dispatchId, event.id, channel.id, event.eventType]
      )
      .catch(() => {});

    this.sendWithRetry(channel, event, dispatchId, 1);
  }

  private sendWithRetry(
    channel: Channel,
    event: ChannelEvent,
    dispatchId: string,
    attempt: number,
    maxAttempts = 3
  ): void {
    channel
      .send(event)
      .then(() => {
        pool
          .query(
            `UPDATE notification_dispatches
             SET status = 'sent', sent_at = now(), attempts = $2
             WHERE id = $1`,
            [dispatchId, attempt]
          )
          .catch(() => {});
      })
      .catch((err: unknown) => {
        const errMsg = err instanceof Error ? err.message : String(err);
        logger.warn(
          `Channel ${channel.id} dispatch attempt ${attempt}/${maxAttempts} failed: ${errMsg}`
        );

        if (attempt < maxAttempts) {
          const delayMs = Math.pow(2, attempt - 1) * 5_000; // 5s, 10s, 20s
          setTimeout(
            () => this.sendWithRetry(channel, event, dispatchId, attempt + 1, maxAttempts),
            delayMs
          );
        } else {
          logger.warn(
            `Channel ${channel.id} dispatch permanently failed after ${maxAttempts} attempts`
          );
          pool
            .query(
              `UPDATE notification_dispatches
               SET status = 'failed', last_error = $2, attempts = $3
               WHERE id = $1`,
              [dispatchId, errMsg, attempt]
            )
            .catch(() => {});
        }
      });
  }

  /**
   * Mark all pending dispatches for a request as stale (already resolved via another channel).
   */
  async markStale(requestId: string): Promise<void> {
    await pool
      .query(
        `UPDATE notification_dispatches
         SET status = 'stale'
         WHERE event_id = $1 AND status = 'pending'`,
        [requestId]
      )
      .catch(() => {});
  }
}

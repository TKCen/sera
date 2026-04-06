/**
 * GitHubChannelWiring — sets up default ChannelRouter routing rules for GitHub events.
 * Called once during sera-core startup to ensure default rules exist.
 */

import { v4 as uuidv4 } from 'uuid';
import { pool } from '../lib/database.js';
import { Logger } from '../lib/logger.js';

const logger = new Logger('GitHubChannelWiring');

interface DefaultRule {
  eventType: string;
  minSeverity: 'info' | 'warning' | 'critical';
}

/**
 * Default routing rules for GitHub events.
 * `github:ci_passed` is intentionally omitted — silent unless explicitly configured.
 */
const DEFAULT_GITHUB_RULES: DefaultRule[] = [
  { eventType: 'github:ci_failed', minSeverity: 'warning' },
  { eventType: 'github:pr_merged', minSeverity: 'info' },
  { eventType: 'github:pr_opened', minSeverity: 'info' },
];

/**
 * Ensures default GitHub routing rules exist in the database.
 * Each rule routes to all channels (empty channel_ids array = broadcast).
 * Idempotent: uses ON CONFLICT DO NOTHING keyed on event_type.
 */
export async function ensureGitHubRoutingRules(): Promise<void> {
  try {
    for (const rule of DEFAULT_GITHUB_RULES) {
      const { rowCount } = await pool.query(
        `SELECT 1 FROM notification_routing_rules WHERE event_type = $1 LIMIT 1`,
        [rule.eventType]
      );

      if (rowCount && rowCount > 0) {
        logger.debug(`GitHub routing rule for '${rule.eventType}' already exists — skipping`);
        continue;
      }

      const id = uuidv4();
      await pool.query(
        `INSERT INTO notification_routing_rules
           (id, event_type, channel_ids, filter, min_severity, enabled, priority, target_agent_id)
         VALUES ($1, $2, $3, NULL, $4, true, 0, NULL)
         ON CONFLICT DO NOTHING`,
        [id, rule.eventType, [], rule.minSeverity]
      );

      logger.info(`Created default GitHub routing rule: ${rule.eventType} → min_severity=${rule.minSeverity}`);
    }
  } catch (err) {
    logger.warn('Failed to ensure GitHub routing rules:', err);
  }
}

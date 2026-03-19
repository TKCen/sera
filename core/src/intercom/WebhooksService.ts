import crypto from 'crypto';
import { pool } from '../lib/database.js';
import { Logger } from '../lib/logger.js';
import type { IntercomService } from './IntercomService.js';

const logger = new Logger('Webhooks');

export class WebhooksService {
  constructor(private readonly intercom: IntercomService) {}

  /**
   * Verify signature of an incoming webhook.
   * HMAC-SHA256(secret, timestamp + "." + body)
   */
  verifySignature(secret: string, body: string, signature: string, timestamp: string): boolean {
    const now = Date.now();
    const ts = parseInt(timestamp, 10);

    // Replay protection: within 5 minutes
    if (isNaN(ts) || Math.abs(now - ts) > 5 * 60 * 1000) {
      logger.warn(`Webhook timestamp out of range: ${timestamp}`);
      return false;
    }

    const hmac = crypto.createHmac('sha256', secret);
    const expected = hmac.update(`${timestamp}.${body}`).digest('hex');

    return crypto.timingSafeEqual(Buffer.from(signature), Buffer.from(expected));
  }

  /**
   * Process an incoming webhook trigger.
   */
  async handleIncoming(slug: string, rawBody: string, signature: string, timestamp: string): Promise<void> {
    const res = await pool.query('SELECT * FROM webhooks WHERE url_path = $1 AND enabled = true', [slug]);
    const webhook = res.rows[0];

    if (!webhook) {
      throw new Error(`Webhook not found or disabled: ${slug}`);
    }

    if (!this.verifySignature(webhook.secret, rawBody, signature, timestamp)) {
      throw new Error('Invalid webhook signature');
    }

    const payload = JSON.parse(rawBody);

    // Record delivery (Story 9.8)
    const deliveryRes = await pool.query(
      'INSERT INTO webhook_deliveries (webhook_id, payload, status) VALUES ($1, $2, $3) RETURNING id',
      [webhook.id, payload, 'pending']
    );
    const deliveryId = deliveryRes.rows[0].id;

    // Asynchronous delivery to Intercom
    const publishAndLog = async () => {
      try {
        await this.intercom.publishSystem(webhook.event_type, payload);
        await pool.query(
          'UPDATE webhook_deliveries SET status = $1, processed_at = $2 WHERE id = $3',
          ['success', new Date(), deliveryId]
        );
        logger.info(`Successfully delivered webhook ${slug} to system.${webhook.event_type}`);
      } catch (err: any) {
        await pool.query(
          'UPDATE webhook_deliveries SET status = $1, error_message = $2, processed_at = $3 WHERE id = $4',
          ['failed', err.message, new Date(), deliveryId]
        );
        logger.error(`Failed to deliver webhook ${slug}: ${err.message}`);
      }
    };

    publishAndLog();
  }

  /**
   * Create a new webhook registration.
   */
  async createWebhook(name: string, urlPath: string, secret: string, eventType: string): Promise<any> {
    const res = await pool.query(
      'INSERT INTO webhooks (name, url_path, secret, event_type) VALUES ($1, $2, $3, $4) RETURNING *',
      [name, urlPath, secret, eventType]
    );
    return res.rows[0];
  }

  /**
   * List all webhooks.
   */
  async listWebhooks(): Promise<any[]> {
    const res = await pool.query('SELECT id, name, url_path, event_type, enabled, created_at FROM webhooks ORDER BY created_at DESC');
    return res.rows;
  }
}

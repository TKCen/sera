import crypto from 'crypto';
import { pool } from '../lib/database.js';
import { Logger } from '../lib/logger.js';
import type { IntercomService } from './IntercomService.js';

const logger = new Logger('Webhooks');

export class WebhooksService {
  /** Nonce cache for replay protection: nonce -> expiry timestamp */
  private nonces = new Map<string, number>();
  private nonceCleanupInterval: NodeJS.Timeout;

  constructor(private readonly intercom: IntercomService) {
    // Prune nonces every minute
    this.nonceCleanupInterval = setInterval(() => {
      const now = Date.now();
      for (const [nonce, expiry] of this.nonces.entries()) {
        if (now > expiry) {
          this.nonces.delete(nonce);
        }
      }
    }, 60 * 1000);
  }

  /**
   * Verify signature of an incoming webhook.
   * HMAC-SHA256(secret, timestamp + "." + body)
   * Story 9.8: Timing-safe, timestamp range, and nonce check.
   */
  verifySignature(
    secret: string, 
    body: string, 
    signature: string, 
    timestamp: string,
    nonce?: string
  ): boolean {
    const now = Date.now();
    const ts = parseInt(timestamp, 10);

    // 1. Timestamp validation (reject if absent or > 5 minutes old)
    if (isNaN(ts) || Math.abs(now - ts) > 5 * 60 * 1000) {
      logger.warn(`Webhook timestamp out of range: ${timestamp}`);
      return false;
    }

    // 2. Replay protection (nonce)
    if (nonce) {
      if (this.nonces.has(nonce)) {
        logger.warn(`Duplicate webhook nonce detected: ${nonce}`);
        return false;
      }
      // Store nonce with 5-minute expiry
      this.nonces.set(nonce, now + 5 * 60 * 1000);
    }

    // 3. HMAC Validation
    const hmac = crypto.createHmac('sha256', secret);
    const expected = hmac.update(`${timestamp}.${body}`).digest('hex');

    // Use timing-safe comparison
    try {
      return crypto.timingSafeEqual(
        Buffer.from(signature, 'hex'), 
        Buffer.from(expected, 'hex')
      );
    } catch {
      return false;
    }
  }

  /**
   * Process an incoming webhook trigger.
   */
  async handleIncoming(
    slug: string, 
    rawBody: string, 
    signature: string, 
    timestamp: string,
    nonce?: string
  ): Promise<void> {
    const res = await pool.query('SELECT * FROM webhooks WHERE url_path = $1 AND enabled = true', [slug]);
    const webhook = res.rows[0];

    if (!webhook) {
      throw new Error(`Webhook not found or disabled: ${slug}`);
    }

    if (!this.verifySignature(webhook.secret, rawBody, signature, timestamp, nonce)) {
      throw new Error('Invalid webhook signature');
    }

    // Story 9.8: Treat as untrusted. Wrap in delimiters.
    const wrappedPayload = `<webhook_payload source="${webhook.id}">\n${rawBody}\n</webhook_payload>`;
    const payload = JSON.parse(rawBody);

    // Record delivery
    const deliveryRes = await pool.query(
      'INSERT INTO webhook_deliveries (webhook_id, payload, status) VALUES ($1, $2, $3) RETURNING id',
      [webhook.id, payload, 'pending']
    );
    const deliveryId = deliveryRes.rows[0].id;

    // Asynchronous delivery to Intercom
    const publishAndLog = async () => {
      try {
        // Publish the wrapped payload as content
        await this.intercom.publishSystemEvent(webhook.event_type, { 
          raw: rawBody,
          wrapped: wrappedPayload,
          data: payload 
        });

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

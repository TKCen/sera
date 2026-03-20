import crypto from 'node:crypto';
import axios from 'axios';
import { Logger } from '../../lib/logger.js';
import type { Channel, ChannelEvent, ChannelHealth } from '../channel.interface.js';
import { ActionTokenService } from '../ActionTokenService.js';

const logger = new Logger('WebhookChannel');

interface WebhookConfig {
  url: string;
  secret?: string | undefined;
  timeout: number;
}

export class WebhookChannel implements Channel {
  readonly type = 'webhook';
  private cfg: WebhookConfig;

  constructor(
    readonly id: string,
    readonly name: string,
    config: Record<string, unknown>,
  ) {
    const secret = config['secret'];
    this.cfg = {
      url: config['url'] as string,
      ...(typeof secret === 'string' ? { secret } : {}),
      timeout: typeof config['timeout'] === 'number' ? config['timeout'] : 10_000,
    };
  }

  async send(event: ChannelEvent): Promise<void> {
    const instanceId = process.env['SERA_INSTANCE_ID'] ?? 'sera';
    const payload: Record<string, unknown> = {
      event,
      timestamp: new Date().toISOString(),
      instanceId,
    };

    if (event.actions) {
      const svc = ActionTokenService.getInstance();
      const { approveUrl, denyUrl } = svc.buildActionUrls(
        event.actions.approveToken,
        event.actions.denyToken,
      );
      payload['approveUrl'] = approveUrl;
      payload['denyUrl'] = denyUrl;
    }

    const bodyStr = JSON.stringify(payload);

    const headers: Record<string, string> = { 'Content-Type': 'application/json' };
    if (this.cfg.secret) {
      const sig = crypto
        .createHmac('sha256', this.cfg.secret)
        .update(bodyStr)
        .digest('hex');
      headers['X-Sera-Signature'] = `sha256=${sig}`;
    }

    await axios.post(this.cfg.url, payload, {
      headers,
      ...(this.cfg.timeout !== undefined ? { timeout: this.cfg.timeout } : {}),
      validateStatus: (status) => status < 500,
    });

    logger.info(`Webhook sent: ${event.eventType} → ${this.cfg.url}`);
  }

  async healthCheck(): Promise<ChannelHealth> {
    const start = Date.now();
    try {
      await axios.get(this.cfg.url, { timeout: 5_000, validateStatus: () => true });
      return { healthy: true, latencyMs: Date.now() - start };
    } catch (err: unknown) {
      return { healthy: false, error: err instanceof Error ? err.message : String(err) };
    }
  }
}

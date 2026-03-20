import axios from 'axios';
import { WebClient } from '@slack/web-api';
import { Logger } from '../../lib/logger.js';
import type { Channel, ChannelEvent, ChannelHealth, ReplyHandler } from '../channel.interface.js';
import { ActionTokenService } from '../ActionTokenService.js';

const logger = new Logger('SlackChannel');

interface SlackConfig {
  webhookUrl: string;
  appToken?: string | undefined;
  signingSecret?: string | undefined;
  botToken?: string | undefined;
}

export class SlackChannel implements Channel {
  readonly type = 'slack';
  private cfg: SlackConfig;
  private webClient: WebClient | null = null;
  private replyHandler: ReplyHandler | null = null;

  constructor(
    readonly id: string,
    readonly name: string,
    config: Record<string, unknown>,
  ) {
    const appToken = config['appToken'];
    const signingSecret = config['signingSecret'];
    const botToken = config['botToken'];
    this.cfg = {
      webhookUrl: config['webhookUrl'] as string,
      ...(typeof appToken === 'string' ? { appToken } : {}),
      ...(typeof signingSecret === 'string' ? { signingSecret } : {}),
      ...(typeof botToken === 'string' ? { botToken } : {}),
    };

    if (this.cfg.botToken) {
      this.webClient = new WebClient(this.cfg.botToken);
    }
  }

  onReply(handler: ReplyHandler): void {
    this.replyHandler = handler;
  }

  /**
   * Handle inbound Slack interactive payload (called from the webhook route).
   * Validates the Slack signature before calling.
   */
  async handleInteractivePayload(payload: {
    type: string;
    actions?: Array<{ action_id: string; value: string }>;
    user?: { id: string };
  }): Promise<void> {
    if (!this.replyHandler) return;

    const action = payload.actions?.[0];
    if (!action) return;

    const match = action.action_id.match(/^sera-(approve|deny):(.+)$/);
    if (!match) return;

    const decision = match[1] as 'approve' | 'deny';
    const requestId = match[2]!;
    const userId = payload.user?.id ?? 'unknown';

    await this.replyHandler(requestId, decision, userId);
  }

  async send(event: ChannelEvent): Promise<void> {
    const severityEmoji: Record<string, string> = {
      info: 'ℹ️',
      warning: '⚠️',
      critical: '🚨',
    };
    const emoji = severityEmoji[event.severity] ?? '📢';

    const blocks: unknown[] = [
      {
        type: 'header',
        text: { type: 'plain_text', text: `${emoji} ${event.title}`, emoji: true },
      },
      {
        type: 'section',
        text: { type: 'mrkdwn', text: event.body },
      },
      {
        type: 'context',
        elements: [
          {
            type: 'mrkdwn',
            text: `*SERA* · ${event.eventType} · ${event.timestamp}`,
          },
        ],
      },
    ];

    if (event.actions) {
      const svc = ActionTokenService.getInstance();
      const { approveUrl, denyUrl } = svc.buildActionUrls(
        event.actions.approveToken,
        event.actions.denyToken,
      );

      blocks.push({
        type: 'actions',
        elements: [
          {
            type: 'button',
            text: { type: 'plain_text', text: '✅ Approve', emoji: true },
            style: 'primary',
            url: approveUrl,
            action_id: `sera-approve:${event.actions.requestId}`,
          },
          {
            type: 'button',
            text: { type: 'plain_text', text: '❌ Deny', emoji: true },
            style: 'danger',
            url: denyUrl,
            action_id: `sera-deny:${event.actions.requestId}`,
          },
        ],
      });
    }

    await axios.post(this.cfg.webhookUrl, { blocks });
    logger.info(`Slack message sent: ${event.eventType}`);
  }

  async healthCheck(): Promise<ChannelHealth> {
    const start = Date.now();
    try {
      if (this.webClient) {
        await this.webClient.auth.test();
        return { healthy: true, latencyMs: Date.now() - start };
      }
      await axios.post(this.cfg.webhookUrl, { text: 'ping' }, { timeout: 5_000, validateStatus: () => true });
      return { healthy: true, latencyMs: Date.now() - start };
    } catch (err: unknown) {
      return { healthy: false, error: err instanceof Error ? err.message : String(err) };
    }
  }
}

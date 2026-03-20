import axios from 'axios';
import { Client, Events, GatewayIntentBits } from 'discord.js';
import { Logger } from '../../lib/logger.js';
import type { Channel, ChannelEvent, ChannelHealth, ReplyHandler } from '../channel.interface.js';
import { ActionTokenService } from '../ActionTokenService.js';

const logger = new Logger('DiscordChannel');

const SEVERITY_COLOR: Record<string, number> = {
  info: 0x3b82f6, // blue
  warning: 0xf59e0b, // amber
  critical: 0xef4444, // red
};

interface DiscordConfig {
  webhookUrl: string;
  botToken?: string | undefined;
  approvalChannelId?: string | undefined;
}

export class DiscordChannel implements Channel {
  readonly type = 'discord';
  private cfg: DiscordConfig;
  private client: Client | null = null;
  private replyHandler: ReplyHandler | null = null;

  constructor(
    readonly id: string,
    readonly name: string,
    config: Record<string, unknown>
  ) {
    const botToken = config['botToken'];
    const approvalChannelId = config['approvalChannelId'];
    this.cfg = {
      webhookUrl: config['webhookUrl'] as string,
      ...(typeof botToken === 'string' ? { botToken } : {}),
      ...(typeof approvalChannelId === 'string' ? { approvalChannelId } : {}),
    };

    if (this.cfg.botToken) {
      this.initBot();
    }
  }

  private initBot(): void {
    const bot = new Client({
      intents: [
        GatewayIntentBits.Guilds,
        GatewayIntentBits.GuildMessages,
        GatewayIntentBits.MessageContent,
        GatewayIntentBits.DirectMessages,
      ],
    });

    bot.on(Events.ClientReady, () => {
      logger.info(`Discord bot ready: ${bot.user?.tag}`);
    });

    bot.on(Events.MessageCreate, async (message) => {
      if (message.author.bot) return;
      if (!this.replyHandler) return;

      const text = message.content.trim();
      const approveMatch = text.match(/^\/sera\s+approve\s+(\S+)/i);
      const denyMatch = text.match(/^\/sera\s+deny\s+(\S+)/i);

      if (!approveMatch && !denyMatch) return;

      const requestId = (approveMatch ?? denyMatch)![1]!;
      const decision: 'approve' | 'deny' = approveMatch ? 'approve' : 'deny';
      const userId = message.author.id;

      try {
        await this.replyHandler(requestId, decision, userId);
        await message.reply(`✅ Decision recorded: ${decision}`);
      } catch (err: unknown) {
        logger.warn('Failed to process Discord reply:', err);
        await message
          .reply('❌ Failed to process decision. The request may have expired.')
          .catch(() => {});
      }
    });

    bot.login(this.cfg.botToken).catch((err: unknown) => {
      logger.warn('Discord bot login failed:', err);
    });

    this.client = bot;
  }

  onReply(handler: ReplyHandler): void {
    this.replyHandler = handler;
  }

  async send(event: ChannelEvent): Promise<void> {
    const color = SEVERITY_COLOR[event.severity] ?? 0x6b7280;

    const embed: Record<string, unknown> = {
      title: event.title,
      description: event.body,
      color,
      timestamp: event.timestamp,
      footer: { text: `SERA · ${event.eventType}` },
    };

    const components: unknown[] = [];

    if (event.actions) {
      const svc = ActionTokenService.getInstance();
      const { approveUrl, denyUrl } = svc.buildActionUrls(
        event.actions.approveToken,
        event.actions.denyToken
      );

      components.push({
        type: 1, // ActionRow
        components: [
          {
            type: 2, // Button
            style: 5, // Link
            label: '✅ Approve',
            url: approveUrl,
          },
          {
            type: 2,
            style: 5,
            label: '❌ Deny',
            url: denyUrl,
          },
        ],
      });
    }

    await axios.post(this.cfg.webhookUrl, {
      embeds: [embed],
      ...(components.length > 0 ? { components } : {}),
    });

    logger.info(`Discord embed sent: ${event.eventType}`);
  }

  async healthCheck(): Promise<ChannelHealth> {
    const start = Date.now();
    try {
      const parts = this.cfg.webhookUrl.split('/');
      const id = parts[parts.length - 2];
      const token = parts[parts.length - 1];
      await axios.get(`https://discord.com/api/webhooks/${id}/${token}`, { timeout: 5_000 });
      return { healthy: true, latencyMs: Date.now() - start };
    } catch (err: unknown) {
      return { healthy: false, error: err instanceof Error ? err.message : String(err) };
    }
  }

  async destroy(): Promise<void> {
    await this.client?.destroy();
    this.client = null;
  }
}

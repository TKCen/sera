import WebSocket from 'ws';
import axios from 'axios';
import { ChannelAdapter, type IncomingMessage } from '../ChannelAdapter.js';
import type { Orchestrator } from '../../agents/Orchestrator.js';
import type { SessionStore } from '../../sessions/SessionStore.js';
import type { ChatMessage } from '../../agents/types.js';

export class DiscordAdapter extends ChannelAdapter {
  private ws: WebSocket | null = null;
  private heartbeatInterval: NodeJS.Timeout | null = null;
  private lastSequence: number | null = null;
  private sessionId: string | null = null;
  private running: boolean = false;

  constructor(
    private token: string,
    private orchestrator: Orchestrator,
    private sessionStore: SessionStore,
    options?: { rateLimitWindow?: number; maxMessagesPerWindow?: number }
  ) {
    super('Discord', options);
  }

  async start(): Promise<void> {
    this.running = true;
    this.connect();
  }

  async stop(): Promise<void> {
    this.running = false;
    if (this.heartbeatInterval) clearInterval(this.heartbeatInterval);
    if (this.ws) this.ws.close();
    await this.shutdownBase();
    this.logger.info('Discord adapter stopped');
  }

  private connect() {
    this.logger.info('Connecting to Discord Gateway...');
    this.ws = new WebSocket('wss://gateway.discord.gg/?v=10&encoding=json');

    this.ws.on('open', () => {
      this.logger.info('Discord Gateway connection opened');
    });

    this.ws.on('message', (data: any) => {
      try {
        const payload = JSON.parse(data.toString());
        this.handlePayload(payload);
      } catch (err: any) {
        this.logger.error('Failed to parse Discord payload:', err.message);
      }
    });

    this.ws.on('close', () => {
      this.logger.warn('Discord Gateway connection closed');
      if (this.running) {
        setTimeout(() => this.connect(), 5000);
      }
    });

    this.ws.on('error', (err: any) => {
      this.logger.error('Discord Gateway error:', err.message);
    });
  }

  private handlePayload(payload: any) {
    const { op, d, s, t } = payload;
    if (s !== null) this.lastSequence = s;

    switch (op) {
      case 10: // Hello
        const { heartbeat_interval } = d;
        this.startHeartbeat(heartbeat_interval);
        this.identify();
        break;
      case 11: // Heartbeat ACK
        // Heartbeat acknowledged
        break;
      case 0: // Dispatch
        if (t === 'MESSAGE_CREATE') {
          this.handleMessage(d);
        } else if (t === 'READY') {
          this.sessionId = d.session_id;
          this.logger.info(`Discord adapter ready as ${d.user.username}#${d.user.discriminator}`);
        }
        break;
    }
  }

  private startHeartbeat(interval: number) {
    if (this.heartbeatInterval) clearInterval(this.heartbeatInterval);
    this.heartbeatInterval = setInterval(() => {
      this.sendPayload(1, this.lastSequence);
    }, interval);
  }

  private identify() {
    // Intents:
    // GUILDS (1 << 0) = 1
    // GUILD_MESSAGES (1 << 9) = 512
    // DIRECT_MESSAGES (1 << 12) = 4096
    // MESSAGE_CONTENT (1 << 15) = 32768
    // Total = 1 + 512 + 4096 + 32768 = 37377
    this.sendPayload(2, {
      token: this.token,
      intents: 37377,
      properties: {
        os: 'linux',
        browser: 'sera',
        device: 'sera'
      }
    });
  }

  private sendPayload(op: number, d: any) {
    if (this.ws && this.ws.readyState === WebSocket.OPEN) {
      this.ws.send(JSON.stringify({ op, d }));
    }
  }

  private async handleMessage(message: any) {
    // Ignore bot messages
    if (message.author.bot) return;

    const incoming: IncomingMessage = {
      platform: 'Discord',
      userId: message.author.id,
      userName: message.author.username,
      chatId: message.channel_id,
      text: message.content || '',
    };

    if (this.isRateLimited(incoming.userId)) {
      this.logger.warn(`Rate limit exceeded for user ${incoming.userId}`);
      await this.sendMessage(incoming.chatId, '⚠️ You are sending messages too fast. Please slow down.');
      return;
    }

    // Only respond to DMs or if mentioned (basic logic)
    // For this implementation, we respond to everything the bot can see

    try {
      const agent = this.orchestrator.getPrimaryAgent();
      if (!agent) {
        await this.sendMessage(incoming.chatId, 'Sorry, no agent is currently available.');
        return;
      }

      const response = await agent.process(incoming.text, []);
      const reply = response.finalAnswer || response.thought || 'No response generated.';

      await this.sendMessage(incoming.chatId, reply);
    } catch (err: any) {
      this.logger.error('Error processing Discord message:', err.message);
    }
  }

  async sendMessage(chatId: string, text: string): Promise<void> {
    try {
      await axios.post(`https://discord.com/api/v10/channels/${chatId}/messages`, {
        content: text,
      }, {
        headers: {
          Authorization: `Bot ${this.token}`,
        }
      });
    } catch (err: any) {
      this.logger.error(`Failed to send Discord message to ${chatId}:`, err.message);
    }
  }
}

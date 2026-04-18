import axios from 'axios';
import { ChannelAdapter, type IncomingMessage } from '../ChannelAdapter.js';
import type { Orchestrator } from '../../agents/Orchestrator.js';
import type { SessionStore } from '../../sessions/SessionStore.js';

export class WhatsAppAdapter extends ChannelAdapter {
  constructor(
    private token: string,
    private phoneNumberId: string,
    private orchestrator: Orchestrator,
    private sessionStore: SessionStore,
    options?: { rateLimitWindow?: number; maxMessagesPerWindow?: number }
  ) {
    super('WhatsApp', options);
  }

  async start(): Promise<void> {
    this.logger.info('WhatsApp adapter started (Webhook mode).');
  }

  async stop(): Promise<void> {
    await this.shutdownBase();
    this.logger.info('WhatsApp adapter stopped.');
  }

  /**
   * Handle incoming message from WhatsApp Webhook.
   */
  async handleWebhookPayload(payload: unknown) {
    const data = payload as Record<string, unknown>;
    const entry = (data['entry'] as unknown[])?.[0] as Record<string, unknown>;
    const change = (entry?.['changes'] as unknown[])?.[0] as Record<string, unknown>;
    const value = change?.['value'] as Record<string, unknown>;
    const message = (value?.['messages'] as unknown[])?.[0] as Record<string, unknown>;

    if (!message || message['type'] !== 'text') return;

    const from = message['from'] as string;
    const profileName = ((value?.['contacts'] as unknown[])?.[0] as Record<string, unknown>)?.[
      'profile'
    ] as Record<string, string>;
    const textBody = (message['text'] as Record<string, string>)?.['body'] || '';

    const incoming: IncomingMessage = {
      platform: 'WhatsApp',
      userId: from,
      userName: profileName?.['name'] || from,
      chatId: from,
      text: textBody,
    };

    if (this.isRateLimited(incoming.userId)) {
      this.logger.warn(`Rate limit exceeded for user ${incoming.userId}`);
      await this.sendMessage(
        incoming.chatId,
        '⚠️ You are sending messages too fast. Please slow down.'
      );
      return;
    }

    // Enqueue for sequential processing per user
    const queueKey = `${incoming.chatId}:${incoming.userId}`;
    this.enqueueMessage(queueKey, async () => {
      const agent = this.orchestrator.getPrimaryAgent();
      if (!agent) {
        await this.sendMessage(incoming.chatId, 'Sorry, no agent is currently available.');
        return;
      }

      const response = await agent.process(incoming.text, []);
      const reply = response.finalAnswer || response.thought || 'No response generated.';

      await this.sendMessage(incoming.chatId, reply);
    });
  }

  async sendMessage(chatId: string, text: string): Promise<void> {
    try {
      await axios.post(
        `https://graph.facebook.com/v17.0/${this.phoneNumberId}/messages`,
        {
          messaging_product: 'whatsapp',
          to: chatId,
          text: { body: text },
        },
        {
          headers: {
            Authorization: `Bearer ${this.token}`,
            'Content-Type': 'application/json',
          },
        }
      );
    } catch (err: unknown) {
      this.logger.error(`Failed to send WhatsApp message to ${chatId}:`, (err as Error).message);
    }
  }
}

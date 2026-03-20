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
    // In a real scenario, this would register a webhook or listen for incoming POST requests.
    // For this task, we provide the skeleton implementation.
  }

  async stop(): Promise<void> {
    await this.shutdownBase();
    this.logger.info('WhatsApp adapter stopped.');
  }

  /**
   * Handle incoming message from WhatsApp Webhook.
   */
  async handleWebhookPayload(payload: any) {
    const entry = payload.entry?.[0];
    const change = entry?.changes?.[0];
    const value = change?.value;
    const message = value?.messages?.[0];

    if (!message || message.type !== 'text') return;

    const incoming: IncomingMessage = {
      platform: 'WhatsApp',
      userId: message.from,
      userName: value.contacts?.[0]?.profile?.name || message.from,
      chatId: message.from,
      text: message.text.body,
    };

    if (this.isRateLimited(incoming.userId)) {
      this.logger.warn(`Rate limit exceeded for user ${incoming.userId}`);
      await this.sendMessage(
        incoming.chatId,
        '⚠️ You are sending messages too fast. Please slow down.'
      );
      return;
    }

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
      this.logger.error('Error processing WhatsApp message:', err.message);
    }
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
    } catch (err: any) {
      this.logger.error(`Failed to send WhatsApp message to ${chatId}:`, err.message);
    }
  }
}

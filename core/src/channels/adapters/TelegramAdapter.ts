import axios from 'axios';
import { v5 as uuidv5 } from 'uuid';
import { ChannelAdapter, type IncomingMessage } from '../ChannelAdapter.js';
import type { Orchestrator } from '../../agents/index.js';
import type { SessionStore } from '../../sessions/index.js';
import type { ChatMessage } from '../../agents/index.js';

// Namespace for SERA platform session UUIDs (generated once)
const SERA_SESSION_NAMESPACE = '6ba7b810-9dad-11d1-80b4-00c04fd430c8';

export class TelegramAdapter extends ChannelAdapter {
  private offset: number = 0;
  private running: boolean = false;
  private apiUrl: string;

  constructor(
    private token: string,
    private orchestrator: Orchestrator,
    private sessionStore: SessionStore,
    options?: { rateLimitWindow?: number; maxMessagesPerWindow?: number }
  ) {
    super('Telegram', options);
    this.apiUrl = `https://api.telegram.org/bot${token}`;
  }

  async start(): Promise<void> {
    this.running = true;
    this.logger.info('Telegram adapter starting...');
    this.poll();
  }

  async stop(): Promise<void> {
    this.running = false;
    await this.shutdownBase();
    this.logger.info('Telegram adapter stopping...');
  }

  private async poll() {
    while (this.running) {
      try {
        const response = await axios.get(`${this.apiUrl}/getUpdates`, {
          params: {
            offset: this.offset,
            timeout: 30,
          },
          timeout: 35000,
        });

        const updates = response.data.result;
        for (const update of updates) {
          this.offset = update.update_id + 1;
          if (update.message && update.message.text) {
            await this.handleMessage(update.message);
          }
        }
      } catch (err: unknown) {
        const error = err as { code?: string; message?: string };
        if (error.code === 'ECONNABORTED') {
          // Timeout is expected
        } else {
          this.logger.error('Error polling Telegram updates:', error.message);
          await new Promise((resolve) => setTimeout(resolve, 5000));
        }
      }
    }
  }

  private async handleMessage(message: Record<string, unknown>) {
    const from = (message.from as Record<string, unknown>) || {};
    const chat = (message.chat as Record<string, unknown>) || {};

    const incoming: IncomingMessage = {
      platform: 'Telegram',
      userId: String(from.id || 'unknown'),
      userName: (from.username as string) || (from.first_name as string) || 'unknown',
      chatId: String(chat.id || 'unknown'),
      text: (message.text as string) || '',
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

      // Use a deterministic UUID for the session based on the Telegram chatId
      // This ensures we always map back to the same SERA session for this chat.
      const sessionId = uuidv5(`telegram:${incoming.chatId}`, SERA_SESSION_NAMESPACE);

      let history: ChatMessage[] = [];
      const session = await this.sessionStore.getSession(sessionId);

      if (session) {
        const msgs = await this.sessionStore.getMessages(sessionId);
        history = msgs.map((m) => ({
          role: m.role as ChatMessage['role'],
          content: m.content,
        }));
      } else {
        // Create the session in the DB with the deterministic ID
        await this.sessionStore.createSession({
          id: sessionId,
          agentName: agent.role,
          title: `Telegram Chat with ${incoming.userName}`,
        });
      }

      const response = await agent.process(incoming.text, history);
      const reply = response.finalAnswer || response.thought || 'No response generated.';

      await this.sendMessage(incoming.chatId, reply);

      // Persist messages to the session
      await this.sessionStore.addMessage({
        sessionId: sessionId,
        role: 'user',
        content: incoming.text,
      });
      await this.sessionStore.addMessage({
        sessionId: sessionId,
        role: 'assistant',
        content: reply,
      });
    } catch (err: unknown) {
      this.logger.error('Error processing Telegram message:', (err as Error).message);
      await this.sendMessage(
        incoming.chatId,
        'Sorry, I encountered an error while processing your message.'
      );
    }
  }

  async sendMessage(chatId: string, text: string): Promise<void> {
    try {
      await axios.post(`${this.apiUrl}/sendMessage`, {
        chat_id: chatId,
        text: text,
      });
    } catch (err: unknown) {
      this.logger.error(`Failed to send Telegram message to ${chatId}:`, (err as Error).message);
    }
  }
}

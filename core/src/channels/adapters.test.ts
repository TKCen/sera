import { describe, it, expect, vi, beforeEach } from 'vitest';
import { TelegramAdapter } from './adapters/TelegramAdapter.js';
import { DiscordAdapter } from './adapters/DiscordAdapter.js';
import { WhatsAppAdapter } from './adapters/WhatsAppAdapter.js';
import axios from 'axios';
import { v5 as uuidv5 } from 'uuid';

vi.mock('axios');
vi.mock('ws');

const SERA_SESSION_NAMESPACE = '6ba7b810-9dad-11d1-80b4-00c04fd430c8';

describe('Channel Adapters', () => {
  let mockOrchestrator: any;
  let mockSessionStore: any;
  let mockAgent: any;

  beforeEach(() => {
    mockAgent = {
      role: 'test-agent',
      process: vi.fn().mockResolvedValue({ finalAnswer: 'Hello from agent' }),
    };
    mockOrchestrator = {
      getPrimaryAgent: vi.fn().mockReturnValue(mockAgent),
    };
    mockSessionStore = {
      getSession: vi.fn().mockResolvedValue({ id: 'existing-session' }),
      createSession: vi.fn().mockResolvedValue({ id: 'test-session' }),
      getMessages: vi.fn().mockResolvedValue([]),
      addMessage: vi.fn().mockResolvedValue({}),
    };
    vi.clearAllMocks();
  });

  describe('TelegramAdapter', () => {
    it('should route messages to the primary agent and use deterministic session ID', async () => {
      const adapter = new TelegramAdapter('fake-token', mockOrchestrator, mockSessionStore);

      const chatId = '456';
      const expectedSessionId = uuidv5(`telegram:${chatId}`, SERA_SESSION_NAMESPACE);

      // Mock axios response for sendMessage
      (axios.post as any).mockResolvedValueOnce({ data: { ok: true } });

      // @ts-ignore
      await adapter.handleMessage({
        from: { id: 123, username: 'testuser' },
        chat: { id: parseInt(chatId) },
        text: 'hi',
      });

      expect(mockOrchestrator.getPrimaryAgent).toHaveBeenCalled();
      expect(mockSessionStore.getSession).toHaveBeenCalledWith(expectedSessionId);
      expect(mockAgent.process).toHaveBeenCalledWith('hi', []);
      expect(axios.post).toHaveBeenCalledWith(
        expect.stringContaining('sendMessage'),
        expect.objectContaining({
          chat_id: chatId,
          text: 'Hello from agent',
        })
      );
      expect(mockSessionStore.addMessage).toHaveBeenCalled();
    });

    it('should enforce rate limits and not call agent', async () => {
      const adapter = new TelegramAdapter('fake-token', mockOrchestrator, mockSessionStore, {
        maxMessagesPerWindow: 2,
      });

      // Mock sendMessage
      (axios.post as any).mockResolvedValue({ data: { ok: true } });

      // Send 3 messages
      for (let i = 0; i < 3; i++) {
        // @ts-ignore
        await adapter.handleMessage({
          from: { id: 123, username: 'testuser' },
          chat: { id: 456 },
          text: 'hi',
        });
      }

      expect(mockAgent.process).toHaveBeenCalledTimes(2);
      expect(axios.post).toHaveBeenCalledWith(
        expect.stringContaining('sendMessage'),
        expect.objectContaining({
          text: expect.stringContaining('too fast'),
        })
      );
    });
  });

  describe('DiscordAdapter', () => {
    it('should handle incoming messages', async () => {
      const adapter = new DiscordAdapter('fake-token', mockOrchestrator, mockSessionStore);

      (axios.post as any).mockResolvedValueOnce({ data: { id: 'msg-id' } });

      // @ts-ignore
      await adapter.handleMessage({
        author: { id: 'user-1', username: 'testuser', bot: false },
        channel_id: 'chan-1',
        content: 'hello discord',
      });

      expect(mockAgent.process).toHaveBeenCalledWith('hello discord', []);
      expect(axios.post).toHaveBeenCalledWith(
        expect.stringContaining('channels/chan-1/messages'),
        expect.objectContaining({
          content: 'Hello from agent',
        }),
        expect.any(Object)
      );
    });
  });

  describe('WhatsAppAdapter', () => {
    it('should handle webhook payloads', async () => {
      const adapter = new WhatsAppAdapter(
        'fake-token',
        'phone-id',
        mockOrchestrator,
        mockSessionStore
      );

      (axios.post as any).mockResolvedValueOnce({ data: { messaging_product: 'whatsapp' } });

      const payload = {
        entry: [
          {
            changes: [
              {
                value: {
                  contacts: [{ profile: { name: 'testuser' } }],
                  messages: [
                    {
                      from: 'whatsapp-id',
                      type: 'text',
                      text: { body: 'hello whatsapp' },
                    },
                  ],
                },
              },
            ],
          },
        ],
      };

      await adapter.handleWebhookPayload(payload);

      expect(mockAgent.process).toHaveBeenCalledWith('hello whatsapp', []);
      expect(axios.post).toHaveBeenCalledWith(
        expect.stringContaining('phone-id/messages'),
        expect.objectContaining({
          to: 'whatsapp-id',
          text: { body: 'Hello from agent' },
        }),
        expect.any(Object)
      );
    });
  });
});

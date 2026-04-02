import { describe, it, expect, vi, beforeEach, afterEach } from 'vitest';
import { Centrifuge } from 'centrifuge';
import {
  getClient,
  disconnectClient,
  subscribeToThoughts,
  subscribeToTerminal,
  subscribeToStream,
} from './centrifugo';

// Mock centrifuge-js
vi.mock('centrifuge', () => {
  class SubscriptionMock {
    on = vi.fn().mockReturnThis();
    subscribe = vi.fn();
    unsubscribe = vi.fn();
    removeAllListeners = vi.fn();
  }

  const CentrifugeMock = vi.fn().mockImplementation(function (this: any) {
    return {
      connect: vi.fn(),
      disconnect: vi.fn(),
      newSubscription: vi.fn().mockImplementation(() => new SubscriptionMock()),
      getSubscription: vi.fn().mockReturnValue(null),
      removeSubscription: vi.fn(),
    };
  });

  return {
    Centrifuge: CentrifugeMock,
  };
});

describe('centrifugo utility', () => {
  beforeEach(() => {
    vi.clearAllMocks();
    vi.stubGlobal('window', {
      location: {
        protocol: 'http:',
        hostname: 'localhost',
      },
    });
    // Reset singleton by calling disconnect
    disconnectClient();
  });

  afterEach(() => {
    vi.unstubAllGlobals();
    vi.unstubAllEnvs();
  });

  describe('getClient', () => {
    it('creates a singleton Centrifuge instance', () => {
      const client1 = getClient();
      const client2 = getClient();

      expect(Centrifuge).toHaveBeenCalledTimes(1);
      expect(client1).toBe(client2);
    });

    it('calls connect on the new client', () => {
      const client = getClient();
      expect(client.connect).toHaveBeenCalled();
    });

    it('uses SSR fallback URL when window is undefined', () => {
      vi.stubGlobal('window', undefined);
      getClient();
      expect(Centrifuge).toHaveBeenCalledWith('ws://localhost:10001/connection/websocket', expect.any(Object));
    });

    it('uses NEXT_PUBLIC_CENTRIFUGO_URL if set', () => {
      vi.stubEnv('NEXT_PUBLIC_CENTRIFUGO_URL', 'wss://custom-centrifugo.com/connection/websocket');
      getClient();
      expect(Centrifuge).toHaveBeenCalledWith('wss://custom-centrifugo.com/connection/websocket', expect.any(Object));
    });

    it('constructs URL from window.location (http)', () => {
      vi.stubGlobal('window', {
        location: {
          protocol: 'http:',
          hostname: 'app.example.com',
        },
      });
      getClient();
      expect(Centrifuge).toHaveBeenCalledWith('ws://app.example.com:10001/connection/websocket', expect.any(Object));
    });

    it('constructs URL from window.location (https)', () => {
      vi.stubGlobal('window', {
        location: {
          protocol: 'https:',
          hostname: 'app.example.com',
        },
      });
      getClient();
      expect(Centrifuge).toHaveBeenCalledWith('wss://app.example.com:10001/connection/websocket', expect.any(Object));
    });
  });

  describe('disconnectClient', () => {
    it('calls disconnect and clears the singleton', () => {
      const client = getClient();
      disconnectClient();
      expect(client.disconnect).toHaveBeenCalled();

      // Next call should create a new client
      getClient();
      expect(Centrifuge).toHaveBeenCalledTimes(2);
    });

    it('does nothing if no client exists', () => {
      expect(() => disconnectClient()).not.toThrow();
    });
  });

  describe('subscription helpers', () => {
    it('subscribeToThoughts uses correct channel and listeners', () => {
      const agentId = 'agent-123';
      const onThought = vi.fn();
      const unsubscribe = subscribeToThoughts(agentId, onThought);

      const client = getClient();
      expect(client.newSubscription).toHaveBeenCalledWith(`internal:agent:${agentId}:thoughts`);

      const sub = vi.mocked(client.newSubscription).mock.results[0].value;
      expect(sub.on).toHaveBeenCalledWith('publication', expect.any(Function));
      expect(sub.subscribe).toHaveBeenCalled();

      // Simulate publication
      const publicationCallback = vi.mocked(sub.on).mock.calls.find(call => call[0] === 'publication')![1];
      publicationCallback({ data: { text: 'hello' } });
      expect(onThought).toHaveBeenCalledWith({ text: 'hello' });

      // Test unsubscribe cleanup
      unsubscribe();
      expect(sub.unsubscribe).toHaveBeenCalled();
      expect(sub.removeAllListeners).toHaveBeenCalled();
      expect(client.removeSubscription).toHaveBeenCalledWith(sub);
    });

    it('subscribeToTerminal uses correct channel', () => {
      const agentId = 'agent-456';
      const onOutput = vi.fn();
      subscribeToTerminal(agentId, onOutput);

      const client = getClient();
      expect(client.newSubscription).toHaveBeenCalledWith(`internal:agent:${agentId}:terminal`);
    });

    it('subscribeToStream handles tokens and auto-cleanup on done', () => {
      const messageId = 'msg-789';
      const onToken = vi.fn();
      const onDone = vi.fn();
      subscribeToStream(messageId, onToken, onDone);

      const client = getClient();
      expect(client.newSubscription).toHaveBeenCalledWith(`internal:stream:${messageId}`);

      const sub = vi.mocked(client.newSubscription).mock.results[0].value;
      const publicationCallback = vi.mocked(sub.on).mock.calls.find(call => call[0] === 'publication')![1];

      // Simulate token
      publicationCallback({ data: { token: 'part1', done: false, messageId } });
      expect(onToken).toHaveBeenCalledWith('part1');
      expect(onDone).not.toHaveBeenCalled();

      // Simulate done
      publicationCallback({ data: { token: '', done: true, messageId } });
      expect(onDone).toHaveBeenCalled();
      expect(sub.unsubscribe).toHaveBeenCalled();
      expect(sub.removeAllListeners).toHaveBeenCalled();
      expect(client.removeSubscription).toHaveBeenCalledWith(sub);
    });

    it('safeNewSubscription logic (via helper) cleans up existing subscription', () => {
      const client = getClient();
      const channel = 'internal:agent:agent-1:thoughts';
      const existingSub = {
        unsubscribe: vi.fn(),
        removeAllListeners: vi.fn(),
      };

      vi.mocked(client.getSubscription).mockReturnValue(existingSub as any);

      subscribeToThoughts('agent-1', vi.fn());

      expect(client.getSubscription).toHaveBeenCalledWith(channel);
      expect(existingSub.unsubscribe).toHaveBeenCalled();
      expect(existingSub.removeAllListeners).toHaveBeenCalled();
      expect(client.removeSubscription).toHaveBeenCalledWith(existingSub);
      expect(client.newSubscription).toHaveBeenCalledWith(channel);
    });
  });
});

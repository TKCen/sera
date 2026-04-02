import { describe, it, expect, vi, beforeEach } from 'vitest';
import { Centrifuge } from 'centrifuge';
import {
  getClient,
  disconnectClient,
  subscribeToThoughts,
  subscribeToTerminal,
  subscribeToStream,
} from './centrifugo';

// Variables prefixed with 'mock' are hoisted and available in vi.mock
const mockSubscription = {
  on: vi.fn().mockReturnThis(),
  subscribe: vi.fn().mockReturnThis(),
  unsubscribe: vi.fn().mockReturnThis(),
  removeAllListeners: vi.fn().mockReturnThis(),
};

const mockCentrifuge = {
  connect: vi.fn(),
  disconnect: vi.fn(),
  getSubscription: vi.fn(),
  newSubscription: vi.fn().mockReturnValue(mockSubscription),
  removeSubscription: vi.fn(),
};

vi.mock('centrifuge', () => {
  return {
    Centrifuge: vi.fn().mockImplementation(function (this: any) {
      return mockCentrifuge;
    }),
  };
});

describe('centrifugo lib', () => {
  beforeEach(() => {
    vi.clearAllMocks();
    // Reset the singleton client by calling disconnectClient
    disconnectClient();
  });

  describe('getClient', () => {
    it('should create and connect a new Centrifuge client', () => {
      const client = getClient();
      expect(Centrifuge).toHaveBeenCalled();
      expect(client.connect).toHaveBeenCalled();
      expect(client).toBe(mockCentrifuge);
    });

    it('should return the same client instance on subsequent calls', () => {
      const client1 = getClient();
      const client2 = getClient();
      expect(Centrifuge).toHaveBeenCalledTimes(1);
      expect(client1).toBe(client2);
    });
  });

  describe('disconnectClient', () => {
    it('should disconnect the client and reset the singleton', () => {
      getClient();
      disconnectClient();
      expect(mockCentrifuge.disconnect).toHaveBeenCalled();

      // Verify singleton is reset by getting a new client
      vi.clearAllMocks();
      getClient();
      expect(Centrifuge).toHaveBeenCalledTimes(1);
    });
  });

  describe('subscribeToThoughts', () => {
    it('should subscribe to the correct thoughts channel', () => {
      const agentId = 'test-agent';
      const onThought = vi.fn();
      subscribeToThoughts(agentId, onThought);

      expect(mockCentrifuge.newSubscription).toHaveBeenCalledWith(
        `internal:agent:${agentId}:thoughts`
      );
      expect(mockSubscription.on).toHaveBeenCalledWith('publication', expect.any(Function));
      expect(mockSubscription.subscribe).toHaveBeenCalled();
    });

    it('should call onThought when a publication is received', () => {
      const agentId = 'test-agent';
      const onThought = vi.fn();
      subscribeToThoughts(agentId, onThought);

      // Get the publication callback
      const publicationCallback = mockSubscription.on.mock.calls.find(
        (call) => call[0] === 'publication'
      )[1];

      const mockEvent = { data: { text: 'thinking' } };
      publicationCallback(mockEvent);

      expect(onThought).toHaveBeenCalledWith(mockEvent.data);
    });

    it('should cleanup when unsubscribe is called', () => {
      const unsubscribe = subscribeToThoughts('agent', vi.fn());
      unsubscribe();

      expect(mockSubscription.unsubscribe).toHaveBeenCalled();
      expect(mockSubscription.removeAllListeners).toHaveBeenCalled();
      expect(mockCentrifuge.removeSubscription).toHaveBeenCalledWith(mockSubscription);
    });
  });

  describe('subscribeToTerminal', () => {
    it('should subscribe to the correct terminal channel', () => {
      const agentId = 'test-agent';
      const onOutput = vi.fn();
      subscribeToTerminal(agentId, onOutput);

      expect(mockCentrifuge.newSubscription).toHaveBeenCalledWith(
        `internal:agent:${agentId}:terminal`
      );
      expect(mockSubscription.subscribe).toHaveBeenCalled();
    });

    it('should call onOutput when a publication is received', () => {
      const onOutput = vi.fn();
      subscribeToTerminal('agent', onOutput);

      const publicationCallback = mockSubscription.on.mock.calls.find(
        (call) => call[0] === 'publication'
      )[1];

      const mockData = { data: 'shell output' };
      publicationCallback(mockData);

      expect(onOutput).toHaveBeenCalledWith(mockData.data);
    });
  });

  describe('subscribeToStream', () => {
    it('should subscribe to the correct stream channel', () => {
      const messageId = 'msg-123';
      subscribeToStream(messageId, vi.fn(), vi.fn());

      expect(mockCentrifuge.newSubscription).toHaveBeenCalledWith(
        `internal:stream:${messageId}`
      );
    });

    it('should call onToken and onDone correctly', () => {
      const onToken = vi.fn();
      const onDone = vi.fn();
      subscribeToStream('msg', onToken, onDone);

      const publicationCallback = mockSubscription.on.mock.calls.find(
        (call) => call[0] === 'publication'
      )[1];

      // Test token
      publicationCallback({ data: { token: 'hello' } });
      expect(onToken).toHaveBeenCalledWith('hello');

      // Test done
      publicationCallback({ data: { done: true } });
      expect(onDone).toHaveBeenCalled();
      expect(mockSubscription.unsubscribe).toHaveBeenCalled();
    });
  });

  describe('safeNewSubscription', () => {
    it('should remove existing subscription if it exists', () => {
      const channel = 'test-channel';
      mockCentrifuge.getSubscription.mockReturnValueOnce(mockSubscription);

      subscribeToThoughts('agent', vi.fn());

      expect(mockCentrifuge.getSubscription).toHaveBeenCalled();
      expect(mockSubscription.unsubscribe).toHaveBeenCalled();
      expect(mockSubscription.removeAllListeners).toHaveBeenCalled();
      expect(mockCentrifuge.removeSubscription).toHaveBeenCalledWith(mockSubscription);
      expect(mockCentrifuge.newSubscription).toHaveBeenCalled();
    });
  });
});

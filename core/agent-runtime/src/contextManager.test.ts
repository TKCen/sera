import { describe, it, expect, beforeEach, afterEach } from 'vitest';
import { ContextManager } from './contextManager.js';
import type { ChatMessage } from './llmClient.js';

describe('ContextManager', () => {
  let mgr: ContextManager;

  beforeEach(() => {
    mgr = new ContextManager('gpt-4o-mini'); // 128k window
  });

  afterEach(() => {
    mgr.free();
  });

  describe('countTokens()', () => {
    it('returns 0 for empty string', () => {
      expect(mgr.countTokens('')).toBe(0);
    });

    it('returns a positive count for non-empty text', () => {
      expect(mgr.countTokens('hello world')).toBeGreaterThan(0);
    });

    it('counts more tokens for longer text', () => {
      const short = mgr.countTokens('hello');
      const long = mgr.countTokens('hello world this is a longer sentence with more tokens');
      expect(long).toBeGreaterThan(short);
    });
  });

  describe('countMessageTokens()', () => {
    it('counts tokens across multiple messages', () => {
      const messages: ChatMessage[] = [
        { role: 'system', content: 'You are a helpful assistant.' },
        { role: 'user', content: 'Hello!' },
        { role: 'assistant', content: 'Hi there!' },
      ];
      const count = mgr.countMessageTokens(messages);
      expect(count).toBeGreaterThan(0);
    });
  });

  describe('truncateToolOutput()', () => {
    it('returns short content unchanged', () => {
      const short = 'hello world';
      expect(mgr.truncateToolOutput(short)).toBe(short);
    });

    it('truncates and appends notice when over limit', () => {
      // Generate content that exceeds 4000 tokens
      const long = 'word '.repeat(10_000);
      const result = mgr.truncateToolOutput(long);
      expect(result).toContain('[SERA: output truncated');
      const tokens = mgr.countTokens(result);
      // Should be close to but not exceed TOOL_OUTPUT_MAX_TOKENS (4000) + notice (~15)
      expect(tokens).toBeLessThan(4200);
    });
  });

  describe('isNearLimit()', () => {
    it('returns false for a small message set', () => {
      const messages: ChatMessage[] = [
        { role: 'system', content: 'System prompt' },
        { role: 'user', content: 'Hello' },
      ];
      expect(mgr.isNearLimit(messages)).toBe(false);
    });

    it('returns true when message tokens exceed high-water mark', () => {
      // Create a ContextManager with a very small context window (100 tokens)
      // by overriding via env
      process.env['MAX_CONTEXT_TOKENS'] = '10';
      const smallMgr = new ContextManager('gpt-4o-mini');
      const messages: ChatMessage[] = [
        { role: 'system', content: 'You are a helpful assistant with a very long identity description that fills up the context.' },
        { role: 'user', content: 'Hello, please tell me something interesting about the world.' },
      ];
      expect(smallMgr.isNearLimit(messages)).toBe(true);
      smallMgr.free();
      delete process.env['MAX_CONTEXT_TOKENS'];
    });
  });

  describe('compact() — sliding-window', () => {
    it('always preserves system message', () => {
      process.env['MAX_CONTEXT_TOKENS'] = '50';
      const smallMgr = new ContextManager('gpt-4o-mini');
      const systemContent = 'You are a helpful assistant.';
      const messages: ChatMessage[] = [
        { role: 'system', content: systemContent },
        { role: 'user', content: 'Message 1 — fill up the context with words.' },
        { role: 'assistant', content: 'Response 1 — more words to fill context window.' },
        { role: 'user', content: 'Message 2 — even more words to ensure compaction occurs here.' },
        { role: 'assistant', content: 'Response 2 — this should push us over the limit for sure.' },
      ];

      const result = smallMgr.compact(messages);
      expect(messages[0]!.content).toBe(systemContent);
      expect(messages[0]!.role).toBe('system');
      expect(result.droppedCount).toBeGreaterThan(0);
      smallMgr.free();
      delete process.env['MAX_CONTEXT_TOKENS'];
    });

    it('returns compaction stats', () => {
      process.env['MAX_CONTEXT_TOKENS'] = '30';
      const smallMgr = new ContextManager('gpt-4o-mini');
      const messages: ChatMessage[] = [
        { role: 'system', content: 'System.' },
        { role: 'user', content: 'User message one with many words to fill up tokens.' },
        { role: 'assistant', content: 'Assistant response one with many words to fill tokens.' },
        { role: 'user', content: 'User message two with many words to fill up tokens.' },
      ];

      const before = smallMgr.countMessageTokens(messages);
      const result = smallMgr.compact(messages);
      const after = smallMgr.countMessageTokens(messages);

      expect(result.tokensBefore).toBe(before);
      expect(result.tokensAfter).toBe(after);
      expect(result.reflectMessage).toContain('Context compacted');
      smallMgr.free();
      delete process.env['MAX_CONTEXT_TOKENS'];
    });
  });
});

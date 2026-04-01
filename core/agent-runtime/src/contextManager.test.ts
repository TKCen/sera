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

  describe('getUtilization()', () => {
    it('returns a ratio between 0 and 1 for normal messages', () => {
      const messages: ChatMessage[] = [
        { role: 'system', content: 'System prompt' },
        { role: 'user', content: 'Hello' },
      ];
      const util = mgr.getUtilization(messages);
      expect(util).toBeGreaterThan(0);
      expect(util).toBeLessThan(1);
    });

    it('returns higher utilization for more messages', () => {
      const small: ChatMessage[] = [{ role: 'user', content: 'Hi' }];
      const big: ChatMessage[] = [
        { role: 'user', content: 'word '.repeat(5000) },
        { role: 'assistant', content: 'word '.repeat(5000) },
      ];
      expect(mgr.getUtilization(big)).toBeGreaterThan(mgr.getUtilization(small));
    });
  });

  describe('aggressiveCompact()', () => {
    it('drops more messages than regular compact for the same input', () => {
      // Use a small context window so that both compact (80% = 80 tokens)
      // and aggressiveCompact (50% = 50 tokens) actually trigger compaction.
      process.env['CONTEXT_WINDOW'] = '100';
      delete process.env['MAX_CONTEXT_TOKENS'];
      const smallMgr = new ContextManager('gpt-4o-mini');
      const makeMessages = (): ChatMessage[] => [
        { role: 'system', content: 'System.' },
        { role: 'user', content: 'Message 1 with many words to fill tokens.' },
        { role: 'assistant', content: 'Response 1 with many words to fill tokens.' },
        { role: 'user', content: 'Message 2 with many words to fill tokens.' },
        { role: 'assistant', content: 'Response 2 with many words to fill tokens.' },
        { role: 'user', content: 'Message 3 with many words to fill tokens.' },
        { role: 'assistant', content: 'Response 3 with many words to fill tokens.' },
      ];

      const msgs1 = makeMessages();
      const regular = smallMgr.compact(msgs1);

      const msgs2 = makeMessages();
      const aggressive = smallMgr.aggressiveCompact(msgs2);

      expect(aggressive.droppedCount).toBeGreaterThanOrEqual(regular.droppedCount);
      expect(aggressive.tokensAfter).toBeLessThanOrEqual(regular.tokensAfter);
      smallMgr.free();
      delete process.env['CONTEXT_WINDOW'];
    });

    it('always preserves system message', () => {
      process.env['CONTEXT_WINDOW'] = '60';
      delete process.env['MAX_CONTEXT_TOKENS'];
      const smallMgr = new ContextManager('gpt-4o-mini');
      const messages: ChatMessage[] = [
        { role: 'system', content: 'You are a helpful assistant.' },
        { role: 'user', content: 'Message 1 with many words.' },
        { role: 'assistant', content: 'Response 1 with many words.' },
        { role: 'user', content: 'Message 2 with many words.' },
      ];
      smallMgr.aggressiveCompact(messages);
      expect(messages[0]!.role).toBe('system');
      expect(messages[0]!.content).toBe('You are a helpful assistant.');
      smallMgr.free();
      delete process.env['CONTEXT_WINDOW'];
    });
  });

  describe('truncateAllToolResults()', () => {
    it('truncates oversized tool messages', () => {
      const messages: ChatMessage[] = [
        { role: 'system', content: 'System.' },
        { role: 'user', content: 'Run a tool.' },
        { role: 'tool', content: 'word '.repeat(5000), tool_call_id: 'call_1' },
      ];
      const count = mgr.truncateAllToolResults(messages, 100);
      expect(count).toBe(1);
      expect(messages[2]!.content).toContain('[SERA: tool result retroactively truncated');
      expect(mgr.countTokens(messages[2]!.content)).toBeLessThan(200);
    });

    it('leaves small tool messages unchanged', () => {
      const shortContent = 'OK';
      const messages: ChatMessage[] = [
        { role: 'tool', content: shortContent, tool_call_id: 'call_1' },
      ];
      const count = mgr.truncateAllToolResults(messages, 100);
      expect(count).toBe(0);
      expect(messages[0]!.content).toBe(shortContent);
    });

    it('only truncates tool role messages', () => {
      const longContent = 'word '.repeat(5000);
      const messages: ChatMessage[] = [
        { role: 'user', content: longContent },
        { role: 'tool', content: longContent, tool_call_id: 'call_1' },
      ];
      const count = mgr.truncateAllToolResults(messages, 100);
      expect(count).toBe(1);
      // User message should be untouched
      expect(messages[0]!.content).toBe(longContent);
    });
  });

  describe('getAvailableBudget()', () => {
    it('returns positive value when under limit', () => {
      const messages: ChatMessage[] = [
        { role: 'system', content: 'System.' },
        { role: 'user', content: 'Hello' },
      ];
      const budget = mgr.getAvailableBudget(messages);
      expect(budget).toBeGreaterThan(0);
    });

    it('returns 0 when at or over high-water mark', () => {
      // Use a tiny window so a few messages exceed it
      process.env['CONTEXT_WINDOW'] = '30';
      delete process.env['MAX_CONTEXT_TOKENS'];
      const tinyMgr = new ContextManager('gpt-4o-mini');
      const messages: ChatMessage[] = [
        { role: 'system', content: 'System prompt with enough words to fill the window.' },
        { role: 'user', content: 'User message with additional words to exceed budget.' },
      ];
      expect(tinyMgr.getAvailableBudget(messages)).toBe(0);
      tinyMgr.free();
      delete process.env['CONTEXT_WINDOW'];
    });

    it('respects custom responseReserve', () => {
      const messages: ChatMessage[] = [{ role: 'user', content: 'Hi' }];
      const withDefault = mgr.getAvailableBudget(messages);
      const withLarger = mgr.getAvailableBudget(messages, 8192);
      expect(withLarger).toBeLessThan(withDefault);
    });
  });

  describe('truncateToContextBudget()', () => {
    it('returns content unchanged when budget is sufficient', () => {
      const messages: ChatMessage[] = [{ role: 'user', content: 'Hi' }];
      const result = mgr.truncateToContextBudget('Short result', messages);
      expect(result.content).toBe('Short result');
      expect(result.budgetExceeded).toBe(false);
      expect(result.compactionNeeded).toBe(false);
    });

    it('truncates content when it exceeds available budget', () => {
      // Window of 500 tokens, high-water 400. Short messages ~30 tokens + 4096 reserve
      // won't fit, so use a small responseReserve to leave some budget for the content.
      process.env['CONTEXT_WINDOW'] = '500';
      delete process.env['MAX_CONTEXT_TOKENS'];
      const smallMgr = new ContextManager('gpt-4o-mini');
      const messages: ChatMessage[] = [
        { role: 'system', content: 'System.' },
        { role: 'user', content: 'Hi' },
      ];
      const longContent = 'word '.repeat(500);
      // Use responseReserve=0 so the budget is highWaterMark(400) - messageTokens(~15) ≈ 385
      const result = smallMgr.truncateToContextBudget(longContent, messages, 0);
      expect(result.budgetExceeded).toBe(true);
      expect(result.compactionNeeded).toBe(false);
      expect(result.content).toContain('[SERA: tool result truncated to fit context budget');
      expect(result.content.length).toBeLessThan(longContent.length);
      smallMgr.free();
      delete process.env['CONTEXT_WINDOW'];
    });

    it('returns compactionNeeded when budget is zero', () => {
      process.env['CONTEXT_WINDOW'] = '30';
      delete process.env['MAX_CONTEXT_TOKENS'];
      const tinyMgr = new ContextManager('gpt-4o-mini');
      const messages: ChatMessage[] = [
        { role: 'system', content: 'System prompt with enough words to completely fill the window and more.' },
        { role: 'user', content: 'User message that pushes well over the tiny limit we set.' },
      ];
      const result = tinyMgr.truncateToContextBudget('any content', messages);
      expect(result.compactionNeeded).toBe(true);
      expect(result.budgetExceeded).toBe(true);
      tinyMgr.free();
      delete process.env['CONTEXT_WINDOW'];
    });
  });

  describe('compact() — summary-based', () => {
    it('always preserves system messages', () => {
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

      smallMgr.compact(messages);
      expect(messages[0]!.role).toBe('system');
      expect(messages[0]!.content).toBe(systemContent);
      smallMgr.free();
      delete process.env['MAX_CONTEXT_TOKENS'];
    });

    it('injects continuation message with summary after system prompt', () => {
      process.env['MAX_CONTEXT_TOKENS'] = '50';
      process.env['CONTEXT_WINDOW'] = '50';
      const smallMgr = new ContextManager('gpt-4o-mini');
      const messages: ChatMessage[] = [
        { role: 'system', content: 'System prompt.' },
        { role: 'user', content: 'I need to fix a bug in core/src/index.ts' },
        { role: 'assistant', content: 'I will look into it. What is the issue?' },
        { role: 'user', content: 'It is a todo: fix the memory leak in agent-runtime/src/loop.ts' },
        { role: 'assistant', content: 'Understood.' },
        { role: 'user', content: 'Actually, let us check package.json first.' },
        { role: 'assistant', content: 'Ok.' },
      ];

      smallMgr.compact(messages);

      // messages[0] is system prompt
      // messages[1] is continuation message
      expect(messages[1]!.role).toBe('system');
      expect(messages[1]!.content).toContain('This session is being continued');
      expect(messages[1]!.content).toContain('Summary:');

      // Check summary content
      const summary = messages[1]!.content;
      expect(summary).toContain('Scope:');
      expect(summary).toContain('Tools mentioned:');
      expect(summary).toContain('Recent user requests:');
      expect(summary).toContain('Pending work:');
      expect(summary).toContain('Key files:');
      expect(summary).toContain('Key timeline:');

      // Verify specific extractions
      expect(summary).toContain('core/src/index.ts');
      expect(summary).toContain('agent-runtime/src/loop.ts');
      expect(summary).toContain('package.json');
      expect(summary).toContain('todo: fix the memory leak');

      smallMgr.free();
      delete process.env['MAX_CONTEXT_TOKENS'];
      delete process.env['CONTEXT_WINDOW'];
    });

    it('preserves N most recent messages verbatim', () => {
      process.env['MAX_CONTEXT_TOKENS'] = '1000';
      process.env['CONTEXT_WINDOW'] = '1000';
      process.env['PRESERVE_RECENT_MESSAGES'] = '2';
      const smallMgr = new ContextManager('gpt-4o-mini');

      const m1: ChatMessage = { role: 'user', content: 'Msg 1' };
      const m2: ChatMessage = { role: 'assistant', content: 'Msg 2' };
      const m3: ChatMessage = { role: 'user', content: 'Msg 3' };
      const m4: ChatMessage = { role: 'assistant', content: 'Msg 4' };

      const messages: ChatMessage[] = [
        { role: 'system', content: 'System' },
        m1, m2, m3, m4
      ];

      // We want to drop exactly 2 messages (m1 and m2)
      // m1, m2 are dropped. m3, m4 are kept because they are the last 2.

      // Total tokens for m3, m4 is roughly 20. Total with system prompt is roughly 30.
      // Set highWaterMark low enough to trigger drop but high enough to keep m3, m4.
      (smallMgr as any).highWaterMark = 100;

      // Mock countMessageTokens to force it to look like we are over limit when all are present,
      // but under when m3, m4 are present.
      const realCount = smallMgr.countMessageTokens;
      smallMgr.countMessageTokens = function(msgs) {
        if (msgs.length === 5) return 200; // All 5
        if (msgs.length === 4 && msgs.some(m => m.content.includes('Summary:'))) return 50; // System + Summary + m3 + m4
        return realCount.call(this, msgs);
      };

      smallMgr.compact(messages);

      // System, Continuation, m3, m4
      expect(messages.length).toBe(4);
      expect(messages[0]!.role).toBe('system');
      expect(messages[1]!.role).toBe('system');
      expect(messages[1]!.content).toContain('Summary:');
      expect(messages[2]).toEqual(m3);
      expect(messages[3]).toEqual(m4);

      smallMgr.free();
      delete process.env['MAX_CONTEXT_TOKENS'];
      delete process.env['CONTEXT_WINDOW'];
      delete process.env['PRESERVE_RECENT_MESSAGES'];
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

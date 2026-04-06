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
        {
          role: 'system',
          content:
            'You are a helpful assistant with a very long identity description that fills up the context.',
        },
        { role: 'user', content: 'Hello, please tell me something interesting about the world.' },
      ];
      expect(smallMgr.isNearLimit(messages)).toBe(true);
      smallMgr.free();
      delete process.env['MAX_CONTEXT_TOKENS'];
    });
  });

  describe('per-message token caching', () => {
    it('caches tokens in the message object', () => {
      const msg: ChatMessage = { role: 'user', content: 'hello world' };
      expect(msg.tokens).toBeUndefined();
      mgr.countMessageTokens([msg]);
      expect(msg.tokens).toBeDefined();
      expect(msg.tokens).toBe(mgr.estimateMessageTokens(msg));
    });

    it('uses cached tokens if present', () => {
      const msg: ChatMessage = { role: 'user', content: 'hello world', tokens: 1000 };
      const count = mgr.countMessageTokens([msg]);
      expect(count).toBe(1000);
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
    it('drops more messages than regular compact for the same input', async () => {
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
      const regular = await smallMgr.compact(msgs1);

      const msgs2 = makeMessages();
      const aggressive = await smallMgr.aggressiveCompact(msgs2);

      expect(aggressive.droppedCount).toBeGreaterThanOrEqual(regular.droppedCount);
      // Tokens after can be larger if more messages were dropped because of the large summary being injected
      // but the total tokens should still be within their respective targets.
      expect(aggressive.tokensAfter).toBeLessThanOrEqual(50 + 400); // 50 is target, allow some overhead for summary
      smallMgr.free();
      delete process.env['CONTEXT_WINDOW'];
    });

    it('always preserves system message', async () => {
      process.env['CONTEXT_WINDOW'] = '60';
      delete process.env['MAX_CONTEXT_TOKENS'];
      const smallMgr = new ContextManager('gpt-4o-mini');
      const messages: ChatMessage[] = [
        { role: 'system', content: 'You are a helpful assistant.' },
        { role: 'user', content: 'Message 1 with many words.' },
        { role: 'assistant', content: 'Response 1 with many words.' },
        { role: 'user', content: 'Message 2 with many words.' },
      ];
      await smallMgr.aggressiveCompact(messages);
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

  describe('clearOldToolResults()', () => {
    it('clears tool results beyond preserveCount', () => {
      const messages: ChatMessage[] = [
        { role: 'user', content: 'ask' },
        { role: 'tool', content: 'result 1', tool_call_id: '1' },
        { role: 'assistant', content: 'thought' },
        { role: 'tool', content: 'result 2', tool_call_id: '2' },
        { role: 'tool', content: 'result 3', tool_call_id: '3' },
        { role: 'tool', content: 'result 4', tool_call_id: '4' },
      ];

      const cleared = mgr.clearOldToolResults(messages, 2);
      expect(cleared).toBe(2);
      expect(messages[1]!.content).toBe('[cleared — re-read if needed]');
      expect(messages[3]!.content).toBe('[cleared — re-read if needed]');
      expect(messages[4]!.content).toBe('result 3');
      expect(messages[5]!.content).toBe('result 4');
    });

    it('does not clear non-tool messages', () => {
      const messages: ChatMessage[] = [
        { role: 'system', content: 'sys' },
        { role: 'user', content: 'usr' },
        { role: 'assistant', content: 'ast' },
      ];
      const cleared = mgr.clearOldToolResults(messages, 0);
      expect(cleared).toBe(0);
      expect(messages[0]!.content).toBe('sys');
      expect(messages[1]!.content).toBe('usr');
      expect(messages[2]!.content).toBe('ast');
    });

    it('updates tokens for cleared messages', () => {
      const msg: ChatMessage = { role: 'tool', content: 'long result...', tokens: 100 };
      const messages = [msg];
      mgr.clearOldToolResults(messages, 0);
      expect(msg.content).toBe('[cleared — re-read if needed]');
      expect(msg.tokens).toBeLessThan(100);
      expect(msg.tokens).toBe(mgr.estimateMessageTokens(msg));
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
        {
          role: 'system',
          content: 'System prompt with enough words to completely fill the window and more.',
        },
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
    it('always preserves system messages', async () => {
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

      await smallMgr.compact(messages);
      expect(messages[0]!.role).toBe('system');
      expect(messages[0]!.content).toBe(systemContent);
      smallMgr.free();
      delete process.env['MAX_CONTEXT_TOKENS'];
    });

    it('injects continuation message with summary after system prompt', async () => {
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

      await smallMgr.compact(messages);
      await smallMgr.compact(messages);

      // messages[0] is system prompt
      // messages[1] is continuation message
      expect(messages[1]!.role).toBe('system');
      expect(messages[1]!.content).toContain('This session is being continued');
      expect(messages[1]!.content).toContain('Summary:');

      // Check summary content — sections produced by generateCompactionSummary
      const summary = messages[1]!.content;
      expect(summary).toContain('Task Context:');
      expect(summary).toContain('Tools Used:');
      expect(summary).toContain('Decisions:');
      expect(summary).toContain('Pending Work:');

      // Verify specific extractions
      expect(summary).toContain('fix a bug in core/src/index.ts');
      expect(summary).toContain('todo: fix the memory leak');

      smallMgr.free();
      delete process.env['MAX_CONTEXT_TOKENS'];
      delete process.env['CONTEXT_WINDOW'];
    });

    it('preserves N most recent messages verbatim', async () => {
      process.env['MAX_CONTEXT_TOKENS'] = '1000';
      process.env['CONTEXT_WINDOW'] = '1000';
      process.env['PRESERVE_RECENT_MESSAGES'] = '2';
      const smallMgr = new ContextManager('gpt-4o-mini');

      const m1: ChatMessage = { role: 'user', content: 'Msg 1' };
      const m2: ChatMessage = { role: 'assistant', content: 'Msg 2' };
      const m3: ChatMessage = { role: 'user', content: 'Msg 3' };
      const m4: ChatMessage = { role: 'assistant', content: 'Msg 4' };

      const messages: ChatMessage[] = [{ role: 'system', content: 'System' }, m1, m2, m3, m4];

      // We want to drop exactly 2 messages (m1 and m2)
      // m1, m2 are dropped. m3, m4 are kept because they are the last 2.

      // Total tokens for m3, m4 is roughly 20. Total with system prompt is roughly 30.
      // Set highWaterMark low enough to trigger drop but high enough to keep m3, m4.
      (smallMgr as any).highWaterMark = 100;

      // Mock countMessageTokens to force it to look like we are over limit when all are present,
      // but under when m3, m4 are present.
      const realCount = smallMgr.countMessageTokens;
      smallMgr.countMessageTokens = function (msgs) {
        if (msgs.length === 5) return 200; // All 5
        if (msgs.length === 4 && msgs.some((m) => m.content.includes('Summary:'))) return 50; // System + Summary + m3 + m4
        return realCount.call(this, msgs);
      };

      await smallMgr.compact(messages);

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

    it('returns compaction stats', async () => {
      process.env['MAX_CONTEXT_TOKENS'] = '30';
      const smallMgr = new ContextManager('gpt-4o-mini');
      const messages: ChatMessage[] = [
        { role: 'system', content: 'System.' },
        { role: 'user', content: 'User message one with many words to fill up tokens.' },
        { role: 'assistant', content: 'Assistant response one with many words to fill tokens.' },
        { role: 'user', content: 'User message two with many words to fill up tokens.' },
      ];

      const before = smallMgr.countMessageTokens(messages);
      const result = await smallMgr.compact(messages);
      const after = smallMgr.countMessageTokens(messages);

      expect(result.tokensBefore).toBe(before);
      expect(result.tokensAfter).toBe(after);
      expect(result.reflectMessage).toContain('Context compacted');
      smallMgr.free();
      delete process.env['MAX_CONTEXT_TOKENS'];
    });
  });

  describe('compact() — LLM-based summarize strategy', () => {
    it('uses LLM to summarize and preserves recent messages', async () => {
      process.env['CONTEXT_COMPACTION_STRATEGY'] = 'summarise';
      process.env['CONTEXT_WINDOW'] = '100';
      const smallMgr = new ContextManager('gpt-4o-mini');

      const messages: ChatMessage[] = [
        { role: 'system', content: 'System prompt.' },
        { role: 'user', content: 'Old message 1'.repeat(100) },
        { role: 'assistant', content: 'Old response 1'.repeat(100) },
        { role: 'user', content: 'Recent message 1' },
        { role: 'assistant', content: 'Recent response 1' },
      ];

      // Mock LLM client
      const mockLLM = {
        chat: vi.fn().mockResolvedValue({
          content: 'This is a summary of the old conversation.',
          usage: { promptTokens: 10, completionTokens: 5, totalTokens: 15 },
        }),
      };

      // We want to ensure it summarizes "Old" and keeps "Recent".
      // By default PRESERVE_RECENT_MESSAGES=4, but we only have 4 non-system messages.
      // Let's set PRESERVE_RECENT_MESSAGES=2.
      process.env['PRESERVE_RECENT_MESSAGES'] = '2';
      const mgrWithK2 = new ContextManager('gpt-4o-mini', 100);

      const result = await mgrWithK2.compact(messages, mockLLM as any);

      expect(result.strategy).toBe('summarise');
      expect(mockLLM.chat).toHaveBeenCalled();
      const lastCall = mockLLM.chat.mock.calls[0];
      expect(lastCall[0][0].content).toContain('Summarize');
      expect(lastCall[0][0].content).toContain('Old message 1');

      // Check messages array: [system, summary-user, summary-ack, recent1, recent2]
      expect(messages.length).toBe(5);
      expect(messages[0]!.role).toBe('system');
      expect(messages[1]!.content).toContain('[Context Summary]');
      expect(messages[1]!.content).toContain('This is a summary');
      expect(messages[2]!.role).toBe('assistant');
      expect(messages[3]!.content).toBe('Recent message 1');
      expect(messages[4]!.content).toBe('Recent response 1');

      delete process.env['CONTEXT_COMPACTION_STRATEGY'];
      delete process.env['CONTEXT_WINDOW'];
      delete process.env['PRESERVE_RECENT_MESSAGES'];
    });

    it('falls back to sliding-window if LLM fails', async () => {
      process.env['CONTEXT_COMPACTION_STRATEGY'] = 'summarise';
      process.env['CONTEXT_WINDOW'] = '2000';
      process.env['MAX_CONTEXT_TOKENS'] = '500';
      process.env['PRESERVE_RECENT_MESSAGES'] = '1';
      const smallMgr = new ContextManager('gpt-4o-mini');

      const messages: ChatMessage[] = [
        { role: 'system', content: 'System.' },
        { role: 'user', content: 'Message to drop'.repeat(200) },
        { role: 'user', content: 'Message to keep' },
      ];

      const mockLLM = {
        chat: vi.fn().mockRejectedValue(new Error('LLM down')),
      };

      // Context tokens before: 1 (system) + ~300 (large user) + ~4 (small user) ≈ 305
      // Target is 50. PRESERVE_RECENT_MESSAGES is 1.
      // Sliding window should drop "Message to drop" and keep "Message to keep".

      const result = await smallMgr.compact(messages, mockLLM as any);

      expect(result.strategy).toBe('sliding-window');
      expect(result.isFallback).toBe(true);
      expect(result.reflectMessage).toContain('fallback');

      // Verification:
      // systemMessages is [system]
      // nonSystemMessages starts as [largeUser, smallUser]
      // keepLimit = min(2, 1) = 1.
      // while loop:
      // nonSystemMessages.length (2) > keepLimit (1) AND tokens (305) >= target (50)
      // -> drops largeUser, nonSystemMessages is [smallUser]
      // nonSystemMessages.length (1) > keepLimit (1) -> FALSE
      // result is [system, continuation, smallUser]

      expect(messages.length).toBe(3);
      expect(messages[0]!.role).toBe('system');
      expect(messages[1]!.role).toBe('system');
      expect(messages[1]!.content).toContain('Summary:');
      expect(messages[2]!.content).toContain('Message to keep');

      delete process.env['CONTEXT_COMPACTION_STRATEGY'];
      delete process.env['CONTEXT_WINDOW'];
      delete process.env['MAX_CONTEXT_TOKENS'];
      delete process.env['PRESERVE_RECENT_MESSAGES'];
    });

    it('strips enrichment tags before summarizing', async () => {
      process.env['CONTEXT_COMPACTION_STRATEGY'] = 'summarise';
      process.env['CONTEXT_WINDOW'] = '100';
      process.env['PRESERVE_RECENT_MESSAGES'] = '0';
      const smallMgr = new ContextManager('gpt-4o-mini');

      const messages: ChatMessage[] = [
        {
          role: 'user',
          content: ('<memory>Secret knowledge</memory>Tell me about the task.' + ' ').repeat(50),
        },
      ];

      const mockLLM = {
        chat: vi.fn().mockResolvedValue({ content: 'Summary' }),
      };

      await smallMgr.compact(messages, mockLLM as any);

      const promptUsed = mockLLM.chat.mock.calls[0][0][0].content;
      expect(promptUsed).not.toContain('<memory>');
      expect(promptUsed).not.toContain('Secret knowledge');
      expect(promptUsed).toContain('Tell me about the task.');

      delete process.env['CONTEXT_COMPACTION_STRATEGY'];
      delete process.env['CONTEXT_WINDOW'];
      delete process.env['PRESERVE_RECENT_MESSAGES'];
    });
  });

  describe('generateCompactionSummary()', () => {
    it('includes all four required sections', async () => {
      const messages: ChatMessage[] = [
        { role: 'user', content: 'Please implement the login feature.' },
        {
          role: 'assistant',
          content: 'I will use JWT for authentication.',
          tool_calls: [
            { id: 'tc1', type: 'function', function: { name: 'shell', arguments: '{}' } },
          ],
        },
        { role: 'tool', content: 'Command succeeded.', tool_call_id: 'tc1' },
        { role: 'assistant', content: 'Next step: need to add password hashing.' },
      ];

      const summary = await mgr.generateCompactionSummary(messages);
      expect(summary).toContain('Task Context:');
      expect(summary).toContain('Tools Used:');
      expect(summary).toContain('Decisions:');
      expect(summary).toContain('Pending Work:');
    });

    it('extracts user task context into Task Context section', async () => {
      const messages: ChatMessage[] = [
        { role: 'user', content: 'Refactor the authentication module.' },
        { role: 'assistant', content: 'Working on it.' },
      ];

      const summary = await mgr.generateCompactionSummary(messages);
      expect(summary).toContain('Refactor the authentication module.');
    });

    it('extracts tool names and result snippets into Tools Used section', async () => {
      const messages: ChatMessage[] = [
        {
          role: 'assistant',
          content: 'Running shell command.',
          tool_calls: [
            { id: 'tc1', type: 'function', function: { name: 'shell', arguments: '{"cmd":"ls"}' } },
          ],
        },
        { role: 'tool', content: 'file1.ts\nfile2.ts', tool_call_id: 'tc1' },
      ];

      const summary = await mgr.generateCompactionSummary(messages);
      expect(summary).toContain('shell');
      expect(summary).toContain('file1.ts');
    });

    it('extracts assistant decisions into Decisions section', async () => {
      const messages: ChatMessage[] = [
        { role: 'user', content: 'Which approach should we use?' },
        { role: 'assistant', content: 'I decided to use the repository pattern for data access.' },
      ];

      const summary = await mgr.generateCompactionSummary(messages);
      expect(summary).toContain('repository pattern');
    });

    it('extracts pending work into Pending Work section', async () => {
      const messages: ChatMessage[] = [
        { role: 'user', content: 'Start the refactor.' },
        { role: 'assistant', content: 'Need to add error handling next.' },
      ];

      const summary = await mgr.generateCompactionSummary(messages);
      expect(summary).toContain('Need to add error handling next.');
    });

    it('returns none placeholders when no tools, decisions, or pending work exist', async () => {
      const messages: ChatMessage[] = [
        { role: 'user', content: 'Hello.' },
        { role: 'assistant', content: 'Hi there.' },
      ];

      const summary = await mgr.generateCompactionSummary(messages);
      // All sections still present
      expect(summary).toContain('Task Context:');
      expect(summary).toContain('Tools Used:');
      expect(summary).toContain('Decisions:');
      expect(summary).toContain('Pending Work:');
      // No-content placeholders
      expect(summary).toContain('none');
    });

    it('skips cleared tool results', async () => {
      const messages: ChatMessage[] = [
        {
          role: 'assistant',
          content: '',
          tool_calls: [
            { id: 'tc1', type: 'function', function: { name: 'read_file', arguments: '{}' } },
          ],
        },
        { role: 'tool', content: '[cleared — re-read if needed]', tool_call_id: 'tc1' },
      ];

      const summary = await mgr.generateCompactionSummary(messages);
      expect(summary).not.toContain('[cleared — re-read if needed]');
    });

    it('injects structured summary as system message during sliding-window compaction', async () => {
      process.env['MAX_CONTEXT_TOKENS'] = '40';
      process.env['CONTEXT_WINDOW'] = '40';
      const smallMgr = new ContextManager('gpt-4o-mini');

      const messages: ChatMessage[] = [
        { role: 'system', content: 'You are a helpful assistant.' },
        {
          role: 'assistant',
          content: 'I will use JWT.',
          tool_calls: [
            { id: 'tc1', type: 'function', function: { name: 'shell', arguments: '{}' } },
          ],
        },
        { role: 'tool', content: 'done', tool_call_id: 'tc1' },
        { role: 'user', content: 'Implement feature X.' },
        { role: 'assistant', content: 'Need to add tests next.' },
        { role: 'user', content: 'Keep going.' },
      ];

      await smallMgr.compact(messages);

      // Find the injected continuation system message
      const continuation = messages.find(
        (m) =>
          m.role === 'system' &&
          typeof m.content === 'string' &&
          (m.content as string).includes('Summary:')
      );
      expect(continuation).toBeDefined();
      const content = continuation!.content as string;
      expect(content).toContain('Task Context:');
      expect(content).toContain('Tools Used:');
      expect(content).toContain('Decisions:');
      expect(content).toContain('Pending Work:');

      smallMgr.free();
      delete process.env['MAX_CONTEXT_TOKENS'];
      delete process.env['CONTEXT_WINDOW'];
    });
  });
});

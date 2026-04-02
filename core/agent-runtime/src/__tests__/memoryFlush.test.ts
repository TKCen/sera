/**
 * Tests for the memory flush logic in ReasoningLoop (Story 5.12).
 *
 * Before context compaction, if enabled, the agent gets one internal turn
 * restricted to memory tools to persist important context.
 */

import { describe, it, expect, vi, beforeEach, afterEach } from 'vitest';
import { ReasoningLoop } from '../loop.js';
import type { LLMResponse, ToolDefinition, ChatMessage } from '../llmClient.js';
import type { RuntimeManifest } from '../manifest.js';

// ── Mocks ────────────────────────────────────────────────────────────────────

const mockChat = vi.fn<(messages: ChatMessage[], tools?: ToolDefinition[], temperature?: number) => Promise<LLMResponse>>();
const mockPublishThought = vi.fn();
const mockPublishStreamToken = vi.fn();
const mockGetToolDefinitions = vi.fn();
const mockExecuteToolCalls = vi.fn();

const mockLlm = { chat: mockChat } as any;
const mockCentrifugo = {
  publishThought: mockPublishThought,
  publishStreamToken: mockPublishStreamToken,
  publishStreamError: vi.fn().mockResolvedValue(undefined),
} as any;
const mockTools = {
  getToolDefinitions: mockGetToolDefinitions,
  executeToolCalls: mockExecuteToolCalls,
} as any;

const manifest: RuntimeManifest = {
  apiVersion: 'v1',
  kind: 'Agent',
  metadata: { name: 'test-agent', displayName: 'Test', icon: 'bot', circle: 'system', tier: 1 },
  identity: { role: 'tester', description: 'Test agent' },
  model: { provider: 'openai', name: 'gpt-4o-mini' },
};

function successResponse(content: string, toolCalls?: any[]): LLMResponse {
  return {
    content,
    toolCalls,
    usage: { promptTokens: 10, completionTokens: 5, cacheCreationTokens: 0, cacheReadTokens: 0, totalTokens: 15 },
  };
}

// ── Setup ────────────────────────────────────────────────────────────────────

let savedEnv: Record<string, string | undefined>;

beforeEach(() => {
  vi.resetAllMocks();
  mockPublishThought.mockResolvedValue(undefined);
  mockPublishStreamToken.mockResolvedValue(undefined);
  mockExecuteToolCalls.mockResolvedValue([]);

  savedEnv = {
    CONTEXT_WINDOW: process.env['CONTEXT_WINDOW'],
    MAX_CONTEXT_TOKENS: process.env['MAX_CONTEXT_TOKENS'],
    MEMORY_FLUSH_BEFORE_COMPACTION: process.env['MEMORY_FLUSH_BEFORE_COMPACTION'],
      CONTEXT_COMPACTION_THRESHOLD: process.env['CONTEXT_COMPACTION_THRESHOLD'],
  };
});

afterEach(() => {
  for (const [k, v] of Object.entries(savedEnv)) {
    if (v === undefined) delete process.env[k];
    else process.env[k] = v;
  }
});

// ── Tests ────────────────────────────────────────────────────────────────────

describe('ReasoningLoop — memory flush', () => {
  it('triggers memory flush when near limit and knowledge-store is available', async () => {
    process.env['CONTEXT_WINDOW'] = '100';
    delete process.env['MAX_CONTEXT_TOKENS'];

    const ksTool: ToolDefinition = {
      type: 'function',
      function: { name: 'knowledge-store', description: 'Store knowledge', parameters: {} },
    };
    mockGetToolDefinitions.mockReturnValue([ksTool]);

    // 1st call: Flush turn - agent calls knowledge-store
    // 2nd call: Main loop turn - agent finishes task
    mockChat
      .mockResolvedValueOnce(successResponse('', [{ id: 'call_1', type: 'function', function: { name: 'knowledge-store', arguments: '{"content":"important"}' } }]))
      .mockResolvedValueOnce(successResponse('Done'));

    mockExecuteToolCalls.mockResolvedValueOnce([{
      message: { role: 'tool', tool_call_id: 'call_1', content: 'Success' },
      toolName: 'knowledge-store',
      argRepaired: false,
      repairStrategy: null,
    }]);

    const loop = new ReasoningLoop(mockLlm, mockTools, mockCentrifugo, manifest);
    const result = await loop.run({ taskId: 't1', task: 'long task '.repeat(10) });

    expect(result.exitReason).toBe('success');

    // Verify flush turn restricted tools
    const flushCallTools = mockChat.mock.calls[0][1];
    expect(flushCallTools).toHaveLength(1);
    expect(flushCallTools![0].function.name).toBe('knowledge-store');

    // Verify 30s timeout was passed
    const flushCallTimeout = mockChat.mock.calls[0][4];
    expect(flushCallTimeout).toBe(30_000);

    // Verify internal thoughts
    const internalThoughts = mockPublishThought.mock.calls.filter(c => c[3]?.internal === true);
    expect(internalThoughts.length).toBeGreaterThan(0);
    expect(internalThoughts.some(c => c[1].includes('triggering memory flush'))).toBe(true);

    // Verify usage stats exclude flush turn
    // Total calls = 2. Each successResponse returns usage with 15 tokens.
    // Loop increments turnCount for each turn that has usage.
    // BUT we need to ensure the flush turn doesn't increment the usage returned in result.
    // In current implementation, LLM responses are accumulated.
    // Wait, the requirement said "Turn not counted against token budget".
    // I need to check how to implement this.
    // Re-reading code:
    // response = await this.llm.chat(...)
    // if (response.usage) { totalPromptTokens += ... }
    // My implementation of flush uses this.llm.chat but DOES NOT accumulate usage if I'm careful.
    // Looking at my loop.ts changes... Oh, I called this.llm.chat directly and didn't accumulate usage!

    expect(result.usage.turns).toBe(1); // Only the "Done" turn
    expect(result.usage.totalTokens).toBe(15);
  });

  it('skips flush when MEMORY_FLUSH_BEFORE_COMPACTION=false', async () => {
    process.env['CONTEXT_WINDOW'] = '100';
    process.env['MEMORY_FLUSH_BEFORE_COMPACTION'] = 'false';

    const ksTool: ToolDefinition = {
      type: 'function',
      function: { name: 'knowledge-store', description: 'Store knowledge', parameters: {} },
    };
    mockGetToolDefinitions.mockReturnValue([ksTool]);

    mockChat.mockResolvedValueOnce(successResponse('Done'));

    const loop = new ReasoningLoop(mockLlm, mockTools, mockCentrifugo, manifest);
    await loop.run({ taskId: 't1', task: 'long task '.repeat(10) });

    // Should only be one chat call (no flush)
    expect(mockChat).toHaveBeenCalledTimes(1);
    const thoughts = mockPublishThought.mock.calls.map(c => c[1]);
    expect(thoughts.some(t => t.includes('triggering memory flush'))).toBe(false);
  });

  it('handles flush turn timeout or error gracefully', async () => {
    process.env['CONTEXT_WINDOW'] = '100';

    const ksTool: ToolDefinition = {
      type: 'function',
      function: { name: 'knowledge-store', description: 'Store knowledge', parameters: {} },
    };
    mockGetToolDefinitions.mockReturnValue([ksTool]);

    // Flush turn throws
    mockChat
      .mockRejectedValueOnce(new Error('LLM Timeout'))
      .mockResolvedValueOnce(successResponse('Done'));

    const loop = new ReasoningLoop(mockLlm, mockTools, mockCentrifugo, manifest);
    const result = await loop.run({ taskId: 't1', task: 'long task '.repeat(10) });

    expect(result.exitReason).toBe('success');
    expect(mockChat).toHaveBeenCalledTimes(2); // 1 failed flush + 1 successful main
  });

  it('triggers memory flush with multiple memory tools available', async () => {
    process.env['CONTEXT_WINDOW'] = '100';

    const ksTool: ToolDefinition = {
      type: 'function',
      function: { name: 'knowledge-store', description: 'Store knowledge', parameters: {} },
    };
    const storeMemoryTool: ToolDefinition = {
      type: 'function',
      function: { name: 'store-memory', description: 'Store memory', parameters: {} },
    };
    const otherTool: ToolDefinition = {
      type: 'function',
      function: { name: 'file-read', description: 'Read file', parameters: {} },
    };
    mockGetToolDefinitions.mockReturnValue([ksTool, storeMemoryTool, otherTool]);

    mockChat
      .mockResolvedValueOnce(successResponse('Saving...'))
      .mockResolvedValueOnce(successResponse('Done'));

    const loop = new ReasoningLoop(mockLlm, mockTools, mockCentrifugo, manifest);
    await loop.run({ taskId: 't1', task: 'long task '.repeat(10) });

    // Verify flush turn restricted tools to memory tools only
    const flushCallTools = mockChat.mock.calls[0][1];
    expect(flushCallTools).toHaveLength(2);
    expect(flushCallTools!.some(t => t.function.name === 'knowledge-store')).toBe(true);
    expect(flushCallTools!.some(t => t.function.name === 'store-memory')).toBe(true);
    expect(flushCallTools!.some(t => t.function.name === 'file-read')).toBe(false);
  });

  it('respects CONTEXT_COMPACTION_THRESHOLD env var', async () => {
    process.env['CONTEXT_WINDOW'] = '1000';
    process.env['CONTEXT_COMPACTION_THRESHOLD'] = '0.2'; // 200 token threshold

    const ksTool: ToolDefinition = {
      type: 'function',
      function: { name: 'knowledge-store', description: 'Store knowledge', parameters: {} },
    };
    mockGetToolDefinitions.mockReturnValue([ksTool]);

    mockChat.mockResolvedValue(successResponse('Done'));

    const loop = new ReasoningLoop(mockLlm, mockTools, mockCentrifugo, manifest);

    // 1. Under threshold
    await loop.run({ taskId: 't1', task: 'tiny' });
    expect(mockChat).toHaveBeenCalledTimes(1);

    mockChat.mockClear();

    // 2. Over threshold (task content ~400 tokens)
    await loop.run({ taskId: 't2', task: 'word '.repeat(200) });
    // In our implementation, if near limit, it fires memory flush turn, which counts as 1 LLM call,
    // then it proceeds to the regular turn (another LLM call). Total 2.
    // However, if the first turn is already over threshold, it might trigger compaction.
    expect(mockChat).toHaveBeenCalledTimes(2);
  });
});

/**
 * Tests for the pre-compaction memory save hook in ReasoningLoop.
 *
 * When the context window is nearly full and the agent has the knowledge-store
 * tool, the loop injects a save-reminder before compacting — giving the agent
 * one iteration to persist important findings.
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

function successResponse(content: string): LLMResponse {
  return { content, usage: { promptTokens: 10, completionTokens: 5, totalTokens: 15 } };
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
  };
});

afterEach(() => {
  for (const [k, v] of Object.entries(savedEnv)) {
    if (v === undefined) delete process.env[k];
    else process.env[k] = v;
  }
});

// ── Tests ────────────────────────────────────────────────────────────────────

describe('ReasoningLoop — pre-compaction memory save hook', () => {
  it('injects save-reminder when knowledge-store is available and context is near limit', async () => {
    // Small context window → isNearLimit triggers immediately
    process.env['CONTEXT_WINDOW'] = '100';
    delete process.env['MAX_CONTEXT_TOKENS'];

    // Include knowledge-store in tool definitions
    const knowledgeStoreDef: ToolDefinition = {
      type: 'function',
      function: { name: 'knowledge-store', description: 'Store knowledge', parameters: {} },
    };
    mockGetToolDefinitions.mockReturnValue([knowledgeStoreDef]);

    // First LLM call: agent responds to the save reminder
    // Second LLM call: after compaction, agent produces final answer
    mockChat
      .mockResolvedValueOnce(successResponse('I will save my findings now'))
      .mockResolvedValueOnce(successResponse('Final answer'));

    const loop = new ReasoningLoop(mockLlm, mockTools, mockCentrifugo, manifest);
    const longTask = 'Analyze this data: ' + 'word '.repeat(20);
    const result = await loop.run({ taskId: 'task-1', task: longTask });

    expect(result.exitReason).toBe('success');

    // Verify the save-reminder system message was injected
    const saveThoughts = result.thoughtStream.filter(
      (t) => t.step === 'reflect' && t.content.includes('save-reminder'),
    );
    expect(saveThoughts.length).toBeGreaterThan(0);
  });

  it('skips hook when knowledge-store is NOT in tool definitions', async () => {
    process.env['CONTEXT_WINDOW'] = '100';
    delete process.env['MAX_CONTEXT_TOKENS'];

    // No knowledge-store tool
    mockGetToolDefinitions.mockReturnValue([]);

    mockChat.mockResolvedValueOnce(successResponse('Done'));

    const loop = new ReasoningLoop(mockLlm, mockTools, mockCentrifugo, manifest);
    const longTask = 'Analyze this data: ' + 'word '.repeat(20);
    const result = await loop.run({ taskId: 'task-1', task: longTask });

    expect(result.exitReason).toBe('success');

    // No save-reminder thoughts
    const saveThoughts = result.thoughtStream.filter(
      (t) => t.step === 'reflect' && t.content.includes('save-reminder'),
    );
    expect(saveThoughts).toHaveLength(0);
  });

  it('hook fires at most once per run', async () => {
    process.env['CONTEXT_WINDOW'] = '100';
    delete process.env['MAX_CONTEXT_TOKENS'];

    const knowledgeStoreDef: ToolDefinition = {
      type: 'function',
      function: { name: 'knowledge-store', description: 'Store knowledge', parameters: {} },
    };
    mockGetToolDefinitions.mockReturnValue([knowledgeStoreDef]);

    // Multiple iterations — each will trigger isNearLimit
    mockChat
      .mockResolvedValueOnce(successResponse('Saving context...'))
      .mockResolvedValueOnce(successResponse('Still working...'))
      .mockResolvedValueOnce(successResponse('Done'));

    const loop = new ReasoningLoop(mockLlm, mockTools, mockCentrifugo, manifest);
    const longTask = 'Big task: ' + 'word '.repeat(20);
    const result = await loop.run({ taskId: 'task-1', task: longTask });

    // Save-reminder should appear exactly once
    const saveThoughts = result.thoughtStream.filter(
      (t) => t.step === 'reflect' && t.content.includes('save-reminder'),
    );
    expect(saveThoughts).toHaveLength(1);
  });

  it('emits reflect thought for the hook', async () => {
    process.env['CONTEXT_WINDOW'] = '100';
    delete process.env['MAX_CONTEXT_TOKENS'];

    const knowledgeStoreDef: ToolDefinition = {
      type: 'function',
      function: { name: 'knowledge-store', description: 'Store knowledge', parameters: {} },
    };
    mockGetToolDefinitions.mockReturnValue([knowledgeStoreDef]);

    mockChat
      .mockResolvedValueOnce(successResponse('Saving...'))
      .mockResolvedValueOnce(successResponse('Done'));

    const loop = new ReasoningLoop(mockLlm, mockTools, mockCentrifugo, manifest);
    const result = await loop.run({ taskId: 'task-1', task: 'Big task: ' + 'word '.repeat(20) });

    // publishThought should have been called with the save-reminder reflect
    const publishCalls = mockPublishThought.mock.calls;
    const hookCall = publishCalls.find(
      (c: unknown[]) => c[0] === 'reflect' && (c[1] as string).includes('save-reminder'),
    );
    expect(hookCall).toBeDefined();
  });
});

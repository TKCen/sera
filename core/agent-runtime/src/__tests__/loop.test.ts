import { describe, it, expect, vi } from 'vitest';
import { ReasoningLoop } from '../loop.js';
import { ScriptedLLMClient, StaticToolExecutor, createMockPublisher } from './testHelpers.js';
import type { RuntimeManifest } from '../manifest.js';
import type { ToolDefinition } from '../llmClient.js';

describe('ReasoningLoop E2E', () => {
  const mockManifest: RuntimeManifest = {
    apiVersion: 'sera.io/v1',
    kind: 'Agent',
    metadata: {
      name: 'test-agent',
      displayName: 'Test Agent',
      icon: 'bot',
      circle: 'test',
      tier: 2,
    },
    identity: {
      role: 'Tester',
      description: 'An agent for testing',
    },
    model: {
      provider: 'openai',
      name: 'gpt-4o',
      temperature: 0,
    },
    tools: { allowed: ['echo'] },
  };

  const echoTool: ToolDefinition = {
    type: 'function',
    function: {
      name: 'echo',
      description: 'Echo back input',
      parameters: {
        type: 'object',
        properties: { text: { type: 'string' } },
        required: ['text'],
      },
    },
  };

  it('completes basic turn with text response', async () => {
    const llm = new ScriptedLLMClient([
      { content: 'Hello, how can I help you today?' },
    ]);
    const tools = new StaticToolExecutor();
    const publisher = createMockPublisher();
    const loop = new ReasoningLoop(llm, tools, publisher, mockManifest);

    const output = await loop.run({
      taskId: 'task-1',
      task: 'Say hello',
    });

    expect(output.result).toBe('Hello, how can I help you today?');
    expect(output.exitReason).toBe('success');
    expect(llm.getCallCount()).toBe(1);
    expect(publisher.publishThought).toHaveBeenCalledWith('observe', expect.stringContaining('Received task'), 0, undefined);
    expect(publisher.publishThought).toHaveBeenCalledWith('reflect', expect.stringContaining('Completed task after 1 iteration(s)'), 1, undefined);
  });

  it('handles tool call cycle', async () => {
    const llm = new ScriptedLLMClient([
      {
        content: 'I will echo that.',
        toolCalls: [
          { id: 'call_1', type: 'function', function: { name: 'echo', arguments: '{"text": "hello"}' } },
        ],
      },
      { content: 'Echoed: hello' },
    ]);
    const tools = new StaticToolExecutor().register(echoTool, (args) => {
      const parsed = JSON.parse(args);
      return parsed.text;
    });
    const publisher = createMockPublisher();
    const loop = new ReasoningLoop(llm, tools, publisher, mockManifest);

    const output = await loop.run({
      taskId: 'task-2',
      task: 'Echo hello',
    });

    expect(output.result).toBe('Echoed: hello');
    expect(output.exitReason).toBe('success');
    expect(llm.getCallCount()).toBe(2);
    expect(publisher.publishThought).toHaveBeenCalledWith('act', expect.stringContaining('Calling tool: echo'), 1, expect.any(Object));
    expect(publisher.publishThought).toHaveBeenCalledWith('reflect', expect.stringContaining('Tool result: hello'), 1, undefined);
    expect(publisher.publishThought).toHaveBeenCalledWith('reflect', expect.stringContaining('Completed task after 2 iteration(s)'), 2, undefined);
  });

  it('handles unknown tool call by returning error to LLM', async () => {
    const llm = new ScriptedLLMClient([
      {
        content: 'Trying unknown tool.',
        toolCalls: [
          { id: 'call_2', type: 'function', function: { name: 'unknown', arguments: '{}' } },
        ],
      },
      { content: 'It failed as expected.' },
    ]);
    const tools = new StaticToolExecutor();
    const publisher = createMockPublisher();
    const loop = new ReasoningLoop(llm, tools, publisher, mockManifest);

    const output = await loop.run({
      taskId: 'task-3',
      task: 'Call unknown tool',
    });

    expect(output.result).toBe('It failed as expected.');
    expect(llm.getCallCount()).toBe(2);
    expect(publisher.publishThought).toHaveBeenCalledWith('reflect', expect.stringContaining('Tool result: Error: Unknown tool "unknown"'), 1, undefined);
    expect(publisher.publishThought).toHaveBeenCalledWith('reflect', expect.stringContaining('Completed task after 2 iteration(s)'), 2, undefined);
  });
});

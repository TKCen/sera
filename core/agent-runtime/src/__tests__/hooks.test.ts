import { describe, it, expect, vi } from 'vitest';
import { HookRunner } from '../tools/hooks.js';
import { RuntimeToolExecutor } from '../tools/executor.js';
import type { RuntimeManifest } from '../manifest.js';
import type { ToolCall } from '../llmClient.js';

describe('HookRunner', () => {
  it('should allow tool execution when hook returns 0', async () => {
    const runner = new HookRunner([{ command: 'exit 0', events: ['before_tool_call'] }]);
    const result = await runner.beforeToolCall({
      toolName: 'test-tool',
      args: { foo: 'bar' },
      agentName: 'test-agent',
      agentInstanceId: 'test-id',
      tier: 1,
    });
    expect(result.status).toBe('allow');
  });

  it('should deny tool execution when hook returns 2', async () => {
    const runner = new HookRunner([
      { command: 'echo "Denied!" >&2; exit 2', events: ['before_tool_call'] },
    ]);
    const result = await runner.beforeToolCall({
      toolName: 'test-tool',
      args: { foo: 'bar' },
      agentName: 'test-agent',
      agentInstanceId: 'test-id',
      tier: 1,
    });
    expect(result.status).toBe('deny');
    expect(result.message).toBe('Denied!');
  });

  it('should modify arguments when before_tool_call hook outputs JSON', async () => {
    const runner = new HookRunner([
      { command: 'echo \'{"foo": "modified"}\'', events: ['before_tool_call'] },
    ]);
    const result = await runner.beforeToolCall({
      toolName: 'test-tool',
      args: { foo: 'bar' },
      agentName: 'test-agent',
      agentInstanceId: 'test-id',
      tier: 1,
    });
    expect(result.modifiedArgs).toEqual({ foo: 'modified' });
  });

  it('should modify result when after_tool_call hook outputs content', async () => {
    const runner = new HookRunner([
      { command: 'echo "Modified Result"', events: ['after_tool_call'] },
    ]);
    const result = await runner.afterToolCall({
      toolName: 'test-tool',
      args: { foo: 'bar' },
      result: 'Original Result',
      agentName: 'test-agent',
      agentInstanceId: 'test-id',
      tier: 1,
    });
    expect(result.modifiedResult).toBe('Modified Result');
  });

  it('should provide context via environment variables', async () => {
    // We use a hook that prints an env var to stdout
    const runner = new HookRunner([
      { command: 'echo $HOOK_TOOL_NAME', events: ['before_tool_call'] },
    ]);
    const result = await runner.beforeToolCall({
      toolName: 'test-tool',
      args: { foo: 'bar' },
      agentName: 'test-agent',
      agentInstanceId: 'test-id',
      tier: 1,
    });
    // The modifiedArgs check will fail because it's not JSON, but we can verify it's skipped or handled
    // Actually our implementation tries to parse JSON, if it fails it might just return status 'allow'
    // Let's make it output valid JSON
    const runner2 = new HookRunner([
      { command: 'echo "{\\\"name\\\": \\\"$HOOK_TOOL_NAME\\\"}"', events: ['before_tool_call'] },
    ]);
    const result2 = await runner2.beforeToolCall({
      toolName: 'my-special-tool',
      args: {},
      agentName: 'test-agent',
      agentInstanceId: 'test-id',
      tier: 1,
    });
    expect(result2.modifiedArgs).toEqual({ name: 'my-special-tool' });
  });
});

describe('RuntimeToolExecutor with Hooks', () => {
  const mockManifest: RuntimeManifest = {
    apiVersion: 'sera/v1',
    kind: 'Agent',
    metadata: { name: 'test-agent', displayName: 'Test Agent', icon: '', circle: '', tier: 1 },
    identity: { role: 'tester', description: 'testing hooks' },
    model: { provider: 'test', name: 'test-model' },
    tools: {
      allowed: ['file-list'],
      hooks: [
        { command: 'echo "{\\\"path\\\": \\\"/modified/path\\\"}"', events: ['before_tool_call'] },
      ],
    },
  };

  it('should apply hooks during executeTool', async () => {
    const executor = new RuntimeToolExecutor('/workspace', 1, mockManifest);

    // Mock the actual tool implementation to avoid file system access if possible,
    // or just use a tool that is easy to mock.
    // Since file-list is a built-in, we might need to mock the fs module.

    const toolCall: ToolCall = {
      id: 'call_1',
      type: 'function',
      function: {
        name: 'file-list',
        arguments: JSON.stringify({ path: '/original/path' }),
      },
    };

    const result = await executor.executeTool(toolCall);

    // We expect the arguments to have been modified by the hook
    // and passed to fileList. Since fileList will fail if the directory doesn't exist,
    // we check the error message for the modified path.
    expect(result.message.content).toContain('/modified/path');
  });

  it('should handle hook denial', async () => {
    const denyManifest: RuntimeManifest = {
      ...mockManifest,
      tools: {
        allowed: ['file-list'],
        hooks: [{ command: 'exit 2', events: ['before_tool_call'] }],
      },
    };
    const executor = new RuntimeToolExecutor('/workspace', 1, denyManifest);
    const toolCall: ToolCall = {
      id: 'call_1',
      type: 'function',
      function: {
        name: 'file-list',
        arguments: JSON.stringify({ path: '/' }),
      },
    };

    const result = await executor.executeTool(toolCall);
    expect(result.message.content).toContain('tool_denied');
  });

  it('should enforce built-in capability check', async () => {
    const restrictManifest: RuntimeManifest = {
      ...mockManifest,
      tools: {
        allowed: ['file-read'], // file-list not allowed
        hooks: [],
      },
    };
    const executor = new RuntimeToolExecutor('/workspace', 1, restrictManifest);
    const toolCall: ToolCall = {
      id: 'call_1',
      type: 'function',
      function: {
        name: 'file-list',
        arguments: JSON.stringify({ path: '/' }),
      },
    };

    const result = await executor.executeTool(toolCall);
    expect(result.message.content).toContain('tool_not_permitted');
  });
});

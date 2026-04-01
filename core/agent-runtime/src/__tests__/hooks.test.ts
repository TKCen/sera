import { describe, it, expect, vi, beforeEach } from 'vitest';
import { HookRunner } from '../hooks.js';
import { spawnSync } from 'child_process';

vi.mock('child_process', () => ({
  spawnSync: vi.fn(),
}));

describe('HookRunner', () => {
  const runner = new HookRunner('/tmp');
  const toolName = 'test-tool';
  const toolInput = JSON.stringify({ arg1: 'val1' });

  beforeEach(() => {
    vi.clearAllMocks();
  });

  it('should allow execution when hook exits 0', async () => {
    (spawnSync as any).mockReturnValue({
      status: 0,
      stdout: 'All good',
      stderr: '',
    });

    const result = await runner.runHooks('PreToolUse', ['test-hook'], toolName, toolInput);

    expect(result.allowed).toBe(true);
    expect(result.feedback).toBe('All good');
    expect(result.warning).toBeUndefined();
    expect(spawnSync).toHaveBeenCalledWith('bash', ['-c', 'test-hook'], expect.any(Object));
  });

  it('should deny execution when hook exits 2', async () => {
    (spawnSync as any).mockReturnValue({
      status: 2,
      stdout: 'Policy violation',
      stderr: '',
    });

    const result = await runner.runHooks('PreToolUse', ['test-hook'], toolName, toolInput);

    expect(result.allowed).toBe(false);
    expect(result.feedback).toBe('Policy violation');
  });

  it('should treat other non-zero exits as warnings', async () => {
    (spawnSync as any).mockReturnValue({
      status: 1,
      stdout: 'Be careful',
      stderr: 'Some stderr',
    });

    const result = await runner.runHooks('PreToolUse', ['test-hook'], toolName, toolInput);

    expect(result.allowed).toBe(true);
    expect(result.warning).toBe('Be careful');
    expect(result.feedback).toBeUndefined();
  });

  it('should combine feedback from multiple hooks', async () => {
    (spawnSync as any)
      .mockReturnValueOnce({ status: 0, stdout: 'Feedback 1' })
      .mockReturnValueOnce({ status: 0, stdout: 'Feedback 2' });

    const result = await runner.runHooks('PostToolUse', ['hook1', 'hook2'], toolName, toolInput, 'output');

    expect(result.allowed).toBe(true);
    expect(result.feedback).toBe('Feedback 1\nFeedback 2');
  });

  it('should combine warnings from multiple hooks', async () => {
    (spawnSync as any)
      .mockReturnValueOnce({ status: 1, stdout: 'Warning 1' })
      .mockReturnValueOnce({ status: 3, stdout: 'Warning 2' });

    const result = await runner.runHooks('PreToolUse', ['hook1', 'hook2'], toolName, toolInput);

    expect(result.allowed).toBe(true);
    expect(result.warning).toBe('Warning 1\nWarning 2');
  });

  it('should pass correct environment variables and stdin', async () => {
    (spawnSync as any).mockReturnValue({ status: 0 });

    await runner.runHooks('PostToolUse', ['test-hook'], toolName, toolInput, 'some-output', true);

    expect(spawnSync).toHaveBeenCalledWith(
      'bash',
      ['-c', 'test-hook'],
      expect.objectContaining({
        input: JSON.stringify({
          event: 'PostToolUse',
          toolName,
          toolInput: { arg1: 'val1' },
          toolOutput: 'some-output',
          isError: true,
        }),
        env: expect.objectContaining({
          HOOK_EVENT: 'PostToolUse',
          HOOK_TOOL_NAME: toolName,
          HOOK_TOOL_INPUT: toolInput,
          HOOK_TOOL_OUTPUT: 'some-output',
          HOOK_TOOL_IS_ERROR: '1',
        }),
      })
    );
  });

  it('should not crash if hook fails to spawn', async () => {
    (spawnSync as any).mockImplementation(() => {
      throw new Error('Spawn failed');
    });

    const result = await runner.runHooks('PreToolUse', ['test-hook'], toolName, toolInput);

    expect(result.allowed).toBe(true);
    expect(result.feedback).toBeUndefined();
  });

  it('should handle JSON in tool output for stdin payload', async () => {
    (spawnSync as any).mockReturnValue({ status: 0 });
    const jsonOutput = JSON.stringify({ res: 'ok' });

    await runner.runHooks('PostToolUse', ['test-hook'], toolName, toolInput, jsonOutput);

    const call = (spawnSync as any).mock.calls[0];
    const payload = JSON.parse(call[2].input);
    expect(payload.toolOutput).toEqual({ res: 'ok' });
  });
});

import { describe, it, expect, vi, beforeEach } from 'vitest';
import { HookRunner } from '../hooks.js';
import { spawn } from 'child_process';
import { EventEmitter } from 'events';

vi.mock('child_process', () => ({
  spawn: vi.fn(),
}));

describe('HookRunner', () => {
  beforeEach(() => {
    vi.clearAllMocks();
  });

  const mockSpawn = (code: number, stdout: string, stderr: string = '') => {
    const child = new EventEmitter() as any;
    child.stdout = new EventEmitter();
    child.stderr = new EventEmitter();
    child.stdin = new EventEmitter() as any;
    child.stdin.write = vi.fn();
    child.stdin.end = vi.fn();

    vi.mocked(spawn).mockReturnValue(child);

    // Simulate process execution
    setImmediate(() => {
      if (stdout) child.stdout.emit('data', Buffer.from(stdout));
      if (stderr) child.stderr.emit('data', Buffer.from(stderr));
      child.emit('exit', code);
    });

    return child;
  };

  it('should allow execution when no hooks are defined', async () => {
    const runner = new HookRunner();
    const result = await runner.run('PreToolUse', { toolName: 'test', toolInput: '{}' });
    expect(result.status).toBe('allow');
  });

  it('should run pre-tool hooks and return allow', async () => {
    mockSpawn(0, 'Hook allowed');
    const runner = new HookRunner({ preToolUse: ['test-hook'] });
    const result = await runner.run('PreToolUse', { toolName: 'test', toolInput: '{}' });

    expect(result.status).toBe('allow');
    expect(result.feedback).toBe('Hook allowed');
    expect(spawn).toHaveBeenCalledWith('/bin/sh', ['-c', 'test-hook'], expect.anything());
  });

  it('should deny execution when hook exits with 2', async () => {
    mockSpawn(2, 'Access denied');
    const runner = new HookRunner({ preToolUse: ['test-hook'] });
    const result = await runner.run('PreToolUse', { toolName: 'test', toolInput: '{}' });

    expect(result.status).toBe('deny');
    expect(result.feedback).toBe('Access denied');
  });

  it('should warn when hook exits with non-zero non-2', async () => {
    mockSpawn(1, 'Warning message');
    const runner = new HookRunner({ preToolUse: ['test-hook'] });
    const result = await runner.run('PreToolUse', { toolName: 'test', toolInput: '{}' });

    expect(result.status).toBe('warn');
    expect(result.feedback).toBe('Warning message');
  });

  it('should concatenate feedback from multiple hooks', async () => {
    let callCount = 0;
    vi.mocked(spawn).mockImplementation((() => {
      const child = new EventEmitter() as any;
      child.stdout = new EventEmitter();
      child.stderr = new EventEmitter();
      child.stdin = new EventEmitter() as any;
      child.stdin.write = vi.fn();
      child.stdin.end = vi.fn();

      const count = ++callCount;
      setImmediate(() => {
        child.stdout.emit('data', Buffer.from(`Feedback ${count}`));
        child.emit('exit', 0);
      });
      return child;
    }) as any);

    const runner = new HookRunner({ preToolUse: ['hook1', 'hook2'] });
    const result = await runner.run('PreToolUse', { toolName: 'test', toolInput: '{}' });

    expect(result.status).toBe('allow');
    expect(result.feedback).toBe('Feedback 1\n\nFeedback 2');
  });

  it('should stop and deny if the first hook denies', async () => {
    vi.mocked(spawn).mockImplementation(((shell: string, args: string[]) => {
      const cmd = args[1];
      const child = new EventEmitter() as any;
      child.stdout = new EventEmitter();
      child.stderr = new EventEmitter();
      child.stdin = new EventEmitter() as any;
      child.stdin.write = vi.fn();
      child.stdin.end = vi.fn();

      setImmediate(() => {
        if (cmd === 'hook1') {
          child.stdout.emit('data', Buffer.from('Denied by 1'));
          child.emit('exit', 2);
        } else {
          child.stdout.emit('data', Buffer.from('Allowed by 2'));
          child.emit('exit', 0);
        }
      });
      return child;
    }) as any);

    const runner = new HookRunner({ preToolUse: ['hook1', 'hook2'] });
    const result = await runner.run('PreToolUse', { toolName: 'test', toolInput: '{}' });

    expect(result.status).toBe('deny');
    expect(result.feedback).toBe('Denied by 1');
    expect(spawn).toHaveBeenCalledTimes(1);
  });

  it('should pass correct environment variables and isolate from process.env', async () => {
    process.env['SENSITIVE_KEY'] = 'secret-value';
    mockSpawn(0, '');
    const runner = new HookRunner({ preToolUse: ['test-hook'] });
    await runner.run('PreToolUse', {
      toolName: 'test-tool',
      toolInput: '{"arg": 1}',
    });

    const spawnCall = vi.mocked(spawn).mock.calls[0];
    const env = spawnCall[2].env;
    expect(env.HOOK_EVENT).toBe('PreToolUse');
    expect(env.HOOK_TOOL_NAME).toBe('test-tool');
    expect(env.HOOK_TOOL_INPUT).toBe('{"arg": 1}');
    expect(env.HOOK_TOOL_IS_ERROR).toBe('0');
    expect(env.PATH).toBeDefined();
    expect(env.SENSITIVE_KEY).toBeUndefined();
    delete process.env['SENSITIVE_KEY'];
  });

  it('should handle post-tool hooks with error status', async () => {
    mockSpawn(0, 'Audit log');
    const runner = new HookRunner({ postToolUse: ['audit-hook'] });
    const result = await runner.run('PostToolUse', {
      toolName: 'test',
      toolInput: '{}',
      toolOutput: 'Error: something failed',
      isError: true
    });

    expect(result.status).toBe('allow');
    expect(result.feedback).toBe('Audit log');

    const env = vi.mocked(spawn).mock.calls[0][2].env;
    expect(env.HOOK_EVENT).toBe('PostToolUse');
    expect(env.HOOK_TOOL_OUTPUT).toBe('Error: something failed');
    expect(env.HOOK_TOOL_IS_ERROR).toBe('1');
  });

  it('should handle spawn errors gracefully', async () => {
    const child = new EventEmitter() as any;
    child.stdin = new EventEmitter() as any;
    child.stdin.write = vi.fn();
    child.stdin.end = vi.fn();
    vi.mocked(spawn).mockReturnValue(child);

    setImmediate(() => {
      child.emit('error', new Error('Spawn failed'));
    });

    const runner = new HookRunner({ preToolUse: ['test-hook'] });
    const result = await runner.run('PreToolUse', { toolName: 'test', toolInput: '{}' });

    expect(result.status).toBe('warn');
    expect(result.feedback).toContain('Hook execution failed: Spawn failed');
  });

  it('should timeout if hook runs too long', async () => {
    vi.useFakeTimers();
    const child = new EventEmitter() as any;
    child.stdout = new EventEmitter();
    child.stderr = new EventEmitter();
    child.stdin = new EventEmitter() as any;
    child.stdin.write = vi.fn();
    child.stdin.end = vi.fn();
    child.kill = vi.fn();
    vi.mocked(spawn).mockReturnValue(child);

    const runner = new HookRunner({ preToolUse: ['long-hook'] });
    const runPromise = runner.run('PreToolUse', { toolName: 'test', toolInput: '{}' });

    await vi.advanceTimersByTimeAsync(30001);

    const result = await runPromise;
    expect(result.status).toBe('warn');
    expect(result.feedback).toContain('Hook execution timed out');
    expect(child.kill).toHaveBeenCalledWith('SIGKILL');
    vi.useRealTimers();
  });
});

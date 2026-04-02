import { describe, it, expect, beforeEach, afterEach } from 'vitest';
import fs from 'fs';
import path from 'path';
import os from 'os';
import { RuntimeToolExecutor, PermissionDeniedError, NotPermittedError } from '../tools/index.js';
import type { ToolCall } from '../llmClient.js';

describe('RuntimeToolExecutor', () => {
  let tempDir: string;
  let executor: RuntimeToolExecutor;

  beforeEach(() => {
    tempDir = fs.mkdtempSync(path.join(os.tmpdir(), 'sera-tools-test-'));
    executor = new RuntimeToolExecutor(tempDir, 2); // tier-2 by default
  });

  afterEach(() => {
    try {
      if (fs.existsSync(tempDir)) {
        fs.rmSync(tempDir, { recursive: true, force: true });
      }
    } catch (err) {
      console.warn('Cleanup failed:', err);
    }
  });

  describe('executeTool() — file-read', () => {
    it('reads an existing file', async () => {
      fs.writeFileSync(path.join(tempDir, 'test.txt'), 'hello world', 'utf-8');
      const result = await executor.executeTool(makeCall('file-read', { path: 'test.txt' }));
      expect(result.message.role).toBe('tool');
      expect(result.message.content).toBe('hello world');
    });

    it('returns error for missing file', async () => {
      const result = await executor.executeTool(makeCall('file-read', { path: 'missing.txt' }));
      expect(result.message.content).toContain('Error: File not found');
    });

    it('blocks path traversal', async () => {
      const result = await executor.executeTool(makeCall('file-read', { path: '../../etc/passwd' }));
      expect(result.message.content).toContain('Path traversal blocked');
    });
  });

  describe('executeTool() — file-write', () => {
    it('creates a file with content', async () => {
      const result = await executor.executeTool(makeCall('file-write', { path: 'new.txt', content: 'data' }));
      expect(result.message.content).toContain('File written');
      expect(fs.readFileSync(path.join(tempDir, 'new.txt'), 'utf-8')).toBe('data');
    });

    it('creates parent directories automatically', async () => {
      const result = await executor.executeTool(makeCall('file-write', { path: 'sub/dir/file.txt', content: 'nested' }));
      expect(result.message.content).toContain('File written');
      expect(fs.existsSync(path.join(tempDir, 'sub', 'dir', 'file.txt'))).toBe(true);
    });
  });

  describe('executeTool() — file-list', () => {
    it('lists directory contents with type and size', async () => {
      fs.writeFileSync(path.join(tempDir, 'a.txt'), 'hello');
      fs.mkdirSync(path.join(tempDir, 'subdir'));
      const result = await executor.executeTool(makeCall('file-list', {}));
      expect(result.message.content).toContain('a.txt');
      expect(result.message.content).toContain('subdir');
      expect(result.message.content).toContain('file');
      expect(result.message.content).toContain('dir');
    });

    it('returns error for non-existent directory', async () => {
      const result = await executor.executeTool(makeCall('file-list', { path: 'nonexistent' }));
      expect(result.message.content).toContain('Error: Directory not found');
    });

    it('returns (empty directory) for empty dirs', async () => {
      fs.mkdirSync(path.join(tempDir, 'empty'));
      const result = await executor.executeTool(makeCall('file-list', { path: 'empty' }));
      expect(result.message.content).toBe('(empty directory)');
    });
  });

  describe('executeTool() — file-delete', () => {
    it('deletes an existing file', async () => {
      fs.writeFileSync(path.join(tempDir, 'del.txt'), 'bye');
      const result = await executor.executeTool(makeCall('file-delete', { path: 'del.txt' }));
      expect(result.message.content).toContain('Deleted file');
      expect(fs.existsSync(path.join(tempDir, 'del.txt'))).toBe(false);
    });

    it('refuses to delete non-empty directory without recursive:true', async () => {
      fs.mkdirSync(path.join(tempDir, 'notempty'));
      fs.writeFileSync(path.join(tempDir, 'notempty', 'x.txt'), '');
      const result = await executor.executeTool(makeCall('file-delete', { path: 'notempty' }));
      expect(result.message.content).toContain('Directory not empty');
    });

    it('deletes non-empty directory with recursive:true', async () => {
      fs.mkdirSync(path.join(tempDir, 'notempty'));
      fs.writeFileSync(path.join(tempDir, 'notempty', 'x.txt'), '');
      const result = await executor.executeTool(makeCall('file-delete', { path: 'notempty', recursive: true }));
      expect(result.message.content).toContain('Deleted directory');
      expect(fs.existsSync(path.join(tempDir, 'notempty'))).toBe(false);
    });

    it('returns error for non-existent path', async () => {
      const result = await executor.executeTool(makeCall('file-delete', { path: 'ghost.txt' }));
      expect(result.message.content).toContain('Error: File not found');
    });
  });

  describe('executeTool() — shell-exec', () => {
    it('executes a simple command', async () => {
      const result = await executor.executeTool(makeCall('shell-exec', { command: 'echo hello' }));
      expect(result.message.content.trim()).toBe('hello');
    });

    it('returns exit code and stderr on failure', async () => {
      const result = await executor.executeTool(makeCall('shell-exec', { command: 'exit 1' }));
      expect(result.message.content).toContain('Exit code: 1');
    });

    it('is blocked on tier-1 agents', async () => {
      const tier1Executor = new RuntimeToolExecutor(tempDir, 1);
      const result = await tier1Executor.executeTool(makeCall('shell-exec', { command: 'echo hi' }));
      expect(result.message.content).toContain('tier-1');
      expect(result.message.content).toContain('Error');
    });
  });

  describe('executeTool() — argument parsing', () => {
    it('handles markdown-wrapped JSON arguments', async () => {
      fs.writeFileSync(path.join(tempDir, 'md.txt'), 'content');
      const call: ToolCall = {
        id: 'call_md',
        type: 'function',
        function: {
          name: 'file-read',
          arguments: '```json\n{"path": "md.txt"}\n```',
        },
      };
      const result = await executor.executeTool(call);
      expect(result.message.content).toBe('content');
    });
  });

  describe('proxy-aware resolution (Story 3.10)', () => {
    it('attempts proxy when file-read targets a path outside workspace', async () => {
      // Set up env vars for proxy
      const origUrl = process.env['SERA_CORE_URL'];
      const origToken = process.env['SERA_IDENTITY_TOKEN'];
      process.env['SERA_CORE_URL'] = 'http://sera-core:3001';
      process.env['SERA_IDENTITY_TOKEN'] = 'test-jwt-token';

      try {
        // Create a fresh executor so it picks up the env vars
        const proxyExecutor = new RuntimeToolExecutor(tempDir, 2);
        const result = await proxyExecutor.executeTool(
          makeCall('file-read', { path: '/outside/workspace/file.txt' }),
        );

        // Should attempt proxy (curl will fail in test env, but shouldn't throw PermissionDeniedError)
        expect(result.message.role).toBe('tool');
        // The result will be an error from proxy failure (curl not finding sera-core), not a path traversal error
        expect(result.message.content).not.toContain('Path traversal blocked');
      } finally {
        if (origUrl !== undefined) process.env['SERA_CORE_URL'] = origUrl;
        else delete process.env['SERA_CORE_URL'];
        if (origToken !== undefined) process.env['SERA_IDENTITY_TOKEN'] = origToken;
        else delete process.env['SERA_IDENTITY_TOKEN'];
      }
    });

    it('shell-exec returns path_requires_restart for outside-workspace paths', async () => {
      const origUrl = process.env['SERA_CORE_URL'];
      const origToken = process.env['SERA_IDENTITY_TOKEN'];
      process.env['SERA_CORE_URL'] = 'http://sera-core:3001';
      process.env['SERA_IDENTITY_TOKEN'] = 'test-jwt-token';

      try {
        const proxyExecutor = new RuntimeToolExecutor(tempDir, 2);
        const result = await proxyExecutor.executeTool(
          makeCall('shell-exec', { command: 'cat /outside/secret.txt' }),
        );

        expect(result.message.role).toBe('tool');
        const parsed = JSON.parse(result.message.content);
        expect(parsed.error).toBe('path_requires_restart');
        expect(parsed.hint).toContain('persistent grant');
      } finally {
        if (origUrl !== undefined) process.env['SERA_CORE_URL'] = origUrl;
        else delete process.env['SERA_CORE_URL'];
        if (origToken !== undefined) process.env['SERA_IDENTITY_TOKEN'] = origToken;
        else delete process.env['SERA_IDENTITY_TOKEN'];
      }
    });

    it('shell-exec runs normally for workspace-only commands when proxy is available', async () => {
      const origUrl = process.env['SERA_CORE_URL'];
      const origToken = process.env['SERA_IDENTITY_TOKEN'];
      process.env['SERA_CORE_URL'] = 'http://sera-core:3001';
      process.env['SERA_IDENTITY_TOKEN'] = 'test-jwt-token';

      try {
        const proxyExecutor = new RuntimeToolExecutor(tempDir, 2);
        const result = await proxyExecutor.executeTool(
          makeCall('shell-exec', { command: 'echo hello' }),
        );

        expect(result.message.role).toBe('tool');
        expect(result.message.content.trim()).toBe('hello');
      } finally {
        if (origUrl !== undefined) process.env['SERA_CORE_URL'] = origUrl;
        else delete process.env['SERA_CORE_URL'];
        if (origToken !== undefined) process.env['SERA_IDENTITY_TOKEN'] = origToken;
        else delete process.env['SERA_IDENTITY_TOKEN'];
      }
    });

    it('still blocks path traversal when proxy is NOT available', async () => {
      // Ensure no proxy env vars
      const origUrl = process.env['SERA_CORE_URL'];
      const origToken = process.env['SERA_IDENTITY_TOKEN'];
      delete process.env['SERA_CORE_URL'];
      delete process.env['SERA_IDENTITY_TOKEN'];

      try {
        const noProxyExecutor = new RuntimeToolExecutor(tempDir, 2);
        const result = await noProxyExecutor.executeTool(
          makeCall('file-read', { path: '/outside/workspace/file.txt' }),
        );

        expect(result.message.content).toContain('Path traversal blocked');
      } finally {
        if (origUrl !== undefined) process.env['SERA_CORE_URL'] = origUrl;
        if (origToken !== undefined) process.env['SERA_IDENTITY_TOKEN'] = origToken;
      }
    });
  });

  describe('executeToolCalls() — parallel execution', () => {
    it('executes multiple read tools and preserves result order', async () => {
      // Create 3 files and read them all in one batch
      fs.writeFileSync(path.join(tempDir, 'a.txt'), 'content-a');
      fs.writeFileSync(path.join(tempDir, 'b.txt'), 'content-b');
      fs.writeFileSync(path.join(tempDir, 'c.txt'), 'content-c');

      const calls = [
        makeCallWithId('call_1', 'file-read', { path: 'a.txt' }),
        makeCallWithId('call_2', 'file-read', { path: 'b.txt' }),
        makeCallWithId('call_3', 'file-read', { path: 'c.txt' }),
      ];

      const results = await executor.executeToolCalls(calls);
      expect(results).toHaveLength(3);
      expect(results[0]!.message.content).toBe('content-a');
      expect(results[1]!.message.content).toBe('content-b');
      expect(results[2]!.message.content).toBe('content-c');
    });

    it('write tools complete without data races', async () => {
      const calls = [
        makeCallWithId('call_1', 'file-write', { path: 'out1.txt', content: 'data1' }),
        makeCallWithId('call_2', 'file-write', { path: 'out2.txt', content: 'data2' }),
      ];

      const results = await executor.executeToolCalls(calls);
      expect(results).toHaveLength(2);
      expect(results[0]!.message.content).toContain('File written');
      expect(results[1]!.message.content).toContain('File written');
      expect(fs.readFileSync(path.join(tempDir, 'out1.txt'), 'utf-8')).toBe('data1');
      expect(fs.readFileSync(path.join(tempDir, 'out2.txt'), 'utf-8')).toBe('data2');
    });

    it('individual failure does not block other tools', async () => {
      fs.writeFileSync(path.join(tempDir, 'good.txt'), 'ok');
      const calls = [
        makeCallWithId('call_1', 'file-read', { path: 'nonexistent.txt' }),
        makeCallWithId('call_2', 'file-read', { path: 'good.txt' }),
      ];

      const results = await executor.executeToolCalls(calls);
      expect(results).toHaveLength(2);
      expect(results[0]!.message.content).toContain('Error');
      expect(results[1]!.message.content).toBe('ok');
    });

    it('single call takes fast path', async () => {
      fs.writeFileSync(path.join(tempDir, 'solo.txt'), 'alone');
      const results = await executor.executeToolCalls([
        makeCallWithId('call_1', 'file-read', { path: 'solo.txt' }),
      ]);
      expect(results).toHaveLength(1);
      expect(results[0]!.message.content).toBe('alone');
    });
  });

  describe('getToolDefinitions()', () => {
    it('returns all 15 built-in tools when no filter given', () => {
      const tools = executor.getToolDefinitions();
      expect(tools.length).toBe(15);
      const names = tools.map((t) => t.function.name).sort();
      expect(names).toEqual([
        'code-eval',
        'file-delete',
        'file-list',
        'file-read',
        'file-write',
        'glob',
        'grep',
        'http-request',
        'image-view',
        'pdf-read',
        'read_file',
        'run-tool',
        'shell-exec',
        'spawn-subagent',
        'tool-search',
      ]);
    });

    it('filters to allowed list', () => {
      const tools = executor.getToolDefinitions(['file-read', 'file-write']);
      expect(tools.length).toBe(2);
      expect(tools.map((t) => t.function.name).sort()).toEqual(['file-read', 'file-write']);
    });
  });
});

function makeCall(name: string, args: Record<string, unknown>): ToolCall {
  return {
    id: 'call_test',
    type: 'function',
    function: { name, arguments: JSON.stringify(args) },
  };
}

function makeCallWithId(id: string, name: string, args: Record<string, unknown>): ToolCall {
  return {
    id,
    type: 'function',
    function: { name, arguments: JSON.stringify(args) },
  };
}

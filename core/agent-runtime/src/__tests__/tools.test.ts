import { describe, it, expect, beforeEach, afterEach } from 'vitest';
import fs from 'fs';
import path from 'path';
import os from 'os';
import { RuntimeToolExecutor, PermissionDeniedError, NotPermittedError } from '../tools.js';
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
    it('reads an existing file', () => {
      fs.writeFileSync(path.join(tempDir, 'test.txt'), 'hello world', 'utf-8');
      const result = executor.executeTool(makeCall('file-read', { path: 'test.txt' }));
      expect(result.role).toBe('tool');
      expect(result.content).toBe('hello world');
    });

    it('returns error for missing file', () => {
      const result = executor.executeTool(makeCall('file-read', { path: 'missing.txt' }));
      expect(result.content).toContain('Error: File not found');
    });

    it('blocks path traversal', () => {
      const result = executor.executeTool(makeCall('file-read', { path: '../../etc/passwd' }));
      expect(result.content).toContain('Path traversal blocked');
    });
  });

  describe('executeTool() — file-write', () => {
    it('creates a file with content', () => {
      const result = executor.executeTool(makeCall('file-write', { path: 'new.txt', content: 'data' }));
      expect(result.content).toContain('File written');
      expect(fs.readFileSync(path.join(tempDir, 'new.txt'), 'utf-8')).toBe('data');
    });

    it('creates parent directories automatically', () => {
      const result = executor.executeTool(makeCall('file-write', { path: 'sub/dir/file.txt', content: 'nested' }));
      expect(result.content).toContain('File written');
      expect(fs.existsSync(path.join(tempDir, 'sub', 'dir', 'file.txt'))).toBe(true);
    });
  });

  describe('executeTool() — file-list', () => {
    it('lists directory contents with type and size', () => {
      fs.writeFileSync(path.join(tempDir, 'a.txt'), 'hello');
      fs.mkdirSync(path.join(tempDir, 'subdir'));
      const result = executor.executeTool(makeCall('file-list', {}));
      expect(result.content).toContain('a.txt');
      expect(result.content).toContain('subdir');
      expect(result.content).toContain('file');
      expect(result.content).toContain('dir');
    });

    it('returns error for non-existent directory', () => {
      const result = executor.executeTool(makeCall('file-list', { path: 'nonexistent' }));
      expect(result.content).toContain('Error: Directory not found');
    });

    it('returns (empty directory) for empty dirs', () => {
      fs.mkdirSync(path.join(tempDir, 'empty'));
      const result = executor.executeTool(makeCall('file-list', { path: 'empty' }));
      expect(result.content).toBe('(empty directory)');
    });
  });

  describe('executeTool() — file-delete', () => {
    it('deletes an existing file', () => {
      fs.writeFileSync(path.join(tempDir, 'del.txt'), 'bye');
      const result = executor.executeTool(makeCall('file-delete', { path: 'del.txt' }));
      expect(result.content).toContain('Deleted file');
      expect(fs.existsSync(path.join(tempDir, 'del.txt'))).toBe(false);
    });

    it('refuses to delete non-empty directory without recursive:true', () => {
      fs.mkdirSync(path.join(tempDir, 'notempty'));
      fs.writeFileSync(path.join(tempDir, 'notempty', 'x.txt'), '');
      const result = executor.executeTool(makeCall('file-delete', { path: 'notempty' }));
      expect(result.content).toContain('Directory not empty');
    });

    it('deletes non-empty directory with recursive:true', () => {
      fs.mkdirSync(path.join(tempDir, 'notempty'));
      fs.writeFileSync(path.join(tempDir, 'notempty', 'x.txt'), '');
      const result = executor.executeTool(makeCall('file-delete', { path: 'notempty', recursive: true }));
      expect(result.content).toContain('Deleted directory');
      expect(fs.existsSync(path.join(tempDir, 'notempty'))).toBe(false);
    });

    it('returns error for non-existent path', () => {
      const result = executor.executeTool(makeCall('file-delete', { path: 'ghost.txt' }));
      expect(result.content).toContain('Error: File not found');
    });
  });

  describe('executeTool() — shell-exec', () => {
    it('executes a simple command', () => {
      const result = executor.executeTool(makeCall('shell-exec', { command: 'echo hello' }));
      expect(result.content.trim()).toBe('hello');
    });

    it('returns exit code and stderr on failure', () => {
      const result = executor.executeTool(makeCall('shell-exec', { command: 'exit 1' }));
      expect(result.content).toContain('Exit code: 1');
    });

    it('is blocked on tier-1 agents', () => {
      const tier1Executor = new RuntimeToolExecutor(tempDir, 1);
      const result = tier1Executor.executeTool(makeCall('shell-exec', { command: 'echo hi' }));
      expect(result.content).toContain('tier-1');
      expect(result.content).toContain('Error');
    });
  });

  describe('executeTool() — argument parsing', () => {
    it('handles markdown-wrapped JSON arguments', () => {
      fs.writeFileSync(path.join(tempDir, 'md.txt'), 'content');
      const call: ToolCall = {
        id: 'call_md',
        type: 'function',
        function: {
          name: 'file-read',
          arguments: '```json\n{"path": "md.txt"}\n```',
        },
      };
      const result = executor.executeTool(call);
      expect(result.content).toBe('content');
    });
  });

  describe('proxy-aware resolution (Story 3.10)', () => {
    it('attempts proxy when file-read targets a path outside workspace', () => {
      // Set up env vars for proxy
      const origUrl = process.env['SERA_CORE_URL'];
      const origToken = process.env['SERA_IDENTITY_TOKEN'];
      process.env['SERA_CORE_URL'] = 'http://sera-core:3001';
      process.env['SERA_IDENTITY_TOKEN'] = 'test-jwt-token';

      try {
        // Create a fresh executor so it picks up the env vars
        const proxyExecutor = new RuntimeToolExecutor(tempDir, 2);
        const result = proxyExecutor.executeTool(
          makeCall('file-read', { path: '/outside/workspace/file.txt' }),
        );

        // Should attempt proxy (curl will fail in test env, but shouldn't throw PermissionDeniedError)
        expect(result.role).toBe('tool');
        // The result will be an error from proxy failure (curl not finding sera-core), not a path traversal error
        expect(result.content).not.toContain('Path traversal blocked');
      } finally {
        if (origUrl !== undefined) process.env['SERA_CORE_URL'] = origUrl;
        else delete process.env['SERA_CORE_URL'];
        if (origToken !== undefined) process.env['SERA_IDENTITY_TOKEN'] = origToken;
        else delete process.env['SERA_IDENTITY_TOKEN'];
      }
    });

    it('shell-exec returns path_requires_restart for outside-workspace paths', () => {
      const origUrl = process.env['SERA_CORE_URL'];
      const origToken = process.env['SERA_IDENTITY_TOKEN'];
      process.env['SERA_CORE_URL'] = 'http://sera-core:3001';
      process.env['SERA_IDENTITY_TOKEN'] = 'test-jwt-token';

      try {
        const proxyExecutor = new RuntimeToolExecutor(tempDir, 2);
        const result = proxyExecutor.executeTool(
          makeCall('shell-exec', { command: 'cat /outside/secret.txt' }),
        );

        expect(result.role).toBe('tool');
        const parsed = JSON.parse(result.content);
        expect(parsed.error).toBe('path_requires_restart');
        expect(parsed.hint).toContain('persistent grant');
      } finally {
        if (origUrl !== undefined) process.env['SERA_CORE_URL'] = origUrl;
        else delete process.env['SERA_CORE_URL'];
        if (origToken !== undefined) process.env['SERA_IDENTITY_TOKEN'] = origToken;
        else delete process.env['SERA_IDENTITY_TOKEN'];
      }
    });

    it('shell-exec runs normally for workspace-only commands when proxy is available', () => {
      const origUrl = process.env['SERA_CORE_URL'];
      const origToken = process.env['SERA_IDENTITY_TOKEN'];
      process.env['SERA_CORE_URL'] = 'http://sera-core:3001';
      process.env['SERA_IDENTITY_TOKEN'] = 'test-jwt-token';

      try {
        const proxyExecutor = new RuntimeToolExecutor(tempDir, 2);
        const result = proxyExecutor.executeTool(
          makeCall('shell-exec', { command: 'echo hello' }),
        );

        expect(result.role).toBe('tool');
        expect(result.content.trim()).toBe('hello');
      } finally {
        if (origUrl !== undefined) process.env['SERA_CORE_URL'] = origUrl;
        else delete process.env['SERA_CORE_URL'];
        if (origToken !== undefined) process.env['SERA_IDENTITY_TOKEN'] = origToken;
        else delete process.env['SERA_IDENTITY_TOKEN'];
      }
    });

    it('still blocks path traversal when proxy is NOT available', () => {
      // Ensure no proxy env vars
      const origUrl = process.env['SERA_CORE_URL'];
      const origToken = process.env['SERA_IDENTITY_TOKEN'];
      delete process.env['SERA_CORE_URL'];
      delete process.env['SERA_IDENTITY_TOKEN'];

      try {
        const noProxyExecutor = new RuntimeToolExecutor(tempDir, 2);
        const result = noProxyExecutor.executeTool(
          makeCall('file-read', { path: '/outside/workspace/file.txt' }),
        );

        expect(result.content).toContain('Path traversal blocked');
      } finally {
        if (origUrl !== undefined) process.env['SERA_CORE_URL'] = origUrl;
        if (origToken !== undefined) process.env['SERA_IDENTITY_TOKEN'] = origToken;
      }
    });
  });

  describe('getToolDefinitions()', () => {
    it('returns all 5 built-in tools when no filter given', () => {
      const tools = executor.getToolDefinitions();
      expect(tools.length).toBe(5);
      const names = tools.map((t) => t.function.name).sort();
      expect(names).toEqual(['file-delete', 'file-list', 'file-read', 'file-write', 'shell-exec']);
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

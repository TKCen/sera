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
    fs.rmSync(tempDir, { recursive: true, force: true });
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

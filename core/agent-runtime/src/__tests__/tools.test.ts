import { describe, it, expect, beforeEach, afterEach } from 'vitest';
import fs from 'fs';
import path from 'path';
import os from 'os';
import { RuntimeToolExecutor } from '../tools.js';
import type { ToolCall } from '../llmClient.js';

describe('RuntimeToolExecutor', () => {
  let tempDir: string;
  let executor: RuntimeToolExecutor;

  beforeEach(() => {
    tempDir = fs.mkdtempSync(path.join(os.tmpdir(), 'sera-tools-test-'));
    executor = new RuntimeToolExecutor(tempDir);
  });

  afterEach(() => {
    fs.rmSync(tempDir, { recursive: true, force: true });
  });

  describe('executeTool()', () => {
    it('handles file-read of a real temp file', () => {
      const filePath = path.join(tempDir, 'test.txt');
      fs.writeFileSync(filePath, 'hello world', 'utf-8');

      const toolCall: ToolCall = {
        id: 'call_123',
        type: 'function',
        function: {
          name: 'file-read',
          arguments: JSON.stringify({ path: 'test.txt' }),
        },
      };

      const result = executor.executeTool(toolCall);
      expect(result.role).toBe('tool');
      expect(result.content).toBe('hello world');
    });

    it('handles file-write and creates the file', () => {
      const toolCall: ToolCall = {
        id: 'call_123',
        type: 'function',
        function: {
          name: 'file-write',
          arguments: JSON.stringify({ path: 'new-file.txt', content: 'new content' }),
        },
      };

      const result = executor.executeTool(toolCall);
      expect(result.role).toBe('tool');
      expect(result.content).toContain('File written: new-file.txt');

      const fileContent = fs.readFileSync(path.join(tempDir, 'new-file.txt'), 'utf-8');
      expect(fileContent).toBe('new content');
    });

    it('blocks path traversal (../../etc/passwd)', () => {
      const toolCall: ToolCall = {
        id: 'call_123',
        type: 'function',
        function: {
          name: 'file-read',
          arguments: JSON.stringify({ path: '../../etc/passwd' }),
        },
      };

      const result = executor.executeTool(toolCall);
      expect(result.role).toBe('tool');
      expect(result.content).toContain('Path traversal blocked');
    });

    it('handles shell-exec (echo hello)', () => {
      const toolCall: ToolCall = {
        id: 'call_123',
        type: 'function',
        function: {
          name: 'shell-exec',
          arguments: JSON.stringify({ command: 'echo hello' }),
        },
      };

      const result = executor.executeTool(toolCall);
      expect(result.role).toBe('tool');
      expect(result.content.trim()).toBe('hello');
    });
  });

  describe('getToolDefinitions()', () => {
    it('returns all 3 built-in tools when no filter is given', () => {
      const tools = executor.getToolDefinitions();
      expect(tools.length).toBe(3);
      expect(tools.map((t) => t.function.name).sort()).toEqual(['file-read', 'file-write', 'shell-exec']);
    });

    it('filters tools by allowed list', () => {
      const tools = executor.getToolDefinitions(['file-read']);
      expect(tools.length).toBe(1);
      expect(tools[0].function.name).toBe('file-read');
    });
  });
});

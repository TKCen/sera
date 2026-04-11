import { describe, it, expect, beforeEach, afterEach } from 'vitest';
import fs from 'fs';
import path from 'path';
import os from 'os';
import { RuntimeToolExecutor } from '../tools/index.js';
import type { ToolCall } from '../llmClient.js';

describe('RuntimeToolExecutor — Search Tools', () => {
  let tempDir: string;
  let executor: RuntimeToolExecutor;

  beforeEach(() => {
    tempDir = fs.mkdtempSync(path.join(os.tmpdir(), 'sera-search-test-'));
    executor = new RuntimeToolExecutor(tempDir, 2);
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

  describe('glob', () => {
    it('finds files matching a pattern', async () => {
      fs.writeFileSync(path.join(tempDir, 'test.ts'), 'content');
      fs.writeFileSync(path.join(tempDir, 'test.js'), 'content');

      const result = await executor.executeTool(makeCall('glob', { pattern: '*.ts' }));
      const parsed = JSON.parse(result.message.content);

      expect(parsed.files).toContain('test.ts');
      expect(parsed.files).not.toContain('test.js');
      expect(parsed.total).toBe(1);
    });

    it('works with recursive globs', async () => {
        fs.mkdirSync(path.join(tempDir, 'sub'), { recursive: true });
        fs.writeFileSync(path.join(tempDir, 'sub', 'inner.ts'), 'content');

        const result = await executor.executeTool(makeCall('glob', { pattern: '**/*.ts' }));
        const parsed = JSON.parse(result.message.content);

        expect(parsed.files).toContain('sub/inner.ts');
    });
  });

  describe('grep', () => {
    it('searches for content and returns matches', async () => {
      fs.writeFileSync(path.join(tempDir, 'file.txt'), 'hello world\nfoo bar\nhello again');

      const result = await executor.executeTool(makeCall('grep', { pattern: 'hello', mode: 'content' }));
      const parsed = JSON.parse(result.message.content);

      expect(parsed.matches).toHaveLength(2);
      expect(parsed.matches[0].content).toBe('hello world');
      expect(parsed.matches[1].content).toBe('hello again');
      expect(parsed.matches[0].line_number).toBe(1);
      expect(parsed.matches[1].line_number).toBe(3);
    });

    it('returns files_with_matches', async () => {
      fs.writeFileSync(path.join(tempDir, 'a.txt'), 'match');
      fs.writeFileSync(path.join(tempDir, 'b.txt'), 'other');

      const result = await executor.executeTool(makeCall('grep', { pattern: '\\bmatch\\b', mode: 'files_with_matches' }));
      const parsed = JSON.parse(result.message.content);

      expect(parsed.files).toContain('a.txt');
      expect(parsed.files).not.toContain('b.txt');
    });

    it('returns count of matches', async () => {
      fs.writeFileSync(path.join(tempDir, 'count.txt'), 'one\ntwo\nthree\none');

      const result = await executor.executeTool(makeCall('grep', { pattern: '\\bone\\b', mode: 'count' }));
      const parsed = JSON.parse(result.message.content);

      expect(parsed.counts).toBeDefined();
      expect(Object.values(parsed.counts)[0]).toBe(2);
    });
  });

  describe('read_file', () => {
    it('reads a file with offset and limit', async () => {
      const lines = Array.from({ length: 10 }, (_, i) => `line ${i + 1}`).join('\n');
      fs.writeFileSync(path.join(tempDir, 'lines.txt'), lines);

      const result = await executor.executeTool(makeCall('read_file', { path: 'lines.txt', offset: 3, limit: 2 }));
      const parsed = JSON.parse(result.message.content);

      expect(parsed.content).toBe('line 3\nline 4');
      expect(parsed.total_lines).toBe(10);
      expect(parsed.offset).toBe(3);
    });

    it('handles defaults for offset and limit', async () => {
      fs.writeFileSync(path.join(tempDir, 'small.txt'), 'only line');

      const result = await executor.executeTool(makeCall('read_file', { path: 'small.txt' }));
      const parsed = JSON.parse(result.message.content);

      expect(parsed.content).toBe('only line');
      expect(parsed.offset).toBe(1);
    });

    it('truncates long lines', async () => {
        const longLine = 'a'.repeat(1500);
        fs.writeFileSync(path.join(tempDir, 'long.txt'), longLine);

        const result = await executor.executeTool(makeCall('read_file', { path: 'long.txt' }));
        const parsed = JSON.parse(result.message.content);

        expect(parsed.content.length).toBeLessThan(1100);
        expect(parsed.content).toContain('[TRUNCATED]');
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

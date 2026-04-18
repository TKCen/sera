import { describe, it, expect, beforeEach, afterEach, vi } from 'vitest';
import fs from 'fs';
import path from 'path';
import os from 'os';
import axios from 'axios';
import { RuntimeToolExecutor } from '../tools/index.js';
import { ToolCall } from '../llmClient.js';

vi.mock('axios');

describe('New Built-in Tools', () => {
  let tempDir: string;
  let executor: RuntimeToolExecutor;

  beforeEach(() => {
    tempDir = fs.mkdtempSync(path.join(os.tmpdir(), 'sera-new-tools-test-'));
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

  describe('image-view', () => {
    it('reads an image and returns vision request metadata', async () => {
      const imgPath = path.join(tempDir, 'test.png');
      fs.writeFileSync(imgPath, Buffer.from([0x89, 0x50, 0x4e, 0x47, 0x0d, 0x0a, 0x1a, 0x0a])); // PNG header

      const result = await executor.executeTool(
        makeCall('image-view', { path: 'test.png', prompt: 'What is this?' })
      );
      expect(result.message.role).toBe('tool');
      const parsed = JSON.parse(result.message.content);
      expect(parsed.__type).toBe('vision_request');
      expect(parsed.path).toBe('test.png');
      expect(parsed.prompt).toBe('What is this?');
      expect(parsed.image_url).toContain('data:image/png;base64,');
    });

    it('returns error for missing image', async () => {
      const result = await executor.executeTool(makeCall('image-view', { path: 'missing.png' }));
      expect(result.message.content).toContain('Error: Image file not found');
    });
  });

  describe('pdf-read', () => {
    it('returns error for missing PDF', async () => {
      const result = await executor.executeTool(makeCall('pdf-read', { path: 'missing.pdf' }));
      expect(result.message.content).toContain('Error: PDF file not found');
    });

    // Note: Testing actual pdf-parse requires a real PDF buffer which is hard to mock easily in a unit test
    // without including a PDF library. We'll rely on the integration test and the error path for unit test.
  });

  describe('code-eval', () => {
    it('executes simple JS code', async () => {
      const result = await executor.executeTool(makeCall('code-eval', { code: 'return 1 + 2' }));
      const parsed = JSON.parse(result.message.content);
      expect(parsed.result).toBe(3);
    });

    it('captures console.log output', async () => {
      const result = await executor.executeTool(
        makeCall('code-eval', { code: 'console.log("hello"); return 42' })
      );
      const parsed = JSON.parse(result.message.content);
      expect(parsed.result).toBe(42);
      expect(parsed.stdout).toBe('hello');
    });

    it('enforces timeout', async () => {
      const result = await executor.executeTool(
        makeCall('code-eval', {
          code: 'return (async () => { while(true) { await new Promise(r => setTimeout(r, 10)); } })()',
          timeout: 100,
        })
      );
      const parsed = JSON.parse(result.message.content);
      expect(parsed.error).toContain('timed out');
    });
  });

  describe('http-request', () => {
    it('makes a GET request', async () => {
      vi.mocked(axios).mockResolvedValueOnce({
        status: 200,
        statusText: 'OK',
        headers: { 'content-type': 'application/json' },
        data: { foo: 'bar' },
      } as any);

      const result = await executor.executeTool(
        makeCall('http-request', { url: 'https://api.example.com/data' })
      );
      const parsed = JSON.parse(result.message.content);
      expect(parsed.status).toBe(200);
      expect(parsed.data).toBe('{"foo":"bar"}');
    });

    it('blocks private addresses', async () => {
      const result = await executor.executeTool(
        makeCall('http-request', { url: 'http://192.168.1.1' })
      );
      expect(result.message.content).toContain('not allowed');
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

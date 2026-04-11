import { describe, it, expect, beforeEach, afterEach, vi } from 'vitest';
import fs from 'fs';
import path from 'path';
import os from 'os';
import axios from 'axios';
import { RuntimeToolExecutor } from '../tools/index.js';
import { ToolOutputEvent, ToolResultEvent } from '../centrifugo.js';

vi.mock('axios');
const mockedAxios = vi.mocked(axios);

describe('Tool Streaming', () => {
  let tempDir: string;
  let executor: RuntimeToolExecutor;

  beforeEach(() => {
    tempDir = fs.mkdtempSync(path.join(os.tmpdir(), 'sera-streaming-test-'));
    executor = new RuntimeToolExecutor(tempDir, 2);
  });

  afterEach(() => {
    vi.clearAllMocks();
    try {
      if (fs.existsSync(tempDir)) {
        fs.rmSync(tempDir, { recursive: true, force: true });
      }
    } catch (err) {
      console.warn('Cleanup failed:', err);
    }
  });

  it('streams output for shell-exec', async () => {
    const events: (ToolOutputEvent | ToolResultEvent)[] = [];
    const onOutput = (ev: ToolOutputEvent | ToolResultEvent) => {
      events.push(ev);
    };

    const result = await executor.executeTool(
      {
        id: 'call_1',
        type: 'function',
        function: {
          name: 'shell-exec',
          arguments: JSON.stringify({ command: 'echo line1 && echo line2' }),
        },
      },
      onOutput
    );

    expect(result.message.content.trim()).toContain('line1');
    expect(result.message.content.trim()).toContain('line2');

    const progressEvents = events.filter((e) => 'type' in e && e.type === 'stdout');
    expect(progressEvents.length).toBeGreaterThanOrEqual(2);
    expect((progressEvents[0] as ToolOutputEvent).content).toBe('line1');
    expect((progressEvents[1] as ToolOutputEvent).content).toBe('line2');

    const resultEvent = events.find((e) => !('type' in e)) as ToolResultEvent;
    expect(resultEvent).toBeDefined();
    expect(resultEvent.toolName).toBe('shell-exec');
    expect(resultEvent.error).toBe(false);
    expect(resultEvent.duration).toBeGreaterThanOrEqual(0);
  });

  it('streams output for web-fetch', async () => {
    const events: (ToolOutputEvent | ToolResultEvent)[] = [];
    const onOutput = (ev: ToolOutputEvent | ToolResultEvent) => {
      events.push(ev);
    };

    const mockStream = {
      on: vi.fn((event, cb) => {
        if (event === 'data') {
          cb(Buffer.from('chunk1'));
          cb(Buffer.from('chunk2'));
        }
        if (event === 'end') {
          cb();
        }
        return mockStream;
      }),
    };

    mockedAxios.get.mockResolvedValueOnce({
      data: mockStream,
      status: 200,
      headers: { 'content-type': 'text/plain' },
    });

    const result = await executor.executeTool(
      {
        id: 'call_2',
        type: 'function',
        function: { name: 'web-fetch', arguments: JSON.stringify({ url: 'http://example.com' }) },
      },
      onOutput
    );

    expect(result.message.content).toBe('chunk1chunk2');

    const progressEvents = events.filter((e) => 'type' in e && e.type === 'progress');
    expect(progressEvents.length).toBe(2);
    expect((progressEvents[0] as ToolOutputEvent).content).toBe('chunk1');
    expect((progressEvents[1] as ToolOutputEvent).content).toBe('chunk2');

    const resultEvent = events.find((e) => !('type' in e)) as ToolResultEvent;
    expect(resultEvent).toBeDefined();
    expect(resultEvent.toolName).toBe('web-fetch');
    expect(resultEvent.error).toBe(false);
  });

  it('streams output for large file-read', async () => {
    const events: (ToolOutputEvent | ToolResultEvent)[] = [];
    const onOutput = (ev: ToolOutputEvent | ToolResultEvent) => {
      events.push(ev);
    };

    // Create a file > 16KB
    const largeContent = 'a'.repeat(20000);
    fs.writeFileSync(path.join(tempDir, 'large.txt'), largeContent);

    const result = await executor.executeTool(
      {
        id: 'call_3',
        type: 'function',
        function: { name: 'file-read', arguments: JSON.stringify({ path: 'large.txt' }) },
      },
      onOutput
    );

    expect(result.message.content).toBe(largeContent);

    const progressEvents = events.filter((e) => 'type' in e && e.type === 'progress');
    expect(progressEvents.length).toBeGreaterThan(1);

    let combined = '';
    progressEvents.forEach((e) => (combined += (e as ToolOutputEvent).content));
    expect(combined).toBe(largeContent);

    const resultEvent = events.find((e) => !('type' in e)) as ToolResultEvent;
    expect(resultEvent).toBeDefined();
    expect(resultEvent.toolName).toBe('file-read');
  });

  it('emits ToolResultEvent even for small non-streaming tools', async () => {
    const events: (ToolOutputEvent | ToolResultEvent)[] = [];
    const onOutput = (ev: ToolOutputEvent | ToolResultEvent) => {
      events.push(ev);
    };

    await executor.executeTool(
      {
        id: 'call_4',
        type: 'function',
        function: { name: 'tool-search', arguments: JSON.stringify({ query: 'test' }) },
      },
      onOutput
    );

    const resultEvent = events.find((e) => !('type' in e)) as ToolResultEvent;
    expect(resultEvent).toBeDefined();
    expect(resultEvent.toolName).toBe('tool-search');
    expect(resultEvent.duration).toBeGreaterThanOrEqual(0);
  });
});

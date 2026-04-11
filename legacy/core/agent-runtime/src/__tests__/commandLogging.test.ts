import { describe, it, expect, beforeEach, afterEach, vi } from 'vitest';
import fs from 'fs';
import path from 'path';
import os from 'os';
import { RuntimeToolExecutor } from '../tools/index.js';
import type { RuntimeManifest } from '../manifest.js';

describe('RuntimeToolExecutor — Command Logging', () => {
  let tempDir: string;
  let executor: RuntimeToolExecutor;
  let mockFetch: any;

  const manifest: RuntimeManifest = {
    apiVersion: 'sera/v1',
    kind: 'Agent',
    metadata: {
      name: 'test-agent',
      displayName: 'Test Agent',
      icon: '🤖',
      circle: 'test-circle',
      tier: 2,
    },
    identity: {
      role: 'tester',
      description: 'tester',
    },
    model: {
      provider: 'test',
      name: 'test-model',
    },
    logging: {
      commands: true,
    },
  };

  beforeEach(() => {
    tempDir = fs.mkdtempSync(path.join(os.tmpdir(), 'sera-logging-test-'));
    executor = new RuntimeToolExecutor(tempDir, 2, manifest);

    mockFetch = vi.fn().mockResolvedValue({
      ok: true,
      status: 201,
      json: async () => ({ success: true }),
    });
    vi.stubGlobal('fetch', mockFetch);

    process.env['SERA_CORE_URL'] = 'http://sera-core:3001';
    process.env['SERA_IDENTITY_TOKEN'] = 'test-token';
    process.env['AGENT_INSTANCE_ID'] = 'test-instance-id';
  });

  afterEach(() => {
    vi.unstubAllGlobals();
    try {
      if (fs.existsSync(tempDir)) {
        fs.rmSync(tempDir, { recursive: true, force: true });
      }
    } catch (err) {}
  });

  it('sends command log to core when logging is enabled', async () => {
    fs.writeFileSync(path.join(tempDir, 'test.txt'), 'hello');

    await executor.executeTool({
      id: 'call_1',
      type: 'function',
      function: {
        name: 'file-read',
        arguments: JSON.stringify({ path: 'test.txt' }),
      },
    }, undefined, 'test-session-id');

    expect(mockFetch).toHaveBeenCalledWith(
      expect.stringContaining('/api/agents/test-instance-id/command-logs'),
      expect.objectContaining({
        method: 'POST',
        body: expect.stringContaining('"toolName":"file-read"'),
      })
    );

    const body = JSON.parse(mockFetch.mock.calls[0][1].body);
    expect(body.sessionId).toBe('test-session-id');
    expect(body.toolName).toBe('file-read');
    expect(body.arguments).toEqual({ path: 'test.txt' });
    expect(body.result).toBe('hello');
    expect(body.status).toBe('success');
    expect(typeof body.durationMs).toBe('number');
  });

  it('redacts sensitive arguments in command log', async () => {
    await executor.executeTool({
      id: 'call_2',
      type: 'function',
      function: {
        name: 'tool-search',
        arguments: JSON.stringify({ query: 'test', api_key: 'secret-123', password: 'hidden-password' }),
      },
    }, undefined, 'test-session-id');

    const body = JSON.parse(mockFetch.mock.calls[0][1].body);
    expect(body.arguments.query).toBe('test');
    expect(body.arguments.api_key).toBe('[REDACTED]');
    expect(body.arguments.password).toBe('[REDACTED]');
  });

  it('truncates long results in command log', async () => {
    const longResult = 'A'.repeat(3000);

    // We use tool-search because it just returns the query we send it in this test context if we were to mock it,
    // but actually tool-search is implemented in executor.ts itself.
    // Let's use a mock for the tool implementation or just rely on a tool that returns what we want.
    // Actually, I'll just mock the return value of searchTools which is private,
    // or better, just use file-read with a large file.

    fs.writeFileSync(path.join(tempDir, 'large.txt'), longResult);

    await executor.executeTool({
      id: 'call_3',
      type: 'function',
      function: {
        name: 'file-read',
        arguments: JSON.stringify({ path: 'large.txt' }),
      },
    }, undefined, 'test-session-id');

    const body = JSON.parse(mockFetch.mock.calls[0][1].body);
    expect(body.result.length).toBeLessThan(3000);
    expect(body.result).toContain('[TRUNCATED]');
  });

  it('does not send command log when logging is disabled', async () => {
    const disabledManifest = { ...manifest, logging: { commands: false } };
    const disabledExecutor = new RuntimeToolExecutor(tempDir, 2, disabledManifest);

    fs.writeFileSync(path.join(tempDir, 'test.txt'), 'hello');
    await disabledExecutor.executeTool({
      id: 'call_4',
      type: 'function',
      function: {
        name: 'file-read',
        arguments: JSON.stringify({ path: 'test.txt' }),
      },
    }, undefined, 'test-session-id');

    expect(mockFetch).not.toHaveBeenCalledWith(
      expect.stringContaining('/api/agents/test-instance-id/command-logs'),
      expect.any(Object)
    );
  });
});

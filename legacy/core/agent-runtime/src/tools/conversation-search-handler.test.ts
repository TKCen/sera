/**
 * Tests for the conversation-search tool handler.
 */

import { describe, it, expect, vi, beforeEach, afterEach } from 'vitest';
import { conversationSearch } from './conversation-search-handler.js';

// Mock fetch globally
const mockFetch = vi.fn();
vi.stubGlobal('fetch', mockFetch);

const CORE_URL = 'http://sera-core:3000';
const TOKEN = 'test-token';
const AGENT_ID = 'agent-instance-abc123';

function setEnv() {
  process.env['SERA_CORE_URL'] = CORE_URL;
  process.env['SERA_IDENTITY_TOKEN'] = TOKEN;
  process.env['AGENT_INSTANCE_ID'] = AGENT_ID;
}

function clearEnv() {
  delete process.env['SERA_CORE_URL'];
  delete process.env['SERA_IDENTITY_TOKEN'];
  delete process.env['AGENT_INSTANCE_ID'];
}

function makeMessages(
  overrides: Partial<{
    id: string;
    sessionId: string;
    role: string;
    content: string;
    createdAt: string;
  }>[] = []
) {
  const defaults = [
    {
      id: 'msg-1',
      sessionId: 'sess-1',
      role: 'user',
      content: 'Hello, what is the capital of France?',
      createdAt: '2025-06-01T10:00:00Z',
    },
    {
      id: 'msg-2',
      sessionId: 'sess-1',
      role: 'assistant',
      content: 'The capital of France is Paris.',
      createdAt: '2025-06-01T10:00:05Z',
    },
  ];
  return overrides.length > 0
    ? overrides.map((o, i) => ({ ...defaults[i % defaults.length]!, ...o }))
    : defaults;
}

describe('conversationSearch', () => {
  beforeEach(() => {
    setEnv();
    mockFetch.mockReset();
  });

  afterEach(() => {
    clearEnv();
  });

  it('returns error when SERA_CORE_URL is not set', async () => {
    delete process.env['SERA_CORE_URL'];
    const result = JSON.parse(await conversationSearch({ query: 'test' }));
    expect(result.error).toContain('SERA_CORE_URL not configured');
  });

  it('returns error when AGENT_INSTANCE_ID is not set', async () => {
    delete process.env['AGENT_INSTANCE_ID'];
    const result = JSON.parse(await conversationSearch({ query: 'test' }));
    expect(result.error).toContain('AGENT_INSTANCE_ID not set');
  });

  it('returns error for empty query', async () => {
    const result = JSON.parse(await conversationSearch({ query: '' }));
    expect(result.error).toContain('query must not be empty');
  });

  it('calls core with correct URL and headers', async () => {
    const messages = makeMessages();
    mockFetch.mockResolvedValueOnce({
      ok: true,
      json: async () => messages,
    });

    await conversationSearch({ query: 'France' });

    expect(mockFetch).toHaveBeenCalledOnce();
    const [url, init] = mockFetch.mock.calls[0]!;
    expect(url).toContain('/api/sessions/search');
    expect(url).toContain(`agentInstanceId=${AGENT_ID}`);
    expect(url).toContain('q=France');
    expect((init as RequestInit).headers as Record<string, string>).toMatchObject({
      Authorization: `Bearer ${TOKEN}`,
    });
  });

  it('returns matching messages with truncated content', async () => {
    const longContent = 'x'.repeat(600);
    const messages = makeMessages([{ content: longContent }]);
    mockFetch.mockResolvedValueOnce({ ok: true, json: async () => messages });

    const result = JSON.parse(await conversationSearch({ query: 'xxx' }));
    expect(result.count).toBe(1);
    expect(result.results[0].content).toHaveLength(500 + '... [truncated]'.length);
    expect(result.results[0].content).toContain('... [truncated]');
  });

  it('does not truncate content within limit', async () => {
    const shortContent = 'Paris is the capital of France.';
    const messages = makeMessages([{ content: shortContent }]);
    mockFetch.mockResolvedValueOnce({ ok: true, json: async () => messages });

    const result = JSON.parse(await conversationSearch({ query: 'Paris' }));
    expect(result.results[0].content).toBe(shortContent);
  });

  it('passes roles filter to query string', async () => {
    mockFetch.mockResolvedValueOnce({ ok: true, json: async () => [] });

    await conversationSearch({ query: 'test', roles: ['user', 'assistant'] });

    const [url] = mockFetch.mock.calls[0]!;
    expect(url).toContain('roles=user%2Cassistant');
  });

  it('passes date filters to query string', async () => {
    mockFetch.mockResolvedValueOnce({ ok: true, json: async () => [] });

    await conversationSearch({
      query: 'test',
      start_date: '2025-01-01T00:00:00Z',
      end_date: '2025-12-31T23:59:59Z',
    });

    const [url] = mockFetch.mock.calls[0]!;
    expect(url).toContain('startDate=');
    expect(url).toContain('endDate=');
  });

  it('clamps limit to max 50', async () => {
    mockFetch.mockResolvedValueOnce({ ok: true, json: async () => [] });

    await conversationSearch({ query: 'test', limit: 999 });

    const [url] = mockFetch.mock.calls[0]!;
    expect(url).toContain('limit=50');
  });

  it('defaults limit to 10', async () => {
    mockFetch.mockResolvedValueOnce({ ok: true, json: async () => [] });

    await conversationSearch({ query: 'test' });

    const [url] = mockFetch.mock.calls[0]!;
    expect(url).toContain('limit=10');
  });

  it('returns error message on HTTP failure', async () => {
    mockFetch.mockResolvedValueOnce({
      ok: false,
      status: 500,
      text: async () => 'Internal Server Error',
    });

    const result = JSON.parse(await conversationSearch({ query: 'test' }));
    expect(result.error).toContain('HTTP 500');
  });

  it('returns error message on network failure', async () => {
    mockFetch.mockRejectedValueOnce(new Error('ECONNREFUSED'));

    const result = JSON.parse(await conversationSearch({ query: 'test' }));
    expect(result.error).toContain('ECONNREFUSED');
  });

  it('returns results with correct shape', async () => {
    const messages = makeMessages();
    mockFetch.mockResolvedValueOnce({ ok: true, json: async () => messages });

    const result = JSON.parse(await conversationSearch({ query: 'France' }));
    expect(result.query).toBe('France');
    expect(result.count).toBe(2);
    expect(result.results[0]).toMatchObject({
      id: 'msg-1',
      session_id: 'sess-1',
      role: 'user',
      timestamp: '2025-06-01T10:00:00Z',
    });
  });
});

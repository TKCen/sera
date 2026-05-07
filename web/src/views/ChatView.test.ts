import { describe, it, expect, vi, beforeEach, afterEach } from 'vitest';
import { ApiError, parseChatSseEvent, runOperatorTask, setToken } from '@/lib/api';

describe('parseChatSseEvent', () => {
  it('parses a message delta event', () => {
    const result = parseChatSseEvent(
      'message',
      JSON.stringify({ delta: 'Hello ', message_id: 'msg_abc', session_id: 'sess_1' }),
    );
    expect(result).toEqual({
      type: 'delta',
      delta: 'Hello ',
      messageId: 'msg_abc',
      sessionId: 'sess_1',
    });
  });

  it('parses a done event', () => {
    const result = parseChatSseEvent(
      'done',
      JSON.stringify({
        status: 'complete',
        usage: { prompt_tokens: 10, completion_tokens: 20, total_tokens: 30 },
      }),
    );
    expect(result).toEqual({
      type: 'done',
      usage: { promptTokens: 10, completionTokens: 20, totalTokens: 30 },
    });
  });

  it('returns null for unknown event types', () => {
    const result = parseChatSseEvent('unknown_event', JSON.stringify({ foo: 'bar' }));
    expect(result).toBeNull();
  });

  it('returns null for malformed JSON in a message event', () => {
    const result = parseChatSseEvent('message', 'not-valid-json{{{');
    expect(result).toBeNull();
  });

  it('returns done with zero usage when usage field is absent', () => {
    const result = parseChatSseEvent('done', JSON.stringify({ status: 'complete' }));
    expect(result).toEqual({
      type: 'done',
      usage: { promptTokens: 0, completionTokens: 0, totalTokens: 0 },
    });
  });

  it('parses an error event with session/message ids', () => {
    const result = parseChatSseEvent(
      'error',
      JSON.stringify({
        error: 'runtime returned empty reply',
        session_id: 'sess_1',
        message_id: 'msg_abc',
      }),
    );
    expect(result).toEqual({
      type: 'error',
      error: 'runtime returned empty reply',
      sessionId: 'sess_1',
      messageId: 'msg_abc',
    });
  });

  it('parses an error event without optional ids', () => {
    const result = parseChatSseEvent(
      'error',
      JSON.stringify({ error: 'runtime timed out' }),
    );
    expect(result).toEqual({
      type: 'error',
      error: 'runtime timed out',
      sessionId: undefined,
      messageId: undefined,
    });
  });

  it('returns null for malformed JSON in an error event', () => {
    const result = parseChatSseEvent('error', 'not-json');
    expect(result).toBeNull();
  });

  it('accumulates deltas correctly when applied sequentially', () => {
    const events = [
      { event: 'message', data: JSON.stringify({ delta: 'Hello ', message_id: 'm1', session_id: 's1' }) },
      { event: 'message', data: JSON.stringify({ delta: 'world', message_id: 'm1', session_id: 's1' }) },
      { event: 'message', data: JSON.stringify({ delta: '!', message_id: 'm1', session_id: 's1' }) },
      { event: 'done', data: JSON.stringify({ status: 'complete', usage: { prompt_tokens: 5, completion_tokens: 3, total_tokens: 8 } }) },
    ];

    let accumulated = '';
    let done = false;
    for (const ev of events) {
      const parsed = parseChatSseEvent(ev.event, ev.data);
      if (parsed?.type === 'delta') accumulated += parsed.delta;
      if (parsed?.type === 'done') done = true;
    }

    expect(accumulated).toBe('Hello world!');
    expect(done).toBe(true);
  });
});

describe('runOperatorTask', () => {
  beforeEach(() => {
    setToken('test-key');
  });
  afterEach(() => {
    setToken(null);
    vi.restoreAllMocks();
  });

  it('POSTs to /api/operator/tasks with bearer auth and returns the status card', async () => {
    const card = {
      accepted_task: 'do thing',
      active_agent: 'sera',
      spawned_helper: { agent: 'serahelper', task_id: 'sera:abc/h1', status: 'idle', count: 1, total: 1 },
      handoff_tool: 'handoff_to_serahelper',
      latest_event: 'intercom:operator.task delivered=true',
      status: 'complete',
      blocked: false,
      result: 'ok',
      audit_id: 'aud_1',
      session_key: 'sera:abc',
    };

    const fetchMock = vi.fn(async () =>
      new Response(JSON.stringify(card), {
        status: 200,
        headers: { 'Content-Type': 'application/json' },
      }),
    );
    vi.stubGlobal('fetch', fetchMock);

    const result = await runOperatorTask({
      task: 'do thing',
      agent: 'sera',
      helper: 'serahelper',
    });

    expect(result).toEqual(card);
    expect(fetchMock).toHaveBeenCalledTimes(1);
    const [url, init] = fetchMock.mock.calls[0] as unknown as [string, RequestInit];
    expect(url).toBe('/api/operator/tasks');
    expect(init.method).toBe('POST');
    expect(JSON.parse(init.body as string)).toEqual({
      task: 'do thing',
      agent: 'sera',
      helper: 'serahelper',
    });
    const headers = new Headers(init.headers);
    expect(headers.get('Authorization')).toBe('Bearer test-key');
    expect(headers.get('Content-Type')).toBe('application/json');
  });

  it('throws ApiError with status 401 on auth failure', async () => {
    const fetchMock = vi.fn(async () =>
      new Response(JSON.stringify({ error: 'unauthorized' }), {
        status: 401,
        headers: { 'Content-Type': 'application/json' },
      }),
    );
    vi.stubGlobal('fetch', fetchMock);

    await expect(runOperatorTask({ task: 'hi' })).rejects.toMatchObject({
      name: 'ApiError',
      status: 401,
    });
    await expect(runOperatorTask({ task: 'hi' })).rejects.toBeInstanceOf(ApiError);
  });

  it('tolerates a permissive response shape with missing optional fields', async () => {
    const fetchMock = vi.fn(async () =>
      new Response(JSON.stringify({ status: 'complete', result: 'ok' }), {
        status: 200,
        headers: { 'Content-Type': 'application/json' },
      }),
    );
    vi.stubGlobal('fetch', fetchMock);

    const result = await runOperatorTask({ task: 'hi' });
    expect(result.status).toBe('complete');
    expect(result.result).toBe('ok');
    expect(result.spawned_helper).toBeUndefined();
  });
});

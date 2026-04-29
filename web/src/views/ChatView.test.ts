import { describe, it, expect } from 'vitest';
import { parseChatSseEvent } from '@/lib/api';

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

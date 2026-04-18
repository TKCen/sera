import { describe, it, expect, vi } from 'vitest';
import {
  pipe,
  wrapIdleTimeout,
  wrapToolNameTrim,
  wrapToolCallArgumentRepair,
  wrapSanitizeMalformedToolCalls,
  wrapReasoningFilter,
  type Chunk,
} from '../streamWrappers.js';
import { LLMTimeoutError } from '../llmClient.js';

describe('streamWrappers', () => {
  async function* toAsyncIterable(chunks: Chunk[]): AsyncIterable<Chunk> {
    for (const chunk of chunks) {
      yield chunk;
    }
  }

  async function collect(stream: AsyncIterable<Chunk>): Promise<Chunk[]> {
    const result: Chunk[] = [];
    for await (const chunk of stream) {
      result.push(chunk);
    }
    return result;
  }

  describe('pipe', () => {
    it('should compose multiple transformers', async () => {
      const transformer1 = async function* (s: AsyncIterable<Chunk>) {
        for await (const c of s) yield { ...c, content: (c.content || '') + '1' };
      };
      const transformer2 = async function* (s: AsyncIterable<Chunk>) {
        for await (const c of s) yield { ...c, content: (c.content || '') + '2' };
      };

      const stream = toAsyncIterable([{ content: 'a' }]);
      const wrapped = pipe(stream, transformer1, transformer2);
      const result = await collect(wrapped);

      expect(result[0].content).toBe('a12');
    });
  });

  describe('wrapIdleTimeout', () => {
    it('should pass through chunks if they arrive in time', async () => {
      const chunks = [{ content: 'a' }, { content: 'b' }];
      const stream = toAsyncIterable(chunks);
      const wrapped = wrapIdleTimeout(100)(stream);
      const result = await collect(wrapped);
      expect(result).toEqual(chunks);
    });

    it('should throw LLMTimeoutError if stream stalls', async () => {
      async function* stallingStream() {
        yield { content: 'a' };
        await new Promise((r) => setTimeout(r, 200));
        yield { content: 'b' };
      }

      const wrapped = wrapIdleTimeout(50)(stallingStream());
      await expect(collect(wrapped)).rejects.toThrow(LLMTimeoutError);
    });
  });

  describe('wrapToolNameTrim', () => {
    it('should sanitize and lowercase tool names', async () => {
      const chunks: Chunk[] = [
        { toolCallDelta: { index: 0, name: '  Search_Web  ' } },
        { toolCallDelta: { index: 1, name: 'FILE-WRITE' } },
      ];
      const wrapped = wrapToolNameTrim()(toAsyncIterable(chunks));
      const result = await collect(wrapped);

      expect(result[0].toolCallDelta?.name).toBe('search_web');
      expect(result[1].toolCallDelta?.name).toBe('file-write');
    });
  });

  describe('wrapToolCallArgumentRepair', () => {
    it('should assemble and repair JSON arguments', async () => {
      const chunks: Chunk[] = [
        { toolCallDelta: { index: 0, id: 'call_1', name: 'test', arguments: '{"foo":' } },
        { toolCallDelta: { index: 0, arguments: '"bar"' } }, // Missing closing brace
      ];
      const wrapped = wrapToolCallArgumentRepair()(toAsyncIterable(chunks));
      const result = await collect(wrapped);

      expect(result.length).toBe(1);
      expect(JSON.parse(result[0].toolCallDelta!.arguments!)).toEqual({ foo: 'bar' });
    });

    it('should not swallow non-delta fields in mixed chunks', async () => {
      const chunks: Chunk[] = [
        {
          content: 'metadata',
          toolCallDelta: { index: 0, id: 'call_1', name: 'test', arguments: '{}' },
        },
      ];
      const wrapped = wrapToolCallArgumentRepair()(toAsyncIterable(chunks));
      const result = await collect(wrapped);

      expect(result.some((c) => c.content === 'metadata')).toBe(true);
      expect(result.some((c) => c.toolCallDelta?.index === 0)).toBe(true);
    });
  });

  describe('wrapSanitizeMalformedToolCalls', () => {
    it('should drop tool calls with unparseable JSON', async () => {
      const chunks: Chunk[] = [
        { toolCallDelta: { index: 0, name: 'test', arguments: '{"valid": true}' } },
        { toolCallDelta: { index: 1, name: 'test', arguments: 'NOT_JSON' } },
      ];
      const wrapped = wrapSanitizeMalformedToolCalls()(toAsyncIterable(chunks));
      const result = await collect(wrapped);

      expect(result.length).toBe(1);
      expect(result[0].toolCallDelta?.index).toBe(0);
    });

    it('should drop tool calls without a name', async () => {
      const chunks: Chunk[] = [{ toolCallDelta: { index: 0, name: '', arguments: '{}' } }];
      const wrapped = wrapSanitizeMalformedToolCalls()(toAsyncIterable(chunks));
      const result = await collect(wrapped);

      expect(result.length).toBe(0);
    });

    it('should not drop non-delta data in malformed chunks', async () => {
      const chunks: Chunk[] = [
        { content: 'metadata', toolCallDelta: { index: 1, name: 'test', arguments: 'NOT_JSON' } },
      ];
      const wrapped = wrapSanitizeMalformedToolCalls()(toAsyncIterable(chunks));
      const result = await collect(wrapped);

      expect(result.length).toBe(1);
      expect(result[0].content).toBe('metadata');
      expect(result[0].toolCallDelta).toBeUndefined();
    });
  });

  describe('wrapReasoningFilter', () => {
    it('should drop reasoning if level is off', async () => {
      const chunks: Chunk[] = [{ content: 'hello', reasoning: 'thinking' }];
      const wrapped = wrapReasoningFilter('off')(toAsyncIterable(chunks));
      const result = await collect(wrapped);

      expect(result[0].content).toBe('hello');
      expect(result[0].reasoning).toBeUndefined();
    });

    it('should keep reasoning if level is not off', async () => {
      const chunks: Chunk[] = [{ content: 'hello', reasoning: 'thinking' }];
      const wrapped = wrapReasoningFilter('medium')(toAsyncIterable(chunks));
      const result = await collect(wrapped);

      expect(result[0].reasoning).toBe('thinking');
      expect(result[0].content).toBe('hello');
    });

    it('should only drop reasoning if other content exists', async () => {
      const chunks: Chunk[] = [{ content: 'hello', reasoning: 'thinking' }];
      const wrapped = wrapReasoningFilter('off')(toAsyncIterable(chunks));
      const result = await collect(wrapped);

      expect(result.length).toBe(1);
      expect(result[0].content).toBe('hello');
      expect(result[0].reasoning).toBeUndefined();
    });
  });
});

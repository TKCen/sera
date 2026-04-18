import { LLMTimeoutError, type ThinkingLevel } from './llmClient.js';
import { sanitizeToolName, repairToolArguments } from './toolArgumentRepair.js';

/**
 * Internal chunk format for LLM streaming deltas.
 */
export interface Chunk {
  content?: string;
  reasoning?: string;
  toolCallDelta?: {
    index: number;
    id?: string;
    name?: string;
    arguments?: string;
  };
  usage?: {
    promptTokens: number;
    completionTokens: number;
    cacheCreationTokens: number;
    cacheReadTokens: number;
    totalTokens: number;
  };
  finishReason?: string;
}

export type StreamTransformer = (stream: AsyncIterable<Chunk>) => AsyncIterable<Chunk>;

/**
 * Compose multiple stream transformers into a single chain.
 */
export function pipe(
  stream: AsyncIterable<Chunk>,
  ...transformers: StreamTransformer[]
): AsyncIterable<Chunk> {
  return transformers.reduce((s, transformer) => transformer(s), stream);
}

/**
 * Detect stalled LLM responses.
 * Throws LLMTimeoutError if no chunks are received within the specified duration.
 */
export function wrapIdleTimeout(timeoutMs: number): StreamTransformer {
  return async function* (stream) {
    const iterator = stream[Symbol.asyncIterator]();
    try {
      while (true) {
        let timer: NodeJS.Timeout | undefined;
        const result = await Promise.race([
          iterator.next(),
          new Promise<IteratorResult<Chunk>>((_, reject) => {
            timer = setTimeout(() => {
              reject(new LLMTimeoutError(`LLM stalled: no tokens received for ${timeoutMs}ms`));
            }, timeoutMs);
          }),
        ]);
        if (timer) clearTimeout(timer);

        if (result.done) break;
        yield result.value;
      }
    } finally {
      if (iterator.return) {
        await iterator.return();
      }
    }
  };
}

/**
 * Normalize tool names (strip whitespace, lowercase).
 */
export function wrapToolNameTrim(): StreamTransformer {
  return async function* (stream) {
    for await (const chunk of stream) {
      if (chunk.toolCallDelta?.name) {
        chunk.toolCallDelta.name = sanitizeToolName(chunk.toolCallDelta.name).toLowerCase();
      }
      yield chunk;
    }
  };
}

/**
 * Fix JSON issues in tool call arguments after they are assembled from deltas.
 * Buffers tool call deltas until the end of the stream, then yields repaired tool calls.
 */
export function wrapToolCallArgumentRepair(): StreamTransformer {
  return async function* (stream) {
    const buffers = new Map<number, { id: string; name: string; arguments: string }>();

    for await (const chunk of stream) {
      if (chunk.toolCallDelta) {
        const delta = chunk.toolCallDelta;
        let buffer = buffers.get(delta.index);
        if (!buffer) {
          buffer = { id: '', name: '', arguments: '' };
          buffers.set(delta.index, buffer);
        }
        if (delta.id) buffer.id = delta.id;
        if (delta.name) buffer.name = delta.name;
        if (delta.arguments) buffer.arguments += delta.arguments;

        const { toolCallDelta: _, ...rest } = chunk;
        if (Object.keys(rest).length > 0) {
          yield rest;
        }
      } else {
        yield chunk;
      }
    }

    for (const [index, buffer] of buffers.entries()) {
      try {
        const repairResult = repairToolArguments(buffer.arguments);
        yield {
          toolCallDelta: {
            index,
            id: buffer.id,
            name: buffer.name,
            arguments: JSON.stringify(repairResult.parsed),
          },
        };
      } catch {
        // Fallback to unrepaired if all else fails
        yield {
          toolCallDelta: {
            index,
            id: buffer.id,
            name: buffer.name,
            arguments: buffer.arguments,
          },
        };
      }
    }
  };
}

/**
 * Drop tool calls that remain unparseable or lack mandatory fields.
 */
export function wrapSanitizeMalformedToolCalls(): StreamTransformer {
  return async function* (stream) {
    for await (const chunk of stream) {
      if (chunk.toolCallDelta) {
        const { name, arguments: args } = chunk.toolCallDelta;
        if (!name || args === undefined) {
          const { toolCallDelta: _, ...rest } = chunk;
          if (Object.keys(rest).length > 0) {
            yield rest;
          }
          continue;
        }
        try {
          JSON.parse(args);
          yield chunk;
        } catch {
          // Drop malformed tool calls, but yield other data in chunk
          const { toolCallDelta: _, ...rest } = chunk;
          if (Object.keys(rest).length > 0) {
            yield rest;
          }
          continue;
        }
      } else {
        yield chunk;
      }
    }
  };
}

/**
 * Filter reasoning blocks based on thinking level config.
 */
export function wrapReasoningFilter(thinkingLevel?: ThinkingLevel): StreamTransformer {
  return async function* (stream) {
    for await (const chunk of stream) {
      if (thinkingLevel === 'off' && chunk.reasoning) {
        const { reasoning: _, ...rest } = chunk;
        if (Object.keys(rest).length > 0) {
          yield rest;
        }
        continue;
      }
      yield chunk;
    }
  };
}

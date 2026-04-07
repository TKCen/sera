/**
 * LLM Client — HTTP client for the SERA Core LLM Proxy.
 *
 * Makes OpenAI-compatible requests to the Core proxy endpoint,
 * authenticating with the container's JWT identity token.
 */

import axios, { type AxiosInstance, AxiosError } from 'axios';
import { log } from './logger.js';
import { safeStringify } from './json.js';
import {
  pipe,
  wrapIdleTimeout,
  wrapToolNameTrim,
  wrapToolCallArgumentRepair,
  wrapSanitizeMalformedToolCalls,
  wrapReasoningFilter,
  type Chunk,
} from './streamWrappers.js';
import type { Readable } from 'stream';

// ── Error Types ───────────────────────────────────────────────────────────────

export class BudgetExceededError extends Error {
  constructor(message: string) {
    super(message);
    this.name = 'BudgetExceededError';
  }
}

export class ProviderUnavailableError extends Error {
  constructor(message: string) {
    super(message);
    this.name = 'ProviderUnavailableError';
  }
}

export class ContextOverflowError extends Error {
  constructor(message: string) {
    super(message);
    this.name = 'ContextOverflowError';
  }
}

export class LLMTimeoutError extends Error {
  constructor(message: string) {
    super(message);
    this.name = 'LLMTimeoutError';
  }
}

/** Patterns that indicate a context window overflow in LLM error responses. */
const OVERFLOW_PATTERNS = [
  'context_length_exceeded',
  'maximum context length',
  'prompt is too long',
  'context length',
  'token limit',
];

// ── Types ─────────────────────────────────────────────────────────────────────

export interface ToolCallFunction {
  name: string;
  arguments: string;
}

export interface ToolCall {
  id: string;
  type: 'function';
  function: ToolCallFunction;
}

export interface ToolDefinition {
  type: 'function';
  function: {
    name: string;
    description: string;
    parameters: Record<string, unknown>;
  };
}

export interface ChatMessage {
  role: 'user' | 'assistant' | 'system' | 'tool';
  content: string | MessageContentBlock[];
  tool_calls?: ToolCall[];
  tool_call_id?: string;
  /** Internal messages are hidden from the chat UI (Story 5.12). */
  internal?: boolean;
  /** Estimated token count for this message. */
  tokens?: number;
}

export interface MessageContentBlock {
  type: 'text' | 'image_url';
  text?: string;
  image_url?: {
    url: string;
    detail?: 'auto' | 'low' | 'high';
  };
}

export type ThinkingLevel = 'off' | 'minimal' | 'low' | 'medium' | 'high' | 'max';

export interface LLMResponse {
  content: string;
  /** Chain-of-thought text, e.g. Qwen / DeepSeek reasoning_content. */
  reasoning?: string;
  toolCalls?: ToolCall[];
  citations?: Array<{ blockId: string; scope: string; relevance: number }>;
  usage?: {
    promptTokens: number;
    completionTokens: number;
    cacheCreationTokens: number;
    cacheReadTokens: number;
    totalTokens: number;
  };
}

// ── Interface ─────────────────────────────────────────────────────────────────

export interface ILLMClient {
  chat(
    messages: ChatMessage[],
    tools?: ToolDefinition[],
    temperature?: number,
    thinkingLevel?: ThinkingLevel,
    timeoutMs?: number,
    model?: string,
    streaming?: boolean
  ): Promise<LLMResponse>;
}

// ── Client ────────────────────────────────────────────────────────────────────

export class LLMClient implements ILLMClient {
  private http: AxiosInstance;
  private model: string;

  constructor(coreUrl: string, identityToken: string, model: string) {
    const timeoutMs = process.env['LLM_TIMEOUT_MS']
      ? parseInt(process.env['LLM_TIMEOUT_MS'], 10)
      : 120_000;

    this.model = model;
    // Prefer SERA_LLM_PROXY_URL (BYOH contract) over constructing from SERA_CORE_URL
    const llmBaseUrl = process.env['SERA_LLM_PROXY_URL'] || `${coreUrl}/v1/llm`;
    this.http = axios.create({
      baseURL: llmBaseUrl,
      headers: {
        'Content-Type': 'application/json',
        Authorization: `Bearer ${identityToken}`,
      },
      timeout: timeoutMs,
    });
  }

  /**
   * Send a chat completion request through the Core LLM proxy.
   * Returns parsed content + optional tool calls.
   *
   * @throws BudgetExceededError on HTTP 429
   * @throws ProviderUnavailableError on HTTP 503
   */
  async chat(
    messages: ChatMessage[],
    tools?: ToolDefinition[],
    temperature?: number,
    thinkingLevel?: ThinkingLevel,
    timeoutMs?: number,
    model?: string,
    streaming: boolean = true
  ): Promise<LLMResponse> {
    const body: Record<string, unknown> = {
      model: model || this.model,
      messages: messages.map((m) => ({
        role: m.role,
        content: m.content,
        ...(m.tool_calls ? { tool_calls: m.tool_calls } : {}),
        ...(m.tool_call_id ? { tool_call_id: m.tool_call_id } : {}),
      })),
      stream: streaming,
    };

    if (tools && tools.length > 0) {
      body['tools'] = tools;
    }

    if (temperature !== undefined) {
      body['temperature'] = temperature;
    }

    if (thinkingLevel && thinkingLevel !== 'off') {
      body['thinking_level'] = thinkingLevel;
    }

    // Debug: log tool names and message roles sent to LLM
    if (tools && tools.length > 0) {
      const toolNames = tools.map((t) => t.function.name).join(', ');
      log(
        'debug',
        `LLM request: ${(body['messages'] as unknown[]).length} messages, ${tools.length} tools: [${toolNames}]`
      );
    }

    try {
      // Pre-serialize to detect and strip cyclic references before axios tries JSON.stringify.
      // Bun's JSON.stringify throws on cycles; safeStringify handles them gracefully.
      const safeBody = safeStringify(body);

      if (!streaming) {
        // ── Non-streaming path ─────────────────────────────────────────────
        const res = await this.http.post<{
          choices: Array<{
            message: {
              content: string | null;
              tool_calls?: Array<{
                id: string;
                type: string;
                function: { name: string; arguments: string };
              }>;
            };
          }>;
          usage?: {
            prompt_tokens: number;
            completion_tokens: number;
            cache_creation_input_tokens?: number;
            cache_read_input_tokens?: number;
            total_tokens: number;
          };
        }>('/chat/completions', safeBody, {
          ...(timeoutMs ? { timeout: timeoutMs } : {}),
        });

        const choice = res.data.choices[0];
        const content = choice?.message.content ?? '';
        const rawUsage = res.data.usage;
        const usage: LLMResponse['usage'] = rawUsage
          ? {
              promptTokens: rawUsage.prompt_tokens,
              completionTokens: rawUsage.completion_tokens,
              cacheCreationTokens: rawUsage.cache_creation_input_tokens ?? 0,
              cacheReadTokens: rawUsage.cache_read_input_tokens ?? 0,
              totalTokens: rawUsage.total_tokens,
            }
          : undefined;

        const rawToolCalls = choice?.message.tool_calls;
        const toolCalls: ToolCall[] | undefined =
          rawToolCalls && rawToolCalls.length > 0
            ? rawToolCalls.map((tc) => ({
                id: tc.id,
                type: 'function' as const,
                function: { name: tc.function.name, arguments: tc.function.arguments },
              }))
            : undefined;

        return { content, toolCalls, usage };
      }

      // ── Streaming path (default) ───────────────────────────────────────
      const res = await this.http.post('/chat/completions', safeBody, {
        responseType: 'stream',
        ...(timeoutMs ? { timeout: timeoutMs } : {}),
      });

      const rawStream = res.data as Readable;
      const chunkStream = this.parseSSE(rawStream);

      const idleTimeout =
        timeoutMs ||
        (process.env['LLM_TIMEOUT_MS'] ? parseInt(process.env['LLM_TIMEOUT_MS'], 10) : 120_000);

      const wrappedStream = pipe(
        chunkStream,
        wrapIdleTimeout(idleTimeout),
        wrapToolNameTrim(),
        wrapToolCallArgumentRepair(),
        wrapSanitizeMalformedToolCalls(),
        wrapReasoningFilter(thinkingLevel)
      );

      let content = '';
      let reasoning = '';
      const toolCallsMap = new Map<number, ToolCall>();
      let usage: LLMResponse['usage'];
      let citations: LLMResponse['citations'];

      for await (const chunk of wrappedStream) {
        if (chunk.content) content += chunk.content;
        if (chunk.reasoning) reasoning += chunk.reasoning;
        if (chunk.toolCallDelta) {
          const delta = chunk.toolCallDelta;
          let tc = toolCallsMap.get(delta.index);
          if (!tc) {
            tc = {
              id: delta.id || '',
              type: 'function',
              function: { name: delta.name || '', arguments: '' },
            };
            toolCallsMap.set(delta.index, tc);
          }
          if (delta.id) tc.id = delta.id;
          if (delta.name) tc.function.name = delta.name;
          if (delta.arguments) tc.function.arguments += delta.arguments;
        }
        if (chunk.usage) {
          usage = chunk.usage;
        }
      }

      const toolCalls = toolCallsMap.size > 0 ? Array.from(toolCallsMap.values()) : undefined;

      return { content, reasoning, toolCalls, usage, citations };
    } catch (err) {
      if (err instanceof AxiosError) {
        // Timeout detection — must come first; timeout errors may lack a response
        if (err.code === 'ECONNABORTED' || err.code === 'ETIMEDOUT') {
          throw new LLMTimeoutError(`LLM request timed out: ${err.message}`);
        }

        const status = err.response?.status;
        const body = err.response?.data as Record<string, unknown> | undefined;
        // `body['error']` may be a string (e.g. 'rate_limited') or an object with { message, code }
        const rawError = body?.['error'];
        const errorObj =
          rawError !== null && typeof rawError === 'object'
            ? (rawError as Record<string, unknown>)
            : undefined;
        const errorMsg: string | undefined =
          typeof rawError === 'string' ? rawError : (errorObj?.['message'] as string | undefined);
        const errorCode = errorObj?.['code'] as string | undefined;
        // Retry-After in seconds from the proxy response body (set by llmProxy when available)
        const retryAfterSec = body?.['retryAfter'] as number | undefined;

        if (status === 429) {
          const retryHint = retryAfterSec !== undefined ? ` Retry after: ${retryAfterSec}s.` : '';
          throw new BudgetExceededError(
            `Rate limited by upstream provider (HTTP 429): ${errorMsg ?? 'rate_limited'}.${retryHint}`
          );
        }

        if (status === 503) {
          throw new ProviderUnavailableError(
            `LLM provider unavailable (circuit open): ${errorMsg ?? err.message}`
          );
        }

        // Context overflow detection — HTTP 400 with overflow-related error text
        if (status === 400) {
          const combined = `${errorCode ?? ''} ${errorMsg ?? ''}`.toLowerCase();
          if (OVERFLOW_PATTERNS.some((p) => combined.includes(p))) {
            throw new ContextOverflowError(
              `Context window overflow: ${errorMsg ?? errorCode ?? 'context too long'}`
            );
          }
        }

        log('error', `LLM proxy error: ${status} — ${safeStringify(body)}`);
        throw new Error(`LLM proxy returned ${status}: ${errorMsg ?? err.message}`);
      }
      throw err;
    }
  }

  /**
   * Parse OpenAI-compatible SSE stream into an AsyncIterable of Chunks.
   */
  private async *parseSSE(stream: Readable): AsyncIterable<Chunk> {
    let buffer = '';
    for await (const chunk of stream) {
      buffer += chunk.toString();
      const lines = buffer.split('\n');
      buffer = lines.pop() || '';

      for (const line of lines) {
        const trimmed = line.trim();
        if (!trimmed || !trimmed.startsWith('data:')) continue;

        const dataStr = trimmed.substring(5).trim();
        if (dataStr === '[DONE]') return;

        try {
          const data = JSON.parse(dataStr);
          const chunk: Chunk = {};

          if (data.usage) {
            chunk.usage = {
              promptTokens: data.usage.prompt_tokens ?? 0,
              completionTokens: data.usage.completion_tokens ?? 0,
              cacheCreationTokens: data.usage.cache_creation_input_tokens ?? 0,
              cacheReadTokens: data.usage.cache_read_input_tokens ?? 0,
              totalTokens: data.usage.total_tokens ?? 0,
            };
          }

          const choice = data.choices?.[0];
          if (choice) {
            const delta = choice.delta;
            if (delta.content) chunk.content = delta.content;
            if (delta.reasoning_content) chunk.reasoning = delta.reasoning_content;
            if (choice.finish_reason) chunk.finishReason = choice.finish_reason;

            if (delta.tool_calls && Array.isArray(delta.tool_calls)) {
              for (const tc of delta.tool_calls) {
                yield {
                  ...chunk,
                  toolCallDelta: {
                    index: tc.index,
                    id: tc.id,
                    name: tc.function?.name,
                    arguments: tc.function?.arguments,
                  },
                };
              }
              continue;
            }
          }

          if (Object.keys(chunk).length > 0) {
            yield chunk;
          }
        } catch (e) {
          log('warn', `Failed to parse SSE data: ${dataStr} - ${e}`);
        }
      }
    }
  }
}

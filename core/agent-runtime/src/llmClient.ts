/**
 * LLM Client — HTTP client for the SERA Core LLM Proxy.
 *
 * Makes OpenAI-compatible requests to the Core proxy endpoint,
 * authenticating with the container's JWT identity token.
 */

import axios, { type AxiosInstance, AxiosError } from 'axios';
import { log } from './logger.js';

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
  content: string;
  tool_calls?: ToolCall[];
  tool_call_id?: string;
}

export interface LLMResponse {
  content: string;
  /** Chain-of-thought text, e.g. Qwen / DeepSeek reasoning_content. */
  reasoning?: string;
  toolCalls?: ToolCall[];
  usage?: {
    promptTokens: number;
    completionTokens: number;
    totalTokens: number;
  };
}

// ── Client ────────────────────────────────────────────────────────────────────

export class LLMClient {
  private http: AxiosInstance;
  private model: string;

  constructor(
    coreUrl: string,
    identityToken: string,
    model: string,
  ) {
    const timeoutMs = process.env['LLM_TIMEOUT_MS']
      ? parseInt(process.env['LLM_TIMEOUT_MS'], 10)
      : 120_000;

    this.model = model;
    this.http = axios.create({
      baseURL: `${coreUrl}/v1/llm`,
      headers: {
        'Content-Type': 'application/json',
        'Authorization': `Bearer ${identityToken}`,
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
  ): Promise<LLMResponse> {
    const body: Record<string, unknown> = {
      model: this.model,
      messages: messages.map((m) => ({
        role: m.role,
        content: m.content,
        ...(m.tool_calls ? { tool_calls: m.tool_calls } : {}),
        ...(m.tool_call_id ? { tool_call_id: m.tool_call_id } : {}),
      })),
    };

    if (tools && tools.length > 0) {
      body['tools'] = tools;
    }

    if (temperature !== undefined) {
      body['temperature'] = temperature;
    }

    // Debug: log tool names and message roles sent to LLM
    if (tools && tools.length > 0) {
      const toolNames = tools.map((t) => t.function.name).join(', ');
      log('debug', `LLM request: ${(body['messages'] as unknown[]).length} messages, ${tools.length} tools: [${toolNames}]`);
    }

    try {
      const res = await this.http.post('/chat/completions', body);
      const data = res.data as Record<string, unknown>;

      const choices = data['choices'] as Array<Record<string, unknown>> | undefined;
      const choice = choices?.[0];
      if (!choice) {
        return { content: '' };
      }

      const message = choice['message'] as Record<string, unknown>;
      const content = (message['content'] as string | null) || '';
      const reasoning = (message as Record<string, unknown>)['reasoning_content'] as string | undefined;

      let toolCalls: ToolCall[] | undefined;
      const rawToolCalls = message['tool_calls'] as Array<Record<string, unknown>> | undefined;
      if (rawToolCalls && rawToolCalls.length > 0) {
        toolCalls = rawToolCalls.map((tc) => {
          const fn = tc['function'] as Record<string, unknown>;
          return {
            id: tc['id'] as string,
            type: 'function' as const,
            function: {
              name: fn['name'] as string,
              arguments: fn['arguments'] as string,
            },
          };
        });
      }

      const rawUsage = data['usage'] as Record<string, number> | undefined;
      const usage = rawUsage
        ? {
            promptTokens: rawUsage['prompt_tokens'] ?? 0,
            completionTokens: rawUsage['completion_tokens'] ?? 0,
            totalTokens: rawUsage['total_tokens'] ?? 0,
          }
        : undefined;

      return { content, reasoning, toolCalls, usage };
    } catch (err) {
      if (err instanceof AxiosError) {
        // Timeout detection — must come first; timeout errors may lack a response
        if (err.code === 'ECONNABORTED' || err.code === 'ETIMEDOUT') {
          throw new LLMTimeoutError(`LLM request timed out: ${err.message}`);
        }

        const status = err.response?.status;
        const body = err.response?.data as Record<string, unknown> | undefined;
        const errorObj = body?.['error'] as Record<string, unknown> | undefined;
        const errorMsg = errorObj?.['message'] as string | undefined;
        const errorCode = errorObj?.['code'] as string | undefined;

        log('error', `LLM proxy error: ${status} — ${JSON.stringify(body)}`);

        if (status === 429) {
          throw new BudgetExceededError(`Token budget exceeded: ${errorMsg ?? 'rate limited'}`);
        }

        if (status === 503) {
          throw new ProviderUnavailableError(`LLM provider unavailable (circuit open): ${errorMsg ?? err.message}`);
        }

        // Context overflow detection — HTTP 400 with overflow-related error text
        if (status === 400) {
          const combined = `${errorCode ?? ''} ${errorMsg ?? ''}`.toLowerCase();
          if (OVERFLOW_PATTERNS.some((p) => combined.includes(p))) {
            throw new ContextOverflowError(`Context window overflow: ${errorMsg ?? errorCode ?? 'context too long'}`);
          }
        }

        throw new Error(`LLM proxy returned ${status}: ${errorMsg ?? err.message}`);
      }
      throw err;
    }
  }
}

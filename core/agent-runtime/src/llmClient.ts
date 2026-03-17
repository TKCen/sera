/**
 * LLM Client — HTTP client for the SERA Core LLM Proxy.
 *
 * Makes OpenAI-compatible requests to the Core proxy endpoint,
 * authenticating with the container's JWT identity token.
 */

import axios, { type AxiosInstance, AxiosError } from 'axios';
import { log } from './logger.js';

// ── Types ────────────────────────────────────────────────────────────────────

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
  toolCalls?: ToolCall[];
  usage?: {
    promptTokens: number;
    completionTokens: number;
    totalTokens: number;
  };
}

// ── Client ───────────────────────────────────────────────────────────────────

export class LLMClient {
  private http: AxiosInstance;
  private model: string;

  constructor(
    coreUrl: string,
    identityToken: string,
    model: string,
  ) {
    this.model = model;
    this.http = axios.create({
      baseURL: `${coreUrl}/v1/llm`,
      headers: {
        'Content-Type': 'application/json',
        'Authorization': `Bearer ${identityToken}`,
      },
      timeout: 120_000, // 2 minutes for LLM calls
    });
  }

  /**
   * Send a chat completion request through the Core LLM proxy.
   * Returns parsed content + optional tool calls.
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
      body.tools = tools;
    }

    if (temperature !== undefined) {
      body.temperature = temperature;
    }

    try {
      const res = await this.http.post('/chat/completions', body);
      const data = res.data;

      // Parse OpenAI-compatible response
      const choice = data.choices?.[0];
      if (!choice) {
        return { content: '' };
      }

      const message = choice.message;
      const content = message.content || '';

      let toolCalls: ToolCall[] | undefined;
      if (message.tool_calls && message.tool_calls.length > 0) {
        toolCalls = message.tool_calls.map((tc: any) => ({
          id: tc.id,
          type: 'function' as const,
          function: {
            name: tc.function.name,
            arguments: tc.function.arguments,
          },
        }));
      }

      const usage = data.usage
        ? {
            promptTokens: data.usage.prompt_tokens,
            completionTokens: data.usage.completion_tokens,
            totalTokens: data.usage.total_tokens,
          }
        : undefined;

      return { content, toolCalls, usage };
    } catch (err) {
      if (err instanceof AxiosError) {
        const status = err.response?.status;
        const body = err.response?.data;
        log('error', `LLM proxy error: ${status} — ${JSON.stringify(body)}`);

        if (status === 429) {
          throw new Error(`Token budget exceeded: ${body?.error?.message || 'rate limited'}`);
        }

        throw new Error(`LLM proxy returned ${status}: ${body?.error?.message || err.message}`);
      }
      throw err;
    }
  }
}

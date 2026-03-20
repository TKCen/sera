/**
 * LiteLLMClient — thin HTTP client for the LiteLLM proxy.
 *
 * sera-core uses this to forward agent LLM requests to LiteLLM, which
 * handles provider routing, retries, and fallbacks. All governance
 * (budget, metering, circuit breaking) stays in sera-core.
 *
 * @see docs/ARCHITECTURE.md § Provider Gateway: LiteLLM
 * @see docs/epics/04-llm-proxy-and-governance.md § Story 4.1
 */

import axios from 'axios';
import type { AxiosInstance, AxiosResponse } from 'axios';
import type { IncomingMessage } from 'http';
import { Logger } from '../lib/logger.js';

const logger = new Logger('LiteLLMClient');

// ── Types ─────────────────────────────────────────────────────────────────────

export interface ChatMessage {
  role: 'system' | 'user' | 'assistant' | 'tool';
  content: string | null;
  tool_calls?: unknown[];
  tool_call_id?: string;
}

export interface ChatCompletionRequest {
  model: string;
  messages: ChatMessage[];
  temperature?: number;
  tools?: unknown[];
  stream?: boolean;
}

export interface ChatCompletionUsage {
  prompt_tokens: number;
  completion_tokens: number;
  total_tokens: number;
}

export interface ChatCompletionResponse {
  id: string;
  object: 'chat.completion';
  created: number;
  model: string;
  choices: {
    index: number;
    message: {
      role: string;
      content: string | null;
      tool_calls?: unknown[];
    };
    finish_reason: string;
  }[];
  usage?: ChatCompletionUsage;
}

// ── Client ────────────────────────────────────────────────────────────────────

const LITELLM_BASE_URL = process.env.LLM_BASE_URL ?? 'http://litellm:4000/v1';
const LITELLM_API_KEY = process.env.LLM_API_KEY ?? 'sera-master-key';

export class LiteLLMClient {
  private readonly http: AxiosInstance;

  constructor(baseUrl: string = LITELLM_BASE_URL, apiKey: string = LITELLM_API_KEY) {
    this.http = axios.create({
      baseURL: baseUrl,
      headers: {
        Authorization: `Bearer ${apiKey}`,
        'Content-Type': 'application/json',
      },
      timeout: 120_000,
    });
  }

  /**
   * Send a non-streaming chat completion to LiteLLM.
   * Adds the X-SERA-Agent-Id header so LiteLLM logs correlate to agents.
   */
  async chatCompletion(
    request: ChatCompletionRequest,
    agentId: string,
    latencyStart: number = Date.now()
  ): Promise<{ response: ChatCompletionResponse; latencyMs: number }> {
    const requestBody = { ...request, stream: false };

    logger.debug(
      `LiteLLM request | agent=${agentId} model=${request.model} messages=${request.messages.length}`
    );

    const axiosResponse: AxiosResponse<ChatCompletionResponse> = await this.http.post(
      '/chat/completions',
      requestBody,
      {
        headers: {
          'X-SERA-Agent-Id': agentId,
        },
      }
    );

    const latencyMs = Date.now() - latencyStart;
    return { response: axiosResponse.data, latencyMs };
  }

  /**
   * Send a streaming chat completion request and return the raw Node.js stream.
   * The caller is responsible for piping the response to the client.
   */
  async chatCompletionStream(
    request: ChatCompletionRequest,
    agentId: string
  ): Promise<IncomingMessage> {
    const requestBody = { ...request, stream: true };

    const axiosResponse = await this.http.post('/chat/completions', requestBody, {
      headers: {
        'X-SERA-Agent-Id': agentId,
        Accept: 'text/event-stream',
      },
      responseType: 'stream',
    });

    return axiosResponse.data as IncomingMessage;
  }

  /**
   * List available models from LiteLLM's model info endpoint.
   */
  async listModels(): Promise<{ id: string; object: string; owned_by?: string }[]> {
    try {
      const response = await this.http.get('/model/info');
      // LiteLLM returns { data: [...] } or a custom format
      const data = response.data as Record<string, unknown>;
      if (Array.isArray(data.data)) {
        return (data.data as Record<string, unknown>[]).map((m) => ({
          id: String(m['model_name'] ?? m['id'] ?? ''),
          object: 'model',
          owned_by: String(m['owned_by'] ?? 'litellm'),
        }));
      }
    } catch (err: any) {
      logger.warn(`Failed to fetch LiteLLM model list: ${err.message}`);
    }
    return [];
  }

  /**
   * Add a new model/provider to LiteLLM's live configuration.
   */
  async addModel(modelConfig: Record<string, unknown>): Promise<Record<string, unknown>> {
    const response = await this.http.post('/model/new', modelConfig);
    return response.data as Record<string, unknown>;
  }

  /**
   * Remove a model from LiteLLM's live configuration.
   */
  async deleteModel(modelName: string): Promise<void> {
    await this.http.post('/model/delete', { model_name: modelName });
  }

  /**
   * Send a minimal test completion to verify a model is reachable.
   */
  async testModel(modelName: string): Promise<{ ok: boolean; latencyMs: number; error?: string }> {
    const start = Date.now();
    try {
      await this.http.post(
        '/chat/completions',
        {
          model: modelName,
          messages: [{ role: 'user', content: 'ping' }],
          max_tokens: 1,
        },
        { headers: { 'X-SERA-Agent-Id': 'sera-core-test' } }
      );
      return { ok: true, latencyMs: Date.now() - start };
    } catch (err: any) {
      return { ok: false, latencyMs: Date.now() - start, error: err.message };
    }
  }
}

// Singleton for module-level use
let _instance: LiteLLMClient | null = null;

export function getLiteLLMClient(): LiteLLMClient {
  if (!_instance) {
    _instance = new LiteLLMClient();
  }
  return _instance;
}

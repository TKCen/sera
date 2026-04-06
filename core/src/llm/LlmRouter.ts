/**
 * LlmRouter — in-process LLM gateway powered by @mariozechner/pi-ai.
 *
 * Replaces the LiteLLM sidecar container.  All provider calls happen inside the
 * sera-core process; routing is driven by ProviderRegistry (backed by
 * core/config/providers.json).
 *
 * Responsibilities:
 *   • Convert incoming OpenAI-format ChatCompletionRequest → pi-mono Context
 *   • Dispatch to the correct pi-mono provider (openai-completions / anthropic-messages)
 *   • Convert pi-mono AssistantMessage → OpenAI-format ChatCompletionResponse
 *   • For streaming: translate pi-mono's async event stream to OpenAI SSE bytes
 *     piped through a Node.js Readable
 *
 * API key resolution order (per call):
 *   1. config.apiKey      — literal key stored in providers.json
 *   2. config.apiKeyEnvVar — runtime env var name  (e.g. 'LM_STUDIO_KEY')
 *   3. pi-mono fallback   — getEnvApiKey(provider) reads OPENAI_API_KEY,
 *                           ANTHROPIC_API_KEY, GROQ_API_KEY, … automatically
 *
 * @see core/src/llm/ProviderRegistry.ts  — provider config and model-name mapping
 * @see docs/epics/04-llm-proxy-and-governance.md
 */

import { PassThrough } from 'stream';
import type { Readable } from 'stream';
import type {
  Model,
  Api,
  Provider,
  Context,
  Message,
  AssistantMessage,
  UserMessage,
  ToolResultMessage,
  TextContent,
  ToolCall,
  StreamOptions,
} from '@mariozechner/pi-ai';
import { streamOpenAICompletions } from '@mariozechner/pi-ai/openai-completions';
import { streamAnthropic } from '@mariozechner/pi-ai/anthropic';
import type { AssistantMessageEventStream } from '@mariozechner/pi-ai';
import { Logger } from '../lib/logger.js';
import type { ProviderConfig, ProviderRegistry } from './ProviderRegistry.js';
import type { ChatMessage } from '../agents/types.js';
import { validateProviderBaseUrl } from './url-validation.js';

const logger = new Logger('LlmRouter');

// ── Re-exported types (backward-compat with LiteLLMClient consumers) ──────────

export type { ChatMessage };

export interface ChatCompletionRequest {
  model: string;
  messages: ChatMessage[];
  temperature?: number;
  tools?: unknown[];
  stream?: boolean;
  /** Thinking/reasoning level for capable models (maps to pi-mono reasoning option). */
  thinkingLevel?: string;
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

// ── Internal helpers ──────────────────────────────────────────────────────────

const ZERO_USAGE = {
  input: 0,
  output: 0,
  cacheRead: 0,
  cacheWrite: 0,
  totalTokens: 0,
  cost: { input: 0, output: 0, cacheRead: 0, cacheWrite: 0, total: 0 },
};

/**
 * Convert an OpenAI-format message list to a pi-mono Context.
 * System messages become context.systemPrompt (pi-mono keeps it separate).
 */
function toContext(request: ChatCompletionRequest): Context {
  const messages: Message[] = [];
  let systemPrompt: string | undefined;

  for (const msg of request.messages) {
    const ts = Date.now();

    switch (msg.role) {
      case 'system':
        systemPrompt = msg.content ?? '';
        break;

      case 'user': {
        const userMsg: UserMessage = {
          role: 'user',
          content: msg.content ?? '',
          timestamp: ts,
        };
        messages.push(userMsg);
        break;
      }

      case 'assistant': {
        const content: (TextContent | ToolCall)[] = [];

        if (msg.content) {
          content.push({ type: 'text', text: msg.content });
        }

        if (Array.isArray(msg.tool_calls)) {
          for (const tc of msg.tool_calls as unknown as Record<string, unknown>[]) {
            let parsedArgs: Record<string, unknown> = {};
            try {
              const functionBlock = tc['function'] as Record<string, unknown>;
              parsedArgs = functionBlock?.['arguments']
                ? (JSON.parse(functionBlock['arguments'] as string) as Record<string, unknown>)
                : {};
            } catch {
              parsedArgs = {};
            }
            const toolCall: ToolCall = {
              type: 'toolCall',
              id: (tc['id'] as string) ?? '',
              name: ((tc['function'] as Record<string, unknown>)?.['name'] as string) ?? '',
              arguments: parsedArgs,
            };
            content.push(toolCall);
          }
        }

        // Construct a minimal historical AssistantMessage.
        // pi-mono providers use role + content for history; api/provider/model
        // are metadata that don't affect how the message is sent upstream.
        const asstMsg: AssistantMessage = {
          role: 'assistant',
          content,
          api: 'openai-completions',
          provider: 'openai',
          model: request.model,
          usage: ZERO_USAGE,
          stopReason: 'stop',
          timestamp: ts,
        };
        messages.push(asstMsg);
        break;
      }

      case 'tool': {
        // OpenAI tool-result messages don't include toolName; pass empty string.
        const toolResult: ToolResultMessage = {
          role: 'toolResult',
          toolCallId: msg.tool_call_id ?? '',
          toolName: '',
          content: [{ type: 'text', text: msg.content ?? '' }],
          isError: false,
          timestamp: ts,
        };
        messages.push(toolResult);
        break;
      }
    }
  }

  // Convert OpenAI tool definitions to pi-mono Tool format.
  const tools =
    Array.isArray(request.tools) && request.tools.length > 0
      ? (request.tools as Record<string, unknown>[])
          .filter((t) => t['type'] === 'function')
          .map((t) => {
            const fn = t['function'] as Record<string, unknown>;
            return {
              name: (fn?.['name'] as string) ?? '',
              description: (fn?.['description'] as string) ?? '',
              parameters: (fn?.['parameters'] as Record<string, unknown>) ?? {},
            };
          })
      : undefined;

  const ctx: Context = {
    messages,
    systemPrompt: systemPrompt ?? '',
  };
  if (tools && tools.length > 0) {
    // Assert NonNullable since we already checked tools is truthy
    ctx.tools = tools as NonNullable<Context['tools']>;
  }
  return ctx;
}

/** Map a pi-mono StopReason to an OpenAI finish_reason string. */
function toFinishReason(stopReason: string): string {
  if (stopReason === 'toolUse') return 'tool_calls';
  if (stopReason === 'length') return 'length';
  return 'stop';
}

/** Convert a completed pi-mono AssistantMessage to OpenAI ChatCompletionResponse. */
function toCompletionResponse(msg: AssistantMessage, modelName: string): ChatCompletionResponse {
  let textContent = '';
  const toolCalls: unknown[] = [];

  for (const block of msg.content) {
    if (block.type === 'text') {
      textContent += block.text;
    } else if (block.type === 'toolCall') {
      toolCalls.push({
        id: block.id,
        type: 'function',
        function: {
          name: block.name,
          arguments:
            typeof block.arguments === 'string' ? block.arguments : JSON.stringify(block.arguments),
        },
      });
    }
  }

  return {
    id: `chatcmpl-${Date.now()}`,
    object: 'chat.completion',
    created: Math.floor(Date.now() / 1000),
    model: modelName,
    choices: [
      {
        index: 0,
        message: {
          role: 'assistant',
          content: textContent || null,
          ...(toolCalls.length > 0 ? { tool_calls: toolCalls } : {}),
        },
        finish_reason: toFinishReason(msg.stopReason),
      },
    ],
    usage: {
      prompt_tokens: msg.usage.input,
      completion_tokens: msg.usage.output,
      total_tokens: msg.usage.totalTokens,
    },
  };
}

/** Inactivity timeout for streaming responses (ms). Configurable via env var. */
const STREAM_INACTIVITY_TIMEOUT_MS = parseInt(
  process.env['STREAM_INACTIVITY_TIMEOUT_MS'] ?? '30000',
  10
);

/**
 * Bridge a pi-mono AssistantMessageEventStream to a Node.js Readable that emits
 * OpenAI Server-Sent Events.  The caller can pipe() this to an Express Response.
 *
 * Events translated:
 *   text_delta      → data: { choices[0].delta.content }
 *   toolcall_end    → data: { choices[0].delta.tool_calls }
 *   done            → data: { choices[0].finish_reason } + data: [DONE]
 *   error           → stream.destroy(err)
 *
 * An inactivity watchdog fires if no chunk is received for
 * STREAM_INACTIVITY_TIMEOUT_MS milliseconds, sending an error SSE and
 * destroying the stream.
 */
function eventStreamToReadable(
  eventStream: AssistantMessageEventStream,
  modelName: string
): Readable {
  const passThrough = new PassThrough();
  const id = `chatcmpl-${Date.now()}`;
  const created = Math.floor(Date.now() / 1000);

  const chunk = (delta: object, finishReason: string | null) =>
    `data: ${JSON.stringify({
      id,
      object: 'chat.completion.chunk',
      created,
      model: modelName,
      choices: [{ index: 0, delta, finish_reason: finishReason }],
    })}\n\n`;

  // ── Inactivity watchdog ────────────────────────────────────────────────────
  let inactivityTimer: ReturnType<typeof setTimeout> | null = null;

  const resetTimer = () => {
    if (inactivityTimer !== null) clearTimeout(inactivityTimer);
    inactivityTimer = setTimeout(() => {
      if (!passThrough.destroyed) {
        logger.warn(
          `Stream inactivity timeout after ${STREAM_INACTIVITY_TIMEOUT_MS}ms | model=${modelName}`
        );
        const errorEvent = `data: ${JSON.stringify({
          id,
          object: 'chat.completion.chunk',
          created,
          model: modelName,
          choices: [{ index: 0, delta: {}, finish_reason: 'error' }],
          error: { message: 'Stream inactivity timeout', type: 'timeout' },
        })}\n\n`;
        passThrough.push(errorEvent);
        passThrough.destroy(new Error('Stream inactivity timeout'));
      }
    }, STREAM_INACTIVITY_TIMEOUT_MS);
  };

  const clearTimer = () => {
    if (inactivityTimer !== null) {
      clearTimeout(inactivityTimer);
      inactivityTimer = null;
    }
  };
  // ──────────────────────────────────────────────────────────────────────────

  (async () => {
    resetTimer();
    try {
      for await (const event of eventStream) {
        if (passThrough.destroyed) break;
        resetTimer();

        if (event.type === 'text_delta') {
          passThrough.push(chunk({ content: event.delta }, null));
        } else if (event.type === 'toolcall_end') {
          const tc = event.toolCall;
          passThrough.push(
            chunk(
              {
                tool_calls: [
                  {
                    index: 0,
                    id: tc.id,
                    type: 'function',
                    function: {
                      name: tc.name,
                      arguments:
                        typeof tc.arguments === 'string'
                          ? tc.arguments
                          : JSON.stringify(tc.arguments),
                    },
                  },
                ],
              },
              null
            )
          );
        } else if (event.type === 'done') {
          clearTimer();
          passThrough.push(chunk({}, toFinishReason(event.reason)));
          passThrough.push('data: [DONE]\n\n');
          passThrough.push(null);
        } else if (event.type === 'error') {
          clearTimer();
          const errMsg = event.error.errorMessage ?? 'LLM provider error';
          passThrough.destroy(new Error(errMsg));
        }
      }
    } catch (err) {
      clearTimer();
      passThrough.destroy(err as Error);
    }
  })();

  return passThrough;
}

// ── Public types ──────────────────────────────────────────────────────────────

/** Sanitised model entry returned by GET /api/providers — no API keys. */
export interface ModelListItem {
  modelName: string;
  api: string;
  provider?: string;
  baseUrl?: string;
  description?: string;
  dynamicProviderId?: string;
}

// ── LlmRouter ─────────────────────────────────────────────────────────────────

export class LlmRouter {
  constructor(private readonly registry: ProviderRegistry) {}

  /** Expose the underlying registry for default-model management. */
  getRegistry(): ProviderRegistry {
    return this.registry;
  }

  // ── Internal ───────────────────────────────────────────────────────────────

  private buildModel(config: ProviderConfig): Model<Api> {
    const isGoogleAIStudio =
      config.provider === 'google' ||
      (config.baseUrl ?? '').includes('generativelanguage.googleapis.com');

    // Google AI Studio's OpenAI-compat endpoint rejects `store` and
    // `developer` role.  Override pi-mono's auto-detection which marks
    // every non-"isNonStandard" provider as supporting those features.
    const compat =
      config.api === 'openai-completions' && isGoogleAIStudio
        ? { supportsStore: false, supportsDeveloperRole: false }
        : undefined;

    // Auto-detect reasoning/thinking models by name if not explicitly set.
    // These models emit `reasoning_content` tokens before `content` tokens;
    // pi-mono needs to know so it correctly separates thinking from answer.
    const reasoning = config.reasoning ?? LlmRouter.isReasoningModel(config.modelName);

    return {
      id: config.modelName,
      name: config.description ?? config.modelName,
      api: config.api as Api,
      provider: (config.provider ?? 'openai') as Provider,
      baseUrl: config.baseUrl ?? '',
      reasoning,
      input: ['text'],
      cost: { input: 0, output: 0, cacheRead: 0, cacheWrite: 0 },
      contextWindow: config.contextWindow ?? 128_000,
      maxTokens: config.maxTokens ?? 4_096,
      ...(compat ? { compat } : {}),
    };
  }

  /**
   * Heuristic: detect whether a model name indicates a reasoning/thinking model.
   * These models separate reasoning_content from content in streaming.
   */
  /**
   * Map agent-facing thinking level names to pi-mono ThinkingLevel values.
   * 'off' is handled before this is called (not passed to pi-mono).
   */
  private static mapThinkingLevel(level: string): string {
    const mapping: Record<string, string> = {
      minimal: 'minimal',
      low: 'low',
      medium: 'medium',
      high: 'high',
      max: 'xhigh',
    };
    return mapping[level] ?? 'medium';
  }

  private static isReasoningModel(modelName: string): boolean {
    const lower = modelName.toLowerCase();
    return (
      /qwen3/i.test(lower) ||
      /deepseek-r1/i.test(lower) ||
      /\bo[13]-/i.test(lower) || // o1-*, o3-*
      /\bo[13]$/i.test(lower) || // just "o1" or "o3"
      /thinking/i.test(lower) ||
      /reasoner/i.test(lower)
    );
  }

  /**
   * Resolve the API key for a config entry.
   * Returns undefined when no explicit key is set — pi-mono will then fall back
   * to getEnvApiKey(model.provider) automatically.
   */
  private resolveApiKey(config: ProviderConfig): string | undefined {
    if (config.apiKey) return config.apiKey;
    if (config.apiKeyEnvVar) {
      // sera-secret: refs are resolved at startup by hydrateSecrets() and placed in apiKey.
      // If we reach here with a sera-secret: ref, hydration didn't run or failed — skip.
      if (config.apiKeyEnvVar.startsWith('sera-secret:')) return undefined;
      return process.env[config.apiKeyEnvVar];
    }
    // Local providers don't require auth — return placeholder so pi-mono
    // includes an Authorization header (LM Studio accepts any value).
    if (config.provider === 'lmstudio' || config.provider === 'ollama') {
      return 'lm-studio';
    }
    // Standard env var fallback for cloud providers
    if (config.provider) {
      const standardEnvVars: Record<string, string[]> = {
        openai: ['OPENAI_API_KEY'],
        anthropic: ['ANTHROPIC_API_KEY'],
        google: ['GOOGLE_API_KEY', 'GEMINI_API_KEY'],
        groq: ['GROQ_API_KEY'],
        mistral: ['MISTRAL_API_KEY'],
        openrouter: ['OPENROUTER_API_KEY'],
        kilocode: ['KILOCODE_API_KEY'],
      };
      const envVars = standardEnvVars[config.provider];
      if (envVars) {
        for (const v of envVars) {
          if (process.env[v]) return process.env[v];
        }
      }
    }
    return undefined;
  }

  /** Dispatch to the appropriate pi-mono provider function. */
  private dispatch(
    config: ProviderConfig,
    context: Context,
    extraOptions?: StreamOptions
  ): AssistantMessageEventStream {
    // SSRF protection: validate baseUrl before sending any API-key-bearing request.
    if (config.baseUrl) {
      const check = validateProviderBaseUrl(config.baseUrl, config.provider);
      if (!check.valid) {
        throw new Error(`Provider baseUrl rejected: ${check.reason}`);
      }
    }

    const model = this.buildModel(config);
    const apiKey = this.resolveApiKey(config);
    const opts: StreamOptions = {
      ...(apiKey ? { apiKey } : {}),
      ...extraOptions,
    };

    switch (config.api) {
      case 'openai-completions':
        return streamOpenAICompletions(model as Model<'openai-completions'>, context, opts);

      case 'anthropic-messages':
        return streamAnthropic(model as Model<'anthropic-messages'>, context, opts);

      default:
        throw new Error(
          `Unsupported provider API '${String(config.api)}' for model '${config.modelName}'`
        );
    }
  }

  // ── Public interface (mirrors LiteLLMClient) ───────────────────────────────

  /**
   * Execute a non-streaming chat completion.
   * Token usage is extracted from the completed AssistantMessage.
   * Iterates the failoverModels chain on dispatch errors.
   */
  async chatCompletion(
    request: ChatCompletionRequest,
    agentId: string,
    latencyStart: number = Date.now()
  ): Promise<{ response: ChatCompletionResponse; latencyMs: number }> {
    logger.debug(
      `LlmRouter | agent=${agentId} model=${request.model} messages=${request.messages.length}`
    );

    const config = this.registry.resolve(request.model);
    const failoverChain = [request.model, ...(config.failoverModels ?? [])];
    let lastError: Error | null = null;

    for (const modelName of failoverChain) {
      try {
        const modelConfig = this.registry.resolve(modelName);
        const context = toContext(request);
        const opts: StreamOptions = {
          ...(request.temperature !== undefined ? { temperature: request.temperature } : {}),
          ...(request.thinkingLevel
            ? { reasoning: LlmRouter.mapThinkingLevel(request.thinkingLevel) }
            : {}),
        };

        const eventStream = this.dispatch(modelConfig, context, opts);
        const msg = await eventStream.result();

        const latencyMs = Date.now() - latencyStart;
        if (modelName !== request.model) {
          logger.warn(`Failover: ${request.model} → ${modelName} | agent=${agentId}`);
        }
        logger.debug(
          `LlmRouter done | agent=${agentId} model=${modelName} tokens=${msg.usage.totalTokens} latency=${latencyMs}ms`
        );

        return { response: toCompletionResponse(msg, modelName), latencyMs };
      } catch (err) {
        lastError = err as Error;
        logger.warn(
          `LlmRouter dispatch failed for model=${modelName} | agent=${agentId}: ${lastError.message}`
        );
        // Continue to next model in failover chain
      }
    }

    throw lastError ?? new Error('All models in failover chain failed');
  }

  /**
   * Start a streaming completion and return a Readable that emits OpenAI SSE.
   * The caller is responsible for piping the stream to the HTTP response.
   * Iterates the failoverModels chain on dispatch errors.
   */
  async chatCompletionStream(request: ChatCompletionRequest, agentId: string): Promise<Readable> {
    logger.debug(`LlmRouter stream | agent=${agentId} model=${request.model}`);

    const config = this.registry.resolve(request.model);
    const failoverChain = [request.model, ...(config.failoverModels ?? [])];
    let lastError: Error | null = null;

    for (const modelName of failoverChain) {
      try {
        const modelConfig = this.registry.resolve(modelName);
        const context = toContext(request);
        const opts: StreamOptions = {
          ...(request.temperature !== undefined ? { temperature: request.temperature } : {}),
          ...(request.thinkingLevel
            ? { reasoning: LlmRouter.mapThinkingLevel(request.thinkingLevel) }
            : {}),
        };

        const eventStream = this.dispatch(modelConfig, context, opts);
        if (modelName !== request.model) {
          logger.warn(`Failover: ${request.model} → ${modelName} | agent=${agentId}`);
        }
        return eventStreamToReadable(eventStream, modelName);
      } catch (err) {
        lastError = err as Error;
        logger.warn(
          `LlmRouter stream dispatch failed for model=${modelName} | agent=${agentId}: ${lastError.message}`
        );
        // Continue to next model in failover chain
      }
    }

    throw lastError ?? new Error('All models in failover chain failed');
  }

  /** List all explicitly registered models with enough info for the UI. API keys are omitted. */
  async listModels(): Promise<ModelListItem[]> {
    return this.registry.listWithStatus().map((cfg) => {
      const item: ModelListItem = { modelName: cfg.modelName, api: cfg.api };
      if (cfg.provider !== undefined) item.provider = cfg.provider;
      if (cfg.baseUrl !== undefined) item.baseUrl = cfg.baseUrl;
      if (cfg.description !== undefined) item.description = cfg.description;
      if (cfg.dynamicProviderId !== undefined) item.dynamicProviderId = cfg.dynamicProviderId;
      const extra = item as unknown as Record<string, unknown>;
      extra.authStatus = cfg.authStatus;
      if (cfg.contextWindow !== undefined) extra.contextWindow = cfg.contextWindow;
      if (cfg.maxTokens !== undefined) extra.maxTokens = cfg.maxTokens;
      if (cfg.contextStrategy !== undefined) extra.contextStrategy = cfg.contextStrategy;
      if (cfg.contextHighWaterMark !== undefined)
        extra.contextHighWaterMark = cfg.contextHighWaterMark;
      if (cfg.contextCompactionModel !== undefined)
        extra.contextCompactionModel = cfg.contextCompactionModel;
      return item;
    });
  }

  /** Register a new provider and persist to config file. */
  async addModel(config: ProviderConfig): Promise<{ modelName: string; api: string }> {
    this.registry.register(config);
    await this.registry.save();
    logger.info(`Provider registered | model=${config.modelName} api=${config.api}`);
    return { modelName: config.modelName, api: config.api };
  }

  /** Remove a provider and persist to config file. */
  async deleteModel(modelName: string): Promise<void> {
    const removed = this.registry.unregister(modelName);
    if (!removed) {
      throw new Error(`No provider registered for model '${modelName}'`);
    }
    await this.registry.save();
    logger.info(`Provider removed | model=${modelName}`);
  }

  /**
   * Return the raw pi-mono AssistantMessageEventStream for a request.
   * Used by LlmRouterProvider to implement the LLMProvider interface.
   */
  getEventStream(request: ChatCompletionRequest): AssistantMessageEventStream {
    const cfg = this.registry.resolve(request.model);
    const context = toContext(request);
    const opts: StreamOptions = {
      ...(request.temperature !== undefined ? { temperature: request.temperature } : {}),
    };
    return this.dispatch(cfg, context, opts);
  }

  /** Send a minimal test completion to verify a model is reachable. */
  async testModel(modelName: string): Promise<{ ok: boolean; latencyMs: number; error?: string }> {
    const start = Date.now();
    try {
      const config = this.registry.resolve(modelName);
      const context: Context = {
        messages: [{ role: 'user', content: 'ping', timestamp: Date.now() } as UserMessage],
      };
      const eventStream = this.dispatch(config, context, { maxTokens: 1 });
      await eventStream.result();
      return { ok: true, latencyMs: Date.now() - start };
    } catch (err: unknown) {
      return { ok: false, latencyMs: Date.now() - start, error: (err as Error).message };
    }
  }
}

// Singleton
let _instance: LlmRouter | null = null;

export function getLlmRouter(registry: ProviderRegistry): LlmRouter {
  if (!_instance) {
    _instance = new LlmRouter(registry);
  }
  return _instance;
}

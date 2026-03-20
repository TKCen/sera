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

const logger = new Logger('LlmRouter');

// ── Re-exported types (backward-compat with LiteLLMClient consumers) ──────────

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
        // pi-mono separates systemPrompt from the message list
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
          for (const tc of msg.tool_calls as Record<string, any>[]) {
            let parsedArgs: Record<string, unknown> = {};
            try {
              parsedArgs = tc['function']?.['arguments']
                ? (JSON.parse(tc['function']['arguments'] as string) as Record<string, unknown>)
                : {};
            } catch {
              parsedArgs = {};
            }
            const toolCall: ToolCall = {
              type: 'toolCall',
              id: tc['id'] ?? '',
              name: tc['function']?.['name'] ?? '',
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
      ? (request.tools as Record<string, any>[])
          .filter((t) => t['type'] === 'function')
          .map((t) => ({
            name: t['function']?.['name'] ?? '',
            description: t['function']?.['description'] ?? '',
            parameters: t['function']?.['parameters'] ?? {},
          }))
      : undefined;

  return {
    ...(systemPrompt !== undefined ? { systemPrompt } : {}),
    messages,
    ...(tools ? { tools } : {}),
  };
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

/**
 * Bridge a pi-mono AssistantMessageEventStream to a Node.js Readable that emits
 * OpenAI Server-Sent Events.  The caller can pipe() this to an Express Response.
 *
 * Events translated:
 *   text_delta      → data: { choices[0].delta.content }
 *   toolcall_end    → data: { choices[0].delta.tool_calls }
 *   done            → data: { choices[0].finish_reason } + data: [DONE]
 *   error           → stream.destroy(err)
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

  (async () => {
    try {
      for await (const event of eventStream) {
        if (passThrough.destroyed) break;

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
          passThrough.push(chunk({}, toFinishReason(event.reason)));
          passThrough.push('data: [DONE]\n\n');
          passThrough.push(null);
        } else if (event.type === 'error') {
          const errMsg = event.error.errorMessage ?? 'LLM provider error';
          passThrough.destroy(new Error(errMsg));
        }
      }
    } catch (err) {
      passThrough.destroy(err as Error);
    }
  })();

  return passThrough;
}

// ── LlmRouter ─────────────────────────────────────────────────────────────────

export class LlmRouter {
  constructor(private readonly registry: ProviderRegistry) {}

  // ── Internal ───────────────────────────────────────────────────────────────

  private buildModel(config: ProviderConfig): Model<Api> {
    return {
      id: config.modelName,
      name: config.description ?? config.modelName,
      api: config.api as Api,
      provider: (config.provider ?? 'openai') as Provider,
      baseUrl: config.baseUrl ?? '',
      reasoning: false,
      input: ['text'],
      cost: { input: 0, output: 0, cacheRead: 0, cacheWrite: 0 },
      contextWindow: 128_000,
      maxTokens: 4_096,
    };
  }

  /**
   * Resolve the API key for a config entry.
   * Returns undefined when no explicit key is set — pi-mono will then fall back
   * to getEnvApiKey(model.provider) automatically.
   */
  private resolveApiKey(config: ProviderConfig): string | undefined {
    if (config.apiKey) return config.apiKey;
    if (config.apiKeyEnvVar) return process.env[config.apiKeyEnvVar];
    return undefined;
  }

  /** Dispatch to the appropriate pi-mono provider function. */
  private dispatch(
    config: ProviderConfig,
    context: Context,
    extraOptions?: StreamOptions
  ): AssistantMessageEventStream {
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
    const context = toContext(request);
    const opts: StreamOptions = {
      ...(request.temperature !== undefined ? { temperature: request.temperature } : {}),
    };

    const eventStream = this.dispatch(config, context, opts);
    const msg = await eventStream.result();

    const latencyMs = Date.now() - latencyStart;
    logger.debug(
      `LlmRouter done | agent=${agentId} tokens=${msg.usage.totalTokens} latency=${latencyMs}ms`
    );

    return { response: toCompletionResponse(msg, request.model), latencyMs };
  }

  /**
   * Start a streaming completion and return a Readable that emits OpenAI SSE.
   * The caller is responsible for piping the stream to the HTTP response.
   */
  async chatCompletionStream(request: ChatCompletionRequest, agentId: string): Promise<Readable> {
    logger.debug(`LlmRouter stream | agent=${agentId} model=${request.model}`);

    const config = this.registry.resolve(request.model);
    const context = toContext(request);
    const opts: StreamOptions = {
      ...(request.temperature !== undefined ? { temperature: request.temperature } : {}),
    };

    const eventStream = this.dispatch(config, context, opts);
    return eventStreamToReadable(eventStream, request.model);
  }

  /** List all explicitly registered providers as OpenAI-style model objects. */
  async listModels(): Promise<{ id: string; object: string; owned_by?: string }[]> {
    return this.registry.list().map((cfg) => ({
      id: cfg.modelName,
      object: 'model',
      owned_by: cfg.provider ?? 'custom',
    }));
  }

  /** Register a new provider and persist to config file. */
  async addModel(config: ProviderConfig): Promise<Record<string, unknown>> {
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
    } catch (err: any) {
      return { ok: false, latencyMs: Date.now() - start, error: err.message as string };
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

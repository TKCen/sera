/**
 * LlmRouterProvider — implements the LLMProvider interface backed by
 * LlmRouter (pi-mono in-process gateway).
 *
 * Replaces OpenAIProvider for agents that use the new provider registry
 * (core/config/providers.json).  All provider calls are routed in-process
 * via LlmRouter → ProviderRegistry → @mariozechner/pi-ai.
 */

import type { LLMProvider, LLMResponse, LLMStreamChunk, ToolDefinition } from './types.js';
import type { ChatMessage } from '../../agents/types.js';
import type { LlmRouter, ChatCompletionRequest } from '../../llm/LlmRouter.js';

export class LlmRouterProvider implements LLMProvider {
  constructor(
    private readonly router: LlmRouter,
    private readonly modelName: string,
    private readonly temperature?: number
  ) {}

  // ── chat ──────────────────────────────────────────────────────────────────

  async chat(messages: ChatMessage[], tools?: ToolDefinition[]): Promise<LLMResponse> {
    const request: ChatCompletionRequest = {
      model: this.modelName,
      messages,
      ...(this.temperature !== undefined ? { temperature: this.temperature } : {}),
      ...(tools && tools.length > 0 ? { tools } : {}),
    };

    const eventStream = this.router.getEventStream(request);
    const msg = await eventStream.result();


    let textContent = '';
    const toolCalls: LLMResponse['toolCalls'] = [];

    for (const block of msg.content) {
      if (block.type === 'text') {
        textContent += block.text;
      } else if (block.type === 'toolCall') {
        toolCalls!.push({
          id: block.id,
          type: 'function',
          function: {
            name: block.name,
            arguments:
              typeof block.arguments === 'string'
                ? block.arguments
                : JSON.stringify(block.arguments),
          },
        });
      }
    }

    const response: LLMResponse = {
      content: textContent,
      usage: {
        promptTokens: msg.usage.input,
        completionTokens: msg.usage.output,
        totalTokens: msg.usage.totalTokens,
      },
    };

    if (toolCalls && toolCalls.length > 0) {
      response.toolCalls = toolCalls;
    }

    return response;
  }

  // ── chatStream ────────────────────────────────────────────────────────────

  async *chatStream(messages: ChatMessage[]): AsyncIterable<LLMStreamChunk> {
    const request: ChatCompletionRequest = {
      model: this.modelName,
      messages,
      ...(this.temperature !== undefined ? { temperature: this.temperature } : {}),
    };

    const eventStream = this.router.getEventStream(request);

    for await (const event of eventStream) {
      if (event.type === 'text_delta') {
        yield { token: event.delta, done: false };
      } else if (event.type === 'thinking_delta') {
        // Emit reasoning tokens (Qwen / DeepSeek chain-of-thought)
        yield { token: '', reasoning: event.delta, done: false };
      } else if (event.type === 'done') {
        yield {
          token: '',
          done: true,
          usage: {
            promptTokens: event.message.usage.input,
            completionTokens: event.message.usage.output,
            totalTokens: event.message.usage.totalTokens,
          },
        };
      } else if (event.type === 'error') {
        throw new Error(event.error.errorMessage ?? 'LLM stream error');
      }
    }
  }
}

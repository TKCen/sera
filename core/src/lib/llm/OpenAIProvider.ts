import OpenAI from 'openai';
import type {
  LLMProvider,
  LLMResponse,
  LLMStreamChunk,
  ToolDefinition,
  ToolCall,
} from './types.js';
import type { ChatMessage } from '../../agents/types.js';
import { config } from '../config.js';
import { Logger } from '../logger.js';

const logger = new Logger('OpenAIProvider');

interface OpenAIProviderConfig {
  baseUrl: string;
  apiKey: string;
  model: string;
  temperature?: number;
}

export class OpenAIProvider implements LLMProvider {
  private client: OpenAI;
  private configOverride: OpenAIProviderConfig | undefined;

  constructor(override?: OpenAIProviderConfig) {
    this.configOverride = override;
    const baseURL = override?.baseUrl || config.llm.baseUrl;
    const apiKey = override?.apiKey || config.llm.apiKey;

    this.client = new OpenAI({ baseURL, apiKey });
  }

  private get model(): string {
    return this.configOverride?.model || config.llm.model;
  }

  /**
   * Convert internal ChatMessage[] to the OpenAI API format.
   * Handles tool_calls and tool_call_id fields correctly.
   */
  private static toOpenAIMessages(
    messages: ChatMessage[],
  ): OpenAI.Chat.Completions.ChatCompletionMessageParam[] {
    return messages.map((msg) => {
      if (msg.role === 'tool') {
        return {
          role: 'tool' as const,
          content: msg.content,
          tool_call_id: msg.tool_call_id ?? '',
        };
      }
      if (msg.role === 'assistant' && msg.tool_calls && msg.tool_calls.length > 0) {
        return {
          role: 'assistant' as const,
          content: msg.content || null,
          tool_calls: msg.tool_calls.map((tc) => ({
            id: tc.id,
            type: 'function' as const,
            function: {
              name: tc.function.name,
              arguments: tc.function.arguments,
            },
          })),
        };
      }
      return {
        role: msg.role as 'user' | 'assistant' | 'system',
        content: msg.content,
      };
    });
  }

  async chat(messages: ChatMessage[], tools?: ToolDefinition[]): Promise<LLMResponse> {
    try {
      const openAIMessages = OpenAIProvider.toOpenAIMessages(messages);

      const params: OpenAI.Chat.Completions.ChatCompletionCreateParamsNonStreaming = {
        model: this.model,
        messages: openAIMessages,
        temperature: this.configOverride?.temperature ?? 0.7,
      };

      // Only attach tools if provided and non-empty
      if (tools && tools.length > 0) {
        params.tools = tools.map((t) => ({
          type: 'function' as const,
          function: {
            name: t.function.name,
            description: t.function.description,
            parameters: t.function.parameters as unknown as
              Record<string, unknown>,
          },
        }));
      }

      const response = await this.client.chat.completions.create(params);

      const choice = response.choices[0];
      const rawToolCalls = choice?.message?.tool_calls;
      const toolCalls: ToolCall[] = rawToolCalls
        ? rawToolCalls.map((tc: any) => ({
            id: tc.id as string,
            type: 'function' as const,
            function: {
              name: tc.function.name as string,
              arguments: tc.function.arguments as string,
            },
          }))
        : [];

      const result: LLMResponse = {
        content: choice?.message?.content || '',
        usage: {
          promptTokens: response.usage?.prompt_tokens || 0,
          completionTokens: response.usage?.completion_tokens || 0,
          totalTokens: response.usage?.total_tokens || 0,
        },
      };
      if (toolCalls.length > 0) {
        result.toolCalls = toolCalls;
      }
      return result;
    } catch (error: any) {
      logger.error('LLM Chat Error:', error);
      throw new Error(`LLM provider failed: ${error.message}`);
    }
  }

  async *chatStream(messages: ChatMessage[]): AsyncIterable<LLMStreamChunk> {
    try {
      const stream = await this.client.chat.completions.create({
        model: this.model,
        messages: OpenAIProvider.toOpenAIMessages(messages),
        temperature: this.configOverride?.temperature ?? 0.7,
        stream: true,
      });

      for await (const chunk of stream) {
        const token = chunk.choices[0]?.delta?.content || '';
        if (token) {
          yield { token, done: false };
        }
      }

      yield { token: '', done: true };
    } catch (error: any) {
      logger.error('LLM Stream Error:', error);
      throw new Error(`LLM stream failed: ${error.message}`);
    }
  }
}

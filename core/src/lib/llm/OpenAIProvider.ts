import OpenAI from 'openai';
import type { LLMProvider, LLMResponse, LLMStreamChunk } from './types.js';
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

  async chat(messages: ChatMessage[]): Promise<LLMResponse> {
    try {
      const response = await this.client.chat.completions.create({
        model: this.model,
        messages: messages as any,
        temperature: this.configOverride?.temperature ?? 0.7,
      });

      return {
        content: response.choices[0]?.message?.content || '',
        usage: {
          promptTokens: response.usage?.prompt_tokens || 0,
          completionTokens: response.usage?.completion_tokens || 0,
          totalTokens: response.usage?.total_tokens || 0,
        },
      };
    } catch (error: any) {
      logger.error('LLM Chat Error:', error);
      throw new Error(`LLM provider failed: ${error.message}`);
    }
  }

  async *chatStream(messages: ChatMessage[]): AsyncIterable<LLMStreamChunk> {
    try {
      const stream = await this.client.chat.completions.create({
        model: this.model,
        messages: messages as any,
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

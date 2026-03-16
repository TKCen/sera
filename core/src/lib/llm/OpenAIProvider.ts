import OpenAI from 'openai';
import type { LLMProvider, LLMResponse } from './types.js';
import type { ChatMessage } from '../../agents/types.js';
import { config } from '../config.js';

interface OpenAIProviderConfig {
  baseUrl: string;
  apiKey: string;
  model: string;
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
        temperature: 0.7,
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
      console.error('LLM Chat Error:', error);
      throw new Error(`LLM provider failed: ${error.message}`);
    }
  }
}

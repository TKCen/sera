import OpenAI from 'openai';
import type { LLMProvider, LLMResponse } from './types.js';
import type { ChatMessage } from '../../agents/types.js';
import { config } from '../config.js';

export class OpenAIProvider implements LLMProvider {
  private client: OpenAI;

  constructor() {
    this.client = new OpenAI({
      baseURL: config.llm.baseUrl,
      apiKey: config.llm.apiKey,
    });
  }

  async chat(messages: ChatMessage[]): Promise<LLMResponse> {
    try {
      const response = await this.client.chat.completions.create({
        model: config.llm.model,
        messages: messages as any, // types match logically
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

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

  async *chat(messages: ChatMessage[]): AsyncGenerator<string, void, unknown> {
    try {
      const stream = await this.client.chat.completions.create({
        model: config.llm.model,
        messages: messages as any, // types match logically
        temperature: 0.7,
        stream: true,
      });

      for await (const chunk of stream) {
        const content = chunk.choices[0]?.delta?.content || '';
        if (content) {
          process.stdout.write(content);
          yield content;
        }
      }
      console.log(''); // newline after stream finishes
    } catch (error: any) {
      console.error('\nLLM Chat Error:', error);

      let errorMessage = error.message || 'Unknown error occurred';

      if (error.code === 'ECONNREFUSED' || error.message?.includes('ECONNREFUSED')) {
        errorMessage = 'Connection Refused: Ensure LM Studio (or your local LLM) is running and accessible.';
      } else if (error.status === 404 || error.message?.includes('not found') || error.code === 'model_not_found') {
        errorMessage = 'Model Not Found: Check if the correct model is loaded in LM Studio/OpenAI.';
      }

      throw new Error(`LLM provider failed: ${errorMessage}`);
    }
  }
}

import type { ChatMessage } from '../../agents/types.js';

export interface LLMResponse {
  content: string;
  usage?: {
    promptTokens: number;
    completionTokens: number;
    totalTokens: number;
  };
}

export interface LLMProvider {
  chat(messages: ChatMessage[]): Promise<LLMResponse>;
}

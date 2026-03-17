import type { ChatMessage } from '../../agents/types.js';

export interface LLMResponse {
  content: string;
  usage?: {
    promptTokens: number;
    completionTokens: number;
    totalTokens: number;
  };
}

export interface LLMStreamChunk {
  token: string;
  done: boolean;
}

export interface LLMProvider {
  chat(messages: ChatMessage[]): Promise<LLMResponse>;
  chatStream(messages: ChatMessage[]): AsyncIterable<LLMStreamChunk>;
}

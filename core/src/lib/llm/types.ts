import type { ChatMessage } from '../../agents/types.js';

// ── Tool Definitions (OpenAI format) ────────────────────────────────────────────

export interface ToolFunctionDefinition {
  name: string;
  description: string;
  parameters: Record<string, unknown>;
}

export interface ToolDefinition {
  type: 'function';
  function: ToolFunctionDefinition;
}

// ── Tool Calls (parsed from LLM response) ───────────────────────────────────────

export interface ToolCallFunction {
  name: string;
  arguments: string;
}

export interface ToolCall {
  id: string;
  type: 'function';
  function: ToolCallFunction;
}

// ── LLM Response ────────────────────────────────────────────────────────────────

export interface LLMUsage {
  promptTokens: number;
  completionTokens: number;
  totalTokens: number;
}

export interface LLMResponse {
  content: string;
  toolCalls?: ToolCall[];
  usage?: LLMUsage;
}

export interface LLMStreamChunk {
  token: string;
  done: boolean;
}

// ── LLM Provider ────────────────────────────────────────────────────────────────

export interface LLMProvider {
  chat(messages: ChatMessage[], tools?: ToolDefinition[]): Promise<LLMResponse>;
  chatStream(messages: ChatMessage[]): AsyncIterable<LLMStreamChunk>;
}

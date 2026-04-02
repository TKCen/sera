import type { ChatMessage } from '../../agents/index.js';

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
  /** Chain-of-thought text, e.g. DeepSeek / Qwen reasoning_content. */
  reasoning?: string;
  toolCalls?: ToolCall[];
  usage?: LLMUsage;
}

export interface LLMStreamChunk {
  token: string;
  /** Reasoning/thinking token from models that surface chain-of-thought. */
  reasoning?: string;
  done: boolean;
  usage?: LLMUsage;
}

// ── LLM Provider ────────────────────────────────────────────────────────────────

export interface LLMProvider {
  chat(messages: ChatMessage[], tools?: ToolDefinition[]): Promise<LLMResponse>;
  chatStream(messages: ChatMessage[]): AsyncIterable<LLMStreamChunk>;
}

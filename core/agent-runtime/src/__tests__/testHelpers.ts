import { vi } from 'vitest';
import type {
  ILLMClient,
  ChatMessage,
  ToolDefinition,
  LLMResponse,
  ThinkingLevel,
  ToolCall,
} from '../llmClient.js';
import type { IToolExecutor, ToolExecutionResult } from '../tools/executor.js';
import type { CentrifugoPublisher, ToolOutputCallback } from '../centrifugo.js';

/**
 * ScriptedLLMClient returns a sequence of pre-defined responses.
 */
export class ScriptedLLMClient implements ILLMClient {
  private callCount = 0;
  constructor(private responses: LLMResponse[]) {}

  async chat(
    _messages: ChatMessage[],
    _tools?: ToolDefinition[],
    _temperature?: number,

    _thinkingLevel?: ThinkingLevel,
    _timeoutMs?: number,
    _model?: string
  ): Promise<LLMResponse> {
    const response = this.responses[this.callCount];
    if (!response) {
      throw new Error(
        `ScriptedLLMClient: No more responses defined (call count: ${this.callCount})`
      );
    }
    this.callCount++;
    return response;
  }

  getCallCount(): number {
    return this.callCount;
  }
}

/**
 * StaticToolExecutor allows registering manual handlers for specific tool names.
 */
export class StaticToolExecutor implements IToolExecutor {
  private handlers = new Map<string, (args: string) => string | Promise<string>>();
  private toolDefinitions: ToolDefinition[] = [];

  register(definition: ToolDefinition, handler: (args: string) => string | Promise<string>): this {
    this.toolDefinitions.push(definition);
    this.handlers.set(definition.function.name, handler);
    return this;
  }

  getToolDefinitions(allowedTools?: string[]): ToolDefinition[] {
    if (!allowedTools || allowedTools.length === 0) {
      return this.toolDefinitions;
    }
    return this.toolDefinitions.filter((t) => allowedTools.includes(t.function.name));
  }

  async executeToolCalls(
    toolCalls: ToolCall[],
    _onOutput?: ToolOutputCallback
  ): Promise<ToolExecutionResult[]> {
    const results: ToolExecutionResult[] = [];
    for (const tc of toolCalls) {
      const handler = this.handlers.get(tc.function.name);
      let content: string;
      if (!handler) {
        content = `Error: Unknown tool "${tc.function.name}"`;
      } else {
        try {
          content = await handler(tc.function.arguments);
        } catch (err) {
          content = `Error: ${err instanceof Error ? err.message : String(err)}`;
        }
      }
      results.push({
        message: { role: 'tool', tool_call_id: tc.id, content },
        toolName: tc.function.name,
        argRepaired: false,
        repairStrategy: null,
      });
    }
    return results;
  }
}

/**
 * Minimal mock for CentrifugoPublisher.
 */
export function createMockPublisher(): CentrifugoPublisher {
  return {
    publish: vi.fn().mockResolvedValue(undefined),
    publishThought: vi.fn().mockResolvedValue(undefined),
    publishStreamToken: vi.fn().mockResolvedValue(undefined),
    publishToolOutput: vi.fn().mockResolvedValue(undefined),
    publishStreamError: vi.fn().mockResolvedValue(undefined),
  } as unknown as CentrifugoPublisher;
}

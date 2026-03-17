/**
 * ReasoningLoop — the Observe → Think → Act cycle that runs inside the container.
 *
 * This mirrors the behavior of BaseAgent.processStream() in Core, but
 * executes entirely within the container using the LLM Proxy and native tools.
 */

import { v4 as uuidv4 } from 'uuid';
import type { LLMClient, ChatMessage, ToolCall, ToolDefinition } from './llmClient.js';
import type { RuntimeToolExecutor } from './tools.js';
import type { CentrifugoPublisher } from './centrifugo.js';
import type { RuntimeManifest } from './manifest.js';
import { generateSystemPrompt } from './manifest.js';
import { log } from './logger.js';

/** Maximum tool-call loop iterations before forcing a text response. */
const MAX_TOOL_ITERATIONS = 10;

export class ReasoningLoop {
  private llm: LLMClient;
  private tools: RuntimeToolExecutor;
  private centrifugo: CentrifugoPublisher;
  private manifest: RuntimeManifest;
  private systemPrompt: string;
  private toolDefs: ToolDefinition[];

  constructor(
    llm: LLMClient,
    tools: RuntimeToolExecutor,
    centrifugo: CentrifugoPublisher,
    manifest: RuntimeManifest,
  ) {
    this.llm = llm;
    this.tools = tools;
    this.centrifugo = centrifugo;
    this.manifest = manifest;
    this.systemPrompt = generateSystemPrompt(manifest);
    this.toolDefs = tools.getToolDefinitions(manifest.tools?.allowed);
  }

  /**
   * Run a single reasoning cycle for the given input.
   * Returns the final text response from the agent.
   */
  async run(input: string, history: ChatMessage[] = []): Promise<string> {
    const messageId = uuidv4();
    const hasTools = this.toolDefs.length > 0;

    // Observe
    await this.centrifugo.publishThought('observe',
      `Observing: "${input.substring(0, 100)}${input.length > 100 ? '...' : ''}"`);

    // Plan
    const toolNames = this.toolDefs.map((t) => t.function.name).join(', ') || 'none';
    await this.centrifugo.publishThought('plan',
      `Planning approach using tools: ${toolNames}`);

    // Build message array
    const messages: ChatMessage[] = [
      { role: 'system', content: this.systemPrompt },
      ...history,
      { role: 'user', content: input },
    ];

    try {
      let iterations = 0;

      // ── Agentic Loop ──────────────────────────────────────────────────────
      while (iterations < MAX_TOOL_ITERATIONS) {
        iterations++;

        if (hasTools) {
          const response = await this.llm.chat(messages, this.toolDefs, this.manifest.model.temperature);

          if (response.toolCalls && response.toolCalls.length > 0) {
            // ── Tool Call Phase ────────────────────────────────────────────
            for (const tc of response.toolCalls) {
              await this.centrifugo.publishThought('tool-call',
                `Calling tool: **${tc.function.name}**(${tc.function.arguments})`);
            }

            // Add assistant message with tool_calls to the conversation
            const assistantMsg: ChatMessage = {
              role: 'assistant',
              content: response.content || '',
              tool_calls: response.toolCalls,
            };
            messages.push(assistantMsg);

            // Execute tools natively
            const toolResults = await this.tools.executeToolCalls(response.toolCalls);

            // Publish results and add to conversation
            for (const result of toolResults) {
              const preview = result.content.length > 200
                ? result.content.substring(0, 200) + '...'
                : result.content;
              await this.centrifugo.publishThought('tool-result', preview);
              messages.push(result);
            }

            // Continue the loop — LLM will see the tool results
            continue;
          }

          // ── Text Response (no tool calls) ─────────────────────────────────
          const reply = response.content || 'No response generated.';
          await this.publishFinalResponse(reply, messageId);
          await this.centrifugo.publishThought('reflect', `Completed task: "${input.substring(0, 50)}..."`);

          log('info', `Reasoning complete after ${iterations} iteration(s) — ${reply.length} chars`);
          return reply;
        } else {
          // No tools — single LLM call
          const response = await this.llm.chat(messages, undefined, this.manifest.model.temperature);
          const reply = response.content || 'No response generated.';
          await this.publishFinalResponse(reply, messageId);
          await this.centrifugo.publishThought('reflect', `Completed task: "${input.substring(0, 50)}..."`);

          log('info', `Reasoning complete (no tools) — ${reply.length} chars`);
          return reply;
        }
      }

      // ── Max iterations reached ──────────────────────────────────────────────
      log('warn', `Tool loop hit max iterations (${MAX_TOOL_ITERATIONS}), forcing final response`);
      await this.centrifugo.publishThought('reflect',
        `Reached maximum tool iterations (${MAX_TOOL_ITERATIONS}). Generating final response.`);

      // Force a final response without tools
      const fallback = await this.llm.chat(messages);
      const reply = fallback.content || 'Max tool iterations reached. Unable to complete task.';
      await this.publishFinalResponse(reply, messageId);
      return reply;

    } catch (error: unknown) {
      const errMsg = error instanceof Error ? error.message : String(error);
      log('error', `Reasoning loop error: ${errMsg}`);
      await this.centrifugo.publishThought('reflect', `⚠️ Error: ${errMsg}`);
      return `Error during reasoning: ${errMsg}`;
    }
  }

  /**
   * Stream the final response text to Centrifugo in chunks.
   */
  private async publishFinalResponse(text: string, messageId: string): Promise<void> {
    const chunkSize = 20;
    for (let i = 0; i < text.length; i += chunkSize) {
      const chunk = text.substring(i, i + chunkSize);
      await this.centrifugo.publishStreamToken(messageId, chunk, false);
    }
    await this.centrifugo.publishStreamToken(messageId, '', true);
  }
}

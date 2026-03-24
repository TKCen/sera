import { BaseAgent } from './BaseAgent.js';
import { AuditService } from '../audit/AuditService.js';
import type { AgentResponse, ChatMessage } from './types.js';
import type { LLMProvider } from '../lib/llm/types.js';
import type { AgentManifest } from './manifest/types.js';
import { IdentityService } from './identity/IdentityService.js';
import { parseJson } from '../lib/json.js';

export class WorkerAgent extends BaseAgent {
  constructor(
    manifest: AgentManifest,
    llmProvider: LLMProvider,
    intercom?: import('../intercom/IntercomService.js').IntercomService,
    agentInstanceId?: string,
    memoryManager?: import('../memory/manager.js').MemoryManager
  ) {
    super(manifest, llmProvider, intercom, agentInstanceId, memoryManager);
  }

  async process(input: string, history: ChatMessage[] = []): Promise<AgentResponse> {
    await this.observe(input);
    await this.plan(input);

    // ── Quota Check ───────────────────────────────────────────────────
    if (this.agentScheduler && this.manifest.resources?.maxLlmTokensPerHour) {
      const allowed = await this.agentScheduler.isWithinQuota(
        this.agentInstanceId || this.role,
        this.manifest.resources.maxLlmTokensPerHour
      );
      if (!allowed) {
        const errorMsg = '⚠️ Hourly token quota exceeded. Request denied.';
        await this.publishThought('reflect', errorMsg);
        return { thought: 'Quota exceeded', finalAnswer: errorMsg };
      }
    }

    let dynamicContext = '';
    let memoryDegraded = false;
    if (this.memoryManager) {
      try {
        dynamicContext = await this.memoryManager.assembleContext(input);
      } catch {
        memoryDegraded = true;
        this.logger.warn('Memory context unavailable — embedding service may be down');
      }
    }

    // Resolve circle project context
    const circleContext = this.circleContextResolver?.();

    // Use the streaming (natural text) prompt for web chat — the JSON format
    // in generateSystemPrompt() causes LLMs to frequently fail parsing.
    // Container-based agents use the LLM proxy path which has its own context assembly.
    let systemPrompt = IdentityService.generateStreamingSystemPrompt(
      this.manifest,
      circleContext,
      dynamicContext
    );
    if (memoryDegraded) {
      systemPrompt += '\n\n**Note:** Knowledge search is currently unavailable (embedding service down). ' +
        'Do not attempt to use knowledge-query or knowledge-store tools.';
    }

    const fullHistory = [...history, { role: 'user', content: input } as ChatMessage];

    // Get tool definitions if ToolExecutor is available
    const tools = this.toolExecutor ? this.toolExecutor.getToolDefinitions(this.manifest) : [];

    const messages: ChatMessage[] = [{ role: 'system', content: systemPrompt }, ...fullHistory];

    // ── Agentic Tool Loop ──────────────────────────────────────────────────
    const MAX_TOOL_ITERATIONS = 10;
    let iterations = 0;

    while (iterations < MAX_TOOL_ITERATIONS) {
      iterations++;
      const response = await this.llmProvider.chat(messages, tools.length > 0 ? tools : undefined);

      // Record usage
      if (response.usage && this.meteringEngine) {
        await this.meteringEngine.record({
          agentId: this.agentInstanceId || this.role,
          model: this.manifest.model.name,
          ...response.usage,
        });
      }

      if (response.toolCalls && response.toolCalls.length > 0 && this.toolExecutor) {
        // Add assistant message with tool_calls to history
        messages.push({
          role: 'assistant',
          content: response.content || '',
          tool_calls: response.toolCalls,
        });

        // Execute tool calls
        const toolResults = await this.toolExecutor.executeToolCalls(
          response.toolCalls,
          this.manifest,
          this.agentInstanceId,
          this.containerId
        );

        // Add tool results to history
        for (const result of toolResults) {
          messages.push(result);
        }

        // Continue loop — LLM will see tool results and decide next action
        continue;
      }

      // No tool calls — we have a final text response
      this.history = [
        ...fullHistory,
        { role: 'assistant', content: response.content } as ChatMessage,
      ];

      try {
        await AuditService.getInstance().record({
          actorType: 'agent',
          actorId: this.agentInstanceId || this.name,
          actingContext: null,
          eventType: 'chat',
          payload: {
            input,
            response:
              response.content.length > 500
                ? response.content.substring(0, 500) + '...'
                : response.content,
          },
        });
      } catch (auditErr) {
        this.logger.error('Failed to record audit entry:', auditErr);
      }

      // Try JSON parse first (agent-to-agent structured responses),
      // fall back to natural text (web chat)
      try {
        const parsed = parseJson(response.content);
        if (parsed && typeof parsed === 'object' && 'finalAnswer' in parsed) {
          const result = parsed as AgentResponse;
          await this.reflect({ thought: result.thought, finalAnswer: result.finalAnswer });
          return result;
        }
      } catch {
        // Not JSON — expected for natural text responses
      }

      const result: AgentResponse = {
        thought: `Completed task: ${input}`,
        finalAnswer: response.content,
      };
      await this.reflect(result);
      return result;
    }

    // Exhausted tool iterations
    return {
      thought: 'Reached maximum tool call iterations.',
      finalAnswer: 'I was unable to complete the task within the allowed number of steps.',
    };
  }
}

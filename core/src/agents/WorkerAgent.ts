import { BaseAgent } from './BaseAgent.js';
import { AuditService } from '../audit/AuditService.js';
import type { AgentResponse, ChatMessage } from './types.js';
import type { LLMProvider } from '../lib/llm/types.js';
import type { AgentManifest } from './manifest/types.js';
import { IdentityService } from './identity/IdentityService.js';

export class WorkerAgent extends BaseAgent {
  constructor(
    manifest: AgentManifest,
    llmProvider: LLMProvider,
    intercom?: import('../intercom/IntercomService.js').IntercomService,
    agentInstanceId?: string,
    memoryManager?: import('../memory/manager.js').MemoryManager,
  ) {
    super(manifest, llmProvider, intercom, agentInstanceId, memoryManager);
  }

  async process(input: string, history: ChatMessage[] = []): Promise<AgentResponse> {
    await this.observe(input);
    await this.plan(input);

    // ── Quota Check (Epic 14) ──────────────────────────────────────────
    if (this.agentScheduler && this.manifest.resources?.maxLlmTokensPerHour) {
      const allowed = await this.agentScheduler.isWithinQuota(
        this.agentInstanceId || this.role,
        this.manifest.resources.maxLlmTokensPerHour,
      );
      if (!allowed) {
        const errorMsg = '⚠️ Hourly token quota exceeded. Request denied.';
        await this.publishThought('reflect', errorMsg);
        return { thought: 'Quota exceeded', finalAnswer: errorMsg };
      }
    }

    let dynamicContext = '';
    if (this.memoryManager) {
      dynamicContext = await this.memoryManager.assembleContext(input);
    }

    const systemPrompt = IdentityService.generateSystemPrompt(this.manifest, undefined, dynamicContext);

    const fullHistory = [...history, { role: 'user', content: input } as ChatMessage];

    const response = await this.llmProvider.chat([
      { role: 'system', content: systemPrompt },
      ...fullHistory
    ]);

    // Record usage
    if (response.usage && this.meteringEngine) {
      await this.meteringEngine.record({
        agentId: this.agentInstanceId || this.role,
        model: this.manifest.model.name,
        ...response.usage,
      });
    }

    this.history = [...fullHistory, { role: 'assistant', content: response.content } as ChatMessage];

    if (response.toolCalls && response.toolCalls.length > 0 && this.toolExecutor) {
      // ── Agentic Tool Loop ──────────────────────────────────────────────────
      // Note: WorkerAgent.process currently doesn't implement the full tool loop
      // like processStream does. This is a known limitation in the current core.
      // However, if we ever add it, we should record audit entries.
    }

    const auditService = AuditService.getInstance();
    const auditId = this.agentInstanceId || this.role;
    try {
      await auditService.record(auditId, 'chat', {
        input,
        response: response.content.length > 500 ? response.content.substring(0, 500) + '...' : response.content
      });
    } catch (auditErr) {
      this.logger.error('Failed to record audit entry:', auditErr);
    }

    try {
      const jsonMatch = response.content.match(/\{[\s\S]*\}/);
      if (jsonMatch) {
        const parsed = JSON.parse(jsonMatch[0]);
        await this.reflect({ thought: parsed.thought, finalAnswer: parsed.finalAnswer });
        return parsed;
      }
      const result: AgentResponse = {
        thought: `Completed task: ${input}`,
        finalAnswer: response.content,
      };
      await this.reflect(result);
      return result;
    } catch (error) {
      this.logger.error('Failed to parse worker response:', error);
      return {
        thought: 'Error parsing response.',
        finalAnswer: response.content
      };
    }
  }
}

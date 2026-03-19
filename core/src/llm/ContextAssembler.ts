import type { Pool } from 'pg';
import { SkillInjector } from '../skills/SkillInjector.js';
import type { ChatMessage } from './LiteLLMClient.js';
import { Orchestrator } from '../agents/Orchestrator.js';

export class ContextAssembler {
  private skillInjector: SkillInjector;

  constructor(private pool: Pool, private orchestrator: Orchestrator) {
    this.skillInjector = new SkillInjector(pool);
  }

  /**
   * Assembles the context for an LLM call.
   * Currently handles skill injection. In the future, it will handle Memory RAG.
   */
  async assemble(agentId: string, messages: ChatMessage[]): Promise<ChatMessage[]> {
    const systemMessage = messages.find(m => m.role === 'system');
    if (!systemMessage) return messages;

    // 1. Get agent manifest to know declared skills
    const manifest = this.orchestrator.getManifestByInstanceId(agentId) 
                  || this.orchestrator.getManifest(agentId);

    if (!manifest) return messages;

    // 2. Identify current user message for auto-triggering
    const lastUserMessage = [...messages].reverse().find(m => m.role === 'user');
    const content = lastUserMessage?.content || '';

    // 3. Inject skills
    const newSystemPrompt = await this.skillInjector.inject(
      systemMessage.content || '',
      manifest.skills || [],
      (manifest as any).skillPackages || [],
      content
    );

    // 4. Return updated messages
    return messages.map(m => m.role === 'system' ? { ...m, content: newSystemPrompt } : m);
  }
}

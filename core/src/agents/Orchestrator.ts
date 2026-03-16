import { BaseAgent } from './BaseAgent.js';
import type { AgentRole, AgentTask } from './types.js';
import type { LLMProvider } from '../lib/llm/types.js';

export class Orchestrator {
  private agents: Map<string, BaseAgent> = new Map();
  private tasks: AgentTask[] = [];

  registerAgent(agent: BaseAgent) {
    this.agents.set(agent.role, agent);
  }

  updateLlmProvider(llmProvider: LLMProvider) {
    for (const agent of this.agents.values()) {
      agent.updateLlmProvider(llmProvider);
    }
  }

  async executeTask(description: string) {
    console.log(`[Orchestrator] Starting task: ${description}`);

    // Simple delegation logic for POC
    const primaryAgent = this.agents.get('primary');
    if (!primaryAgent) throw new Error('Primary agent not registered');

    const response = await primaryAgent.process(description);

    if (response.action) {
      console.log(`[Orchestrator] Agent requested tool: ${response.action.tool}`);
      // Future: execute tool via MCPRegistry
    }

    if (response.delegation) {
      console.log(`[Orchestrator] Delegating to ${response.delegation.agentRole}`);
      const worker = this.agents.get(response.delegation.agentRole);
      if (worker) {
        const workerResponse = await worker.process(response.delegation.task);
        return workerResponse.finalAnswer;
      } else {
        throw new Error(`Agent with role ${response.delegation.agentRole} not found`);
      }
    }

    return response.finalAnswer || "No answer provided.";
  }
}

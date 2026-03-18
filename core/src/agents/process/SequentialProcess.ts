/**
 * SequentialProcess — runs tasks one after another.
 *
 * The output of task N is appended to the context of task N+1,
 * creating a chain of dependent processing.
 */

import type { BaseAgent } from '../BaseAgent.js';
import type {
  ProcessStrategy,
  ProcessTask,
  ProcessResult,
  ProcessRunResult,
  FlowState,
} from './types.js';

export class SequentialProcess implements ProcessStrategy {
  readonly type = 'sequential' as const;

  async execute(
    tasks: ProcessTask[],
    agents: Map<string, BaseAgent>,
  ): Promise<ProcessRunResult> {
    const startTime = Date.now();
    const results: ProcessResult[] = [];
    let previousOutput = '';
    const state: FlowState = {};

    for (const task of tasks) {
      const agent = this.resolveAgent(task, agents);
      if (!agent) {
        results.push({
          taskId: task.id,
          agentName: task.assignedAgent ?? 'unassigned',
          output: '',
          status: 'failed',
          error: `Agent "${task.assignedAgent ?? '(none)'}" not found`,
          durationMs: 0,
        });
        continue;
      }

      const taskStart = Date.now();

      // Build context from previous steps in the chain
      const contextualInput = previousOutput
        ? `Previous context:\n${previousOutput}\n\nCurrent task:\n${task.description}`
        : task.description;

      try {
        const response = await agent.process(contextualInput);
        const output = response.finalAnswer || response.thought || '';

        results.push({
          taskId: task.id,
          agentName: agent.role,
          output,
          status: 'completed',
          durationMs: Date.now() - taskStart,
        });

        previousOutput = output;
        state[task.id] = output;
      } catch (err) {
        const error = err instanceof Error ? err.message : String(err);
        results.push({
          taskId: task.id,
          agentName: agent.role,
          output: '',
          status: 'failed',
          error,
          durationMs: Date.now() - taskStart,
        });

        // Continue with empty context on failure
        previousOutput = '';
      }
    }

    return {
      processType: 'sequential',
      results,
      finalOutput: previousOutput,
      totalDurationMs: Date.now() - startTime,
    };
  }

  private resolveAgent(
    task: ProcessTask,
    agents: Map<string, BaseAgent>,
  ): BaseAgent | undefined {
    if (task.assignedAgent) {
      return agents.get(task.assignedAgent);
    }
    // Default: use the first available agent
    return agents.values().next().value;
  }
}

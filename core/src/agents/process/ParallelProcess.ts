/**
 * ParallelProcess — runs independent tasks concurrently.
 *
 * All tasks execute via Promise.all and results are aggregated.
 */

import type { BaseAgent } from '../BaseAgent.js';
import type {
  ProcessStrategy,
  ProcessTask,
  ProcessResult,
  ProcessRunResult,
} from './types.js';

export class ParallelProcess implements ProcessStrategy {
  readonly type = 'parallel' as const;

  async execute(
    tasks: ProcessTask[],
    agents: Map<string, BaseAgent>,
  ): Promise<ProcessRunResult> {
    const startTime = Date.now();

    const promises = tasks.map(task => this.executeOne(task, agents));
    const results = await Promise.all(promises);

    // Aggregate all completed outputs
    const completedOutputs = results
      .filter(r => r.status === 'completed' && r.output)
      .map(r => `[${r.agentName}]: ${r.output}`);

    return {
      processType: 'parallel',
      results,
      finalOutput: completedOutputs.join('\n\n'),
      totalDurationMs: Date.now() - startTime,
    };
  }

  private async executeOne(
    task: ProcessTask,
    agents: Map<string, BaseAgent>,
  ): Promise<ProcessResult> {
    const taskStart = Date.now();
    const agent = task.assignedAgent
      ? agents.get(task.assignedAgent)
      : agents.values().next().value;

    if (!agent) {
      return {
        taskId: task.id,
        agentName: task.assignedAgent ?? 'unassigned',
        output: '',
        status: 'failed',
        error: `Agent "${task.assignedAgent ?? '(none)'}" not found`,
        durationMs: 0,
      };
    }

    try {
      const response = await agent.process(task.description);
      return {
        taskId: task.id,
        agentName: agent.role,
        output: response.finalAnswer || response.thought || '',
        status: 'completed',
        durationMs: Date.now() - taskStart,
      };
    } catch (err) {
      return {
        taskId: task.id,
        agentName: agent.role,
        output: '',
        status: 'failed',
        error: err instanceof Error ? err.message : String(err),
        durationMs: Date.now() - taskStart,
      };
    }
  }
}

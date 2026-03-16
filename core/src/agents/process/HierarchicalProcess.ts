/**
 * HierarchicalProcess — manager-worker pattern.
 *
 * A manager agent reviews worker results and can reject/retry them.
 * The manager produces a final consolidated answer.
 */

import type { BaseAgent } from '../BaseAgent.js';
import type {
  ProcessStrategy,
  ProcessTask,
  ProcessResult,
  ProcessRunResult,
} from './types.js';
import { Logger } from '../../lib/logger.js';

const logger = new Logger('HierarchicalProcess');
const DEFAULT_MAX_RETRIES = 2;

export class HierarchicalProcess implements ProcessStrategy {
  readonly type = 'hierarchical' as const;

  private maxRetries: number;

  constructor(maxRetries: number = DEFAULT_MAX_RETRIES) {
    this.maxRetries = maxRetries;
  }

  async execute(
    tasks: ProcessTask[],
    agents: Map<string, BaseAgent>,
    managerAgent?: BaseAgent,
  ): Promise<ProcessRunResult> {
    const startTime = Date.now();

    if (!managerAgent) {
      throw new Error(
        'HierarchicalProcess requires a managerAgent to validate worker results',
      );
    }

    const results: ProcessResult[] = [];

    for (const task of tasks) {
      const worker = task.assignedAgent
        ? agents.get(task.assignedAgent)
        : this.pickWorker(agents, managerAgent);

      if (!worker) {
        results.push({
          taskId: task.id,
          agentName: task.assignedAgent ?? 'unassigned',
          output: '',
          status: 'failed',
          error: `Worker agent not found`,
          durationMs: 0,
        });
        continue;
      }

      const result = await this.executeWithReview(
        task,
        worker,
        managerAgent,
      );
      results.push(result);
    }

    // Manager produces final consolidated answer
    const workerOutputs = results
      .filter(r => r.status === 'completed')
      .map(r => `[${r.agentName}] Task: ${r.taskId}\n${r.output}`)
      .join('\n\n---\n\n');

    let finalOutput = workerOutputs;

    if (workerOutputs) {
      try {
        const consolidation = await managerAgent.process(
          `You are the manager. Review and consolidate the following worker outputs into a final answer:\n\n${workerOutputs}`,
        );
        finalOutput = consolidation.finalAnswer || consolidation.thought || workerOutputs;
      } catch {
        // Fall back to raw worker outputs
      }
    }

    return {
      processType: 'hierarchical',
      results,
      finalOutput,
      totalDurationMs: Date.now() - startTime,
    };
  }

  /**
   * Execute a task with a worker, then have the manager review.
   * Retries up to maxRetries if the manager rejects.
   */
  private async executeWithReview(
    task: ProcessTask,
    worker: BaseAgent,
    manager: BaseAgent,
  ): Promise<ProcessResult> {
    const taskStart = Date.now();
    let lastOutput = '';
    let lastError: string | undefined;

    for (let attempt = 0; attempt <= this.maxRetries; attempt++) {
      try {
        const input = attempt === 0
          ? task.description
          : `The manager rejected your previous answer and asked you to retry.\nPrevious answer: ${lastOutput}\n\nOriginal task: ${task.description}`;

        const workerResponse = await worker.process(input);
        lastOutput = workerResponse.finalAnswer || workerResponse.thought || '';

        // Have manager validate the result
        const reviewResponse = await manager.process(
          `Review this worker output for the task "${task.description}":\n\n${lastOutput}\n\nRespond with either:\n- APPROVED: <reason> if the output is acceptable\n- REJECTED: <feedback> if the output needs improvement`,
        );

        const reviewText = (reviewResponse.finalAnswer || reviewResponse.thought || '').toUpperCase();

        if (reviewText.includes('APPROVED')) {
          return {
            taskId: task.id,
            agentName: worker.role,
            output: lastOutput,
            status: 'completed',
            durationMs: Date.now() - taskStart,
          };
        }

        logger.info(
          `Manager rejected attempt ${attempt + 1} for task "${task.id}"`,
        );
      } catch (err) {
        lastError = err instanceof Error ? err.message : String(err);
      }
    }

    // All retries exhausted — return last output as-is
    const result: ProcessResult = {
      taskId: task.id,
      agentName: worker.role,
      output: lastOutput,
      status: lastOutput ? 'completed' : 'failed',
      durationMs: Date.now() - taskStart,
    };
    if (lastError !== undefined) {
      result.error = lastError;
    }
    return result;
  }

  /** Pick the first worker agent that isn't the manager. */
  private pickWorker(
    agents: Map<string, BaseAgent>,
    manager: BaseAgent,
  ): BaseAgent | undefined {
    for (const agent of agents.values()) {
      if (agent.role !== manager.role) return agent;
    }
    return undefined;
  }
}

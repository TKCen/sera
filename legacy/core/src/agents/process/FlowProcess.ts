/**
 * FlowProcess — complex, conditional agent orchestration.
 *
 * Supports sequential, parallel, and conditional execution
 * with shared state/memory across steps.
 */

import type { BaseAgent } from '../BaseAgent.js';
import type {
  ProcessStrategy,
  ProcessTask,
  ProcessResult,
  ProcessRunResult,
  FlowState,
} from './types.js';
import { Logger } from '../../lib/logger.js';

const logger = new Logger('FlowProcess');

export class FlowProcess implements ProcessStrategy {
  readonly type = 'flow' as const;

  private state: FlowState = {};

  async execute(tasks: ProcessTask[], agents: Map<string, BaseAgent>): Promise<ProcessRunResult> {
    const startTime = Date.now();
    const results: ProcessResult[] = [];
    this.state = {};

    const completedTaskIds = new Set<string>();
    const failedTaskIds = new Set<string>();
    const pendingTasks = [...tasks];

    while (pendingTasks.length > 0) {
      const readyTasks: ProcessTask[] = [];
      const remainingTasks: ProcessTask[] = [];

      for (const task of pendingTasks) {
        if (this.isTaskReady(task, completedTaskIds, failedTaskIds)) {
          readyTasks.push(task);
        } else {
          remainingTasks.push(task);
        }
      }

      if (readyTasks.length === 0 && pendingTasks.length > 0) {
        logger.error(
          `Circular dependency or dead-end reached. Remaining tasks: ${pendingTasks.map((t) => t.id).join(', ')}`
        );
        break;
      }

      // Execute ready tasks in parallel if multiple, though dependencies often linearize them
      const taskPromises = readyTasks.map(async (task) => {
        const result = await this.executeOne(task, agents);
        results.push(result);
        if (result.status === 'completed') {
          completedTaskIds.add(task.id);
          this.state[task.id] = result.output;
        } else {
          failedTaskIds.add(task.id);
        }
      });

      await Promise.all(taskPromises);
      pendingTasks.length = 0;
      pendingTasks.push(...remainingTasks);
    }

    const finalOutput =
      results.length > 0 ? results[results.length - 1]!.output : 'No tasks were executed.';

    return {
      processType: 'flow',
      results,
      finalOutput,
      totalDurationMs: Date.now() - startTime,
    };
  }

  private isTaskReady(
    task: ProcessTask,
    completedTaskIds: Set<string>,
    _failedTaskIds: Set<string>
  ): boolean {
    if (!task.dependsOn || task.dependsOn.length === 0) {
      return true;
    }

    const routingType = task.routingType || 'and';

    if (routingType === 'and') {
      return task.dependsOn.every((id) => completedTaskIds.has(id));
    } else {
      // 'or'
      return task.dependsOn.some((id) => completedTaskIds.has(id));
    }
  }

  private async executeOne(
    task: ProcessTask,
    agents: Map<string, BaseAgent>
  ): Promise<ProcessResult> {
    const taskStart = Date.now();
    const agent = this.resolveAgent(task, agents);

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

    // Build context from dependencies
    const relevantState = (task.dependsOn || [])
      .map((id) => `[Task ${id}]: ${this.state[id] || '(no output)'}`)
      .join('\n\n');

    const contextualInput = relevantState
      ? `State context:\n${relevantState}\n\nCurrent task: ${task.description}`
      : task.description;

    try {
      const response = await agent.process(contextualInput);
      const output = response.finalAnswer || response.thought || '';

      // Check condition if present
      if (task.condition && !output.toLowerCase().includes(task.condition.toLowerCase())) {
        return {
          taskId: task.id,
          agentName: agent.role,
          output: `Condition [${task.condition}] not met. Output: ${output}`,
          status: 'failed',
          error: `Condition not met`,
          durationMs: Date.now() - taskStart,
        };
      }

      return {
        taskId: task.id,
        agentName: agent.role,
        output,
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

  private resolveAgent(task: ProcessTask, agents: Map<string, BaseAgent>): BaseAgent | undefined {
    if (task.assignedAgent) {
      return agents.get(task.assignedAgent);
    }
    return agents.values().next().value;
  }
}

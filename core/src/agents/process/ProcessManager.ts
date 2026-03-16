/**
 * ProcessManager — selects and runs the appropriate process strategy.
 *
 * Provides a single entry point for the Orchestrator to execute tasks
 * using sequential, parallel, or hierarchical patterns.
 */

import type { BaseAgent } from '../BaseAgent.js';
import type {
  ProcessType,
  ProcessTask,
  ProcessRunResult,
  ProcessStrategy,
} from './types.js';
import { SequentialProcess } from './SequentialProcess.js';
import { ParallelProcess } from './ParallelProcess.js';
import { HierarchicalProcess } from './HierarchicalProcess.js';

export class ProcessManager {
  private strategies: Map<ProcessType, ProcessStrategy> = new Map();

  constructor() {
    this.strategies.set('sequential', new SequentialProcess());
    this.strategies.set('parallel', new ParallelProcess());
    this.strategies.set('hierarchical', new HierarchicalProcess());
  }

  /**
   * Run a set of tasks using the specified process pattern.
   *
   * @param type - Process pattern to use
   * @param tasks - Tasks to execute
   * @param agents - Available agents (name → instance)
   * @param managerAgent - Required for 'hierarchical' pattern
   */
  async run(
    type: ProcessType,
    tasks: ProcessTask[],
    agents: Map<string, BaseAgent>,
    managerAgent?: BaseAgent,
  ): Promise<ProcessRunResult> {
    const strategy = this.strategies.get(type);
    if (!strategy) {
      throw new Error(`Unknown process type: "${type}"`);
    }

    if (type === 'hierarchical' && !managerAgent) {
      throw new Error(
        'Hierarchical process requires a managerAgent. Pass the managing agent explicitly.',
      );
    }

    console.log(
      `[ProcessManager] Running ${type} process with ${tasks.length} task(s) and ${agents.size} agent(s)`,
    );

    return strategy.execute(tasks, agents, managerAgent);
  }

  /**
   * Convenience: run a single task description sequentially with one agent.
   */
  async runSingle(
    description: string,
    agent: BaseAgent,
  ): Promise<ProcessRunResult> {
    const agents = new Map<string, BaseAgent>();
    agents.set(agent.role, agent);

    return this.run(
      'sequential',
      [{ id: 'single', description, assignedAgent: agent.role }],
      agents,
    );
  }
}

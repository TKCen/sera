/**
 * Process Pattern types for Orchestrator V2.
 *
 * Defines the strategy interface and shared types used by
 * Sequential, Parallel, and Hierarchical process patterns.
 */

import type { BaseAgent } from '../BaseAgent.js';

// ── Process Types ───────────────────────────────────────────────────────────────

export type ProcessType = 'sequential' | 'parallel' | 'hierarchical';

// ── Task & Result ───────────────────────────────────────────────────────────────

export interface ProcessTask {
  id: string;
  description: string;
  /** Name of the agent to assign this task to (optional — orchestrator may decide). */
  assignedAgent?: string;
  /** IDs of tasks that must complete before this one (sequential/hierarchical). */
  dependsOn?: string[];
}

export interface ProcessResult {
  taskId: string;
  agentName: string;
  output: string;
  status: 'completed' | 'failed';
  error?: string;
  durationMs: number;
}

// ── Aggregate Result ────────────────────────────────────────────────────────────

export interface ProcessRunResult {
  processType: ProcessType;
  results: ProcessResult[];
  finalOutput: string;
  totalDurationMs: number;
}

// ── Strategy Interface ──────────────────────────────────────────────────────────

export interface ProcessStrategy {
  readonly type: ProcessType;

  /**
   * Execute a set of tasks using the agents provided.
   *
   * @param tasks - The tasks to execute
   * @param agents - Map of agent name → agent instance
   * @param managerAgent - (Hierarchical only) The agent that validates results
   */
  execute(
    tasks: ProcessTask[],
    agents: Map<string, BaseAgent>,
    managerAgent?: BaseAgent,
  ): Promise<ProcessRunResult>;
}

/**
 * Dogfeed module — SERA's self-improvement loop.
 *
 * Phase 0 bootstrap: TypeScript orchestration proving the loop concept.
 * Phase 1+: Rust implementation in sera-core-rs replaces this module.
 */

export { DogfeedLoop } from './loop.js';
export { DogfeedAnalyzer } from './analyzer.js';
export { AgentSpawner } from './agent-spawner.js';
export { VerifyMerge } from './verify-merge.js';
export { createDefaultConfig } from './constants.js';
export type {
  DogfeedTask,
  DogfeedCycleResult,
  DogfeedConfig,
  AgentTier,
  TaskCategory,
  CyclePhase,
  CycleStatus,
} from './types.js';

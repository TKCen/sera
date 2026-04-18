/**
 * Dogfeed loop types — domain model for SERA's self-improvement cycle.
 */

// ── Agent Tier ──────────────────────────────────────────────────────────────────

/** Which coding agent handles a task */
export type AgentTier = 'pi-agent' | 'omc';

/** Task complexity level — drives agent routing */
export type TaskComplexity = 'trivial' | 'complex';

// ── Task Category ───────────────────────────────────────────────────────────────

export type TaskCategory =
  | 'lint'
  | 'type-error'
  | 'todo'
  | 'dead-code'
  | 'test'
  | 'refactor'
  | 'feature'
  | 'research'
  | 'infra';

/** Categories considered trivial — routed to pi-agent (free tier) */
export const TRIVIAL_CATEGORIES: ReadonlySet<TaskCategory> = new Set<TaskCategory>([
  'lint',
  'todo',
  'dead-code',
]);

// ── Task ────────────────────────────────────────────────────────────────────────

export interface DogfeedTask {
  /** Priority level (0 = highest) */
  priority: number;
  /** Category tag */
  category: TaskCategory;
  /** Human-readable description */
  description: string;
  /** Current status */
  status: 'ready' | 'in-progress' | 'done' | 'failed';
  /** Optional file path hint */
  filePath?: string;
  /** Optional line number hint */
  line?: number;
}

// ── Cycle Status ────────────────────────────────────────────────────────────────

export type CyclePhase =
  | 'idle'
  | 'analyzing'
  | 'branching'
  | 'executing'
  | 'verifying'
  | 'merging'
  | 'recording'
  | 'failed'
  | 'completed';

export interface CycleStatus {
  phase: CyclePhase;
  task?: DogfeedTask;
  branch?: string;
  agent?: AgentTier;
  startedAt?: string;
  error?: string;
}

// ── Cycle Result ────────────────────────────────────────────────────────────────

export interface DogfeedCycleResult {
  success: boolean;
  task: DogfeedTask;
  agent: AgentTier;
  branch: string;
  /** CI outcome */
  ciPassed: boolean;
  /** Whether the branch was merged to main */
  merged: boolean;
  /** Duration in milliseconds */
  durationMs: number;
  /** Estimated token usage */
  estimatedTokens: number;
  /** Files changed count */
  filesChanged: number;
  /** Lines added/removed */
  linesAdded: number;
  linesRemoved: number;
  /** Error message if failed */
  error?: string;
  /** Agent stdout/stderr (truncated) */
  agentOutput?: string;
  /** CI output (truncated) */
  ciOutput?: string;
}

// ── Config ──────────────────────────────────────────────────────────────────────

export interface DogfeedConfig {
  /** Path to the sera repo root */
  repoRoot: string;
  /** Path to the task tracker markdown file */
  taskFile: string;
  /** Path to the phase log markdown file */
  phaseLog: string;
  /** Agent timeout in milliseconds */
  agentTimeoutMs: number;
  /** pi-agent model identifier */
  piAgentModel: string;
  /** pi-agent provider name */
  piAgentProvider: string;
  /** Whether to push branches to remote */
  pushToRemote: boolean;
  /** Whether to auto-merge on CI pass */
  autoMerge: boolean;
  /** Git user name for dogfeed commits */
  gitUserName: string;
  /** Git user email for dogfeed commits */
  gitUserEmail: string;
}

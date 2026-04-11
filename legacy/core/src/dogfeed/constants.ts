/**
 * Dogfeed loop constants — defaults, CI commands, and configuration.
 */

import type { DogfeedConfig } from './types.js';
import path from 'node:path';

/** Default repo root — resolved relative to core/ */
const DEFAULT_REPO_ROOT = path.resolve(import.meta.dirname, '..', '..', '..');

/** CI verification commands — run sequentially, all must pass */
export const CI_COMMANDS = [
  { name: 'typecheck', cmd: 'bun', args: ['run', 'typecheck'] },
  { name: 'lint', cmd: 'bun', args: ['run', 'lint'] },
  { name: 'test', cmd: 'bun', args: ['test'] },
] as const;

/** Default agent timeout: 30 minutes */
export const DEFAULT_AGENT_TIMEOUT_MS = 30 * 60 * 1000;

/** Max agent output to store in cycle result (characters) */
export const MAX_AGENT_OUTPUT_LENGTH = 10_000;

/** Max CI output to store in cycle result (characters) */
export const MAX_CI_OUTPUT_LENGTH = 5_000;

/** Branch prefix for dogfeed work */
export const DOGFEED_BRANCH_PREFIX = 'dogfeed/';

/** Commit message prefix */
export const DOGFEED_COMMIT_PREFIX = 'dogfeed';

/** Co-author line for dogfeed commits */
export const DOGFEED_CO_AUTHOR = 'Co-Authored-By: SERA Dogfeed <noreply@sera.dev>';

/** pi-agent CLI command */
export const PI_AGENT_CMD = 'pi';

/** Default pi-agent model for local Qwen 3.5 35B */
export const DEFAULT_PI_AGENT_MODEL = 'qwen/qwen3.5-35b-a3b';

/** Default pi-agent provider */
export const DEFAULT_PI_AGENT_PROVIDER = 'lmstudio';

/** OMC Docker image name */
export const OMC_DOCKER_IMAGE = 'sera-dogfeed-agent:latest';

/** Container label for dogfeed agent containers */
export const DOGFEED_CONTAINER_LABEL = 'sera.dogfeed=true';

/** Default config factory */
export function createDefaultConfig(overrides?: Partial<DogfeedConfig>): DogfeedConfig {
  return {
    repoRoot: DEFAULT_REPO_ROOT,
    taskFile: path.join(DEFAULT_REPO_ROOT, 'docs', 'DOGFEED-TASKS.md'),
    phaseLog: path.join(DEFAULT_REPO_ROOT, 'docs', 'DOGFEED-PHASE0-LOG.md'),
    agentTimeoutMs: DEFAULT_AGENT_TIMEOUT_MS,
    piAgentModel: DEFAULT_PI_AGENT_MODEL,
    piAgentProvider: DEFAULT_PI_AGENT_PROVIDER,
    pushToRemote: true,
    autoMerge: true,
    gitUserName: 'SERA Dogfeed',
    gitUserEmail: 'dogfeed@sera.dev',
    ...overrides,
  };
}

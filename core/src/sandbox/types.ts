/**
 * Sandbox types — interfaces for the container management layer.
 * @see sera/docs/reimplementation/agent-workspace-architecture.md § Sandbox Manager API
 */

import type { SecurityTier } from '../agents/manifest/types.js';

// ── Container Type ──────────────────────────────────────────────────────────────

export type SandboxType = 'subagent' | 'tool';

// ── Tier Limits ─────────────────────────────────────────────────────────────────

export type NetworkMode = 'none' | 'sera_net' | 'bridge';
export type FilesystemMode = 'ro' | 'rw';

export interface TierLimits {
  tier: SecurityTier;
  cpuShares: number;          // Docker CPU shares (relative weight)
  memoryBytes: number;        // Memory limit in bytes
  networkMode: NetworkMode;
  filesystemMode: FilesystemMode;
}

// ── Spawn Request ───────────────────────────────────────────────────────────────

export interface SpawnRequest {
  /** Name of the requesting agent (must match an AGENT.yaml) */
  agentName: string;
  /** Container type */
  type: SandboxType;
  /** Docker image to use */
  image: string;
  /** Command to run inside the container */
  command?: string[];
  /** Environment variables */
  env?: Record<string, string>;
  /** Working directory inside the container */
  workDir?: string;
  /** For subagents: the role of the subagent to spawn */
  subagentRole?: string;
  /** For subagents: the task description */
  task?: string;
}

// ── Exec Request ────────────────────────────────────────────────────────────────

export interface ExecRequest {
  /** Container ID to exec into */
  containerId: string;
  /** Name of the requesting agent */
  agentName: string;
  /** Command to execute */
  command: string[];
}

// ── Tool Run Request ────────────────────────────────────────────────────────────

export interface ToolRunRequest {
  /** Name of the requesting agent */
  agentName: string;
  /** Tool name (must be in agent's tools.allowed) */
  toolName: string;
  /** Command to execute in the tool container */
  command: string[];
  /** Docker image to use (defaults to alpine) */
  image?: string;
  /** Timeout in seconds (default 60, max 300) */
  timeoutSeconds?: number;
}

// ── Tool Result ─────────────────────────────────────────────────────────────────

export interface ToolResult {
  exitCode: number;
  stdout: string;
  stderr: string;
  timedOut: boolean;
  durationMs: number;
}

// ── Sandbox Info ────────────────────────────────────────────────────────────────

export interface SandboxInfo {
  containerId: string;
  agentName: string;
  type: SandboxType;
  image: string;
  status: 'running' | 'stopped' | 'removing';
  createdAt: string;
  tier: SecurityTier;
  /** For subagents: the parent agent name */
  parentAgent?: string;
  /** For subagents: the role */
  subagentRole?: string;
}

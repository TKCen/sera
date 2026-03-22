/**
 * Sandbox types — interfaces for the container management layer.
 */

import type { SecurityTier } from '../agents/manifest/types.js';

// ── Container Type ──────────────────────────────────────────────────────────────

export type SandboxType = 'agent' | 'subagent' | 'tool' | 'mcp-server';

// ── Tier Limits ─────────────────────────────────────────────────────────────────

export type NetworkMode = 'none' | 'agent_net';
export type FilesystemMode = 'ro' | 'rw';

export interface TierLimits {
  tier: SecurityTier;
  cpuShares: number;
  memoryBytes: number;
  networkMode: NetworkMode;
  filesystemMode: FilesystemMode;
}

// ── Spawn Request ───────────────────────────────────────────────────────────────

export interface SpawnRequest {
  agentName: string;
  type: SandboxType;
  image: string;
  command?: string[];
  env?: Record<string, string>;
  workDir?: string;
  hostWorkspacePath?: string;
  subagentRole?: string;
  task?: string;
  /** Pre-signed agent identity JWT — included as SERA_IDENTITY_TOKEN env var */
  token?: string;
  /** Lifecycle mode of the agent being spawned */
  lifecycleMode?: 'persistent' | 'ephemeral';
  /** Parent instance ID for lineage tracking */
  parentInstanceId?: string;
}

// ── Exec Request ────────────────────────────────────────────────────────────────

export interface ExecRequest {
  containerId: string;
  agentName: string;
  command: string[];
}

// ── Tool Run Request ────────────────────────────────────────────────────────────

export interface ToolRunRequest {
  agentName: string;
  toolName: string;
  command: string[];
  image?: string;
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
  status: 'running' | 'stopped' | 'removing' | 'error';
  createdAt: string;
  tier: number;
  instanceId: string;
  parentAgent?: string;
  subagentRole?: string;
  lifecycleMode?: 'persistent' | 'ephemeral';
  /** Whether outbound traffic routes through the egress proxy (Story 20.3) */
  proxyEnabled?: boolean;
  /** Container IP on agent_net — used by EgressAclManager for per-agent ACLs (Story 20.2) */
  containerIp?: string;
}

// ── Docker Event ─────────────────────────────────────────────────────────────────

export interface DockerLifecycleEvent {
  action: 'start' | 'stop' | 'die' | 'oom';
  containerId: string;
  instanceId: string;
  agentName: string;
  exitCode?: number;
}

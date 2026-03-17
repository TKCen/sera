/**
 * AgentManifest — TypeScript interface matching the AGENT.yaml schema.
 * @see sera/docs/reimplementation/agent-workspace-architecture.md
 */

// ── Security Tiers ──────────────────────────────────────────────────────────────
export type SecurityTier = 1 | 2 | 3;

// ── Metadata ────────────────────────────────────────────────────────────────────
export interface AgentMetadata {
  name: string;
  displayName: string;
  icon: string;
  circle: string;
  additionalCircles?: string[];
  tier: SecurityTier;
}

// ── Identity (BMAD-inspired) ────────────────────────────────────────────────────
export interface AgentIdentity {
  role: string;
  description: string;
  communicationStyle?: string;
  principles?: string[];
}

// ── Model Configuration ─────────────────────────────────────────────────────────
export interface ModelFallback {
  provider: string;
  name: string;
  maxComplexity?: number;
}

export interface ModelConfig {
  provider: string;
  name: string;
  temperature?: number;
  fallback?: ModelFallback[];
}

// ── Tools ───────────────────────────────────────────────────────────────────────
export interface ToolsConfig {
  allowed?: string[];
  denied?: string[];
}

// ── Subagents ───────────────────────────────────────────────────────────────────
export interface SubagentAllowedEntry {
  role: string;
  maxInstances?: number;
  requiresApproval?: boolean;
}

export interface SubagentsConfig {
  allowed?: SubagentAllowedEntry[];
}

// ── Intercom ────────────────────────────────────────────────────────────────────
export interface IntercomChannels {
  publish?: string[];
  subscribe?: string[];
}

export interface IntercomConfig {
  canMessage?: string[];
  channels?: IntercomChannels;
}

// ── Resources ───────────────────────────────────────────────────────────────────
export interface ResourcesConfig {
  memory?: string;
  cpu?: string;
  maxLlmTokensPerHour?: number;
}

// ── Workspace ───────────────────────────────────────────────────────────────────
export interface WorkspaceConfig {
  provider?: string;
  path?: string;
}

// ── Memory ──────────────────────────────────────────────────────────────────────
export interface MemoryConfig {
  personalMemory?: string;
  sharedKnowledge?: string;
}

// ── Permissions ─────────────────────────────────────────────────────────────────
export interface PermissionsConfig {
  canExec?: boolean;
  canSpawnSubagents?: boolean;
}

// ── Full Manifest ───────────────────────────────────────────────────────────────
export interface AgentManifest {
  apiVersion: string;
  kind: 'Agent';
  metadata: AgentMetadata;
  identity: AgentIdentity;
  model: ModelConfig;
  tools?: ToolsConfig;
  skills?: string[];
  subagents?: SubagentsConfig;
  intercom?: IntercomConfig;
  resources?: ResourcesConfig;
  workspace?: WorkspaceConfig;
  memory?: MemoryConfig;
  permissions?: PermissionsConfig;
}

// ── Known field names for validation ────────────────────────────────────────────
export const KNOWN_TOP_LEVEL_FIELDS = new Set([
  'apiVersion', 'kind', 'metadata', 'identity', 'model',
  'tools', 'skills', 'subagents', 'intercom', 'resources',
  'workspace', 'memory', 'permissions',
]);

export const VALID_TIERS: readonly SecurityTier[] = [1, 2, 3] as const;

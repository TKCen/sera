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
  circle?: string;
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
export interface HybridSearchConfig {
  vectorWeight: number;
  textWeight: number;
  minScore: number;
  maxResults: number;
  mmr?: {
    enabled: boolean;
    lambda: number;
    candidateMultiplier: number;
  };
  temporalDecay?: {
    enabled: boolean;
    halfLifeDays: number;
  };
}

export interface MemoryConfig {
  personalMemory?: string;
  sharedKnowledge?: string;
  citations?: 'full' | 'brief' | 'off';
  search?: HybridSearchConfig;
}

// ── Permissions ─────────────────────────────────────────────────────────────────
export interface PermissionsConfig {
  canExec?: boolean;
  canSpawnSubagents?: boolean;
}

// ── Schedules ───────────────────────────────────────────────────────────────────
export interface ScheduleManifest {
  name: string;
  description?: string;
  type: 'cron' | 'once';
  expression: string;
  task: string;
  status?: 'active' | 'paused';
  category?: string;
}

// ── Full Manifest ───────────────────────────────────────────────────────────────
export interface AgentManifest {
  apiVersion: string;
  kind: 'Agent';
  metadata: AgentMetadata;
  identity: AgentIdentity;
  model: ModelConfig;
  spec?: {
    identity?: {
      role?: string;
      principles?: string[];
    };
    model?: {
      provider?: string;
      name?: string;
      temperature?: number;
      fallback?: ModelFallback[];
    };
    sandboxBoundary?: string;
    policyRef?: string;
    capabilities?: Record<string, unknown>;
    lifecycle?: {
      mode: 'persistent' | 'ephemeral';
    };
    skills?: string[];
    skillPackages?: string[];
    tools?: {
      allowed?: string[];
      denied?: string[];
    };
    subagents?: {
      allowed?: Array<{
        templateRef: string;
        maxInstances?: number;
        lifecycle?: 'persistent' | 'ephemeral';
        requiresApproval?: boolean;
      }>;
    };
    resources?: {
      cpu?: string;
      memory?: string;
      maxLlmTokensPerHour?: number;
      maxLlmTokensPerDay?: number;
    };
    workspace?: Record<string, unknown>;
    memory?: Record<string, unknown>;
    schedules?: Array<{
      name: string;
      description?: string;
      type: 'cron' | 'once';
      expression: string;
      task: string;
      status?: 'active' | 'paused';
      category?: string;
    }>;
    contextFiles?: Array<{
      path: string;
      label: string;
      maxTokens?: number;
      priority?: 'high' | 'normal' | 'low';
    }>;
    bootContext?: {
      files?: Array<{
        path: string;
        label: string;
        maxTokens?: number;
      }>;
      directory?: string;
    };
    notes?: string;
  };
  tools?: ToolsConfig;
  skills?: Array<string | { name: string; version: string }>;
  skillPackages?: string[];
  subagents?: SubagentsConfig;
  intercom?: IntercomConfig;
  resources?: ResourcesConfig;
  workspace?: WorkspaceConfig;
  memory?: MemoryConfig;
  permissions?: PermissionsConfig;
  capabilities?: string[];
  logging?: {
    commands?: boolean;
  };
  bootContext?: {
    files?: Array<{
      path: string;
      label: string;
      maxTokens?: number;
    }>;
    directory?: string;
  };
  schedules?: ScheduleManifest[];
  mounts?: Array<{ hostPath: string; containerPath: string; mode: 'ro' | 'rw' }>;
  overrides?: Record<string, unknown>;
}

// ── Resolved Capabilities (Runtime) ─────────────────────────────────────────────
export interface ResolvedCapabilities {
  filesystem?: {
    write?: boolean;
    maxWorkspaceSizeGB?: number;
  };
  fs?: {
    write?: boolean;
  };
  network?: {
    outbound?: string[];
  };
  exec?: {
    commands?: string[];
  };
  resources?: {
    cpu_shares?: number;
    memory_limit?: number;
  };
  security?: {
    readonlyRootfs?: boolean;
  };
  secrets?: {
    access?: string[];
  };
  capabilities?: string[];
  skillPackages?: string[];
  [key: string]: unknown;
}

// ── Known field names for validation ────────────────────────────────────────────
export const KNOWN_TOP_LEVEL_FIELDS = new Set([
  'apiVersion',
  'kind',
  'metadata',
  'spec',
  'identity',
  'model',
  'tools',
  'skills',
  'skillPackages',
  'subagents',
  'intercom',
  'resources',
  'workspace',
  'memory',
  'permissions',
  'capabilities',
  'logging',
  'bootContext',
  'schedules',
  'mounts',
  'overrides',
]);

export const VALID_TIERS: readonly SecurityTier[] = [1, 2, 3] as const;

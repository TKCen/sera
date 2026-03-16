/**
 * CircleManifest — TypeScript interface matching the CIRCLE.yaml schema.
 * @see sera/docs/reimplementation/agent-workspace-architecture.md
 */

// ── Metadata ────────────────────────────────────────────────────────────────────
export interface CircleMetadata {
  name: string;
  displayName: string;
  description?: string;
}

// ── Project Context (BMAD-inspired constitution) ────────────────────────────────
export interface ProjectContextConfig {
  path: string;
  autoLoad?: boolean;
}

// ── Knowledge Scope ─────────────────────────────────────────────────────────────
export interface KnowledgeConfig {
  qdrantCollection: string;
  postgresSchema?: string;
}

// ── Intercom Channels ───────────────────────────────────────────────────────────
export type ChannelType = 'persistent' | 'ephemeral';

export interface ChannelConfig {
  name: string;
  type: ChannelType;
}

// ── Party Mode ──────────────────────────────────────────────────────────────────
export type SelectionStrategy = 'relevance' | 'round-robin' | 'all';

export interface PartyModeConfig {
  enabled: boolean;
  orchestrator?: string;
  selectionStrategy?: SelectionStrategy;
}

// ── Circle Connections (Federation) ─────────────────────────────────────────────
export interface CircleConnectionAuth {
  type: string;
  certPath?: string;
}

export interface CircleConnection {
  circle: string;
  bridgeChannels?: string[];
  auth?: string | CircleConnectionAuth;
}

// ── Full Circle Manifest ────────────────────────────────────────────────────────
export interface CircleManifest {
  apiVersion: string;
  kind: 'Circle';
  metadata: CircleMetadata;
  projectContext?: ProjectContextConfig;
  agents: string[];
  knowledge?: KnowledgeConfig;
  channels?: ChannelConfig[];
  partyMode?: PartyModeConfig;
  connections?: CircleConnection[];
}

// ── Known field names for validation ────────────────────────────────────────────
export const KNOWN_CIRCLE_FIELDS = new Set([
  'apiVersion', 'kind', 'metadata', 'projectContext', 'agents',
  'knowledge', 'channels', 'partyMode', 'connections',
]);

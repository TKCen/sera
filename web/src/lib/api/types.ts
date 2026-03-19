export interface AgentManifest {
  apiVersion?: string;
  kind?: string;
  metadata: {
    name: string;
    displayName?: string;
    templateRef?: string;
    circle?: string;
    icon?: string;
  };
  spec?: {
    identity?: {
      role?: string;
      principles?: string[];
    };
    model?: {
      provider?: string;
      name?: string;
      temperature?: number;
    };
    sandboxBoundary?: string;
    lifecycle?: {
      mode?: 'persistent' | 'ephemeral';
    };
    skills?: string[];
    tools?: {
      allowed?: string[];
      denied?: string[];
    };
    resources?: {
      cpu?: string;
      memory?: string;
      maxLlmTokensPerHour?: number;
      maxLlmTokensPerDay?: number;
    };
  };
  overrides?: Record<string, unknown>;
}

export interface AgentInfo {
  name: string;
  displayName?: string;
  status?: 'running' | 'stopped' | 'error';
  containerId?: string;
  circle?: string;
  templateRef?: string;
  resources?: Record<string, unknown>;
  skills?: string[];
  model?: {
    provider?: string;
    name?: string;
  };
}

export interface CircleSummary {
  name: string;
  displayName?: string;
  memberCount?: number;
}

export interface CircleManifest {
  apiVersion?: string;
  kind?: string;
  metadata: {
    name: string;
    displayName?: string;
  };
  spec?: {
    constitution?: string;
    members?: string[];
  };
}

export interface CircleDetails {
  name: string;
  displayName?: string;
  projectContext?: {
    content?: string;
    updatedAt?: string;
  };
  members?: string[];
  spec?: Record<string, unknown>;
}

export interface PartySessionInfo {
  id: string;
  circleId?: string;
  active?: boolean;
  createdAt?: string;
}

export interface MemoryBlock {
  id: string;
  type: string;
  entries?: MemoryEntry[];
  updatedAt?: string;
}

export interface MemoryEntry {
  id: string;
  title: string;
  content: string;
  refs?: string[];
  tags?: string[];
  source?: string;
  createdAt?: string;
  updatedAt?: string;
}

export interface MemoryGraphNode {
  id: string;
  label?: string;
  type?: string;
}

export interface MemoryGraphEdge {
  source: string;
  target: string;
}

export interface MemoryGraph {
  nodes: MemoryGraphNode[];
  edges: MemoryGraphEdge[];
}

export interface SearchResult {
  id: string;
  score: number;
  entry?: MemoryEntry;
}

export interface SkillInfo {
  id: string;
  name?: string;
  description?: string;
  usedBy?: string[];
}

export interface ContainerInfo {
  id: string;
  agentName?: string;
  status?: string;
  image?: string;
}

export interface ContainerResult {
  id: string;
  agentName?: string;
}

export interface ExecResult {
  exitCode: number;
  stdout: string;
  stderr: string;
}

export interface ToolRunResult {
  exitCode: number;
  output: string;
}

export interface SubagentRunResult {
  id: string;
  result?: string;
}

export interface MessageObject {
  id: string;
  agent: string;
  channel?: string;
  payload: unknown;
  timestamp?: string;
}

export interface LLMConfig {
  baseUrl: string;
  apiKey?: string;
  model: string;
}

export interface ProviderConfig {
  id: string;
  name: string;
  baseUrl?: string;
  model?: string;
  active?: boolean;
}

export interface ProvidersResponse {
  activeProvider: string;
  providers: ProviderConfig[];
}

export interface ErrorResponse {
  error: string;
  code?: string;
}

export interface RtTokenResponse {
  token: string;
  expiresAt: number;
}

export interface HealthResponse {
  status: string;
  service: string;
  timestamp: string;
}

export interface ThoughtEvent {
  timestamp: string;
  stepType: 'observe' | 'plan' | 'act' | 'reflect' | 'tool-call' | 'tool-result' | 'reasoning';
  content: string;
  agentId: string;
  agentDisplayName?: string;
}

export interface AgentTemplate {
  name: string;
  displayName?: string;
  description?: string;
  spec?: Record<string, unknown>;
  lockedFields?: string[];
}

export interface AgentTask {
  id: string;
  agentName: string;
  type: 'chat' | 'cron' | 'event';
  status: 'pending' | 'running' | 'done' | 'error';
  input?: string;
  output?: string;
  messageId?: string;
  createdAt?: string;
  completedAt?: string;
}

export interface AgentSchedule {
  id: string;
  agentName: string;
  cron: string;
  description?: string;
  lastRunAt?: string;
  lastRunStatus?: 'success' | 'error';
  nextRunAt?: string;
  enabled: boolean;
}

export interface AgentMemoryBlock {
  id: string;
  agentName: string;
  scope: 'personal' | 'circle' | 'global';
  type: string;
  title: string;
  content?: string;
  tags?: string[];
  updatedAt?: string;
}

export interface ToolInfo {
  id: string;
  name?: string;
  description?: string;
  server?: string;
}

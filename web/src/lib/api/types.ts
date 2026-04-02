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

export interface AgentInstance {
  id: string;
  name: string;
  display_name?: string;
  template_ref: string;
  status: string;
  circle?: string;
  lifecycle_mode?: 'persistent' | 'ephemeral';
  icon?: string;
  sandbox_boundary?: string;
  overrides?: Record<string, unknown>;
  created_at?: string;
  updated_at?: string;
}

export interface CreateAgentInstanceParams {
  templateRef: string;
  name: string;
  displayName?: string;
  circle?: string;
  overrides?: Record<string, unknown>;
  lifecycleMode?: 'persistent' | 'ephemeral';
  start?: boolean;
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
  displayName: string;
  description?: string;
  agents?: string[];
  hasProjectContext?: boolean;
  channelCount?: number;
}

export interface CircleChannelConfig {
  id?: string;
  name: string;
  type?: 'persistent' | 'ephemeral';
  description?: string;
}

export interface CirclePartyModeConfig {
  enabled: boolean;
  orchestrator?: string;
  selectionStrategy?: 'relevance' | 'round-robin' | 'all';
}

export interface CircleKnowledgeConfig {
  qdrantCollection: string;
  postgresSchema?: string;
}

export interface CircleConnectionConfig {
  circle: string;
  bridgeChannels?: string[];
  auth?: 'internal' | Record<string, unknown>;
}

export interface CircleManifest {
  apiVersion?: string;
  kind?: string;
  metadata: {
    name: string;
    displayName: string;
    description?: string;
  };
  agents: string[];
  channels?: CircleChannelConfig[];
  knowledge?: CircleKnowledgeConfig;
  partyMode?: CirclePartyModeConfig;
  projectContext?: { path: string; autoLoad?: boolean };
  connections?: CircleConnectionConfig[];
}

/** Full manifest + resolved project context content from GET /circles/:name.
 *  DB-sourced circles return flat properties; YAML circles use the metadata wrapper. */
export interface CircleDetails extends CircleManifest {
  /** Flat DB properties (may not have metadata wrapper) */
  id?: string;
  name?: string;
  displayName?: string;
  description?: string;
  constitution?: string;
  members?: string[];
  createdAt?: string;
  updatedAt?: string;
  projectContext?: { path: string; autoLoad?: boolean; content?: string };
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

export interface GuidanceSkillInfo {
  id: string;
  name: string;
  version?: string;
  description?: string;
  category?: string;
  tags?: string[];
  triggers?: string[];
  maxTokens?: number;
  source?: string;
  usedBy?: string[];
}

export interface CreateSkillParams {
  name: string;
  version: string;
  description: string;
  triggers: string[];
  category?: string;
  tags?: string[];
  maxTokens?: number;
  content: string;
}

export interface ExternalSkillEntry {
  id: string;
  name: string;
  description: string;
  author?: string;
  version?: string;
  tags?: string[];
  source: string;
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
  defaultModel?: string | null;
}

/** Sanitised model entry from GET /api/providers — no API keys. */
export interface ProviderConfig {
  modelName: string;
  api: string;
  provider?: string;
  baseUrl?: string;
  description?: string;
  dynamicProviderId?: string;
  authStatus?: string;
  contextWindow?: number;
  maxTokens?: number;
  contextStrategy?: 'summarize' | 'sliding-window' | 'truncate';
  contextHighWaterMark?: number;
  contextCompactionModel?: string;
}

export interface ProvidersResponse {
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

export interface MessageThought {
  timestamp: string;
  stepType: string;
  content: string;
  toolName?: string;
  toolArgs?: Record<string, unknown>;
}

export interface Message {
  id: string;
  role: 'user' | 'agent';
  content: string;
  thoughts: MessageThought[];
  streaming: boolean;
  createdAt: Date;
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
  builtin?: boolean;
  category?: string;
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
  category?: string | null;
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
  description?: string;
  parameters?: Array<{ name: string; type: string; description: string; required: boolean }>;
  source: 'builtin' | 'mcp' | 'custom';
  server?: string;
  minTier?: 1 | 2 | 3;
  capabilityRequired?: string;
  usedBy?: string[];
}

// Capability Grants

export interface CapabilityGrant {
  id: string;
  agent_instance_id: string;
  dimension: string;
  value: string;
  grant_type: 'one-time' | 'session' | 'persistent';
  granted_by?: string;
  granted_by_email?: string;
  granted_by_name?: string;
  expires_at?: string;
  revoked_at?: string;
  created_at: string;
}

export interface CreateGrantParams {
  dimension: string;
  value: string;
  grantType: 'one-time' | 'session' | 'persistent';
  expiresAt?: string;
}

// Permission Requests

export interface PermissionRequest {
  requestId: string;
  agentId: string;
  agentName: string;
  dimension: 'filesystem' | 'network' | 'exec.commands';
  value: string;
  reason?: string;
  requestedAt: string;
  status: 'pending' | 'granted' | 'denied' | 'expired';
}

export interface AgentDelegation {
  id: string;
  principal_id: string;
  principal_name: string;
  scope: {
    service: string;
    permissions: string[];
    resourceConstraints?: Record<string, string[]>;
  };
  grant_type: 'one-time' | 'session' | 'persistent';
  issued_at: string;
  expires_at?: string;
  last_used_at?: string;
  use_count: number;
  status: 'active' | 'revoked' | 'expired';
}

export interface PermissionDecisionParams {
  decision: 'grant' | 'deny';
  grantType?: 'one-time' | 'session' | 'persistent';
  expiresAt?: string;
}

// Epic 14 — Observability types

export interface UsageDataPoint {
  timestamp: string;
  promptTokens: number;
  completionTokens: number;
  totalTokens: number;
  cost?: number;
}

export interface AgentUsage {
  agentName: string;
  promptTokens: number;
  completionTokens: number;
  totalTokens: number;
  pctOfTotal: number;
}

export interface ModelUsage {
  model: string;
  promptTokens: number;
  completionTokens: number;
  totalTokens: number;
}

export interface UsageSummary {
  totalTokensToday: number;
  totalTokensMonth: number;
  estimatedCost?: number;
  mostActiveAgent?: string;
}

export interface UsageResponse {
  summary: UsageSummary;
  timeSeries: UsageDataPoint[];
  byAgent: AgentUsage[];
  byModel: ModelUsage[];
}

export interface AuditEvent {
  id: string;
  sequence: number;
  timestamp: string;
  actorId: string;
  actorType: 'agent' | 'operator';
  actorName?: string;
  eventType: string;
  resourceType?: string;
  resourceId?: string;
  status: 'success' | 'failure';
  payload?: Record<string, unknown>;
  hash?: string;
}

export interface AuditResponse {
  events: AuditEvent[];
  total: number;
  page: number;
  pageSize: number;
}

export interface AuditVerifyResult {
  valid: boolean;
  brokenAtSequence?: number;
  checkedCount: number;
}

export interface ComponentHealth {
  name: string;
  status: 'healthy' | 'degraded' | 'unreachable';
  message?: string;
  latencyMs?: number;
}

export interface AgentStats {
  total: number;
  running: number;
  stopped: number;
  errored: number;
}

export interface HealthDetail {
  status: 'healthy' | 'degraded' | 'unhealthy';
  components: ComponentHealth[];
  agentStats: AgentStats;
  timestamp: string;
}

export interface CircuitBreakerState {
  provider: string;
  state: 'closed' | 'open' | 'half-open';
  failures: number;
  lastFailureAt?: string;
  nextRetryAt?: string;
}

export interface Schedule {
  id: string;
  agentName: string;
  name: string;
  type: 'cron' | 'once';
  expression: string;
  taskPrompt?: string;
  status: 'active' | 'paused';
  source: 'manifest' | 'api';
  category?: string | null;
  lastRunAt?: string;
  lastRunStatus?: 'success' | 'error' | 'missed';
  lastRunOutput?: string;
  nextRunAt?: string;
}

export interface ScheduleRun {
  taskId: string;
  scheduleId: string;
  scheduleName: string;
  scheduleCategory: string | null;
  status: 'queued' | 'running' | 'completed' | 'failed';
  result: unknown;
  error: string | null;
  usage: { promptTokens: number; completionTokens: number; totalTokens: number } | null;
  exitReason: string | null;
  firedAt: string;
  startedAt: string | null;
  completedAt: string | null;
  createdAt: string;
}

export interface AgentBudget {
  maxLlmTokensPerHour?: number;
  maxLlmTokensPerDay?: number;
  currentHourTokens: number;
  currentDayTokens: number;
}

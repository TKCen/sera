import { request, requestText } from './client';
import type {
  AgentInstance,
  AgentManifest,
  AgentInfo,
  AgentTask,
  AgentSchedule,
  Schedule,
  AgentMemoryBlock,
  ThoughtEvent,
  CreateAgentInstanceParams,
  CapabilityGrant,
  CreateGrantParams,
  PermissionRequest,
  PermissionDecisionParams,
  AgentDelegation,
  TemplateDiff,
} from './types';

// ── Instance-based endpoints ─────────────────────────────────────────────────

export function listAgents(): Promise<AgentInstance[]> {
  return request<AgentInstance[]>('/agents');
}

export function getAgentInstance(id: string): Promise<AgentInstance> {
  return request<AgentInstance>(`/agents/instances/${encodeURIComponent(id)}`);
}

export function createAgentInstance(params: CreateAgentInstanceParams): Promise<AgentInstance> {
  return request<AgentInstance>('/agents/instances', {
    method: 'POST',
    body: JSON.stringify(params),
  });
}

export function startAgent(id: string): Promise<AgentInstance> {
  return request<AgentInstance>(`/agents/instances/${encodeURIComponent(id)}/start`, {
    method: 'POST',
  });
}

export function stopAgent(id: string): Promise<AgentInstance> {
  return request<AgentInstance>(`/agents/instances/${encodeURIComponent(id)}/stop`, {
    method: 'POST',
  });
}

export function updateAgentInstance(
  id: string,
  params: {
    name?: string;
    displayName?: string;
    circle?: string;
    lifecycleMode?: string;
    overrides?: Record<string, unknown>;
  }
): Promise<AgentInstance> {
  return request<AgentInstance>(`/agents/instances/${encodeURIComponent(id)}`, {
    method: 'PATCH',
    body: JSON.stringify(params),
  });
}

export function deleteAgent(id: string): Promise<void> {
  return request<void>(`/agents/instances/${encodeURIComponent(id)}`, { method: 'DELETE' });
}

// ── Legacy endpoints (kept for compatibility) ───────────────────────────────

export function getAgent(name: string): Promise<AgentInfo> {
  return request<AgentInfo>(`/agents/${encodeURIComponent(name)}`);
}

export function validateAgentManifest(
  manifest: AgentManifest
): Promise<{ valid: boolean; errors?: string[] }> {
  return request<{ valid: boolean; errors?: string[] }>('/agents/validate', {
    method: 'POST',
    body: JSON.stringify(manifest),
  });
}

// ── Instance-scoped data endpoints ──────────────────────────────────────────

export function getAgentLogs(id: string): Promise<string> {
  return requestText(`/agents/${encodeURIComponent(id)}/logs`);
}

export function getAgentMemory(id: string, scope?: string): Promise<AgentMemoryBlock[]> {
  const params = scope ? `?scope=${encodeURIComponent(scope)}` : '';
  return request<AgentMemoryBlock[]>(`/memory/${encodeURIComponent(id)}/blocks${params}`);
}

export async function getAgentSchedules(id: string): Promise<AgentSchedule[]> {
  // The schedule list endpoint is /api/schedules?agentId=:id (not /api/agents/:id/schedules)
  const schedules = await request<Schedule[]>(`/schedules?agentId=${encodeURIComponent(id)}`);
  return schedules.map((s) => ({
    id: s.id,
    agentName: s.agentName,
    cron: s.expression,
    description: s.name,
    category: s.category,
    lastRunAt: s.lastRunAt,
    lastRunStatus: s.lastRunStatus as AgentSchedule['lastRunStatus'],
    nextRunAt: s.nextRunAt,
    enabled: s.status === 'active',
    source: s.source,
  }));
}

export function getAgentTasks(id: string, status?: string): Promise<AgentTask[]> {
  const params = status ? `?status=${encodeURIComponent(status)}` : '';
  return request<AgentTask[]>(`/agents/${encodeURIComponent(id)}/tasks${params}`);
}

export function cancelAgentTask(agentId: string, taskId: string): Promise<{ taskId: string }> {
  return request<{ taskId: string }>(
    `/agents/${encodeURIComponent(agentId)}/tasks/${encodeURIComponent(taskId)}/cancel`,
    { method: 'POST' }
  );
}

export function clearStaleTasks(agentId: string, timeout?: number): Promise<{ cleared: number }> {
  const params = timeout ? `?timeout=${timeout}` : '';
  return request<{ cleared: number }>(
    `/agents/${encodeURIComponent(agentId)}/tasks/clear-stale${params}`,
    { method: 'POST' }
  );
}

export function createAgentTask(id: string, input: string): Promise<AgentTask> {
  return request<AgentTask>(`/agents/${encodeURIComponent(id)}/tasks`, {
    method: 'POST',
    body: JSON.stringify({ type: 'chat', input }),
  });
}

export function getAgentThoughts(id: string, taskId?: string): Promise<ThoughtEvent[]> {
  const params = taskId ? `?taskId=${encodeURIComponent(taskId)}` : '';
  return request<ThoughtEvent[]>(`/agents/${encodeURIComponent(id)}/thoughts${params}`);
}

export function getAgentTools(id: string): Promise<import('./types').AgentToolsResponse> {
  return request<import('./types').AgentToolsResponse>(
    `/agents/instances/${encodeURIComponent(id)}/tools`
  );
}

export function getAgentDelegations(id: string): Promise<AgentDelegation[]> {
  return request<AgentDelegation[]>(`/agents/${encodeURIComponent(id)}/delegations`);
}

// ── Capability Grants ────────────────────────────────────────────────────────

export function getAgentGrants(
  id: string
): Promise<{ session: CapabilityGrant[]; persistent: CapabilityGrant[] }> {
  return request<{ session: CapabilityGrant[]; persistent: CapabilityGrant[] }>(
    `/agents/${encodeURIComponent(id)}/grants`
  );
}

export function createAgentGrant(id: string, params: CreateGrantParams): Promise<CapabilityGrant> {
  return request<CapabilityGrant>(`/agents/${encodeURIComponent(id)}/grants`, {
    method: 'POST',
    body: JSON.stringify(params),
  });
}

export function revokeAgentGrant(id: string, grantId: string): Promise<void> {
  return request<void>(`/agents/${encodeURIComponent(id)}/grants/${encodeURIComponent(grantId)}`, {
    method: 'DELETE',
  });
}

// ── Permission Requests ─────────────────────────────────────────────────────

export function listPermissionRequests(agentId?: string): Promise<PermissionRequest[]> {
  const params = agentId ? `?agentId=${encodeURIComponent(agentId)}` : '';
  return request<PermissionRequest[]>(`/permission-requests${params}`);
}

export function decidePermissionRequest(
  requestId: string,
  params: PermissionDecisionParams
): Promise<PermissionRequest> {
  return request<PermissionRequest>(
    `/permission-requests/${encodeURIComponent(requestId)}/decision`,
    { method: 'POST', body: JSON.stringify(params) }
  );
}

// ── Template Diff ────────────────────────────────────────────────────────────

export function getTemplateDiff(agentId: string): Promise<TemplateDiff> {
  return request<TemplateDiff>(`/agents/${encodeURIComponent(agentId)}/template-diff`);
}

export function applyTemplateUpdate(
  agentId: string,
  paths?: string[]
): Promise<{ success: boolean }> {
  return request<{ success: boolean }>(
    `/agents/${encodeURIComponent(agentId)}/apply-template-update`,
    {
      method: 'POST',
      body: JSON.stringify({ paths }),
    }
  );
}

// ── Agent Detail Diagnostics ────────────────────────────────────────────────

export interface ContextDebugResponse {
  agentId: string;
  agentName: string;
  testMessage: string;
  systemPromptLength: number;
  events: Array<{ stage: string; detail: Record<string, unknown>; durationMs?: number }>;
}

export function getAgentContextDebug(id: string, message: string): Promise<ContextDebugResponse> {
  return request<ContextDebugResponse>(
    `/agents/${encodeURIComponent(id)}/context-debug?message=${encodeURIComponent(message)}`
  );
}

export interface AgentHealthCheckResult {
  agentId: string;
  agentName?: string;
  overallStatus: string;
  checks: Record<string, { ok: boolean; detail?: string }>;
}

export function getAgentHealthCheck(id: string): Promise<AgentHealthCheckResult> {
  return request<AgentHealthCheckResult>(`/agents/${encodeURIComponent(id)}/health-check`);
}

export function getAgentSystemPrompt(id: string): Promise<{ prompt: string }> {
  return request<{ prompt: string }>(`/agents/${encodeURIComponent(id)}/system-prompt`);
}

export interface AgentSession {
  id: string;
  title: string;
  updatedAt: string;
}

export function getAgentSessions(agentId: string): Promise<AgentSession[]> {
  return request<AgentSession[]>(`/sessions?agentInstanceId=${encodeURIComponent(agentId)}`);
}

export function skipTemplateUpdate(agentId: string): Promise<{ success: boolean }> {
  return request<{ success: boolean }>(
    `/agents/${encodeURIComponent(agentId)}/skip-template-update`,
    {
      method: 'POST',
    }
  );
}

export interface PendingUpdate {
  instanceId: string;
  instanceName: string;
  templateName: string;
  templateUpdatedAt: string;
}

export function getPendingUpdates(): Promise<PendingUpdate[]> {
  return request<PendingUpdate[]>('/agents/pending-updates');
}

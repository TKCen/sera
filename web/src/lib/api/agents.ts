import { request, requestText } from './client';
import type {
  AgentManifest,
  AgentInfo,
  AgentTask,
  AgentSchedule,
  AgentMemoryBlock,
  ThoughtEvent,
} from './types';

export function listAgents(): Promise<AgentManifest[]> {
  return request<AgentManifest[]>('/agents');
}

export function getAgent(name: string): Promise<AgentInfo> {
  return request<AgentInfo>(`/agents/${encodeURIComponent(name)}`);
}

export function createAgent(manifest: AgentManifest): Promise<{ success: boolean }> {
  return request<{ success: boolean }>(
    `/agents/${encodeURIComponent(manifest.metadata.name)}/manifest`,
    { method: 'PUT', body: JSON.stringify(manifest) }
  );
}

export function getAgentManifestRaw(name: string): Promise<string> {
  return requestText(`/agents/${encodeURIComponent(name)}/manifest/raw`);
}

export function updateAgentManifest(
  name: string,
  manifest: AgentManifest
): Promise<{ success: boolean }> {
  return request<{ success: boolean }>(`/agents/${encodeURIComponent(name)}/manifest`, {
    method: 'PUT',
    body: JSON.stringify(manifest),
  });
}

export function validateAgentManifest(
  manifest: AgentManifest
): Promise<{ valid: boolean; errors?: string[] }> {
  return request<{ valid: boolean; errors?: string[] }>('/agents/validate', {
    method: 'POST',
    body: JSON.stringify(manifest),
  });
}

export function reloadAgents(): Promise<{ success: boolean }> {
  return request<{ success: boolean }>('/agents/reload', { method: 'POST' });
}

export function deleteAgent(name: string): Promise<void> {
  return request<void>(`/agents/${encodeURIComponent(name)}`, { method: 'DELETE' });
}

export function startAgent(name: string): Promise<{ success: boolean }> {
  return request<{ success: boolean }>(`/agents/${encodeURIComponent(name)}/start`, {
    method: 'POST',
  });
}

export function stopAgent(name: string): Promise<{ success: boolean }> {
  return request<{ success: boolean }>(`/agents/${encodeURIComponent(name)}/stop`, {
    method: 'POST',
  });
}

export function restartAgent(name: string): Promise<{ success: boolean }> {
  return request<{ success: boolean }>(`/agents/${encodeURIComponent(name)}/restart`, {
    method: 'POST',
  });
}

export function getAgentLogs(name: string): Promise<string> {
  return requestText(`/agents/${encodeURIComponent(name)}/logs`);
}

export function getAgentMemory(name: string, scope?: string): Promise<AgentMemoryBlock[]> {
  const params = scope ? `?scope=${encodeURIComponent(scope)}` : '';
  return request<AgentMemoryBlock[]>(`/agents/${encodeURIComponent(name)}/memory${params}`);
}

export function getAgentSchedules(name: string): Promise<AgentSchedule[]> {
  return request<AgentSchedule[]>(`/agents/${encodeURIComponent(name)}/schedules`);
}

export function getAgentTasks(name: string, type?: string): Promise<AgentTask[]> {
  const params = type ? `?type=${encodeURIComponent(type)}` : '';
  return request<AgentTask[]>(`/agents/${encodeURIComponent(name)}/tasks${params}`);
}

export function createAgentTask(name: string, input: string): Promise<AgentTask> {
  return request<AgentTask>(`/agents/${encodeURIComponent(name)}/tasks`, {
    method: 'POST',
    body: JSON.stringify({ type: 'chat', input }),
  });
}

export function getAgentThoughts(name: string, taskId?: string): Promise<ThoughtEvent[]> {
  const params = taskId ? `?taskId=${encodeURIComponent(taskId)}` : '';
  return request<ThoughtEvent[]>(`/agents/${encodeURIComponent(name)}/thoughts${params}`);
}

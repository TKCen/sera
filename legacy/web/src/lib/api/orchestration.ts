import { request } from './client';
import type { AgentInstance, AgentTask } from './types';

export type BridgeAgent = AgentInstance;

export function getBridgeAgents(): Promise<BridgeAgent[]> {
  return request<BridgeAgent[]>('/agents').then((agents) =>
    agents.filter((a) => a.template_ref?.includes('bridge'))
  );
}

export function getAgentTasks(agentId: string, status?: string): Promise<AgentTask[]> {
  const params = status ? `?status=${encodeURIComponent(status)}` : '';
  return request<AgentTask[]>(`/agents/${encodeURIComponent(agentId)}/tasks${params}`);
}

export function createTask(
  agentId: string,
  task: string,
  priority?: number
): Promise<{ taskId: string }> {
  return request<{ taskId: string }>(`/agents/${encodeURIComponent(agentId)}/tasks`, {
    method: 'POST',
    body: JSON.stringify({ type: 'orchestration', input: task, priority }),
  });
}

export async function getQueueDepth(agentId: string): Promise<number> {
  const tasks = await request<AgentTask[]>(
    `/agents/${encodeURIComponent(agentId)}/tasks?status=queued`
  );
  return tasks.length;
}

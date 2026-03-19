import { request, requestText } from './client';
import type {
  AgentManifest,
  AgentInfo,
} from './types';

export function listAgents(): Promise<AgentManifest[]> {
  return request<AgentManifest[]>('/agents');
}

export function getAgent(name: string): Promise<AgentInfo> {
  return request<AgentInfo>(`/agents/${encodeURIComponent(name)}`);
}

export function getAgentManifestRaw(name: string): Promise<string> {
  return requestText(`/agents/${encodeURIComponent(name)}/manifest/raw`);
}

export function updateAgentManifest(
  name: string,
  manifest: AgentManifest,
): Promise<{ success: boolean }> {
  return request<{ success: boolean }>(
    `/agents/${encodeURIComponent(name)}/manifest`,
    {
      method: 'PUT',
      body: JSON.stringify(manifest),
    },
  );
}

export function reloadAgents(): Promise<{ success: boolean }> {
  return request<{ success: boolean }>('/agents/reload', { method: 'POST' });
}

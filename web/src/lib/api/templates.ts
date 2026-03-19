import { request } from './client';
import type { AgentTemplate } from './types';

export function listTemplates(): Promise<AgentTemplate[]> {
  return request<AgentTemplate[]>('/templates');
}

export function getTemplate(name: string): Promise<AgentTemplate> {
  return request<AgentTemplate>(`/templates/${encodeURIComponent(name)}`);
}

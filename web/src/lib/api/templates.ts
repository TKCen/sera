import { request } from './client';
import type { AgentTemplate } from './types';

export function listTemplates(): Promise<AgentTemplate[]> {
  return request<AgentTemplate[]>('/templates');
}

export function getTemplate(name: string): Promise<AgentTemplate> {
  return request<AgentTemplate>(`/templates/${encodeURIComponent(name)}`);
}

export function createTemplate(template: AgentTemplate): Promise<AgentTemplate> {
  return request<AgentTemplate>('/templates', {
    method: 'POST',
    body: JSON.stringify(template),
  });
}

export function updateTemplate(name: string, template: AgentTemplate): Promise<AgentTemplate> {
  return request<AgentTemplate>(`/templates/${encodeURIComponent(name)}`, {
    method: 'PUT',
    body: JSON.stringify(template),
  });
}

export function deleteTemplate(name: string): Promise<{ success: boolean }> {
  return request<{ success: boolean }>(`/templates/${encodeURIComponent(name)}`, {
    method: 'DELETE',
  });
}

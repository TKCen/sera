import { request } from './client';
import type { GuidanceSkillInfo, CreateSkillParams, ExternalSkillEntry } from './types';

export function listSkills(): Promise<GuidanceSkillInfo[]> {
  return request<GuidanceSkillInfo[]>('/skills');
}

export function createSkill(params: CreateSkillParams): Promise<{ message: string }> {
  return request<{ message: string }>('/skills', {
    method: 'POST',
    body: JSON.stringify(params),
  });
}

export function deleteSkill(name: string): Promise<{ message: string }> {
  return request<{ message: string }>(`/skills/${encodeURIComponent(name)}`, {
    method: 'DELETE',
  });
}

export function searchRegistry(query: string, source?: string): Promise<ExternalSkillEntry[]> {
  const params = new URLSearchParams();
  if (query) params.set('q', query);
  if (source) params.set('source', source);
  return request<ExternalSkillEntry[]>(`/skills/registry/search?${params.toString()}`);
}

export function importSkill(source: string, skillId: string): Promise<{ message: string }> {
  return request<{ message: string }>('/skills/import', {
    method: 'POST',
    body: JSON.stringify({ source, skillId }),
  });
}

import { request } from './client';
import type { SkillInfo } from './types';

export function listSkills(): Promise<SkillInfo[]> {
  return request<SkillInfo[]>('/skills');
}

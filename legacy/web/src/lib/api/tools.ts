import { request } from './client';
import type { ToolInfo } from './types';

export function listTools(): Promise<ToolInfo[]> {
  return request<ToolInfo[]>('/tools');
}

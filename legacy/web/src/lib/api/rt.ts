import { request } from './client';
import type { RtTokenResponse } from './types';

export function getRtToken(): Promise<RtTokenResponse> {
  return request<RtTokenResponse>('/rt/token');
}

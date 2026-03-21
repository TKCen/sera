import { request } from './client';
import type { CircleSummary, CircleManifest, CircleDetails } from './types';

export function listCircles(): Promise<CircleSummary[]> {
  return request<CircleSummary[]>('/circles');
}

export function createCircle(
  manifest: CircleManifest
): Promise<{ success: boolean; name: string }> {
  return request<{ success: boolean; name: string }>('/circles', {
    method: 'POST',
    body: JSON.stringify(manifest),
  });
}

export function getCircle(name: string): Promise<CircleDetails> {
  return request<CircleDetails>(`/circles/${encodeURIComponent(name)}`);
}

export function updateCircle(
  name: string,
  manifest: Partial<CircleManifest>
): Promise<{ success: boolean }> {
  return request<{ success: boolean }>(`/circles/${encodeURIComponent(name)}`, {
    method: 'PUT',
    body: JSON.stringify(manifest),
  });
}

export function deleteCircle(name: string): Promise<{ success: boolean }> {
  return request<{ success: boolean }>(`/circles/${encodeURIComponent(name)}`, {
    method: 'DELETE',
  });
}

export function updateCircleContext(name: string, content: string): Promise<{ success: boolean }> {
  return request<{ success: boolean }>(`/circles/${encodeURIComponent(name)}/context`, {
    method: 'PUT',
    body: JSON.stringify({ content }),
  });
}

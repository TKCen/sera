import { request } from './client';

export interface OperatorRequest {
  id: string;
  agentId: string;
  agentName: string | null;
  type: string;
  title: string;
  payload: Record<string, unknown>;
  status: 'pending' | 'approved' | 'rejected' | 'resolved';
  response: Record<string, unknown> | null;
  createdAt: string;
  resolvedAt: string | null;
}

export function listOperatorRequests(status?: string): Promise<OperatorRequest[]> {
  const params = status ? `?status=${status}` : '';
  return request<OperatorRequest[]>(`/operator-requests${params}`);
}

export function respondToRequest(
  id: string,
  action: 'approved' | 'rejected' | 'resolved',
  response?: string
): Promise<void> {
  return request<void>(`/operator-requests/${encodeURIComponent(id)}/respond`, {
    method: 'POST',
    body: JSON.stringify({ action, response }),
  });
}

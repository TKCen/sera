import { request } from './client';
import type { AuditResponse, AuditVerifyResult } from './types';

export interface AuditParams {
  actorId?: string;
  eventType?: string;
  resourceType?: string;
  from?: string;
  to?: string;
  search?: string;
  page?: number;
  pageSize?: number;
}

export async function getAuditEvents(params: AuditParams = {}): Promise<AuditResponse> {
  const q = new URLSearchParams();
  if (params.actorId) q.set('actorId', params.actorId);
  if (params.eventType) q.set('eventType', params.eventType);
  if (params.resourceType) q.set('resourceType', params.resourceType);
  if (params.from) q.set('from', params.from);
  if (params.to) q.set('to', params.to);
  if (params.search) q.set('search', params.search);

  // Backend expects limit/offset, not page/pageSize
  const pageSize = params.pageSize ?? 50;
  const page = params.page ?? 1;
  q.set('limit', String(pageSize));
  q.set('offset', String((page - 1) * pageSize));

  const qs = q.toString();
  // Backend returns { entries, total } but frontend expects { events, total, page, pageSize }
  const raw = await request<{ entries: unknown[]; total: number }>(`/audit${qs ? `?${qs}` : ''}`);
  return {
    events: raw.entries as AuditResponse['events'],
    total: raw.total,
    page,
    pageSize,
  };
}

export function verifyAuditChain(): Promise<AuditVerifyResult> {
  return request<AuditVerifyResult>('/audit/verify');
}

export function getAuditExportUrl(format: 'jsonl' | 'csv' = 'jsonl'): string {
  return `/audit/export?format=${format}`;
}

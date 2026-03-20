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

export function getAuditEvents(params: AuditParams = {}): Promise<AuditResponse> {
  const q = new URLSearchParams();
  if (params.actorId) q.set('actorId', params.actorId);
  if (params.eventType) q.set('eventType', params.eventType);
  if (params.resourceType) q.set('resourceType', params.resourceType);
  if (params.from) q.set('from', params.from);
  if (params.to) q.set('to', params.to);
  if (params.search) q.set('search', params.search);
  if (params.page !== undefined) q.set('page', String(params.page));
  if (params.pageSize !== undefined) q.set('pageSize', String(params.pageSize));
  const qs = q.toString();
  return request<AuditResponse>(`/audit${qs ? `?${qs}` : ''}`);
}

export function verifyAuditChain(): Promise<AuditVerifyResult> {
  return request<AuditVerifyResult>('/audit/verify');
}

export function getAuditExportUrl(format: 'jsonl' | 'csv' = 'jsonl'): string {
  return `/audit/export?format=${format}`;
}

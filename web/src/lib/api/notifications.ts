import { request } from './client';

export interface NotificationChannel {
  id: string;
  name: string;
  type: 'webhook' | 'email' | 'discord' | 'slack';
  config: Record<string, unknown>;
  enabled: boolean;
  createdAt: string;
}

export interface RoutingRule {
  id: string;
  eventType: string;
  channelIds: string[];
  filter: Record<string, unknown> | null;
  minSeverity: 'info' | 'warning' | 'critical';
  createdAt: string;
}

export interface CreateChannelPayload {
  name: string;
  type: string;
  config: Record<string, unknown>;
}

export interface CreateRoutingRulePayload {
  eventType: string;
  channelIds: string[];
  filter?: Record<string, unknown>;
  minSeverity?: string;
}

export function listChannels(): Promise<NotificationChannel[]> {
  return request<NotificationChannel[]>('/channels');
}

export function createChannel(data: CreateChannelPayload): Promise<NotificationChannel> {
  return request<NotificationChannel>('/channels', {
    method: 'POST',
    body: JSON.stringify(data),
  });
}

export function deleteChannel(id: string): Promise<{ ok: boolean }> {
  return request<{ ok: boolean }>(`/channels/${encodeURIComponent(id)}`, {
    method: 'DELETE',
  });
}

export function testChannel(id: string): Promise<{ ok: boolean; error?: string }> {
  return request<{ ok: boolean; error?: string }>(`/channels/${encodeURIComponent(id)}/test`, {
    method: 'POST',
  });
}

export function listRoutingRules(): Promise<RoutingRule[]> {
  return request<RoutingRule[]>('/notifications/routing');
}

export function createRoutingRule(data: CreateRoutingRulePayload): Promise<RoutingRule> {
  return request<RoutingRule>('/notifications/routing', {
    method: 'POST',
    body: JSON.stringify(data),
  });
}

export function deleteRoutingRule(id: string): Promise<{ ok: boolean }> {
  return request<{ ok: boolean }>(`/notifications/routing/${encodeURIComponent(id)}`, {
    method: 'DELETE',
  });
}

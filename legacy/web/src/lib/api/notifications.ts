import { request } from './client';

export interface NotificationChannel {
  id: string;
  name: string;
  description?: string;
  type: 'webhook' | 'email' | 'discord' | 'slack' | 'discord-chat' | 'telegram' | 'whatsapp';
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
  enabled: boolean;
  priority: number;
  targetAgentId: string | null;
  createdAt: string;
}

export interface CreateChannelPayload {
  name: string;
  description?: string;
  type: string;
  config: Record<string, unknown>;
  enabled?: boolean;
}

export interface CreateRoutingRulePayload {
  eventType: string;
  channelIds: string[];
  filter?: Record<string, unknown>;
  minSeverity?: string;
  enabled?: boolean;
  priority?: number;
  targetAgentId?: string | null;
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

export function updateChannel(
  id: string,
  data: Partial<CreateChannelPayload>
): Promise<NotificationChannel> {
  return request<NotificationChannel>(`/channels/${encodeURIComponent(id)}`, {
    method: 'PATCH',
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

export function updateRoutingRule(
  id: string,
  data: Partial<CreateRoutingRulePayload>
): Promise<RoutingRule> {
  return request<RoutingRule>(`/notifications/routing/${encodeURIComponent(id)}`, {
    method: 'PATCH',
    body: JSON.stringify(data),
  });
}

export function deleteRoutingRule(id: string): Promise<{ ok: boolean }> {
  return request<{ ok: boolean }>(`/notifications/routing/${encodeURIComponent(id)}`, {
    method: 'DELETE',
  });
}

export interface ChannelHealth {
  healthy: boolean;
  error?: string;
}

export function getChannelHealth(id: string): Promise<ChannelHealth> {
  return request<ChannelHealth>(`/channels/${encodeURIComponent(id)}/health`);
}

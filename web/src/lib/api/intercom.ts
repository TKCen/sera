import { request } from './client';
import type { MessageObject } from './types';

export function publishMessage(params: {
  agent: string;
  channel: string;
  type: string;
  payload: unknown;
}): Promise<{ success: boolean; message: MessageObject }> {
  return request('/intercom/publish', {
    method: 'POST',
    body: JSON.stringify(params),
  });
}

export function directMessage(params: {
  from: string;
  to: string;
  payload: unknown;
}): Promise<{ success: boolean; message: MessageObject }> {
  return request('/intercom/dm', {
    method: 'POST',
    body: JSON.stringify(params),
  });
}

export function getChannelHistory(
  channel: string,
  limit?: number
): Promise<{ channel: string; messages: MessageObject[] }> {
  const params = new URLSearchParams({ channel });
  if (limit !== undefined) params.set('limit', String(limit));
  return request(`/intercom/history?${params.toString()}`);
}

export function listChannels(agent: string): Promise<string[]> {
  const params = new URLSearchParams({ agent });
  return request(`/intercom/channels?${params.toString()}`);
}

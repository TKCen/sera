import { request } from './client';

export interface ChatResponse {
  conversationId: string;
  reply: string;
  thought?: string;
}

export function sendChat(message: string, conversationId?: string): Promise<ChatResponse> {
  return request<ChatResponse>('/chat', {
    method: 'POST',
    body: JSON.stringify({ message, conversationId }),
  });
}

export function executeTask(prompt: string): Promise<{ result: unknown }> {
  return request<{ result: unknown }>('/execute', {
    method: 'POST',
    body: JSON.stringify({ prompt }),
  });
}

import { request } from './client';

export interface ChatResponse {
  conversationId: string;
  reply: string;
  thought?: string;
}

export interface ChatStreamResponse {
  sessionId: string;
  messageId: string;
}

export function sendChat(message: string, conversationId?: string): Promise<ChatResponse> {
  return request<ChatResponse>('/chat', {
    method: 'POST',
    body: JSON.stringify({ message, conversationId }),
  });
}

/**
 * Send a message to an agent via the streaming chat endpoint.
 * Returns immediately with { sessionId, messageId }; the actual response
 * arrives via Centrifugo on the `tokens:{agentName}` channel.
 */
export function sendChatStream(
  agentName: string,
  message: string,
  sessionId?: string,
  agentInstanceId?: string
): Promise<ChatStreamResponse> {
  return request<ChatStreamResponse>('/chat', {
    method: 'POST',
    body: JSON.stringify({
      agentName,
      message,
      stream: true,
      ...(sessionId ? { sessionId } : {}),
      ...(agentInstanceId ? { agentInstanceId } : {}),
    }),
  });
}

export function executeTask(prompt: string): Promise<{ result: unknown }> {
  return request<{ result: unknown }>('/execute', {
    method: 'POST',
    body: JSON.stringify({ prompt }),
  });
}

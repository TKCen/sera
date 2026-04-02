import { useMutation } from '@tanstack/react-query';
import { sendChatStream } from '@/lib/api/chat';

export function useChatStream() {
  return useMutation({
    mutationFn: ({
      agentName,
      text,
      sessionId,
      agentInstanceId,
    }: {
      agentName: string;
      text: string;
      sessionId?: string;
      agentInstanceId?: string;
    }) => sendChatStream(agentName, text, sessionId, agentInstanceId),
  });
}

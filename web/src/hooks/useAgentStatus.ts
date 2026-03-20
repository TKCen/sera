import { useChannel } from './useChannel';

interface AgentStatusPayload {
  status: string;
  agentId: string;
  timestamp?: string;
}

export function useAgentStatus(agentId: string): string | null {
  const payload = useChannel<AgentStatusPayload>(agentId ? `agent:${agentId}:status` : '');
  return payload?.status ?? null;
}

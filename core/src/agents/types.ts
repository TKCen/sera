export type AgentRole = string;

export interface ChatMessage {
  role: 'user' | 'assistant' | 'system' | 'tool';
  content: string;
  name?: string;
}

export interface AgentTask {
  id: string;
  description: string;
  assignedTo?: string;
  status: 'pending' | 'in-progress' | 'completed' | 'failed';
  result?: string;
}

export interface AgentResponse {
  thought: string;
  action?: {
    tool: string;
    args: any;
  };
  delegation?: {
    agentRole: AgentRole;
    task: string;
  };
  finalAnswer?: string;
}

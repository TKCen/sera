export interface ChatMessage {
  role: 'user' | 'assistant' | 'system' | 'tool';
  content: string;
  name?: string;
}

export interface AgentResponse {
  thought: string;
  action?: {
    tool: string;
    args: any;
  };
  delegation?: {
    agentRole: string;
    task: string;
  };
  finalAnswer?: string;
}

import type { ToolCall } from '../lib/llm/types.js';

export interface ChatMessage {
  role: 'user' | 'assistant' | 'system' | 'tool';
  content: string;
  name?: string;
  /** Present on assistant messages that contain tool calls. */
  tool_calls?: ToolCall[];
  /** Present on tool result messages — references the tool call ID. */
  tool_call_id?: string;
}

/** A single captured thought event for session persistence. */
export interface CapturedThought {
  timestamp: string;
  stepType: string;
  content: string;
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
  /** Thoughts captured during processing — populated by processStream for persistence. */
  thoughts?: CapturedThought[];
}

export interface AgentInstance {
  id: string;
  templateName: string;
  name: string;
  workspacePath: string;
  containerId?: string;
  status: 'active' | 'inactive' | 'error';
  overrides?: any;
  createdAt: string;
  updatedAt: string;
  // DB columns added in Epics 02/03
  lifecycle_mode?: 'persistent' | 'ephemeral';
  parent_instance_id?: string;
  template_ref?: string;
  resolved_capabilities?: any;
  workspace_path?: string;
  container_id?: string;
}

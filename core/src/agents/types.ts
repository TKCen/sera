import type { ToolCall } from '../lib/llm/types.js';

export interface ChatMessage {
  role: 'user' | 'assistant' | 'system' | 'tool';
  content: string | null;
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
    args: Record<string, unknown>;
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
  name: string;
  display_name?: string;
  template_ref: string;
  circle?: string;
  status:
    | 'created'
    | 'running'
    | 'stopped'
    | 'error'
    | 'unresponsive'
    | 'throttled'
    | 'active'
    | 'inactive';
  overrides?: Record<string, unknown>;
  lifecycle_mode?: 'persistent' | 'ephemeral';
  parent_instance_id?: string;
  resolved_config?: Record<string, unknown>;
  resolved_capabilities?: Record<string, unknown>;
  workspace_path?: string;
  workspace_used_gb?: number;
  container_id?: string;
  circle_id?: string | null;
  last_heartbeat_at?: string | Date;
  updated_at: string | Date;
  created_at: string | Date;
}

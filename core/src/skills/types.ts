/**
 * Skills Framework — Core types for the composable skills system.
 *
 * Skills are modular, reusable capabilities that agents can invoke.
 * They unify built-in operations, MCP tools, and custom handlers behind
 * a single interface.
 */

// ── Skill Parameters ────────────────────────────────────────────────────────────

export interface SkillParameter {
  name: string;
  type: 'string' | 'number' | 'boolean' | 'object' | 'array';
  description: string;
  required: boolean;
}

// ── Skill Result ────────────────────────────────────────────────────────────────

export interface SkillResult {
  success: boolean;
  data?: unknown;
  error?: string;
}

// ── Skill Source ─────────────────────────────────────────────────────────────────

export type SkillSource = 'builtin' | 'mcp' | 'custom';

// ── Agent Context ───────────────────────────────────────────────────────────────

export interface AgentContext {
  agentName: string;
  workspacePath: string;
  tier: number;
  agentInstanceId: string | undefined;
  containerId: string | undefined;
  sandboxManager: import('../sandbox/SandboxManager.js').SandboxManager | undefined;
  manifest?: import('../agents/manifest/types.js').AgentManifest;
}

// ── Skill Handler ───────────────────────────────────────────────────────────────

/**
 * A skill handler receives the validated parameters and returns a result.
 * The optional `invoke` callback allows skill composition — a skill can
 * call other skills through the registry without holding a direct reference.
 */
export type SkillHandler = (
  params: Record<string, unknown>,
  context: AgentContext,
  invoke?: (skillId: string, params: Record<string, unknown>, context: AgentContext) => Promise<SkillResult>,
) => Promise<SkillResult>;

// ── Skill Definition ────────────────────────────────────────────────────────────

export interface SkillDefinition {
  id: string;
  description: string;
  parameters: SkillParameter[];
  handler: SkillHandler;
  source: SkillSource;
}

// ── Serializable Skill Info (for API responses) ─────────────────────────────────

export interface SkillInfo {
  id: string;
  description: string;
  parameters: SkillParameter[];
  source: SkillSource;
}

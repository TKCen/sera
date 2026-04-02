/**
 * Manifest Loader — reads and parses the agent's AGENT.yaml from the workspace.
 *
 * Defines a minimal manifest type that mirrors the Core AgentManifest
 * without importing from Core (keeping the runtime self-contained).
 */

import fs from 'fs';
import path from 'path';
import yaml from 'js-yaml';
import type { ToolDefinition } from './llmClient.js';
import { SystemPromptBuilder } from './systemPromptBuilder.js';

// ── Minimal Manifest Types (mirrors Core's AgentManifest) ───────────────────

export interface RuntimeManifest {
  apiVersion: string;
  kind: string;
  metadata: {
    name: string;
    displayName: string;
    icon: string;
    circle: string;
    tier: number;
    additionalCircles?: string[];
  };
  identity: {
    role: string;
    description: string;
    communicationStyle?: string;
    principles?: string[];
    notes?: string;
  };
  notes?: string;
  model: {
    provider: string;
    name: string;
    temperature?: number;
    thinkingLevel?: string;
    /** Model input capabilities, e.g. ['text', 'image'] */
    input?: string[];
  };
  tools?: {
    allowed?: string[];
    denied?: string[];
    /** Explicit core tools always sent to LLM. Remaining allowed tools are deferred. */
    coreTools?: string[];
  };
  skills?: string[];
  logging?: {
    commands?: boolean;
  };
  memory?: {
    citations?: 'full' | 'brief' | 'off';
  } & Record<string, unknown>;
  intercom?: {
    canMessage?: string[];
    channels?: {
      publish?: string[];
      subscribe?: string[];
    };
  };
  subagents?: {
    allowed?: Array<{
      role: string;
      templateRef?: string;
    }>;
  };
  contextFiles?: Array<{
    path: string;
    label: string;
    maxTokens?: number;
    priority?: 'high' | 'normal' | 'low';
  }>;
  bootContext?: {
    files?: Array<{
      path: string;
      label: string;
      maxTokens?: number;
    }>;
    directory?: string;
  };
  outputFormat?: string;
}

/**
 * Load and parse an AGENT.yaml manifest file.
 * Throws if the file doesn't exist or can't be parsed.
 */
export function loadManifest(manifestPath: string): RuntimeManifest {
  if (!fs.existsSync(manifestPath)) {
    throw new Error(`Manifest not found at ${manifestPath}`);
  }

  const rawText = fs.readFileSync(manifestPath, 'utf-8');
  const raw = yaml.load(rawText) as Record<string, unknown>;

  // Normalize spec-wrapped format to flat (mirrors Orchestrator normalization)
  const spec = raw['spec'] as Record<string, unknown> | undefined;
  if (spec) {
    if (spec['identity'] && !raw['identity']) raw['identity'] = spec['identity'];
    if (spec['model'] && !raw['model']) raw['model'] = spec['model'];
    if (spec['tools'] && !raw['tools']) raw['tools'] = spec['tools'];
    if (spec['skills'] && !raw['skills']) raw['skills'] = spec['skills'];
    if (spec['logging'] && !raw['logging']) raw['logging'] = spec['logging'];
    if (spec['memory'] && !raw['memory']) raw['memory'] = spec['memory'];
    if (spec['intercom'] && !raw['intercom']) raw['intercom'] = spec['intercom'];
    if (spec['subagents'] && !raw['subagents']) raw['subagents'] = spec['subagents'];
    if (spec['contextFiles'] && !raw['contextFiles']) raw['contextFiles'] = spec['contextFiles'];
    if (spec['bootContext'] && !raw['bootContext']) raw['bootContext'] = spec['bootContext'];
    if (spec['outputFormat'] && !raw['outputFormat']) raw['outputFormat'] = spec['outputFormat'];
    if (spec['notes'] && !raw['notes']) raw['notes'] = spec['notes'];
  }

  const parsed = raw as unknown as RuntimeManifest;

  if (!parsed?.metadata?.name) {
    throw new Error(`Invalid manifest: missing metadata.name in ${manifestPath}`);
  }

  if (!parsed?.identity?.role) {
    throw new Error(`Invalid manifest: missing identity.role in ${manifestPath}`);
  }

  return parsed;
}

/**
 * Context for generating the system prompt.
 */
export interface SystemPromptContext {
  tools?: ToolDefinition[];
  timezone?: string;
  circleName?: string;
  circleMembers?: string[];
  circleConstitution?: string;
  availableAgents?: Array<{ name: string; role: string }>;
  tokenBudget?: number;
}

/**
 * Generate a rich, composable system prompt from a manifest and runtime context.
 */
export function generateSystemPrompt(manifest: RuntimeManifest, context: SystemPromptContext = {}): string {
  const builder = new SystemPromptBuilder();

  // 1. Identity (Priority 0, Required)
  builder.addIdentity(manifest);

  // 2. Principles (Priority 10)
  builder.addPrinciples(manifest);

  // 3. Communication Style (Priority 20)
  builder.addCommunicationStyle(manifest);

  // 4. Available Tools (Priority 30, Required)
  builder.addAvailableTools(context.tools || []);

  // 5. Tool Usage Guidelines (Priority 40, Required)
  builder.addToolUsageGuidelines();

  // 6. Memory Instructions (Priority 50, Required)
  builder.addMemoryInstructions();

  // 7. Time & Context (Priority 60, Required)
  builder.addTimeContext(context.timezone);

  // 8. Circle Context (Priority 70)
  if (context.circleName) {
    builder.addCircleContext(
      context.circleName,
      context.circleMembers || [],
      context.circleConstitution
    );
  }

  // 9. Delegation Context (Priority 80)
  if (context.availableAgents) {
    builder.addDelegationContext(context.availableAgents);
  }

  // 10. Agent Notes (Priority 90)
  builder.addAgentNotes(manifest);

  // 11. Workspace Context (Priority 100)
  builder.addWorkspaceContext(manifest);

  // 12. Reasoning Hints (Priority 110)
  builder.addReasoningHints(manifest.model.name);

  // 13. Constraints (Priority 120, Required)
  builder.addConstraints(manifest.metadata.tier || 2);

  // 14. Output Format (Priority 130)
  builder.addOutputFormat(manifest.outputFormat);

  return builder.build(context.tokenBudget);
}

/** Rough heuristic: 1 token ~= 4 characters. Accurate enough for budget trimming. */
const CHARS_PER_TOKEN_ESTIMATE = 4;

/**
 * Build the workspace context section from contextFiles entries.
 * Respects per-file maxTokens, priority-based budget trimming, and blocks path traversal.
 */
export function buildContextSection(
  files: NonNullable<RuntimeManifest['contextFiles']>,
  workspacePath: string,
  budgetTokens: number
): string {
  type Entry = {
    path: string;
    label: string;
    maxTokens?: number;
    priority?: 'high' | 'normal' | 'low';
    content: string;
    tokens: number;
    exists: boolean;
  };

  const entries: Entry[] = files.map((f) => {
    // Block path traversal (e.g. ../../etc/passwd)
    const fullPath = path.join(workspacePath, f.path);
    const resolved = path.resolve(fullPath);
    const wsResolved = path.resolve(workspacePath);
    if (!resolved.startsWith(wsResolved + path.sep) && resolved !== wsResolved) {
      return { ...f, content: `*Path traversal blocked: ${f.path}*`, tokens: 10, exists: false };
    }
    let content: string;
    try {
      content = fs.readFileSync(fullPath, 'utf-8');
    } catch {
      return { ...f, content: `*File not found: ${f.path}*`, tokens: 10, exists: false };
    }
    if (f.maxTokens !== undefined) {
      const maxChars = f.maxTokens * CHARS_PER_TOKEN_ESTIMATE;
      if (content.length > maxChars) {
        content = content.substring(0, maxChars) + '\n...(truncated)';
      }
    }
    const tokens = Math.ceil(content.length / CHARS_PER_TOKEN_ESTIMATE);
    return { ...f, content, tokens, exists: true };
  });

  let totalTokens = entries.reduce((sum, e) => sum + e.tokens, 0);

  if (totalTokens > budgetTokens) {
    const lowEntries = entries.filter((e) => (e.priority ?? 'normal') === 'low' && e.exists);
    for (const entry of lowEntries) {
      if (totalTokens <= budgetTokens) break;
      totalTokens -= entry.tokens;
      entry.content = `*Omitted due to token budget: ${entry.path}*`;
      entry.tokens = 10;
      totalTokens += 10;
    }
  }

  if (totalTokens > budgetTokens) {
    const normalEntries = entries.filter(
      (e) => (e.priority ?? 'normal') === 'normal' && e.exists && e.tokens > 10
    );
    const excessTokens = totalTokens - budgetTokens;
    const normalTotalTokens = normalEntries.reduce((sum, e) => sum + e.tokens, 0);
    if (normalTotalTokens > 0) {
      for (const entry of normalEntries) {
        const reduction = Math.ceil((entry.tokens / normalTotalTokens) * excessTokens);
        const newTokens = Math.max(entry.tokens - reduction, 50);
        const newChars = newTokens * 4;
        if (entry.content.length > newChars) {
          entry.content = entry.content.substring(0, newChars) + '\n...(truncated)';
          totalTokens -= entry.tokens - newTokens;
          entry.tokens = newTokens;
        }
      }
    }
  }

  const lines = ['## Workspace Context'];
  for (const entry of entries) {
    lines.push('');
    lines.push(`### ${entry.label}`);
    lines.push(entry.content);
  }

  return lines.join('\n');
}

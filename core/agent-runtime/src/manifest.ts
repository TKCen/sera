/**
 * Manifest Loader — reads and parses the agent's AGENT.yaml from the workspace.
 *
 * Defines a minimal manifest type that mirrors the Core AgentManifest
 * without importing from Core (keeping the runtime self-contained).
 */

import fs from 'fs';
import yaml from 'js-yaml';

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
  };
  model: {
    provider: string;
    name: string;
    temperature?: number;
    thinkingLevel?: string;
  };
  tools?: {
    allowed?: string[];
    denied?: string[];
  };
  skills?: string[];
  memory?: Record<string, unknown>;
  intercom?: {
    canMessage?: string[];
    channels?: {
      publish?: string[];
      subscribe?: string[];
    };
  };
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
    if (spec['memory'] && !raw['memory']) raw['memory'] = spec['memory'];
    if (spec['intercom'] && !raw['intercom']) raw['intercom'] = spec['intercom'];
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
 * Generate a bootstrap system prompt from a manifest.
 *
 * This is a lightweight fallback prompt. In the primary chat path, ContextAssembler
 * on the Core side replaces this with the full IdentityService-generated prompt
 * (which includes stability guidelines, response format, subagents, circle context,
 * and memory). Tools are provided via the function-calling API — listing them in the
 * system prompt causes models to hallucinate XML-style tool calls.
 */
export function generateSystemPrompt(manifest: RuntimeManifest): string {
  const lines: string[] = [
    `You are ${manifest.metadata.displayName}, a SERA AI agent.`,
    `Role: ${manifest.identity.role}`,
    `Description: ${manifest.identity.description}`,
  ];

  if (manifest.identity.communicationStyle) {
    lines.push(`Communication Style: ${manifest.identity.communicationStyle}`);
  }

  if (manifest.identity.principles?.length) {
    lines.push('Principles:');
    for (const p of manifest.identity.principles) {
      lines.push(`  - ${p}`);
    }
  }

  lines.push('');
  lines.push('## Tool Usage Guidelines');
  lines.push('- When you need to accomplish a task, USE the available tools via function calls.');
  lines.push('- Report results clearly. If a tool errors, explain what happened.');
  lines.push('- Do not call the same tool with identical arguments repeatedly.');
  lines.push('- If you cannot accomplish a task with the tools available, say so.');
  lines.push('- Only use tools provided via function calling. Do NOT fabricate tool calls in XML, JSON, or any other text format.');

  return lines.join('\n');
}

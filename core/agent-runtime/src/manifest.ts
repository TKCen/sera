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
  };
  tools?: {
    allowed?: string[];
    denied?: string[];
  };
  skills?: string[];
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

  const raw = fs.readFileSync(manifestPath, 'utf-8');
  const parsed = yaml.load(raw) as RuntimeManifest;

  if (!parsed?.metadata?.name) {
    throw new Error(`Invalid manifest: missing metadata.name in ${manifestPath}`);
  }

  if (!parsed?.identity?.role) {
    throw new Error(`Invalid manifest: missing identity.role in ${manifestPath}`);
  }

  return parsed;
}

/**
 * Generate a system prompt from a manifest, similar to Core's IdentityService.
 * Optionally includes tool descriptions so the LLM knows what tools are available
 * even if it doesn't fully parse OpenAI function-calling schemas.
 */
export function generateSystemPrompt(
  manifest: RuntimeManifest,
  toolDefs?: Array<{ function: { name: string; description: string } }>
): string {
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

  if (toolDefs && toolDefs.length > 0) {
    lines.push('');
    lines.push('## Available Tools');
    lines.push('You MUST use these tools via function calls when they are relevant to the task:');
    for (const tool of toolDefs) {
      lines.push(`- **${tool.function.name}**: ${tool.function.description}`);
    }
  }

  return lines.join('\n');
}

import type { AgentManifest } from '../manifest/types.js';

/**
 * IdentityService — generates rich system prompts from agent manifests.
 *
 * Assembles the prompt from the agent's identity, tools, skills, and subagent
 * definitions so that each agent behaves consistently according to its AGENT.yaml.
 */
export class IdentityService {
  /**
   * Generate a complete system prompt from an agent manifest.
   * Optionally injects circle project context into the prompt.
   */
  static generateSystemPrompt(manifest: AgentManifest, circleContext?: string): string {
    const sections: string[] = [];

    // ── Role & Persona ────────────────────────────────────────────────────────
    sections.push(
      `You are ${manifest.metadata.displayName}, a ${manifest.identity.role}.`,
    );
    sections.push(manifest.identity.description.trim());

    // ── Communication Style ───────────────────────────────────────────────────
    if (manifest.identity.communicationStyle) {
      sections.push(
        `## Communication Style\n${manifest.identity.communicationStyle.trim()}`,
      );
    }

    // ── Principles ────────────────────────────────────────────────────────────
    if (manifest.identity.principles && manifest.identity.principles.length > 0) {
      const principlesList = manifest.identity.principles
        .map(p => `- ${p}`)
        .join('\n');
      sections.push(`## Guiding Principles\n${principlesList}`);
    }

    // ── Available Tools ───────────────────────────────────────────────────────
    if (manifest.tools?.allowed && manifest.tools.allowed.length > 0) {
      const toolsList = manifest.tools.allowed.map(t => `- ${t}`).join('\n');
      sections.push(`## Available Tools\n${toolsList}`);
    }

    if (manifest.tools?.denied && manifest.tools.denied.length > 0) {
      const deniedList = manifest.tools.denied.map(t => `- ${t}`).join('\n');
      sections.push(`## Denied Tools (never use these)\n${deniedList}`);
    }

    // ── Skills ────────────────────────────────────────────────────────────────
    if (manifest.skills && manifest.skills.length > 0) {
      const skillsList = manifest.skills.map(s => `- ${s}`).join('\n');
      sections.push(`## Available Skills\n${skillsList}`);
    }

    // ── Subagents ─────────────────────────────────────────────────────────────
    if (manifest.subagents?.allowed && manifest.subagents.allowed.length > 0) {
      const subList = manifest.subagents.allowed
        .map(s => {
          let entry = `- ${s.role} (max ${s.maxInstances ?? '∞'} instances)`;
          if (s.requiresApproval) entry += ' [requires human approval]';
          return entry;
        })
        .join('\n');
      sections.push(`## Subagents You Can Spawn\n${subList}`);
    }

    // ── Project Context (Circle Constitution) ─────────────────────────────────
    if (circleContext) {
      sections.push(`## Project Context\nThe following project context is shared by all agents in your circle:\n\n${circleContext.trim()}`);
    }

    // ── Response Format ───────────────────────────────────────────────────────
    sections.push(
      `## Response Format\nYou MUST respond in JSON format with this structure:\n` +
      `{\n` +
      `  "thought": "your inner monologue and reasoning",\n` +
      `  "delegation": { "agentRole": "role-name", "task": "description" },  // optional\n` +
      `  "finalAnswer": "your response to the user"  // optional\n` +
      `}`,
    );

    // ── Circle Context ────────────────────────────────────────────────────────
    sections.push(
      `## Context\nYou belong to the "${manifest.metadata.circle}" circle. ` +
      `Your security tier is ${manifest.metadata.tier}.`,
    );

    return sections.join('\n\n');
  }
}

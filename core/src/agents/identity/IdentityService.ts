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

  /**
   * Generate a system prompt for streaming mode.
   * Same as the standard prompt but instructs the LLM to respond in natural
   * language (not JSON), since tokens stream to the UI in real-time.
   */
  static generateStreamingSystemPrompt(manifest: AgentManifest, circleContext?: string): string {
    const base = IdentityService.generateSystemPrompt(manifest, circleContext);

    // Replace the JSON response format with a natural-language instruction
    const withFormat = base.replace(
      /## Response Format[\s\S]*?(?=## Context)/,
      `## Response Format\n` +
      `Respond directly and naturally in markdown. Do NOT wrap your response in JSON.\n` +
      `Focus on providing a helpful, well-formatted answer to the user's question.\n\n`,
    );

    // Append stability guidelines
    const stabilityGuidelines =
      `\n\n## Stability Guidelines\n` +
      `- Do NOT call the same tool with the same arguments repeatedly. If a call returned ` +
      `insufficient results, try different parameters or a different tool.\n` +
      `- If a tool fails, try an alternative approach instead of retrying the same call.\n` +
      `- Summarize your findings when you have enough information rather than making unlimited ` +
      `additional calls.\n` +
      `- Limit yourself to the minimum number of tool calls necessary to answer the question.\n`;

    return withFormat + stabilityGuidelines;
  }
}

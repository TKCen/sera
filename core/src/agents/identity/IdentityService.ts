import type { AgentManifest } from '../manifest/types.js';

/**
 * IdentityService — generates rich system prompts from agent manifests.
 *
 * Assembles the prompt from the agent's identity, tools, skills, and subagent
 * definitions so that each agent behaves consistently according to its AGENT.yaml.
 * Supports both the flat legacy format (top-level `identity`) and the new
 * spec-wrapped format (`spec.identity`).
 */
export class IdentityService {
  /**
   * Resolve the effective identity from a manifest, supporting both formats.
   */
  private static resolveIdentity(manifest: AgentManifest): {
    role: string;
    description?: string;
    communicationStyle?: string;
    principles?: string[];
  } {
    // Spec-wrapped format takes precedence if present; fall back to flat format
    if (manifest.spec?.identity) {
      return {
        role: manifest.spec.identity.role ?? '',
        ...(manifest.spec.identity.principles !== undefined
          ? { principles: manifest.spec.identity.principles }
          : {}),
      };
    }
    if (manifest.identity) {
      return manifest.identity;
    }
    return { role: '' };
  }

  /**
   * Generate a complete system prompt from an agent manifest.
   * Optionally injects circle project context and dynamic memory context into the prompt.
   */
  static generateSystemPrompt(
    manifest: AgentManifest,
    circleContext?: string,
    dynamicMemoryContext?: string
  ): string {
    const sections: string[] = [];
    const identity = IdentityService.resolveIdentity(manifest);

    // Resolve tools: flat format uses top-level `tools`, spec-wrapped uses `spec.tools`
    const tools = manifest.spec?.tools ?? manifest.tools;

    // Resolve skills: flat format uses top-level `skills`, spec-wrapped uses `spec.skills`
    const skills = manifest.spec?.skills ?? manifest.skills;

    // Resolve subagents: flat format uses top-level `subagents`, spec-wrapped uses `spec.subagents`
    const subagents = manifest.spec?.subagents ?? manifest.subagents;

    // Resolve sandbox tier label for display
    const tierLabel =
      manifest.metadata.tier != null
        ? String(manifest.metadata.tier)
        : (manifest.spec?.sandboxBoundary ?? 'unspecified');

    // ── Role & Persona ────────────────────────────────────────────────────────
    const displayName = manifest.metadata.displayName ?? manifest.metadata.name;
    sections.push(
      identity.role ? `You are ${displayName}, a ${identity.role}.` : `You are ${displayName}.`
    );

    // ── Current datetime ─────────────────────────────────────────────────────
    sections.push(`Current date and time: ${new Date().toISOString()}`);

    if (identity.description) {
      sections.push(identity.description.trim());
    }

    // ── Communication Style ───────────────────────────────────────────────────
    if (identity.communicationStyle) {
      sections.push(`## Communication Style\n${identity.communicationStyle.trim()}`);
    }

    // ── Principles ────────────────────────────────────────────────────────────
    if (identity.principles && identity.principles.length > 0) {
      const principlesList = identity.principles.map((p) => `- ${p}`).join('\n');
      sections.push(`## Guiding Principles\n${principlesList}`);
    }

    // ── Available Tools ───────────────────────────────────────────────────────
    // Note: In streaming mode, this section is stripped (tools provided via
    // function-calling API). The Capabilities section below survives.
    if (tools?.allowed && tools.allowed.length > 0) {
      const toolsList = tools.allowed.map((t) => `- ${t}`).join('\n');
      sections.push(`## Available Tools\n${toolsList}`);
    }

    if (tools?.denied && tools.denied.length > 0) {
      const deniedList = tools.denied.map((t) => `- ${t}`).join('\n');
      sections.push(`## Denied Tools (never use these)\n${deniedList}`);
    }

    // ── Capabilities (survives streaming-mode tool section stripping) ─────────
    if (tools?.allowed && tools.allowed.length > 0) {
      const capabilities = IdentityService.buildCapabilitySummary(tools.allowed);
      if (capabilities.length > 0) {
        sections.push(
          `## Capabilities\n` +
            `You have these capabilities available as function-calling tools:\n` +
            capabilities.join('\n')
        );
      }
    }

    // ── Skills ────────────────────────────────────────────────────────────────
    if (skills && skills.length > 0) {
      const skillsList = skills.map((s) => `- ${s}`).join('\n');
      sections.push(`## Available Skills\n${skillsList}`);
    }

    // ── Subagents ─────────────────────────────────────────────────────────────
    if (subagents?.allowed && subagents.allowed.length > 0) {
      const subList = subagents.allowed
        .map((s) => {
          // Handle both flat format (role) and spec-wrapped format (templateRef)
          const name = 'role' in s ? s.role : 'templateRef' in s ? s.templateRef : 'unknown';
          let entry = `- ${name} (max ${s.maxInstances ?? '∞'} instances)`;
          if (s.requiresApproval) entry += ' [requires human approval]';
          return entry;
        })
        .join('\n');
      sections.push(`## Subagents You Can Spawn\n${subList}`);
    }

    // ── Project Context (Circle Constitution) ─────────────────────────────────
    if (circleContext) {
      sections.push(
        `## Project Context\nThe following project context is shared by all agents in your circle:\n\n${circleContext.trim()}`
      );
    }

    // ── Response Format ───────────────────────────────────────────────────────
    sections.push(
      `## Response Format\nYou MUST respond in JSON format with this structure:\n` +
        `{\n` +
        `  "thought": "your inner monologue and reasoning",\n` +
        `  "delegation": { "agentRole": "role-name", "task": "description" },  // optional\n` +
        `  "finalAnswer": "your response to the user"  // optional\n` +
        `}`
    );

    // ── Memory Context ────────────────────────────────────────────────────────
    if (dynamicMemoryContext) {
      sections.push(dynamicMemoryContext);
    }

    // ── Circle Context ────────────────────────────────────────────────────────
    if (manifest.metadata.circle) {
      sections.push(
        `## Context\nYou belong to the "${manifest.metadata.circle}" circle. ` +
          `Your security tier is ${tierLabel}.`
      );
    }

    return sections.join('\n\n');
  }

  /**
   * Generate a system prompt for streaming mode.
   * Same as the standard prompt but instructs the LLM to respond in natural
   * language (not JSON), since tokens stream to the UI in real-time.
   */
  static generateStreamingSystemPrompt(
    manifest: AgentManifest,
    circleContext?: string,
    dynamicMemoryContext?: string
  ): string {
    const base = IdentityService.generateSystemPrompt(
      manifest,
      circleContext,
      dynamicMemoryContext
    );

    // Replace the JSON response format with a natural-language instruction.
    // The lookahead matches the next section header (\n## ) OR end of string ($) so
    // that agents without a circle (no ## Context section) are handled correctly.
    let withFormat = base.replace(
      /## Response Format[\s\S]*?(?=\n## |$)/,
      `## Response Format\n` +
        `Respond directly and naturally in markdown. Do NOT wrap your response in JSON.\n` +
        `Focus on providing a helpful, well-formatted answer to the user's question.\n\n`
    );

    // Remove the "Available Tools" section from the system prompt — tools are provided
    // via the function-calling API. Listing them again in the prompt causes the LLM to
    // hallucinate XML-style tool calls for names that don't match the actual definitions.
    withFormat = withFormat.replace(/## Available Tools[\s\S]*?(?=\n## |$)/, '');

    // Append stability guidelines
    const stabilityGuidelines =
      `\n\n## Stability Guidelines\n` +
      `- Do NOT call the same tool with the same arguments repeatedly. If a call returned ` +
      `insufficient results, try different parameters or a different tool.\n` +
      `- If a tool fails, try an alternative approach instead of retrying the same call.\n` +
      `- Summarize your findings when you have enough information rather than making unlimited ` +
      `additional calls.\n` +
      `- Limit yourself to the minimum number of tool calls necessary to answer the question.\n` +
      `- Only use tools that are provided to you via function calling. Do NOT invent or ` +
      `fabricate tool calls in XML, JSON, or any other text format.\n`;

    return withFormat + stabilityGuidelines;
  }

  // ── Capability mapping ─────────────────────────────────────────────────────

  /** Maps tool IDs to human-readable capability descriptions for the system prompt. */
  private static readonly CAPABILITY_MAP: Record<
    string,
    { category: string; description: string }
  > = {
    'delegate-task': {
      category: 'Delegation',
      description: 'assign work to other agents or spawn ephemeral helpers',
    },
    'schedule-task': {
      category: 'Scheduling',
      description: 'create reminders, recurring jobs, and scheduled tasks',
    },
    'knowledge-store': {
      category: 'Knowledge',
      description: 'save important facts, decisions, and context to persistent memory',
    },
    'knowledge-query': {
      category: 'Knowledge',
      description: 'retrieve relevant context from persistent memory',
    },
    'web-search': { category: 'Web', description: 'search the web for current information' },
    'web-fetch': { category: 'Web', description: 'fetch and read web page content' },
    'file-read': { category: 'Files', description: 'read file contents from the workspace' },
    'file-write': { category: 'Files', description: 'create or update files in the workspace' },
    'file-list': { category: 'Files', description: 'list directory contents' },
    'file-delete': { category: 'Files', description: 'delete files or directories' },
    'shell-exec': { category: 'Shell', description: 'execute shell commands in the workspace' },
    'spawn-subagent': {
      category: 'Delegation',
      description: 'spawn a child agent in a separate container',
    },
    'run-tool': {
      category: 'Tools',
      description: 'run an ephemeral tool in an isolated container',
    },
  };

  /**
   * Build capability summary lines from a list of allowed tool IDs.
   * Groups by category and deduplicates.
   */
  private static buildCapabilitySummary(allowedTools: string[]): string[] {
    const seen = new Set<string>();
    const lines: string[] = [];

    for (const toolId of allowedTools) {
      const mapping = IdentityService.CAPABILITY_MAP[toolId];
      if (!mapping) continue;

      const key = `${mapping.category}:${toolId}`;
      if (seen.has(key)) continue;
      seen.add(key);

      lines.push(`- **${mapping.category}**: Use \`${toolId}\` to ${mapping.description}`);
    }

    return lines;
  }
}

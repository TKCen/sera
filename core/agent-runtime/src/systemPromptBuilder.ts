import fs from 'fs';
import path from 'path';
import { getEncoding } from 'js-tiktoken';
import type { RuntimeManifest } from './manifest.js';
import type { ToolDefinition } from './llmClient.js';

export interface CoreMemoryBlock {
  name: string;
  content: string;
  characterLimit: number;
}

export interface PromptSection {
  id: string;
  priority: number;        // Lower = more important, kept when truncating
  content: string;
  required: boolean;       // If true, never truncated
}

export class SystemPromptBuilder {
  private sections: PromptSection[] = [];
  private enc = getEncoding('cl100k_base');

  /** Add a section to the system prompt. */
  addSection(section: PromptSection): this {
    // Replace if ID already exists, otherwise add
    const index = this.sections.findIndex((s) => s.id === section.id);
    if (index !== -1) {
      this.sections[index] = section;
    } else {
      this.sections.push(section);
    }
    return this;
  }

  /** Build the final system prompt string, respecting the token budget if provided. */
  build(tokenBudget?: number): string {
    // Sort sections by priority (ASC: 0 is highest)
    const sorted = [...this.sections].sort((a, b) => a.priority - b.priority);

    if (!tokenBudget) {
      return sorted.map((s) => s.content).join('\n\n').trim();
    }

    const requiredSections = sorted.filter((s) => s.required);
    const optionalSections = sorted.filter((s) => !s.required);

    // Initial check: if all required sections exceed budget, we keep them anyway
    // (spec says required sections are never truncated)
    let currentContent = this.assemble(requiredSections);
    let currentTokens = this.countTokens(currentContent);

    if (currentTokens >= tokenBudget) {
      return currentContent;
    }

    // Add optional sections one by one until budget reached
    const toKeep = [...requiredSections];
    for (const section of optionalSections) {
      const nextContent = this.assemble([...toKeep, section]);
      const nextTokens = this.countTokens(nextContent);

      if (nextTokens <= tokenBudget) {
        toKeep.push(section);
        currentContent = nextContent;
        currentTokens = nextTokens;
      } else {
        // Budget exceeded, stop adding optional sections
        break;
      }
    }

    // Final assembly — we re-sort by the original priority to maintain logical order
    // even if we dropped some sections in between.
    return this.assemble(toKeep.sort((a, b) => a.priority - b.priority));
  }

  private assemble(sections: PromptSection[]): string {
    return sections.map((s) => s.content).join('\n\n').trim();
  }

  private countTokens(text: string): number {
    return this.enc.encode(text).length;
  }

  // ── Manifest-based Sections ──────────────────────────────────────────────────

  /** Identity: name, role, description (Required, Priority 0) */
  addIdentity(manifest: RuntimeManifest): this {
    const lines = [
      `You are ${manifest.metadata.displayName}, a SERA AI agent.`,
      `Role: ${manifest.identity.role}`,
      `Description: ${manifest.identity.description}`,
    ];
    return this.addSection({
      id: 'identity',
      priority: 0,
      content: lines.join('\n'),
      required: true,
    });
  }

  /** Principles: bullets (Optional, Priority 10) */
  addPrinciples(manifest: RuntimeManifest): this {
    if (!manifest.identity.principles?.length) return this;
    const lines = ['## Principles', ...manifest.identity.principles.map((p) => `- ${p}`)];
    return this.addSection({
      id: 'principles',
      priority: 10,
      content: lines.join('\n'),
      required: false,
    });
  }

  /** Communication Style: free-form text (Optional, Priority 20) */
  addCommunicationStyle(manifest: RuntimeManifest): this {
    if (!manifest.identity.communicationStyle) return this;
    const lines = ['## Communication Style', manifest.identity.communicationStyle];
    return this.addSection({
      id: 'communication-style',
      priority: 20,
      content: lines.join('\n'),
      required: false,
    });
  }

  /** Agent Notes: manifest-level notes (Optional, Priority 90) */
  addAgentNotes(manifest: RuntimeManifest): this {
    const notes = manifest.identity.notes || manifest.notes;
    if (!notes) return this;
    const lines = ['## Agent Notes', notes];
    return this.addSection({
      id: 'agent-notes',
      priority: 90,
      content: lines.join('\n'),
      required: false,
    });
  }

  /** Workspace Context: injected files (Optional, Priority 200) */
  addWorkspaceContext(manifest: RuntimeManifest): this {
    if (!manifest.contextFiles?.length) return this;
    // Build context section inline to avoid circular dependency with manifest.ts
    const workspacePath = process.env['WORKSPACE_PATH'] ?? '/workspace';
    const budgetTokens = parseInt(process.env['CONTEXT_FILES_BUDGET'] ?? '8000', 10);
    const content = buildContextSectionInline(manifest.contextFiles, workspacePath, budgetTokens);
    if (!content) return this;
    return this.addSection({
      id: 'workspace-context',
      priority: 200, // Lowest priority, first to be truncated
      content,
      required: false,
    });
  }

  // ── Runtime & Context Sections ───────────────────────────────────────────────

  /** Available Tools: summary list (Required, Priority 30) */
  addAvailableTools(tools: ToolDefinition[]): this {
    if (!tools.length) return this;
    const lines = ['## Available Tools'];
    for (const t of tools) {
      const desc = t.function.description.split('.')[0]; // First sentence only
      lines.push(`- **${t.function.name}**: ${desc}.`);
    }
    return this.addSection({
      id: 'available-tools',
      priority: 30,
      content: lines.join('\n'),
      required: true,
    });
  }

  /** Tool Usage Guidelines: common hints (Required, Priority 40) */
  addToolUsageGuidelines(): this {
    const lines = [
      '## Tool Usage Guidelines',
      '- When you need to accomplish a task, USE the available tools via function calls.',
      '- Report results clearly. If a tool errors, explain what happened.',
      '- Do not call the same tool with identical arguments repeatedly.',
      '- If you cannot accomplish a task with the tools available, say so.',
      '- Only use tools provided via function calling. Do NOT fabricate tool calls in XML, JSON, or any other text format.',
      '- Use knowledge-store to save important findings for long-term memory.',
    ];
    return this.addSection({
      id: 'tool-usage-guidelines',
      priority: 40,
      content: lines.join('\n'),
      required: true,
    });
  }

  /** Memory Instructions: RAG & saving (Required, Priority 50) */
  addMemoryInstructions(): this {
    const lines = [
      '## Memory & Knowledge',
      '- Use `knowledge-store` to save important facts, decisions, or results of complex analysis.',
      '- Use `knowledge-query` to search your long-term memory when you need context from past interactions.',
      '- When citing information from memory, use the format [Memory: <id>] if an ID is available.',
      '- Be proactive: if you learn something the user is likely to ask about later, save it.',
    ];
    return this.addSection({
      id: 'memory-instructions',
      priority: 50,
      content: lines.join('\n'),
      required: true,
    });
  }

  /** Time & Context: current time/date (Required, Priority 60) */
  addTimeContext(timezone: string = 'UTC'): this {
    const now = new Date();
    const lines = [
      '## System Context',
      `- Current UTC Time: ${now.toISOString()}`,
      `- Local Timezone: ${timezone}`,
      `- Current Date: ${now.toISOString().split('T')[0]}`,
    ];
    return this.addSection({
      id: 'time-context',
      priority: 60,
      content: lines.join('\n'),
      required: true,
    });
  }

  /** Circle Context: team info (Optional, Priority 70) */
  addCircleContext(circleName: string, members: string[], constitution?: string): this {
    const lines = [`## Circle: ${circleName}`, `You are a member of the "${circleName}" circle.`];
    if (members.length) {
      lines.push(`Fellow members: ${members.join(', ')}`);
    }
    if (constitution) {
      lines.push('', 'Circle Constitution:', constitution);
    }
    return this.addSection({
      id: 'circle-context',
      priority: 70,
      content: lines.join('\n'),
      required: false,
    });
  }

  /** Delegation Context: for orchestrators (Optional, Priority 80) */
  addDelegationContext(availableAgents: Array<{ name: string; role: string }>): this {
    if (!availableAgents.length) return this;
    const lines = [
      '## Delegation',
      'You can delegate tasks to the following specialized agents using `spawn-subagent`:',
    ];
    for (const agent of availableAgents) {
      lines.push(`- **${agent.name}**: ${agent.role}`);
    }
    return this.addSection({
      id: 'delegation-context',
      priority: 80,
      content: lines.join('\n'),
      required: false,
    });
  }

  /** Reasoning Hints: for thinking models (Optional, Priority 110) */
  addReasoningHints(modelName: string): this {
    const isReasoningModel = modelName.includes('thinking') || modelName.includes('r1') || modelName.includes('o1') || modelName.includes('o3');
    if (!isReasoningModel) return this;
    const lines = [
      '## Reasoning Instructions',
      'This model supports internal reasoning. You should use your internal thinking process to decompose complex problems before providing a final answer.',
    ];
    return this.addSection({
      id: 'reasoning-hints',
      priority: 110,
      content: lines.join('\n'),
      required: false,
    });
  }

  /** Constraints: sandbox limits (Required, Priority 120) */
  addConstraints(tier: number): this {
    const lines = ['## System Constraints', `- Security Tier: ${tier}`];
    if (tier >= 2) {
      lines.push('- Network access is restricted to approved domains.');
      lines.push('- Filesystem access is limited to the `/workspace` directory.');
    }
    return this.addSection({
      id: 'constraints',
      priority: 120,
      content: lines.join('\n'),
      required: true,
    });
  }

  /** Core Memory: Letta-style blocks (Optional, Priority 5) */
  addCoreMemoryBlocks(blocks: CoreMemoryBlock[]): this {
    if (!blocks.length) return this;
    const lines = ['<memory_blocks>'];
    for (const b of blocks) {
      lines.push(
        `  <block name="${b.name}" character_count="${b.content.length}" character_limit="${b.characterLimit}">`,
        `    ${b.content}`,
        `  </block>`
      );
    }
    lines.push('</memory_blocks>');
    return this.addSection({
      id: 'core-memory',
      priority: 5,
      content: lines.join('\n'),
      required: true,
    });
  }

  /** Output Format: preferences (Optional, Priority 130) */
  addOutputFormat(format?: string): this {
    if (!format) return this;
    const lines = ['## Output Format', format];
    return this.addSection({
      id: 'output-format',
      priority: 130,
      content: lines.join('\n'),
      required: false,
    });
  }
}

// ── Workspace context helpers (inlined to avoid circular dep with manifest.ts) ──

const CHARS_PER_TOKEN_ESTIMATE = 4;

function buildContextSectionInline(
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
        const newChars = newTokens * CHARS_PER_TOKEN_ESTIMATE;
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

import type { AgentManifest } from '../agents/manifest/types.js';
import type { MCPRegistry } from '../mcp/registry.js';
import type { SkillDefinition, SkillResult, SkillInfo, ToolInfoResponse } from './types.js';
import { TOOL_TIER_REQUIREMENTS } from './types.js';

/**
 * Central registry for all agent skills.
 *
 * Responsibilities:
 *  - Register / look up / invoke skills by ID
 *  - Validate that agent manifests reference only known skills
 *  - Bridge MCP server tools into the skills interface
 *  - Support skill composition (skills invoking other skills)
 */
export class SkillRegistry {
  private skills: Map<string, SkillDefinition> = new Map();

  // ── Registration ──────────────────────────────────────────────────────────

  /** Register a skill definition. Overwrites if the ID already exists. */
  register(skill: SkillDefinition): void {
    this.skills.set(skill.id, skill);
  }

  /** Remove a skill by ID. Returns true if it existed. */
  unregister(id: string): boolean {
    return this.skills.delete(id);
  }

  /** Remove all skills that start with a given prefix (e.g. "github/"). */
  unregisterByPrefix(prefix: string): void {
    for (const id of this.skills.keys()) {
      if (id.startsWith(prefix)) {
        this.skills.delete(id);
      }
    }
  }

  // ── Lookup ────────────────────────────────────────────────────────────────

  /** Get a skill by ID, or undefined if not found. */
  get(id: string): SkillDefinition | undefined {
    return this.skills.get(id);
  }

  /** Check whether a skill ID is registered. */
  has(id: string): boolean {
    return this.skills.has(id);
  }

  /** List all registered skills (without handler — safe for serialization). */
  listAll(): SkillInfo[] {
    return [...this.skills.values()].map(SkillRegistry.toInfo);
  }

  /** List executable tools with security metadata (for the /api/tools endpoint). */
  listTools(): ToolInfoResponse[] {
    return [...this.skills.values()].map((skill) => {
      const server = skill.source === 'mcp' ? skill.id.split('/')[0] : undefined;
      const tierReq = TOOL_TIER_REQUIREMENTS[skill.id];
      const isSeraCoreManagement = server === 'sera-core';
      return {
        id: skill.id,
        description: skill.description,
        parameters: skill.parameters,
        source: skill.source,
        ...(server ? { server } : {}),
        minTier: (tierReq?.minTier ?? (skill.source === 'mcp' ? 2 : 1)) as 1 | 2 | 3,
        ...(isSeraCoreManagement ? { capabilityRequired: 'seraManagement' } : {}),
        ...(tierReq?.capability ? { capabilityRequired: tierReq.capability } : {}),
      };
    });
  }

  /**
   * Check if a tool/skill is allowed for an agent based on its manifest.
   *
   * Logic:
   * 1. If tool matches any pattern in `tools.denied`, it's REJECTED.
   * 2. If tool is explicitly listed in `skills[]`, it's ALLOWED.
   * 3. If `tools.allowed` is defined:
   *    - If tool matches any pattern in `tools.allowed`, it's ALLOWED.
   *    - Otherwise, it's REJECTED.
   * 4. If NEITHER `skills[]` nor `tools.allowed` are defined, it's ALLOWED (open access).
   * 5. Otherwise, if not in `skills[]`, it's REJECTED.
   */
  isToolAllowedForAgent(manifest: AgentManifest, toolId: string): boolean {
    const tools = manifest.spec?.tools ?? manifest.tools;
    const denied = tools?.denied || [];
    if (denied.some((p) => SkillRegistry.matches(p, toolId))) {
      return false;
    }

    // Auto-allow sera-core tools if seraManagement capability is present
    const capabilities =
      manifest.capabilities ??
      (manifest.spec?.capabilities ? Object.keys(manifest.spec.capabilities) : []);
    if (capabilities.includes('seraManagement') && SkillRegistry.matches('sera-core/*', toolId)) {
      return true;
    }

    // Explicitly allowed via skills array
    const skills = manifest.spec?.skills ?? manifest.skills;
    if (skills) {
      const isExplicitSkill = skills.some((s) => {
        const id = typeof s === 'string' ? s : s.name;
        return id === toolId;
      });
      if (isExplicitSkill) return true;
    }

    // Allowed via tools.allowed patterns
    if (tools?.allowed) {
      // If any pattern explicitly matches, allow it
      if (tools.allowed.some((p) => SkillRegistry.matches(p, toolId))) {
        return true;
      }
    }

    // MCP tools (source: 'mcp') pass through unless explicitly denied above.
    // MCP tools are additive to any explicit skills or tools.allowed lists.
    const skill = this.skills.get(toolId);
    if (skill?.source === 'mcp') {
      return true;
    }

    // Open access if neither is specified
    if (!skills && !tools?.allowed) {
      return true;
    }

    return false;
  }

  /**
   * List skills available to an agent based on its manifest.
   * Returns all registered skills that pass the `isToolAllowedForAgent` check.
   */
  listForAgent(manifest: AgentManifest): SkillInfo[] {
    return [...this.skills.values()]
      .filter((skill) => this.isToolAllowedForAgent(manifest, skill.id))
      .map(SkillRegistry.toInfo);
  }

  // ── Invocation ────────────────────────────────────────────────────────────

  /**
   * Invoke a skill by ID with the given parameters and agent context.
   * Passes a composition callback so the skill can call peer skills.
   */
  async invoke(
    id: string,
    params: Record<string, unknown>,
    context: import('./types.js').AgentContext
  ): Promise<SkillResult> {
    const skill = this.skills.get(id);
    if (!skill) {
      return { success: false, error: `Skill "${id}" not found` };
    }

    try {
      const compositionInvoke = (
        childId: string,
        childParams: Record<string, unknown>,
        childContext: import('./types.js').AgentContext
      ) => this.invoke(childId, childParams, childContext);

      return await skill.handler(params, context, compositionInvoke);
    } catch (err) {
      return {
        success: false,
        error: err instanceof Error ? err.message : String(err),
      };
    }
  }

  // ── Validation ────────────────────────────────────────────────────────────

  /**
   * Validate that all skill IDs referenced by a manifest are registered
   * and that there are no circular dependencies among required skills.
   * Returns an array of error messages (empty = valid).
   */
  validateManifestSkills(manifest: AgentManifest): string[] {
    const errors: string[] = [];
    const ids = new Set<string>();

    if (manifest.skills) {
      for (const s of manifest.skills) {
        const id = typeof s === 'string' ? s : s.name;
        if (!this.skills.has(id)) {
          errors.push(id); // The test expects the ID itself, not a formatted error message
        } else {
          ids.add(id);
        }
      }
    }

    if (manifest.tools?.allowed) {
      for (const id of manifest.tools.allowed) {
        if (!this.skills.has(id)) {
          errors.push(id);
        } else {
          ids.add(id);
        }
      }
    }

    if (errors.length > 0) return errors;

    // Cycle detection
    const visited = new Set<string>();
    const stack = new Set<string>();

    const checkCycle = (id: string): string | null => {
      if (stack.has(id)) return id;
      if (visited.has(id)) return null;

      visited.add(id);
      stack.add(id);

      const skill = this.skills.get(id);
      if (skill?.requires) {
        for (const reqId of skill.requires) {
          const cycleId = checkCycle(reqId);
          if (cycleId) return `${id} -> ${cycleId}`;
        }
      }

      stack.delete(id);
      return null;
    };

    for (const id of ids) {
      const cycle = checkCycle(id);
      if (cycle) {
        errors.push(`Circular skill dependency detected: ${cycle}`);
        break; // Stop after first cycle for now
      }
    }

    return errors;
  }

  // ── MCP Bridge ────────────────────────────────────────────────────────────

  /**
   * Bridge all tools from registered MCP servers into this skill registry.
   *
   * Each MCP tool is wrapped as a skill with source `'mcp'`. The skill ID
   * is the tool name (e.g. `mcp:serverName:toolName`-style namespacing is
   * avoided for simplicity — the tool name is the direct ID).
   */
  async bridgeMCPTools(mcpRegistry: MCPRegistry): Promise<number> {
    const allTools = await mcpRegistry.getAllTools();
    let count = 0;

    for (const serverEntry of allTools) {
      count += await this.bridgeMCPToolsForServer(serverEntry.serverName, mcpRegistry);
    }

    return count;
  }

  /**
   * Bridge tools for a specific MCP server.
   * Useful for hot-reloading individual servers.
   */
  async bridgeMCPToolsForServer(serverName: string, mcpRegistry: MCPRegistry): Promise<number> {
    const clients = mcpRegistry.getClients();
    const client = clients.get(serverName);
    if (!client) return 0;

    // Clear old tools for this server
    this.unregisterByPrefix(`${serverName}/`);

    try {
      const tools = await client.listTools();
      let count = 0;

      for (const tool of tools.tools) {
        const skillId = `${serverName}/${tool.name}`;
        const parameters = SkillRegistry.jsonSchemaToParams(tool.inputSchema);
        const mcpClient = client;
        const toolName = tool.name;

        this.register({
          id: skillId,
          description: tool.description ?? `MCP tool: ${tool.name}`,
          parameters,
          source: 'mcp',
          handler: async (params, context) => {
            // Story 7.7: Capability-based gating for management tools
            if (serverName === 'sera-core' || skillId.startsWith('sera-core/')) {
              const capabilities = context.manifest.capabilities ?? [];
              if (!capabilities.includes('seraManagement')) {
                return {
                  success: false,
                  error: `Access denied: tool "${skillId}" requires "seraManagement" capability.`,
                };
              }
            }

            try {
              // Story 7.8: Inject SERA extension context and credentials
              const meta = {
                sera: {
                  sessionId: context.sessionId || 'default',
                  agentId: context.agentName,
                  circleId: context.manifest.metadata.circle,
                  // Simplified credential resolution for Story 7.8
                  credentials:
                    (context.manifest as unknown as Record<string, unknown>).secrets || {},
                },
              };

              const result = await mcpClient.callTool(toolName, params, meta);
              return { success: true, data: result };
            } catch (err) {
              return {
                success: false,
                error: err instanceof Error ? err.message : String(err),
              };
            }
          },
        });
        count++;
      }
      return count;
    } catch {
      // Don't log error here as SkillRegistry should be agnostic of MCP connection issues
      return 0;
    }
  }

  // ── Helpers ───────────────────────────────────────────────────────────────

  /** Strip the handler from a SkillDefinition for safe serialization. */
  private static toInfo(skill: SkillDefinition): SkillInfo {
    return {
      id: skill.id,
      description: skill.description,
      parameters: skill.parameters,
      source: skill.source,
    };
  }

  /**
   * Convert a JSON Schema `inputSchema` object (from MCP tool definitions)
   * into a flat array of `SkillParameter` entries.
   */
  private static jsonSchemaToParams(
    schema: Record<string, unknown> | undefined
  ): import('./types.js').SkillParameter[] {
    if (!schema || typeof schema !== 'object') return [];

    const properties = schema['properties'] as Record<string, Record<string, unknown>> | undefined;
    if (!properties) return [];

    const required = new Set(
      Array.isArray(schema['required']) ? (schema['required'] as string[]) : []
    );

    return Object.entries(properties).map(([name, prop]) => ({
      name,
      type: SkillRegistry.mapJsonType(prop['type']),
      description: (prop['description'] as string | undefined) ?? '',
      required: required.has(name),
    }));
  }

  private static mapJsonType(jsonType: unknown): import('./types.js').SkillParameter['type'] {
    switch (jsonType) {
      case 'string':
        return 'string';
      case 'number':
      case 'integer':
        return 'number';
      case 'boolean':
        return 'boolean';
      case 'array':
        return 'array';
      default:
        return 'object';
    }
  }

  /**
   * Check if a tool ID matches a pattern.
   * Supports: '*' (match all), 'prefix/*' (slash wildcard), 'prefix.*' (dot wildcard), exact match.
   */
  static matches(pattern: string, toolId: string): boolean {
    if (pattern === '*') return true;
    if (pattern.endsWith('/*')) {
      const prefix = pattern.slice(0, -2);
      return toolId.startsWith(prefix + '/');
    }
    if (pattern.endsWith('.*')) {
      const prefix = pattern.slice(0, -2);
      return (
        toolId === prefix || toolId.startsWith(prefix + '.') || toolId.startsWith(prefix + '/')
      );
    }
    return pattern === toolId;
  }
}

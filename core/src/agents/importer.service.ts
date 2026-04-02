import fs from 'fs/promises';
import path from 'path';
import yaml from 'js-yaml';
import {
  AgentTemplateSchema,
  NamedListSchema,
  CapabilityPolicySchema,
  SandboxBoundarySchema,
} from './schemas.js';
import { AgentRegistry } from "./registry.service.js";
import { Logger } from '../lib/logger.js';

const logger = new Logger('ResourceImporter');

export class ResourceImporter {
  constructor(
    private registry: AgentRegistry,
    private baseDir: string
  ) {}

  async importAll() {
    const results = {
      added: [] as string[],
      updated: [] as string[],
      removed: [] as string[],
      errors: [] as string[],
    };

    const lists = await this.importNamedLists();
    const policies = await this.importCapabilityPolicies();
    const boundaries = await this.importSandboxBoundaries();
    const templates = await this.importTemplates();

    const allUpserts = [...lists, ...policies, ...boundaries, ...templates];
    for (const u of allUpserts) {
      if (u.status === 'added') results.added.push(u.name);
      else results.updated.push(u.name);
    }

    const validNames = {
      lists: lists.map((l) => l.name),
      policies: policies.map((p) => p.name),
      boundaries: boundaries.map((b) => b.name),
      templates: templates.map((t) => t.name),
    };

    const cleanupTemplates = await this.registry.deleteTemplatesExcept(validNames.templates);
    const cleanupLists = await this.registry.deleteNamedListsExcept(validNames.lists);
    const cleanupPolicies = await this.registry.deleteCapabilityPoliciesExcept(validNames.policies);
    const cleanupBoundaries = await this.registry.deleteSandboxBoundariesExcept(
      validNames.boundaries
    );

    results.removed.push(
      ...cleanupTemplates.removed,
      ...cleanupLists.removed,
      ...cleanupPolicies.removed,
      ...cleanupBoundaries.removed
    );
    results.errors.push(
      ...cleanupTemplates.errors,
      ...cleanupLists.errors,
      ...cleanupPolicies.errors,
      ...cleanupBoundaries.errors
    );

    return results;
  }

  private async importNamedLists() {
    const allResults: { status: 'added' | 'updated'; name: string }[] = [];
    const types = [
      'network-allowlist',
      'network-denylist',
      'command-allowlist',
      'command-denylist',
      'secret-list',
    ];
    for (const type of types) {
      const dir = path.join(this.baseDir, 'lists', type);
      const results = await this.importDir(dir, NamedListSchema, (data) =>
        this.registry.upsertNamedList(data as import('./schemas.js').NamedList)
      );
      allResults.push(...results);
    }
    return allResults;
  }

  private async importCapabilityPolicies() {
    const dir = path.join(this.baseDir, 'capability-policies');
    return this.importDir(dir, CapabilityPolicySchema, (data) =>
      this.registry.upsertCapabilityPolicy(data)
    );
  }

  private async importSandboxBoundaries() {
    const dir = path.join(this.baseDir, 'sandbox-boundaries');
    return this.importDir(dir, SandboxBoundarySchema, (data) =>
      this.registry.upsertSandboxBoundary(data)
    );
  }

  private async importTemplates() {
    const allResults: { status: 'added' | 'updated'; name: string }[] = [];

    // 1. Builtin templates (templates/builtin)
    const builtinDir = path.join(this.baseDir, 'templates', 'builtin');
    const builtinResults = await this.importDir(builtinDir, AgentTemplateSchema, (data) => {
      (data.metadata as { builtin: boolean }).builtin = true;
      return this.registry.upsertTemplate(data as import('./schemas.js').AgentTemplate);
    });
    allResults.push(...builtinResults);

    // 2. Custom templates (templates/custom)
    const customDir = path.join(this.baseDir, 'templates', 'custom');
    const customResults = await this.importDir(customDir, AgentTemplateSchema, (data) => {
      (data.metadata as { builtin: boolean }).builtin = false;
      return this.registry.upsertTemplate(data as import('./schemas.js').AgentTemplate);
    });
    allResults.push(...customResults);

    // 3. Agent manifests (agents/)
    // These use 'kind: Agent' and have a slightly different structure,
    // but the registry upsertTemplate expects AgentTemplate.
    const agentsDir = path.join(this.baseDir, 'agents');
    try {
      const entries = await fs.readdir(agentsDir, { withFileTypes: true });
      for (const entry of entries) {
        try {
          let filePath: string | undefined;
          if (entry.isFile() && entry.name.endsWith('.agent.yaml')) {
            filePath = path.join(agentsDir, entry.name);
          } else if (entry.isDirectory()) {
            const subFile = path.join(agentsDir, entry.name, 'AGENT.yaml');
            try {
              await fs.access(subFile);
              filePath = subFile;
            } catch {
              /* ignore */
            }
          }

          if (filePath) {
            const content = await fs.readFile(filePath, 'utf8');
            const raw = yaml.load(content) as Record<string, unknown> | null | undefined;

            let template: Record<string, unknown> | undefined;

            // If kind is 'Agent', we need to map it to 'AgentTemplate' for the registry
            if (raw && raw.kind === 'Agent') {
              template = {
                apiVersion: raw.apiVersion,
                kind: 'AgentTemplate',
                metadata: {
                  ...(raw.metadata as Record<string, unknown>),
                  builtin: false,
                },
                spec: {
                  ...(raw.spec as Record<string, unknown>),
                  identity: raw.identity,
                  model: raw.model,
                  tools: raw.tools,
                  skills: raw.skills,
                  skillPackages: raw.skillPackages,
                  subagents: raw.subagents,
                  intercom: raw.intercom,
                  resources: raw.resources,
                  workspace: raw.workspace,
                  memory: raw.memory,
                  permissions: raw.permissions,
                  capabilities: raw.capabilities,
                  schedules: raw.schedules,
                  mounts: raw.mounts,
                },
              };
            } else if (raw && raw.kind === 'AgentTemplate') {
              template = raw;
            }

            if (template) {
              const result = AgentTemplateSchema.safeParse(template);
              if (!result.success) {
                logger.error(`Error validating ${filePath}:`, result.error.format());
                continue;
              }
              const res = await this.registry.upsertTemplate(
                result.data as import('./schemas.js').AgentTemplate
              );
              allResults.push(res as { status: 'added' | 'updated'; name: string });
            }
          }
        } catch (fileErr) {
          logger.error(`Error processing agent manifest in ${entry.name}:`, fileErr);
        }
      }
    } catch (err: unknown) {
      if ((err as { code?: string }).code !== 'ENOENT') {
        logger.error(`Error reading agents directory:`, err);
      }
    }

    return allResults;
  }

  private async importDir<T>(
    dir: string,
    schema: import('zod').ZodSchema<T>,
    upsertFn: (data: T) => Promise<unknown>
  ): Promise<{ status: 'added' | 'updated'; name: string }[]> {
    const results: { status: 'added' | 'updated'; name: string }[] = [];
    try {
      const files = await fs.readdir(dir);
      for (const file of files) {
        if (file.endsWith('.yaml') || file.endsWith('.yml')) {
          const filePath = path.join(dir, file);
          const content = await fs.readFile(filePath, 'utf8');
          const rawData = yaml.load(content);

          const result = schema.safeParse(rawData);
          if (!result.success) {
            logger.error(`Error validating ${filePath}:`, result.error.format());
            continue;
          }

          const res = await upsertFn(result.data);
          results.push(res as { status: 'added' | 'updated'; name: string });
          logger.info(`Imported ${filePath}`);
        }
      }
    } catch (err: unknown) {
      if ((err as { code?: string }).code !== 'ENOENT') {
        logger.error(`Error reading directory ${dir}:`, err);
      }
    }
    return results;
  }
}

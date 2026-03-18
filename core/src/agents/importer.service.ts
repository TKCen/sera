import fs from 'fs/promises';
import path from 'path';
import yaml from 'js-yaml';
import { 
  AgentTemplateSchema, 
  NamedListSchema, 
  CapabilityPolicySchema, 
  SandboxBoundarySchema 
} from './schemas.js';
import { AgentRegistry } from './registry.service.js';

export class ResourceImporter {
  constructor(private registry: AgentRegistry, private baseDir: string) {}

  async importAll() {
    await this.importNamedLists();
    await this.importCapabilityPolicies();
    await this.importSandboxBoundaries();
    await this.importTemplates();
  }

  private async importNamedLists() {
    const types = [
      'network-allowlist',
      'network-denylist',
      'command-allowlist',
      'command-denylist',
      'secret-list'
    ];
    for (const type of types) {
      const dir = path.join(this.baseDir, 'lists', type);
      await this.importDir(dir, NamedListSchema, (data) => this.registry.upsertNamedList(data));
    }
  }

  private async importCapabilityPolicies() {
    const dir = path.join(this.baseDir, 'capability-policies');
    await this.importDir(dir, CapabilityPolicySchema, (data) => this.registry.upsertCapabilityPolicy(data));
  }

  private async importSandboxBoundaries() {
    const dir = path.join(this.baseDir, 'sandbox-boundaries');
    await this.importDir(dir, SandboxBoundarySchema, (data) => this.registry.upsertSandboxBoundary(data));
  }

  private async importTemplates() {
    // Builtin templates
    const builtinDir = path.join(this.baseDir, 'templates', 'builtin');
    await this.importDir(builtinDir, AgentTemplateSchema, (data) => {
      data.metadata.builtin = true;
      return this.registry.upsertTemplate(data);
    });

    // Custom templates
    const customDir = path.join(this.baseDir, 'templates', 'custom');
    await this.importDir(customDir, AgentTemplateSchema, (data) => {
      data.metadata.builtin = false;
      return this.registry.upsertTemplate(data);
    });
  }

  private async importDir(dir: string, schema: any, upsertFn: (data: any) => Promise<any>) {
    try {
      const files = await fs.readdir(dir);
      for (const file of files) {
        if (file.endsWith('.yaml') || file.endsWith('.yml')) {
          const filePath = path.join(dir, file);
          const content = await fs.readFile(filePath, 'utf8');
          const rawData = yaml.load(content);
          
          const result = schema.safeParse(rawData);
          if (!result.success) {
            console.error(`Error validating ${filePath}:`, result.error.format());
            continue;
          }

          await upsertFn(result.data);
          console.log(`Imported ${filePath}`);
        }
      }
    } catch (err: any) {
      if (err.code !== 'ENOENT') {
        console.error(`Error reading directory ${dir}:`, err);
      }
    }
  }
}

import type { SkillRegistry } from '../SkillRegistry.js';
import type { MemoryManager } from '../../memory/manager.js';
import { webSearchSkill } from './web-search.js';
import { webFetchSkill } from './web-fetch.js';
import { fileReadSkill } from './file-read.js';
import { fileWriteSkill } from './file-write.js';
import { fileListSkill } from './file-list.js';
import { createKnowledgeStoreSkill } from './knowledge-store.js';
import { createKnowledgeQuerySkill } from './knowledge-query.js';
import { shellExecSkill } from './shell-exec.js';
import { updateEnvironmentSkill } from './update-environment.js';

/**
 * Register all built-in skills with a SkillRegistry instance.
 *
 * Knowledge skills require a MemoryManager, so it's passed as a dependency.
 */
export function registerBuiltinSkills(
  registry: SkillRegistry,
  memoryManager: MemoryManager,
): void {
  registry.register(webSearchSkill);
  registry.register(webFetchSkill);
  registry.register(fileReadSkill);
  registry.register(fileWriteSkill);
  registry.register(fileListSkill);
  registry.register(shellExecSkill);
  registry.register(updateEnvironmentSkill);
  registry.register(createKnowledgeStoreSkill(memoryManager));
  registry.register(createKnowledgeQuerySkill(memoryManager));
}

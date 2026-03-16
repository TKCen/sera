import type { SkillRegistry } from '../SkillRegistry.js';
import type { MemoryManager } from '../../memory/manager.js';
import { webSearchSkill } from './web-search.js';
import { fileReadSkill } from './file-read.js';
import { fileWriteSkill } from './file-write.js';
import { createKnowledgeStoreSkill } from './knowledge-store.js';
import { createKnowledgeQuerySkill } from './knowledge-query.js';

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
  registry.register(fileReadSkill);
  registry.register(fileWriteSkill);
  registry.register(createKnowledgeStoreSkill(memoryManager));
  registry.register(createKnowledgeQuerySkill(memoryManager));
}

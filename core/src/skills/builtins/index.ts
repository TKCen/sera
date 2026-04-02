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
import { scheduleTaskSkill } from './schedule-task.js';
import { delegateTaskSkill } from './delegate-task.js';
import { imageViewSkill } from './image-view.js';
import { pdfReadSkill } from './pdf-read.js';
import { codeEvalSkill } from './code-eval.js';
import { httpRequestSkill } from './http-request.js';

/**
 * Register all built-in skills with a SkillRegistry instance.
 * The memoryManager parameter is kept for backward compat but no longer used
 * by the knowledge skills (they operate on the scoped stores directly).
 */
export function registerBuiltinSkills(
  registry: SkillRegistry,
  _memoryManager: MemoryManager
): void {
  registry.register(webSearchSkill);
  registry.register(webFetchSkill);
  registry.register(fileReadSkill);
  registry.register(fileWriteSkill);
  registry.register(fileListSkill);
  registry.register(shellExecSkill);
  registry.register(scheduleTaskSkill);
  registry.register(delegateTaskSkill);
  registry.register(imageViewSkill);
  registry.register(pdfReadSkill);
  registry.register(codeEvalSkill);
  registry.register(httpRequestSkill);
  registry.register(createKnowledgeStoreSkill());
  registry.register(createKnowledgeQuerySkill());
}

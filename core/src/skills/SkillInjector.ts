import type { Pool } from 'pg';
import { SkillLibrary } from './SkillLibrary.js';
import { Logger } from '../lib/logger.js';

const logger = new Logger('SkillInjector');

interface SkillPin {
  name: string;
  version?: string | undefined;
}

export class SkillInjector {
  constructor(private pool: Pool) {}

  /**
   * Injects relevant skills into the system prompt.
   *
   * @param systemPrompt The original system prompt
   * @param declaredSkills List of skill names or pins from agent manifest
   * @param declaredPackages List of skill package names from agent manifest
   * @param messageContent Content of the current user message for auto-triggering
   * @param tokenBudget Maximum tokens allowed for skills (hint)
   */
  async inject(
    systemPrompt: string,
    declaredSkills: Array<string | SkillPin>,
    declaredPackages: string[] = [],
    messageContent: string,
    circleId?: string | null,
    tokenBudget: number = 4000
  ): Promise<string> {
    const skillLibrary = SkillLibrary.getInstance(this.pool);

    // 0. Inject Circle Constitution (Story 10.2)
    let constitutionXml = '';
    if (circleId) {
      const res = await this.pool.query('SELECT constitution FROM circles WHERE id = $1', [
        circleId,
      ]);
      if (res.rows.length > 0 && res.rows[0].constitution) {
        const fullConstitution = res.rows[0].constitution;
        const truncated = this.truncateConstitution(fullConstitution, 2048); // Target ~2000 tokens
        constitutionXml = `<circle-constitution>\n${truncated}\n</circle-constitution>\n\n`;
      }
    }

    // 1. Identify skills to load (Map name -> version)
    const skillsToLoad = new Map<string, string | undefined>();

    for (const s of declaredSkills) {
      if (typeof s === 'string') {
        skillsToLoad.set(s, undefined);
      } else {
        skillsToLoad.set(s.name, s.version);
      }
    }

    // 2. Resolve packages
    for (const pkgName of declaredPackages) {
      const pkg = await skillLibrary.getPackage(pkgName);
      if (pkg) {
        for (const s of pkg.skills) {
          if (!skillsToLoad.has(s.name)) {
            skillsToLoad.set(s.name, s.version);
          }
        }
      } else {
        logger.warn(`Skill package not found: ${pkgName}`);
      }
    }

    // 3. Auto-triggering
    const allSkills = await skillLibrary.listSkills();
    for (const skill of allSkills) {
      if (
        skill.triggers.some((trigger) =>
          messageContent.toLowerCase().includes(trigger.toLowerCase())
        )
      ) {
        if (!skillsToLoad.has(skill.name)) {
          skillsToLoad.set(skill.name, undefined);
        }
      }
    }

    if (skillsToLoad.size === 0) {
      return constitutionXml ? systemPrompt + '\n\n' + constitutionXml.trim() : systemPrompt;
    }

    // 4. Fetch full documents and resolve dependencies
    const loadedSkills: import('./schema.js').SkillDocument[] = [];
    const processed = new Set<string>();
    const queue: SkillPin[] = Array.from(skillsToLoad.entries()).map(([name, version]) => ({
      name,
      version,
    }));

    while (queue.length > 0) {
      const pin = queue.shift()!;
      if (processed.has(pin.name)) {
        // Currently, first version encountered wins.
        continue;
      }
      processed.add(pin.name);

      const doc = await skillLibrary.getSkill(pin.name, pin.version);
      if (doc) {
        loadedSkills.push(doc);
        if (doc.requires && doc.requires.length > 0) {
          // Dependencies usually don't have pinned versions in the front-matter schema yet
          queue.push(...doc.requires.map((r) => ({ name: r })));
        }
      }
    }

    // 5. Token budgeting
    let currentTokens = 0;
    const finalSkills: import('./schema.js').SkillDocument[] = [];
    let droppedCount = 0;

    for (const skill of loadedSkills) {
      const estimatedTokens = skill.content.length / 4 + 100;
      if (currentTokens + estimatedTokens <= tokenBudget) {
        finalSkills.push(skill);
        currentTokens += estimatedTokens;
      } else {
        droppedCount++;
      }
    }

    if (droppedCount > 0) {
      logger.warn(
        `Skill budget exceeded: dropped ${droppedCount} skills. [thought:reflect: Skill budget exceeded, dropping context]`
      );
    }

    if (finalSkills.length === 0) {
      return constitutionXml ? systemPrompt + '\n\n' + constitutionXml : systemPrompt;
    }

    // 6. Format as XML
    const skillsXml = [
      '<skills>',
      ...finalSkills.map((s) => {
        const idAttr = s.id ? ` id="${s.id}"` : '';
        return `  <skill${idAttr} name="${s.name}" version="${s.version}">\n${s.content}\n  </skill>`;
      }),
      '</skills>',
    ].join('\n');

    const totalInjection = (constitutionXml + skillsXml).trim();

    // 7. Append to system prompt
    const principlesMatch = systemPrompt.indexOf('## Guiding Principles');
    if (principlesMatch !== -1) {
      const nextSectionMatch = systemPrompt.indexOf('\n## ', principlesMatch + 20);
      const insertIdx = nextSectionMatch !== -1 ? nextSectionMatch : systemPrompt.length;

      return (
        systemPrompt.slice(0, insertIdx).trim() +
        '\n\n' +
        totalInjection +
        '\n\n' +
        systemPrompt.slice(insertIdx).trim()
      ).trim();
    }

    return systemPrompt + '\n\n' + totalInjection;
  }

  /**
   * Truncates constitution to stay within token budget while preserving opening principles.
   */
  private truncateConstitution(text: string, maxChars: number): string {
    if (text.length <= maxChars) return text;

    const lines = text.split('\n').filter((l) => l.trim() !== '');
    if (lines.length <= 1) return text.slice(0, maxChars) + '... [truncated]';

    // Keep the first paragraph/line as it usually contains the "Core Purpose"
    const head = lines[0] || '';
    const tailBudget = maxChars - head.length - 50; // room for truncation notice

    if (tailBudget <= 0) return head.slice(0, maxChars);

    // Truncate from the bottom up (keep top lines)
    let currentLength = head.length;
    const keptLines = [head];

    for (let i = 1; i < lines.length; i++) {
      const line = lines[i];
      if (line === undefined) continue;
      if (currentLength + line.length + 1 > tailBudget) {
        keptLines.push('... [truncated for token budget]');
        break;
      }
      keptLines.push(line);
      currentLength += line.length + 1;
    }

    return keptLines.join('\n');
  }
}

/**
 * DogfeedAnalyzer — reads the task tracker and picks the next task for a cycle.
 *
 * Parses docs/DOGFEED-TASKS.md to extract tasks, then selects the
 * highest-priority ready task. Also supports heuristic scanning (lint/typecheck)
 * to discover new task candidates.
 */

import fs from 'node:fs';
import { Logger } from '../lib/logger.js';
import type { DogfeedTask, TaskCategory } from './types.js';

const logger = new Logger('DogfeedAnalyzer');

/** Valid task categories — used for validation during parsing */
const VALID_CATEGORIES = new Set<string>([
  'lint',
  'type-error',
  'todo',
  'dead-code',
  'test',
  'refactor',
  'feature',
  'research',
  'infra',
]);

/**
 * Regex for task lines in DOGFEED-TASKS.md:
 * `- [ ] P<priority> | <category> | <description>`
 * or `- [x] P<priority> | <category> | <description> | <outcome> | ...`
 */
const TASK_LINE_RE = /^- \[([ x])\] P(\d+)\s*\|\s*(\w[\w-]*)\s*\|\s*(.+?)(?:\s*\|.*)?$/;

/**
 * Extract a file path hint from a task description.
 * Matches patterns like `core/src/foo/bar.ts` or `core/src/foo/bar.ts:42`
 */
const FILE_HINT_RE = /(\S+\.(?:ts|js|tsx|jsx|json|yaml|yml))(?::(\d+))?/;

export class DogfeedAnalyzer {
  constructor(private readonly taskFilePath: string) {}

  /**
   * Parse DOGFEED-TASKS.md and return all tasks in the "Ready" section.
   */
  scanTaskFile(): DogfeedTask[] {
    if (!fs.existsSync(this.taskFilePath)) {
      logger.warn(`Task file not found: ${this.taskFilePath}`);
      return [];
    }

    const content = fs.readFileSync(this.taskFilePath, 'utf-8');
    const lines = content.split('\n');
    const tasks: DogfeedTask[] = [];

    let inReadySection = false;
    let inDoneSection = false;

    for (const line of lines) {
      const trimmed = line.trim();

      // Track which section we're in
      if (trimmed.startsWith('## Ready')) {
        inReadySection = true;
        inDoneSection = false;
        continue;
      }
      if (trimmed.startsWith('## In Progress')) {
        inReadySection = false;
        inDoneSection = false;
        continue;
      }
      if (trimmed.startsWith('## Done')) {
        inReadySection = false;
        inDoneSection = true;
        continue;
      }
      // A new H2 or H1 ends the current section
      if (/^#{1,2}\s/.test(trimmed) && !trimmed.startsWith('###')) {
        inReadySection = false;
        inDoneSection = false;
        continue;
      }

      if (!inReadySection && !inDoneSection) continue;

      const match = TASK_LINE_RE.exec(trimmed);
      if (!match) continue;

      const checked = match[1] === 'x';
      const priority = parseInt(match[2]!, 10);
      const rawCategory = match[3]!;
      const description = match[4]!.trim();

      // Validate priority
      if (priority < 0 || !Number.isFinite(priority)) {
        logger.warn(`Invalid priority ${priority} in task: ${description}`);
        continue;
      }

      // Validate category
      if (!VALID_CATEGORIES.has(rawCategory)) {
        logger.warn(`Unknown category "${rawCategory}" in task: ${description}`);
        continue;
      }
      const category = rawCategory as TaskCategory;

      // Validate description
      if (!description) {
        logger.warn(`Empty description in task line: ${trimmed}`);
        continue;
      }

      const fileMatch = FILE_HINT_RE.exec(description);

      tasks.push({
        priority,
        category,
        description,
        status: checked ? 'done' : inDoneSection ? 'done' : 'ready',
        ...(fileMatch?.[1] ? { filePath: fileMatch[1] } : {}),
        ...(fileMatch?.[2] ? { line: parseInt(fileMatch[2], 10) } : {}),
      });
    }

    return tasks;
  }

  /**
   * Pick the highest-priority ready task.
   * Returns undefined if no tasks are available.
   */
  pickNext(): DogfeedTask | undefined {
    const tasks = this.scanTaskFile();
    const ready = tasks.filter((t) => t.status === 'ready').sort((a, b) => a.priority - b.priority);

    if (ready.length === 0) {
      logger.info('No ready tasks available');
      return undefined;
    }

    const picked = ready[0]!;
    logger.info(`Picked task: P${picked.priority} | ${picked.category} | ${picked.description}`);
    return picked;
  }

  /**
   * Mark a task as done in the task file by replacing `- [ ]` with `- [x]`
   * and moving it to the Done section with outcome metadata.
   */
  markDone(task: DogfeedTask, outcome: string, tokens: number, duration: string): void {
    if (!fs.existsSync(this.taskFilePath)) return;

    let content = fs.readFileSync(this.taskFilePath, 'utf-8');

    // Find and update the task line
    const escapedDesc = task.description.replace(/[.*+?^${}()|[\]\\]/g, '\\$&');
    const pattern = new RegExp(
      `^(- \\[) \\] P${task.priority}\\s*\\|\\s*${task.category}\\s*\\|\\s*${escapedDesc}`,
      'm'
    );

    // Remove from ready section
    content = content.replace(pattern, '');

    // Add to done section
    const doneLine = `- [x] P${task.priority} | ${task.category} | ${task.description} | ${outcome} | ~${tokens}k tokens | ${duration}`;
    const doneMarker = '## Done';
    const doneIdx = content.indexOf(doneMarker);
    if (doneIdx !== -1) {
      const commentEnd = content.indexOf('\n', content.indexOf('<!--', doneIdx));
      if (commentEnd !== -1) {
        content =
          content.substring(0, commentEnd + 1) +
          doneLine +
          '\n' +
          content.substring(commentEnd + 1);
      }
    }

    // Clean up empty lines
    content = content.replace(/\n{3,}/g, '\n\n');

    fs.writeFileSync(this.taskFilePath, content, 'utf-8');
    logger.info(`Marked task done: ${task.description}`);
  }
}

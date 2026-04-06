/**
 * DogfeedService — analyzes the SERA codebase for improvement opportunities
 * and queues tasks for bridge agents via the task queue.
 *
 * Analysis steps:
 *   1. Lint scan     — ESLint auto-fixable issues → OMO (trivial)
 *   2. TypeScript    — tsc --noEmit error count → OMC (medium)
 *   3. TODO scanner  — TODO/FIXME/HACK comments → OMC (medium)
 *   4. Test coverage — source files missing .test.ts → OMC (complex)
 */

import { execFile } from 'node:child_process';
import fs from 'node:fs';
import readline from 'node:readline';
import path from 'node:path';
import { promisify } from 'node:util';
import { v4 as uuidv4 } from 'uuid';
import { pool } from '../lib/database.js';
import { Logger } from '../lib/logger.js';
import { ChannelRouter } from '../channels/ChannelRouter.js';

const execFileAsync = promisify(execFile);
const logger = new Logger('DogfeedService');

// ── Types ─────────────────────────────────────────────────────────────────────

export type DogfeedTool = 'omc' | 'omo';
export type DogfeedComplexity = 'trivial' | 'medium' | 'complex';
export type DogfeedCategory = 'lint' | 'typecheck' | 'todo' | 'test-coverage';

export interface DogfeedTask {
  prompt: string;
  tool: DogfeedTool;
  complexity: DogfeedComplexity;
  category: DogfeedCategory;
}

interface QueueResult {
  queued: number;
  skipped: number;
  bridgeNotFound: string[];
}

// ── Constants ─────────────────────────────────────────────────────────────────

/** Root of the sera repo as seen from inside the sera-core Docker container */
const REPO_ROOT = process.env['DOGFEED_REPO_ROOT'] ?? '/app';

/** Directories to scan for source files */
const SCAN_DIRS = [`${REPO_ROOT}/core/src`, `${REPO_ROOT}/web/src`];

/** File path fragments that indicate files to skip */
const SKIP_PATH_FRAGMENTS = ['/node_modules/', '/dist/', '/__snapshots__/', '.d.ts'];

/** Maximum number of tasks produced per analyzer per cycle */
const MAX_TASKS_PER_CATEGORY = 10;

/** Comment patterns for TODO scanner */
const TODO_RE = /\/\/\s*(TODO|FIXME|HACK)\b/i;

// ── ESLint result types (subset of --format json output) ─────────────────────

interface ESLintMessage {
  ruleId: string | null;
  severity: number;
  message: string;
  line: number;
  column: number;
  fix?: unknown;
}

interface ESLintFileResult {
  filePath: string;
  messages: ESLintMessage[];
  fixableWarningCount: number;
  fixableErrorCount: number;
}

// ── DogfeedService ────────────────────────────────────────────────────────────

export class DogfeedService {
  private static instance: DogfeedService;

  private constructor() {}

  public static getInstance(): DogfeedService {
    if (!DogfeedService.instance) {
      DogfeedService.instance = new DogfeedService();
    }
    return DogfeedService.instance;
  }

  // ── Public API ──────────────────────────────────────────────────────────────

  /**
   * Run all analyzers and return the discovered improvement tasks.
   * Does not touch the database.
   */
  public async analyzeCodebase(): Promise<DogfeedTask[]> {
    logger.info('Starting codebase analysis');

    const [lintTasks, typeTasks, todoTasks, coverageTasks] = await Promise.allSettled([
      this.analyzeLint(),
      this.analyzeTypeScript(),
      this.analyzeTodos(),
      this.analyzeTestCoverage(),
    ]);

    const tasks: DogfeedTask[] = [];

    for (const result of [lintTasks, typeTasks, todoTasks, coverageTasks]) {
      if (result.status === 'fulfilled') {
        tasks.push(...result.value);
      } else {
        logger.warn('Analyzer step failed:', result.reason);
      }
    }

    logger.info(`Analysis complete: ${tasks.length} improvement tasks discovered`);
    return tasks;
  }

  /**
   * Analyze + queue tasks for bridge agents.
   * Returns a summary of what was queued.
   */
  public async runCycle(): Promise<QueueResult> {
    logger.info('DogfeedService cycle starting');
    const tasks = await this.analyzeCodebase();

    if (tasks.length === 0) {
      logger.info('No improvement tasks found — cycle complete');
      return { queued: 0, skipped: 0, bridgeNotFound: [] };
    }

    const result = await this.queueTasks(tasks);

    ChannelRouter.getInstance().route({
      id: uuidv4(),
      eventType: 'dogfeed.cycle.complete',
      title: 'Dogfeed cycle complete',
      body: `Queued ${result.queued} improvement tasks (${result.skipped} skipped)`,
      severity: 'info',
      metadata: {
        queued: result.queued,
        skipped: result.skipped,
        bridgeNotFound: result.bridgeNotFound,
        taskCount: tasks.length,
      },
      timestamp: new Date().toISOString(),
    });

    logger.info(`Cycle complete: ${result.queued} queued, ${result.skipped} skipped`);
    return result;
  }

  // ── Analyzers ───────────────────────────────────────────────────────────────

  /**
   * Run ESLint on core/src/ and return tasks for auto-fixable issues.
   * Routed to OMO (trivial) since eslint --fix handles these mechanically.
   */
  private async analyzeLint(): Promise<DogfeedTask[]> {
    const coreDir = `${REPO_ROOT}/core/src`;

    let stdout = '';
    try {
      const result = await execFileAsync('bunx', ['eslint', '--format', 'json', coreDir], {
        cwd: REPO_ROOT,
        timeout: 60_000,
      });
      stdout = result.stdout;
    } catch (err) {
      // ESLint exits non-zero when it finds issues — that is expected
      const execErr = err as { stdout?: string; code?: number };
      if (execErr.stdout) {
        stdout = execErr.stdout;
      } else {
        logger.warn('ESLint failed to run:', err);
        return [];
      }
    }

    let results: ESLintFileResult[];
    try {
      results = JSON.parse(stdout) as ESLintFileResult[];
    } catch {
      logger.warn('ESLint output was not valid JSON');
      return [];
    }

    const tasks: DogfeedTask[] = [];

    for (const fileResult of results) {
      if (tasks.length >= MAX_TASKS_PER_CATEGORY) break;

      const fixableCount = fileResult.fixableErrorCount + fileResult.fixableWarningCount;
      if (fixableCount === 0) continue;

      const relPath = fileResult.filePath.startsWith(REPO_ROOT)
        ? fileResult.filePath.slice(REPO_ROOT.length + 1)
        : fileResult.filePath;

      const ruleIds = [
        ...new Set(
          fileResult.messages.filter((m) => m.fix !== undefined && m.ruleId).map((m) => m.ruleId!)
        ),
      ]
        .slice(0, 5)
        .join(', ');

      tasks.push({
        prompt:
          `Fix ${fixableCount} auto-fixable ESLint issue(s) in ${relPath}. ` +
          `Run: bunx eslint --fix ${relPath}` +
          (ruleIds ? `. Rules: ${ruleIds}` : '.'),
        tool: 'omo',
        complexity: 'trivial',
        category: 'lint',
      });
    }

    logger.info(`Lint analyzer: ${tasks.length} fixable files found`);
    return tasks;
  }

  /**
   * Run tsc --noEmit and produce a single task if there are type errors.
   * Routed to OMC (medium) since type errors require understanding context.
   */
  private async analyzeTypeScript(): Promise<DogfeedTask[]> {
    const tsconfigPath = `${REPO_ROOT}/core/tsconfig.json`;

    let stderr = '';

    try {
      await execFileAsync('bunx', ['tsc', '--noEmit', '-p', tsconfigPath], {
        cwd: REPO_ROOT,
        timeout: 120_000,
      });
      logger.info('TypeScript: no errors');
      return [];
    } catch (err) {
      const execErr = err as { stderr?: string; stdout?: string };
      stderr = execErr.stderr ?? execErr.stdout ?? '';
    }

    const matches = stderr.match(/error TS\d+/g);
    const errorCount = matches?.length ?? 0;

    if (errorCount === 0) {
      return [];
    }

    const errorLines = stderr
      .split('\n')
      .filter((l) => l.includes('error TS'))
      .slice(0, 5)
      .join('\n');

    logger.info(`TypeScript: ${errorCount} error(s) found`);

    return [
      {
        prompt:
          `Fix ${errorCount} TypeScript error(s) in core/. ` +
          `Run: bunx tsc --noEmit -p core/tsconfig.json\n\nFirst errors:\n${errorLines}`,
        tool: 'omc',
        complexity: 'medium',
        category: 'typecheck',
      },
    ];
  }

  /**
   * Scan source files for TODO/FIXME/HACK comments using Node.js readline.
   * Routed to OMC (medium) since resolving these needs codebase context.
   */
  private async analyzeTodos(): Promise<DogfeedTask[]> {
    const hits: Array<{ file: string; line: number; text: string; kind: string }> = [];

    for (const dir of SCAN_DIRS) {
      if (!fs.existsSync(dir)) continue;
      await this.walkDir(dir, async (filePath) => {
        if (hits.length >= MAX_TASKS_PER_CATEGORY * 3) return;
        if (!filePath.endsWith('.ts') && !filePath.endsWith('.tsx')) return;
        if (SKIP_PATH_FRAGMENTS.some((frag) => filePath.includes(frag))) return;

        await this.scanFileForTodos(filePath, hits);
      });
    }

    if (hits.length === 0) {
      logger.info('TODO scanner: no hits');
      return [];
    }

    const byFile = new Map<string, typeof hits>();
    for (const hit of hits) {
      const group = byFile.get(hit.file) ?? [];
      group.push(hit);
      byFile.set(hit.file, group);
    }

    const tasks: DogfeedTask[] = [];
    for (const [file, fileHits] of byFile) {
      if (tasks.length >= MAX_TASKS_PER_CATEGORY) break;

      const relPath = file.startsWith(REPO_ROOT) ? file.slice(REPO_ROOT.length + 1) : file;
      const lines = fileHits
        .slice(0, 5)
        .map((h) => `  Line ${h.line}: ${h.text.trim()}`)
        .join('\n');

      tasks.push({
        prompt:
          `Resolve ${fileHits.length} TODO/FIXME/HACK comment(s) in ${relPath}:\n${lines}\n\n` +
          `Remove or implement each comment. Do not leave placeholder stubs.`,
        tool: 'omc',
        complexity: 'medium',
        category: 'todo',
      });
    }

    logger.info(
      `TODO scanner: ${hits.length} hits in ${byFile.size} files → ${tasks.length} tasks`
    );
    return tasks;
  }

  /**
   * List source files without a corresponding .test.ts file.
   * Routed to OMC (complex) since writing meaningful tests requires deep understanding.
   */
  private async analyzeTestCoverage(): Promise<DogfeedTask[]> {
    const missing: string[] = [];

    for (const dir of SCAN_DIRS) {
      if (!fs.existsSync(dir)) continue;
      await this.walkDir(dir, async (filePath) => {
        if (missing.length >= MAX_TASKS_PER_CATEGORY * 5) return;
        if (!filePath.endsWith('.ts')) return;
        if (filePath.endsWith('.test.ts') || filePath.endsWith('.d.ts')) return;
        if (SKIP_PATH_FRAGMENTS.some((frag) => filePath.includes(frag))) return;

        const basename = path.basename(filePath);
        if (basename === 'index.ts' || basename === 'types.ts' || basename === 'constants.ts') {
          return;
        }

        const testPath = filePath.replace(/\.ts$/, '.test.ts');
        if (!fs.existsSync(testPath)) {
          missing.push(filePath);
        }
      });
    }

    const tasks: DogfeedTask[] = [];
    for (const filePath of missing.slice(0, MAX_TASKS_PER_CATEGORY)) {
      const relPath = filePath.startsWith(REPO_ROOT)
        ? filePath.slice(REPO_ROOT.length + 1)
        : filePath;

      tasks.push({
        prompt:
          `Write unit tests for ${relPath}. ` +
          `Create ${relPath.replace(/\.ts$/, '.test.ts')} using Vitest. ` +
          `Follow the testing patterns in docs/TESTING.md. ` +
          `Mock external dependencies (database, Docker, HTTP). ` +
          `Target >80% line coverage for the public API surface.`,
        tool: 'omc',
        complexity: 'complex',
        category: 'test-coverage',
      });
    }

    logger.info(`Test coverage: ${missing.length} untested files → ${tasks.length} tasks queued`);
    return tasks;
  }

  // ── Task queue insertion ────────────────────────────────────────────────────

  /**
   * Insert discovered tasks into the bridge agent task queues.
   */
  private async queueTasks(tasks: DogfeedTask[]): Promise<QueueResult> {
    let queued = 0;
    let skipped = 0;
    const bridgeNotFound: string[] = [];

    for (const task of tasks) {
      const bridgeName = `${task.tool}-bridge`;

      const { rows: agentRows } = await pool.query<{ id: string }>(
        'SELECT id FROM agent_instances WHERE name = $1 LIMIT 1',
        [bridgeName]
      );
      const bridgeAgent = agentRows[0];

      if (!bridgeAgent) {
        logger.warn(`Bridge agent "${bridgeName}" not found — skipping task`);
        if (!bridgeNotFound.includes(bridgeName)) {
          bridgeNotFound.push(bridgeName);
        }
        skipped++;
        continue;
      }

      const taskId = uuidv4();
      const context = JSON.stringify({
        dogfeed: {
          category: task.category,
          complexity: task.complexity,
          tool: task.tool,
          queuedAt: new Date().toISOString(),
        },
      });

      // Priority: lower number = higher priority
      // trivial=200 (lowest urgency), medium=100, complex=50 (highest urgency)
      const priority =
        task.complexity === 'trivial' ? 200 : task.complexity === 'complex' ? 50 : 100;

      await pool.query(
        `INSERT INTO task_queue (id, agent_instance_id, task, priority, context, status)
         VALUES ($1, $2, $3, $4, $5, 'queued')`,
        [taskId, bridgeAgent.id, task.prompt, priority, context]
      );

      logger.info(`Queued task ${taskId} → ${bridgeName} [${task.category}/${task.complexity}]`);
      queued++;
    }

    return { queued, skipped, bridgeNotFound };
  }

  // ── Filesystem helpers ──────────────────────────────────────────────────────

  /**
   * Recursively walk a directory, calling visitor for each file.
   * Uses an iterative stack to avoid deep call-stack growth on large trees.
   */
  private async walkDir(dir: string, visitor: (filePath: string) => Promise<void>): Promise<void> {
    const stack = [dir];

    while (stack.length > 0) {
      const current = stack.pop()!;
      let entries: fs.Dirent[];
      try {
        entries = fs.readdirSync(current, { withFileTypes: true });
      } catch {
        continue;
      }

      for (const entry of entries) {
        const fullPath = path.join(current, entry.name);
        if (entry.isDirectory()) {
          if (!SKIP_PATH_FRAGMENTS.some((frag) => fullPath.includes(frag))) {
            stack.push(fullPath);
          }
        } else if (entry.isFile()) {
          await visitor(fullPath);
        }
      }
    }
  }

  /**
   * Scan a single file for TODO/FIXME/HACK comments line-by-line via readline.
   */
  private async scanFileForTodos(
    filePath: string,
    hits: Array<{ file: string; line: number; text: string; kind: string }>
  ): Promise<void> {
    return new Promise((resolve) => {
      let stream: fs.ReadStream;
      try {
        stream = fs.createReadStream(filePath, { encoding: 'utf8' });
      } catch {
        resolve();
        return;
      }

      const rl = readline.createInterface({ input: stream, crlfDelay: Infinity });
      let lineNo = 0;

      rl.on('line', (line) => {
        lineNo++;
        const match = TODO_RE.exec(line);
        if (match) {
          hits.push({
            file: filePath,
            line: lineNo,
            text: line.trim().substring(0, 120),
            kind: match[1]!.toUpperCase(),
          });
        }
      });

      rl.on('close', resolve);
      rl.on('error', () => resolve());
      stream.on('error', () => {
        rl.close();
        resolve();
      });
    });
  }
}

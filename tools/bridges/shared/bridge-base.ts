/**
 * Bridge Base — shared foundation for all SERA poll-based bridges.
 *
 * Subclasses implement `execute()`. The base class handles registration,
 * the poll loop, idempotency, workspace isolation, error handling, backoff,
 * and graceful shutdown.
 *
 * Spec: docs/BRIDGE-AGENT-SPEC.md
 */

import { readFileSync, writeFileSync, mkdirSync, existsSync } from 'node:fs';
import { homedir } from 'node:os';
import { join } from 'node:path';
import { execFileSync, spawn } from 'node:child_process';

// ── Config ────────────────────────────────────────────────────────────────────

export interface BridgeConfig {
  /** Human-readable name, e.g. "omc" — used for registration and log paths */
  toolName: string;
  displayName: string;
  /** Base URL for sera-core, e.g. http://localhost:3001 */
  coreUrl?: string;
  apiKey?: string;
  /** Root of the git repo used for worktree creation */
  repoRoot?: string;
  /** Milliseconds between polls (default 3000) */
  pollIntervalMs?: number;
  /** Milliseconds before a task is timed out locally (default 1 800 000 = 30 min) */
  taskTimeoutMs?: number;
}

// ── Task payload from /tasks/next ────────────────────────────────────────────

interface TaskPayload {
  taskId: string;
  task: string;
  context: {
    tool?: string;
    repo?: string;
    branch?: string;
    files?: string[];
    delegation?: { fromInstanceId?: string };
    [key: string]: unknown;
  };
  priority: number;
  retryCount: number;
  maxRetries: number;
}

// ── Logger ────────────────────────────────────────────────────────────────────

function log(
  level: 'info' | 'warn' | 'error' | 'debug',
  msg: string,
  extra?: Record<string, unknown>
): void {
  const entry: Record<string, unknown> = {
    ts: new Date().toISOString(),
    level,
    msg,
    ...extra,
  };
  // eslint-disable-next-line no-console
  console.error(JSON.stringify(entry));
}

// ── BridgeBase ────────────────────────────────────────────────────────────────

export abstract class BridgeBase {
  protected readonly toolName: string;
  protected readonly displayName: string;
  protected readonly coreUrl: string;
  protected readonly apiKey: string;
  protected readonly repoRoot: string;
  protected readonly pollIntervalMs: number;
  protected readonly taskTimeoutMs: number;

  private agentId: string | null = null;
  private shutdownRequested = false;
  private taskInProgress = false;

  /** In-memory set of processed task IDs (capped at 1 000) */
  private readonly processedIds = new Set<string>();
  /** Persistent log path for idempotency across restarts */
  private readonly processedLogPath: string;

  /** Count of consecutive poll errors (non-204, non-200) */
  private consecutiveErrors = 0;

  constructor(config: BridgeConfig) {
    this.toolName = config.toolName;
    this.displayName = config.displayName;
    this.coreUrl = config.coreUrl ?? process.env['SERA_CORE_URL'] ?? 'http://localhost:3001';
    this.apiKey = config.apiKey ?? process.env['SERA_API_KEY'] ?? 'sera_bootstrap_dev_123';
    this.repoRoot = config.repoRoot ?? process.env['SERA_REPO_ROOT'] ?? process.cwd();
    this.pollIntervalMs =
      config.pollIntervalMs ??
      (process.env['POLL_INTERVAL_MS'] ? parseInt(process.env['POLL_INTERVAL_MS'], 10) : 3000);
    this.taskTimeoutMs =
      config.taskTimeoutMs ??
      (process.env['TASK_TIMEOUT_MS'] ? parseInt(process.env['TASK_TIMEOUT_MS'], 10) : 1_800_000);

    // Set up idempotency log directory
    const stateDir = join(homedir(), '.sera', `bridge-${this.toolName}`);
    mkdirSync(stateDir, { recursive: true });
    this.processedLogPath = join(stateDir, 'processed-tasks.log');

    // Load persisted processed IDs (last 1000 lines)
    this.loadProcessedLog();
  }

  // ── Subclass contract ─────────────────────────────────────────────────────

  /**
   * Execute a task. Receives the task payload and the worktree directory.
   * Must return the result string (stdout / output).
   * Throw an Error to signal task failure.
   */
  protected abstract execute(task: TaskPayload, workdir: string): Promise<string>;

  // ── Public entry point ────────────────────────────────────────────────────

  async start(): Promise<void> {
    this.setupSignalHandlers();

    log('info', `${this.toolName}-bridge starting`, {
      coreUrl: this.coreUrl,
      pollIntervalMs: this.pollIntervalMs,
    });

    this.agentId = await this.register();
    log('info', `${this.toolName}-bridge registered`, { agentId: this.agentId });

    await this.pollLoop();
  }

  // ── Registration ──────────────────────────────────────────────────────────

  private async register(): Promise<string> {
    const name = `${this.toolName}-bridge`;
    const body = JSON.stringify({
      templateRef: 'tool-bridge',
      name,
      displayName: this.displayName,
      lifecycleMode: 'persistent',
      start: false,
    });

    const res = await this.fetch('/api/agents/instances', {
      method: 'POST',
      headers: { 'Content-Type': 'application/json' },
      body,
    });

    if (res.status === 201) {
      const data = (await res.json()) as { id: string };
      return data.id;
    }

    if (res.status === 409) {
      // Already registered — look up by name
      log('info', `${name} already registered, fetching existing id`);
      return this.fetchInstanceIdByName(name);
    }

    const text = await res.text();
    throw new Error(`Registration failed: HTTP ${res.status} — ${text}`);
  }

  private async fetchInstanceIdByName(name: string): Promise<string> {
    const res = await this.fetch(`/api/agents/instances?name=${encodeURIComponent(name)}`);
    if (!res.ok) {
      throw new Error(`Failed to fetch instance list: HTTP ${res.status}`);
    }
    const data = (await res.json()) as Array<{ id: string; name: string }>;
    const match = data.find((inst) => inst.name === name);
    if (!match) {
      throw new Error(`Instance "${name}" not found after 409`);
    }
    return match.id;
  }

  // ── Poll loop ─────────────────────────────────────────────────────────────

  private async pollLoop(): Promise<void> {
    while (!this.shutdownRequested) {
      await this.pollOnce();

      const interval = this.effectiveInterval();
      await sleep(interval);
    }

    log('info', `${this.toolName}-bridge shut down cleanly`);
  }

  private async pollOnce(): Promise<void> {
    const agentId = this.agentId!;

    let res: Response;
    try {
      res = await this.fetch(`/api/agents/${agentId}/tasks/next`);
    } catch (err) {
      this.consecutiveErrors++;
      log('warn', 'Poll request failed', {
        agentId,
        consecutiveErrors: this.consecutiveErrors,
        error: String(err),
      });
      return;
    }

    if (res.status === 204) {
      // Empty queue — normal, reset errors
      this.consecutiveErrors = 0;
      return;
    }

    if (res.status === 409) {
      // Task already running server-side — wait for it
      this.consecutiveErrors = 0;
      log('debug', 'Server reports task already running, waiting');
      return;
    }

    if (res.status === 404) {
      // Agent not found — re-register
      log('warn', 'Agent not found (404), re-registering');
      this.consecutiveErrors++;
      try {
        this.agentId = await this.register();
      } catch (regErr) {
        log('error', 'Re-registration failed', { error: String(regErr) });
      }
      return;
    }

    if (!res.ok) {
      this.consecutiveErrors++;
      log('warn', `Unexpected poll status ${res.status}`, {
        consecutiveErrors: this.consecutiveErrors,
      });
      return;
    }

    // 200 — got a task
    this.consecutiveErrors = 0;
    const task = (await res.json()) as TaskPayload;
    await this.handleTask(task);
  }

  private effectiveInterval(): number {
    if (this.consecutiveErrors >= 5) {
      const doubled = this.pollIntervalMs * Math.pow(2, this.consecutiveErrors - 4);
      return Math.min(doubled, 30_000);
    }
    return this.pollIntervalMs;
  }

  // ── Task execution ────────────────────────────────────────────────────────

  private async handleTask(task: TaskPayload): Promise<void> {
    const { taskId } = task;
    log('info', 'Task received', { taskId, tool: task.context?.tool });

    // Idempotency check
    if (this.processedIds.has(taskId)) {
      log('warn', 'Duplicate task detected, reporting cached result', { taskId });
      await this.completeTask(taskId, null, 'Duplicate task — already processed');
      return;
    }

    this.taskInProgress = true;
    const worktree = join('/tmp', 'bridge-tasks', taskId);

    try {
      const workdir = this.setupWorktree(task, worktree);

      let result: string;
      try {
        result = await this.withTimeout(this.execute(task, workdir), this.taskTimeoutMs, taskId);
      } catch (execErr) {
        const errMsg = execErr instanceof Error ? execErr.message : String(execErr);
        log('error', 'Task execution failed', { taskId, error: errMsg });
        await this.completeTask(taskId, null, errMsg);
        return;
      } finally {
        this.cleanupWorktree(task, worktree);
      }

      this.markProcessed(taskId);
      await this.completeTask(taskId, result, null);
    } finally {
      this.taskInProgress = false;
    }
  }

  private async completeTask(
    taskId: string,
    result: string | null,
    error: string | null
  ): Promise<void> {
    const agentId = this.agentId!;
    const body: Record<string, unknown> = {};

    if (error) {
      body['error'] = error;
      body['exitReason'] = 'error';
    } else {
      body['result'] = result;
      body['exitReason'] = 'success';
    }

    try {
      const res = await this.fetch(`/api/agents/${agentId}/tasks/${taskId}/complete`, {
        method: 'POST',
        headers: { 'Content-Type': 'application/json' },
        body: JSON.stringify(body),
      });

      if (!res.ok) {
        const text = await res.text();
        log('warn', `Task complete call returned ${res.status}`, { taskId, body: text });
      } else {
        log('info', 'Task completed', { taskId, success: !error });
      }
    } catch (err) {
      log('error', 'Failed to report task completion', { taskId, error: String(err) });
    }
  }

  // ── Workspace isolation ───────────────────────────────────────────────────

  private setupWorktree(task: TaskPayload, worktree: string): string {
    const repo = task.context?.repo ?? this.repoRoot;
    const scratchDir = join('/tmp', 'bridge-tasks', task.taskId);

    // If there's no repo context, just use a scratch dir
    if (!existsSync(repo)) {
      mkdirSync(scratchDir, { recursive: true });
      log('debug', 'Using scratch dir (no repo)', { taskId: task.taskId, dir: scratchDir });
      return scratchDir;
    }

    try {
      mkdirSync(join('/tmp', 'bridge-tasks'), { recursive: true });
      execFileSync('git', ['worktree', 'add', worktree, '-b', `bridge/${task.taskId}`], {
        cwd: repo,
        stdio: 'pipe',
      });
      log('debug', 'Created git worktree', { taskId: task.taskId, worktree });
      return worktree;
    } catch (err) {
      // Fallback to scratch dir if worktree creation fails
      log('warn', 'Worktree creation failed, using scratch dir', {
        taskId: task.taskId,
        error: String(err),
      });
      mkdirSync(scratchDir, { recursive: true });
      return scratchDir;
    }
  }

  private cleanupWorktree(task: TaskPayload, worktree: string): void {
    const repo = task.context?.repo ?? this.repoRoot;

    if (!existsSync(worktree)) return;
    if (!existsSync(repo)) {
      // Just a scratch dir
      try {
        execFileSync('rm', ['-rf', worktree], { stdio: 'pipe' });
      } catch {
        /* best-effort */
      }
      return;
    }

    try {
      execFileSync('git', ['worktree', 'remove', worktree, '--force'], {
        cwd: repo,
        stdio: 'pipe',
      });
      execFileSync('git', ['branch', '-D', `bridge/${task.taskId}`], {
        cwd: repo,
        stdio: 'pipe',
      });
      log('debug', 'Cleaned up git worktree', { taskId: task.taskId });
    } catch (err) {
      log('warn', 'Worktree cleanup failed', { taskId: task.taskId, error: String(err) });
    }
  }

  // ── Idempotency ───────────────────────────────────────────────────────────

  private loadProcessedLog(): void {
    if (!existsSync(this.processedLogPath)) return;
    try {
      const lines = readFileSync(this.processedLogPath, 'utf-8').trim().split('\n');
      // Keep last 1000
      const recent = lines.slice(-1000);
      for (const line of recent) {
        const trimmed = line.trim();
        if (trimmed) this.processedIds.add(trimmed);
      }
    } catch {
      /* ignore corrupt log */
    }
  }

  private markProcessed(taskId: string): void {
    this.processedIds.add(taskId);

    // Evict oldest if over 1000
    if (this.processedIds.size > 1000) {
      const first = this.processedIds.values().next().value;
      if (first !== undefined) this.processedIds.delete(first);
    }

    // Append to persistent log
    try {
      writeFileSync(this.processedLogPath, taskId + '\n', { flag: 'a' });
    } catch {
      /* best-effort */
    }
  }

  // ── Timeout helper ────────────────────────────────────────────────────────

  private withTimeout<T>(promise: Promise<T>, ms: number, taskId: string): Promise<T> {
    return new Promise<T>((resolve, reject) => {
      const timer = setTimeout(() => {
        reject(new Error(`Task ${taskId} timed out after ${ms}ms`));
      }, ms);

      promise.then(
        (val) => {
          clearTimeout(timer);
          resolve(val);
        },
        (err: unknown) => {
          clearTimeout(timer);
          reject(err);
        }
      );
    });
  }

  // ── HTTP helper ───────────────────────────────────────────────────────────

  protected fetch(path: string, init?: RequestInit): Promise<Response> {
    const url = `${this.coreUrl}${path}`;
    const headers: Record<string, string> = {
      Authorization: `Bearer ${this.apiKey}`,
      ...(init?.headers as Record<string, string> | undefined),
    };
    return globalThis.fetch(url, { ...init, headers });
  }

  // ── Signal handlers ───────────────────────────────────────────────────────

  private setupSignalHandlers(): void {
    const handler = async (signal: string) => {
      log('info', `Received ${signal}, initiating graceful shutdown`);
      this.shutdownRequested = true;

      if (this.taskInProgress) {
        log('info', 'Waiting for current task to complete before exit');
        // Poll until task finishes (max 35 min to give task headroom)
        const deadline = Date.now() + this.taskTimeoutMs + 5 * 60_000;
        while (this.taskInProgress && Date.now() < deadline) {
          await sleep(1000);
        }
      }

      process.exit(0);
    };

    process.once('SIGTERM', () => void handler('SIGTERM'));
    process.once('SIGINT', () => void handler('SIGINT'));
  }
}

// ── Utilities ─────────────────────────────────────────────────────────────────

function sleep(ms: number): Promise<void> {
  return new Promise((resolve) => setTimeout(resolve, ms));
}

// Re-export spawn for subclass use
export { spawn };

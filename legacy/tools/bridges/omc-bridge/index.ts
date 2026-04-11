/**
 * OMC Bridge — invokes the `claude` CLI for each task received from sera.
 *
 * Extends BridgeBase. The execute() method:
 *   1. Writes the task prompt to a temp file in the worktree
 *   2. Spawns `claude -p <prompt> --output-format json`
 *   3. Returns captured stdout as the result string
 *
 * Spec: docs/BRIDGE-AGENT-SPEC.md
 */

import { writeFileSync, mkdirSync } from 'node:fs';
import { join } from 'node:path';
import { spawn } from 'node:child_process';
import { BridgeBase } from '../shared/bridge-base.ts';

// ── Task payload (subset we care about) ──────────────────────────────────────

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

// ── OMC Bridge ────────────────────────────────────────────────────────────────

class OmcBridge extends BridgeBase {
  constructor() {
    super({
      toolName: 'omc',
      displayName: 'OMC Bridge (host)',
    });
  }

  protected async execute(task: TaskPayload, workdir: string): Promise<string> {
    // Write prompt to a file so we avoid shell quoting issues
    const promptPath = join(workdir, `prompt-${task.taskId}.md`);
    mkdirSync(workdir, { recursive: true });
    writeFileSync(promptPath, task.task, 'utf-8');

    return new Promise<string>((resolve, reject) => {
      const stdout: Buffer[] = [];
      const stderr: Buffer[] = [];

      const child = spawn(
        'claude',
        ['-p', task.task, '--output-format', 'json'],
        {
          cwd: workdir,
          stdio: ['ignore', 'pipe', 'pipe'],
          env: { ...process.env },
        }
      );

      child.stdout.on('data', (chunk: Buffer) => stdout.push(chunk));
      child.stderr.on('data', (chunk: Buffer) => stderr.push(chunk));

      child.on('error', (err) => {
        reject(new Error(`Failed to spawn claude CLI: ${err.message}`));
      });

      child.on('close', (code) => {
        const out = Buffer.concat(stdout).toString('utf-8').trim();
        const errOut = Buffer.concat(stderr).toString('utf-8').trim();

        if (code !== 0) {
          const detail = errOut || out || `exit code ${code}`;
          reject(new Error(`claude CLI exited with code ${code}: ${detail}`));
          return;
        }

        // Return stdout — may be JSON (--output-format json) or plain text
        resolve(out);
      });
    });
  }
}

// ── Entrypoint ────────────────────────────────────────────────────────────────

const bridge = new OmcBridge();
bridge.start().catch((err: unknown) => {
  console.error(JSON.stringify({ ts: new Date().toISOString(), level: 'error', msg: String(err) }));
  process.exit(1);
});

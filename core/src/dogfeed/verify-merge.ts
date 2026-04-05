/**
 * VerifyMerge — git operations, CI verification, and merge for dogfeed cycles.
 *
 * The coding agent runs in Docker with a fresh clone. This module handles
 * the host-side git operations: create branch, pull agent changes, run CI,
 * merge to main, and cleanup.
 */

import { spawn } from 'node:child_process';
import { simpleGit, type SimpleGit } from 'simple-git';
import { Logger } from '../lib/logger.js';
import type { DogfeedConfig } from './types.js';
import { CI_COMMANDS, MAX_CI_OUTPUT_LENGTH } from './constants.js';

const logger = new Logger('VerifyMerge');

export interface CIResult {
  passed: boolean;
  output: string;
  failedStep?: string;
  durationMs: number;
}

export interface CommitStats {
  filesChanged: number;
  insertions: number;
  deletions: number;
}

export class VerifyMerge {
  private git: SimpleGit;

  constructor(private readonly config: DogfeedConfig) {
    this.git = simpleGit(config.repoRoot);
  }

  /**
   * Create a dogfeed branch from the current HEAD and push it.
   * The Docker container will clone and checkout this branch.
   */
  async createBranch(branchName: string): Promise<void> {
    // Create branch from current HEAD (we're on whatever branch, e.g. feat/...)
    await this.git.checkoutLocalBranch(branchName);
    // Push so the Docker container can clone it
    await this.git.push('origin', branchName, ['--set-upstream']);
    // Switch back to original branch
    await this.git.checkout('-');
    logger.info(`Created and pushed branch: ${branchName}`);
  }

  /**
   * Pull the branch after the Docker container has pushed changes.
   * Returns commit stats (how many files/lines changed).
   */
  async pullBranch(branchName: string): Promise<CommitStats> {
    await this.git.fetch('origin', branchName);

    // Compare the branch tip with its merge base to count changes
    const diffOutput = await this.git.diff([
      '--stat',
      '--numstat',
      `origin/${branchName}~1..origin/${branchName}`,
    ]);

    // Parse numstat output: "added\tremoved\tfilename"
    let filesChanged = 0;
    let insertions = 0;
    let deletions = 0;

    for (const line of diffOutput.split('\n')) {
      const match = /^(\d+)\t(\d+)\t/.exec(line);
      if (match) {
        filesChanged++;
        insertions += parseInt(match[1]!, 10);
        deletions += parseInt(match[2]!, 10);
      }
    }

    logger.info(`Branch ${branchName}: ${filesChanged} files, +${insertions}/-${deletions}`);
    return { filesChanged, insertions, deletions };
  }

  /**
   * Run the full CI suite (typecheck + lint + test) in the repo root.
   */
  async runCI(): Promise<CIResult> {
    const cwd = this.config.repoRoot;
    const startTime = Date.now();
    let combinedOutput = '';

    for (const step of CI_COMMANDS) {
      logger.info(`Running CI step: ${step.name}`);

      const result = await this.runCommand(step.cmd, step.args, cwd);
      combinedOutput += `\n=== ${step.name} (exit ${result.exitCode}) ===\n${result.output}\n`;

      if (result.exitCode !== 0) {
        logger.error(`CI step failed: ${step.name}`);
        return {
          passed: false,
          output: truncate(combinedOutput, MAX_CI_OUTPUT_LENGTH),
          failedStep: step.name,
          durationMs: Date.now() - startTime,
        };
      }

      logger.info(`CI step passed: ${step.name}`);
    }

    return {
      passed: true,
      output: truncate(combinedOutput, MAX_CI_OUTPUT_LENGTH),
      durationMs: Date.now() - startTime,
    };
  }

  /**
   * Merge a dogfeed branch into the current working tree.
   * Stashes uncommitted changes, merges, then unstashes.
   */
  async mergeToMain(branchName: string): Promise<void> {
    logger.info(`Merging origin/${branchName} into working tree`);

    const stashResult = await this.git.stash(['push', '-m', 'dogfeed-pre-merge']);
    const didStash = !stashResult.includes('No local changes');

    try {
      await this.git.merge([
        '--no-ff',
        `origin/${branchName}`,
        '-m',
        `dogfeed: merge ${branchName}`,
      ]);
    } catch (err) {
      await this.git.merge(['--abort']).catch(() => {});
      if (didStash) await this.git.stash(['pop']).catch(() => {});
      throw err;
    }

    if (didStash) {
      await this.git.stash(['pop']).catch(() => {
        logger.warn('Could not pop stash after merge — may need manual resolution');
      });
    }
  }

  /**
   * Revert the last merge commit (used when CI fails after merge).
   */
  async revertLastMerge(): Promise<void> {
    logger.info('Reverting last merge commit');
    try {
      await this.git.revert('HEAD', ['--mainline', '1', '--no-edit']);
    } catch {
      logger.warn('Revert failed, resetting to HEAD~1');
      await this.git.reset(['--hard', 'HEAD~1']);
    }
  }

  /**
   * Push current branch to remote.
   */
  async pushMain(): Promise<void> {
    if (!this.config.pushToRemote) return;
    await this.git.push();
    logger.info('Pushed to remote');
  }

  /**
   * Clean up a dogfeed branch (local and remote).
   */
  async cleanup(branchName: string): Promise<void> {
    logger.info(`Cleaning up branch: ${branchName}`);

    try {
      await this.git.deleteLocalBranch(branchName, true);
    } catch {
      logger.warn(`Could not delete local branch: ${branchName}`);
    }

    if (this.config.pushToRemote) {
      try {
        await this.git.push('origin', `:${branchName}`);
      } catch {
        logger.warn(`Could not delete remote branch: ${branchName}`);
      }
    }
  }

  private runCommand(
    cmd: string,
    args: readonly string[],
    cwd: string
  ): Promise<{ exitCode: number; output: string }> {
    return new Promise((resolve) => {
      let output = '';

      const proc = spawn(cmd, [...args], { cwd, stdio: ['ignore', 'pipe', 'pipe'] });

      proc.stdout?.on('data', (data: Buffer) => {
        output += data.toString();
      });
      proc.stderr?.on('data', (data: Buffer) => {
        output += data.toString();
      });

      proc.on('error', (err) => {
        resolve({ exitCode: 1, output: output || err.message });
      });

      proc.on('close', (code) => {
        resolve({ exitCode: code ?? 1, output });
      });
    });
  }
}

function truncate(str: string, maxLen: number): string {
  if (str.length <= maxLen) return str;
  return str.substring(0, maxLen) + '\n... [truncated]';
}

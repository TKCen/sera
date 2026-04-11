/**
 * WorktreeManager — git worktree isolation for concurrent coding agents.
 * Story 3.4
 *
 * Each coding agent gets a dedicated git worktree so multiple agents can
 * work on the same repository concurrently without interfering.
 */

import { execSync } from 'child_process';
import fs from 'fs';
import path from 'path';
import { Logger } from '../lib/logger.js';

const logger = new Logger('WorktreeManager');

export class WorktreeManager {
  /**
   * Create a git worktree for an agent task.
   * Creates `.worktrees/{agentName}-{taskId}` with branch `agent/{agentName}/{taskId}`.
   * Returns the absolute path to the worktree.
   */
  static create(repoPath: string, agentName: string, taskId: string): string {
    const worktreeDirName = `${agentName}-${taskId}`;
    const worktreePath = path.join(repoPath, '.worktrees', worktreeDirName);
    const branchName = `agent/${agentName}/${taskId}`;

    logger.info(`Creating worktree: path=${worktreePath}, branch=${branchName}`);
    fs.mkdirSync(path.join(repoPath, '.worktrees'), { recursive: true });

    try {
      execSync(`git worktree add "${worktreePath}" -b "${branchName}"`, {
        cwd: repoPath,
        stdio: 'pipe',
      });
    } catch (err) {
      logger.error('Failed to create worktree:', err);
      throw err;
    }

    return worktreePath;
  }

  /**
   * Remove the worktree for a completed agent task.
   */
  static remove(repoPath: string, agentName: string, taskId: string): void {
    const worktreeDirName = `${agentName}-${taskId}`;
    const worktreePath = path.join(repoPath, '.worktrees', worktreeDirName);

    logger.info(`Removing worktree: path=${worktreePath}`);
    try {
      execSync(`git worktree remove "${worktreePath}" --force`, { cwd: repoPath, stdio: 'pipe' });
    } catch (err) {
      logger.warn('Failed to remove worktree via git, cleaning up manually:', err);
      if (fs.existsSync(worktreePath)) {
        fs.rmSync(worktreePath, { recursive: true, force: true });
      }
      // Also prune the worktree administrative files
      try {
        execSync('git worktree prune', { cwd: repoPath, stdio: 'pipe' });
      } catch {
        // Best effort
      }
    }
  }

  /**
   * Return the diff between the worktree branch and the base branch (HEAD at worktree creation).
   */
  static diff(repoPath: string, agentName: string, taskId: string): string {
    const worktreePath = path.join(repoPath, '.worktrees', `${agentName}-${taskId}`);
    try {
      return execSync('git diff HEAD', {
        cwd: worktreePath,
        encoding: 'utf-8',
      });
    } catch (err) {
      logger.error('Failed to get worktree diff:', err);
      return '';
    }
  }

  /**
   * Merge the worktree branch into the target branch (default: main).
   * Must be called after the agent container has stopped.
   */
  static merge(repoPath: string, agentName: string, taskId: string, targetBranch = 'main'): void {
    const branchName = `agent/${agentName}/${taskId}`;
    logger.info(`Merging ${branchName} into ${targetBranch}`);
    execSync(
      `git checkout "${targetBranch}" && git merge --no-ff "${branchName}" -m "Merge agent branch ${branchName}"`,
      { cwd: repoPath, stdio: 'pipe', shell: process.platform === 'win32' ? 'cmd.exe' : '/bin/sh' }
    );
  }

  /**
   * Get the worktree path without creating it.
   */
  static getWorktreePath(repoPath: string, agentName: string, taskId: string): string {
    return path.join(repoPath, '.worktrees', `${agentName}-${taskId}`);
  }
}

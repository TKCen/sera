import { execFileSync } from 'child_process';
import { v4 as uuidv4 } from 'uuid';
import { ChannelRouter } from '../channels/ChannelRouter.js';
import type { ChannelSeverity } from '../channels/channel.interface.js';
import { Logger } from '../lib/logger.js';

const logger = new Logger('GitHubEventSource');

export interface GitHubEventSourceConfig {
  repos: string[];
  pollIntervalMs?: number;
  token?: string;
  eventTypes?: string[];
}

interface PullRequest {
  id: number;
  number: number;
  title: string;
  state: 'open' | 'closed';
  merged_at: string | null;
  html_url: string;
  user: { login: string };
}

interface WorkflowRun {
  id: number;
  name: string;
  status: string;
  conclusion: string | null;
  html_url: string;
  head_branch: string;
}

interface RepoCacheEntry {
  prs: Map<number, PullRequest>;
  runs: Map<number, WorkflowRun>;
}

const DEFAULT_POLL_INTERVAL_MS = 30_000;
const BACKOFF_CAP_MS = 5 * 60_000;

export class GitHubEventSource {
  private readonly repos: string[];
  private readonly defaultIntervalMs: number;
  private token: string;
  private readonly allowedEventTypes: string[];
  private timer: ReturnType<typeof setTimeout> | null = null;
  private running = false;
  private currentIntervalMs: number;
  private cache = new Map<string, RepoCacheEntry>();

  constructor(config: GitHubEventSourceConfig) {
    this.repos = config.repos;
    this.defaultIntervalMs = config.pollIntervalMs ?? DEFAULT_POLL_INTERVAL_MS;
    this.token = config.token ?? process.env['GITHUB_TOKEN'] ?? '';
    this.allowedEventTypes = config.eventTypes ?? [
      'github:pr_opened',
      'github:pr_merged',
      'github:pr_closed',
      'github:ci_passed',
      'github:ci_failed',
    ];
    this.currentIntervalMs = this.defaultIntervalMs;
  }

  start(): void {
    if (this.running) {
      logger.warn('GitHubEventSource already running');
      return;
    }
    if (!this.token) {
      this.token = this.resolveTokenFromCli();
    }
    if (!this.token) {
      logger.warn('No GitHub token available — GitHub event source will not start');
      return;
    }
    this.running = true;
    logger.info(`Starting GitHub event source for repos: ${this.repos.join(', ')}`);
    this.scheduleNext(0);
  }

  stop(): void {
    this.running = false;
    if (this.timer !== null) {
      clearTimeout(this.timer);
      this.timer = null;
    }
    logger.info('GitHub event source stopped');
  }

  private scheduleNext(delayMs: number): void {
    this.timer = setTimeout(() => {
      this.poll()
        .catch((err: unknown) => {
          logger.error('Unhandled error in poll:', err);
        })
        .finally(() => {
          if (this.running) {
            this.scheduleNext(this.currentIntervalMs);
          }
        });
    }, delayMs);
  }

  private async poll(): Promise<void> {
    for (const repo of this.repos) {
      await this.pollRepo(repo);
    }
  }

  private async pollRepo(repo: string): Promise<void> {
    const [prsResult, runsResult] = await Promise.allSettled([
      this.fetchPullRequests(repo),
      this.fetchWorkflowRuns(repo),
    ]);

    if (prsResult.status === 'rejected') {
      logger.warn(`Failed to fetch PRs for ${repo}:`, prsResult.reason);
    }
    if (runsResult.status === 'rejected') {
      logger.warn(`Failed to fetch workflow runs for ${repo}:`, runsResult.reason);
    }

    if (prsResult.status === 'fulfilled') {
      this.diffPrs(repo, prsResult.value);
    }
    if (runsResult.status === 'fulfilled') {
      this.diffRuns(repo, runsResult.value);
    }
  }

  private async fetchPullRequests(repo: string): Promise<PullRequest[]> {
    const url = `https://api.github.com/repos/${repo}/pulls?state=open&sort=updated&per_page=30`;
    const response = await this.githubFetch(url);
    if (!response.ok) {
      await this.handleErrorResponse(response);
      return [];
    }
    this.handleRateLimitHeaders(response);
    return response.json() as Promise<PullRequest[]>;
  }

  private async fetchWorkflowRuns(repo: string): Promise<WorkflowRun[]> {
    const url = `https://api.github.com/repos/${repo}/actions/runs?per_page=10`;
    const response = await this.githubFetch(url);
    if (!response.ok) {
      await this.handleErrorResponse(response);
      return [];
    }
    this.handleRateLimitHeaders(response);
    const body = (await response.json()) as { workflow_runs?: WorkflowRun[] };
    return body.workflow_runs ?? [];
  }

  private githubFetch(url: string): Promise<Response> {
    return fetch(url, {
      headers: {
        Authorization: `Bearer ${this.token}`,
        Accept: 'application/vnd.github+json',
        'X-GitHub-Api-Version': '2022-11-28',
      },
    });
  }

  private async handleErrorResponse(response: Response): Promise<void> {
    if (response.status === 403 || response.status === 429) {
      this.currentIntervalMs = Math.min(this.currentIntervalMs * 2, BACKOFF_CAP_MS);
      logger.warn(`Rate limited (${response.status}) — backing off to ${this.currentIntervalMs}ms`);
    } else {
      logger.warn(`GitHub API error: ${response.status} ${response.statusText}`);
    }
  }

  private handleRateLimitHeaders(response: Response): void {
    const remaining = response.headers.get('X-RateLimit-Remaining');
    if (remaining === null) return;
    const remainingNum = parseInt(remaining, 10);
    if (isNaN(remainingNum)) return;

    if (remainingNum < 10) {
      if (this.currentIntervalMs < 60_000) {
        this.currentIntervalMs = 60_000;
        logger.warn(`Rate limit low (${remainingNum} remaining) — slowing to 60s poll`);
      }
    } else if (remainingNum > 100 && this.currentIntervalMs > this.defaultIntervalMs) {
      this.currentIntervalMs = this.defaultIntervalMs;
      logger.info('Rate limit recovered — restoring default poll interval');
    }
  }

  private diffPrs(repo: string, currentPrs: PullRequest[]): void {
    let entry = this.cache.get(repo);
    if (!entry) {
      // First poll — seed the cache without emitting (avoid startup flood)
      entry = { prs: new Map(), runs: new Map() };
      for (const pr of currentPrs) {
        entry.prs.set(pr.id, pr);
      }
      this.cache.set(repo, entry);
      return;
    }

    const cached = entry.prs;
    const currentIds = new Set(currentPrs.map((pr) => pr.id));

    for (const pr of currentPrs) {
      if (!cached.has(pr.id)) {
        this.emit('github:pr_opened', {
          title: `PR #${pr.number}: ${pr.title}`,
          body: `${pr.user.login} opened PR in ${repo}`,
          severity: 'info',
          metadata: { repo, prNumber: pr.number, author: pr.user.login, url: pr.html_url },
        });
      }
      cached.set(pr.id, pr);
    }

    // Detect PRs that left the open list (closed or merged)
    for (const [id, prev] of cached) {
      if (!currentIds.has(id) && prev.state === 'open') {
        cached.delete(id);
        this.fetchAndEmitClosedPr(repo, prev);
      }
    }
  }

  private fetchAndEmitClosedPr(repo: string, prev: PullRequest): void {
    const url = `https://api.github.com/repos/${repo}/pulls/${prev.number}`;
    this.githubFetch(url)
      .then(async (response) => {
        if (!response.ok) return;
        const pr = (await response.json()) as PullRequest;
        if (pr.merged_at !== null) {
          this.emit('github:pr_merged', {
            title: `PR #${pr.number}: ${pr.title}`,
            body: `${pr.user.login} merged PR in ${repo}`,
            severity: 'info',
            metadata: {
              repo,
              prNumber: pr.number,
              author: pr.user.login,
              url: pr.html_url,
              mergedAt: pr.merged_at,
            },
          });
        } else {
          this.emit('github:pr_closed', {
            title: `PR #${pr.number}: ${pr.title}`,
            body: `PR #${pr.number} was closed without merging in ${repo}`,
            severity: 'info',
            metadata: { repo, prNumber: pr.number, author: pr.user.login, url: pr.html_url },
          });
        }
      })
      .catch((err: unknown) => {
        logger.warn(`Failed to fetch closed PR #${prev.number} for ${repo}:`, err);
      });
  }

  private diffRuns(repo: string, currentRuns: WorkflowRun[]): void {
    let entry = this.cache.get(repo);
    if (!entry) {
      entry = { prs: new Map(), runs: new Map() };
      this.cache.set(repo, entry);
    }

    const cached = entry.runs;
    const isFirstPoll = cached.size === 0 && currentRuns.length > 0;

    for (const run of currentRuns) {
      const prev = cached.get(run.id);

      if (prev === undefined) {
        // New run seen — seed without emitting on first poll
        cached.set(run.id, run);
        continue;
      }

      const wasInProgress = prev.status === 'in_progress';
      const nowCompleted = run.status === 'completed';

      if (!isFirstPoll && wasInProgress && nowCompleted) {
        if (run.conclusion === 'success') {
          this.emit('github:ci_passed', {
            title: `CI passed: ${run.name} on ${run.head_branch}`,
            body: `Workflow run completed successfully in ${repo}`,
            severity: 'info',
            metadata: {
              repo,
              runId: run.id,
              workflow: run.name,
              branch: run.head_branch,
              url: run.html_url,
            },
          });
        } else if (run.conclusion === 'failure') {
          this.emit('github:ci_failed', {
            title: `CI failed: ${run.name} on ${run.head_branch}`,
            body: `Workflow run failed in ${repo}`,
            severity: 'warning',
            metadata: {
              repo,
              runId: run.id,
              workflow: run.name,
              branch: run.head_branch,
              url: run.html_url,
            },
          });
        }
      }

      cached.set(run.id, run);
    }
  }

  private emit(
    eventType: string,
    payload: {
      title: string;
      body: string;
      severity: ChannelSeverity;
      metadata: Record<string, unknown>;
    }
  ): void {
    if (!this.allowedEventTypes.includes(eventType)) return;
    ChannelRouter.getInstance().route({
      id: uuidv4(),
      eventType,
      title: payload.title,
      body: payload.body,
      severity: payload.severity,
      metadata: payload.metadata,
      timestamp: new Date().toISOString(),
    });
    logger.info(`Emitted ${eventType}: ${payload.title}`);
  }

  private resolveTokenFromCli(): string {
    try {
      const token = execFileSync('gh', ['auth', 'token'], {
        encoding: 'utf8',
        timeout: 5000,
      }).trim();
      if (token) {
        logger.info('Resolved GitHub token via gh CLI');
      }
      return token;
    } catch {
      logger.debug('gh CLI not available or not authenticated');
      return '';
    }
  }
}

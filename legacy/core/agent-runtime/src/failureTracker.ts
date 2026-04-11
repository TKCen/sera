/**
 * FailureTracker — tracks repeated tool failures within a reasoning session.
 *
 * When a tool fails repeatedly, the agent needs a structured signal to consider
 * alternative approaches. This module tracks failure counts by tool name and
 * generates context injection strings when thresholds are exceeded.
 */

// ── Constants ─────────────────────────────────────────────────────────────────

/** Number of failures before a tool appears in the injected warning. */
const DEFAULT_THRESHOLD = 3;

/** Maximum number of failing tools to show in the injected context. */
const DEFAULT_TOP_N = 3;

// ── FailureTracker ────────────────────────────────────────────────────────────

export class FailureTracker {
  private counts: Map<string, number> = new Map();
  private readonly threshold: number;
  private readonly topN: number;

  constructor(threshold = DEFAULT_THRESHOLD, topN = DEFAULT_TOP_N) {
    this.threshold = threshold;
    this.topN = topN;
  }

  /** Record a failure for the given tool. Returns the new failure count. */
  recordFailure(toolName: string): number {
    const prev = this.counts.get(toolName) ?? 0;
    const next = prev + 1;
    this.counts.set(toolName, next);
    return next;
  }

  /** Reset the failure count for a tool (called on success). */
  recordSuccess(toolName: string): void {
    this.counts.delete(toolName);
  }

  /** Returns true if any tool has reached the failure threshold. */
  hasThresholdExceeded(): boolean {
    for (const count of this.counts.values()) {
      if (count >= this.threshold) return true;
    }
    return false;
  }

  /**
   * Returns the top-N failing tools (by count, descending) that have reached
   * the threshold. Returns an empty array if no tool has hit the threshold.
   */
  getTopFailures(): Array<{ toolName: string; count: number }> {
    const failing: Array<{ toolName: string; count: number }> = [];
    for (const [toolName, count] of this.counts.entries()) {
      if (count >= this.threshold) {
        failing.push({ toolName, count });
      }
    }
    failing.sort((a, b) => b.count - a.count);
    return failing.slice(0, this.topN);
  }

  /**
   * Build the context injection string to prepend to the next iteration.
   * Returns null if no tool has reached the threshold.
   */
  buildContextString(): string | null {
    const failures = this.getTopFailures();
    if (failures.length === 0) return null;

    const parts = failures.map(
      ({ toolName, count }) =>
        `${toolName} has failed ${count} times this session. Consider an alternative approach.`
    );
    return `⚠️ Recent issues: ${parts.join(' ')}`;
  }
}

/**
 * ToolLoopDetector — semantic tool loop detection beyond exact duplicates.
 *
 * Detects three loop patterns:
 * 1. Consecutive: Same tool called N+ times in a row
 * 2. Oscillation: Two tools alternating (A→B→A→B) for N+ cycles
 * 3. Similarity: Same tool called with very similar args (Jaccard > threshold)
 *
 * When a loop is detected, the detector provides an intervention description
 * and after enough warnings, recommends forcing a text-only response.
 */

// ── Types ────────────────────────────────────────────────────────────────────

export interface ToolLoopDetectorConfig {
  /** Number of consecutive same-tool calls to trigger detection (default: 3) */
  consecutiveThreshold: number;
  /** Number of full A-B cycles to trigger oscillation detection (default: 3) */
  oscillationThreshold: number;
  /** Jaccard similarity threshold for near-duplicate detection (default: 0.8) */
  similarityThreshold: number;
  /** Tools that legitimately repeat and should be exempt from consecutive detection */
  exemptTools: Set<string>;
  /** Number of warnings before forcing text response (default: 2) */
  maxWarnings: number;
}

export type LoopKind = 'consecutive' | 'oscillation' | 'similarity';

export interface LoopDetectionVerdict {
  detected: boolean;
  kind?: LoopKind;
  description?: string;
}

interface ToolCallRecord {
  name: string;
  args: Record<string, unknown>;
  flatArgs: Set<string>;
}

// ── Defaults ─────────────────────────────────────────────────────────────────

const DEFAULT_CONFIG: ToolLoopDetectorConfig = {
  consecutiveThreshold: 3,
  oscillationThreshold: 3,
  similarityThreshold: 0.8,
  exemptTools: new Set(['shell-exec']),
  maxWarnings: 2,
};

/** Maximum number of flattened entries to prevent performance issues with large args */
const MAX_FLAT_ENTRIES = 100;

// ── ToolLoopDetector ─────────────────────────────────────────────────────────

export class ToolLoopDetector {
  private history: ToolCallRecord[] = [];
  private warningCount = 0;
  private config: ToolLoopDetectorConfig;

  constructor(config?: Partial<ToolLoopDetectorConfig>) {
    this.config = { ...DEFAULT_CONFIG, ...config };
    if (config?.exemptTools) {
      this.config.exemptTools = config.exemptTools;
    }
  }

  /**
   * Record a tool call and check for loop patterns.
   * Returns a verdict indicating whether a loop was detected.
   */
  record(toolName: string, args: Record<string, unknown>): LoopDetectionVerdict {
    const flatArgs = flattenToSet(args);
    this.history.push({ name: toolName, args, flatArgs });

    // Check each detection type in order of specificity
    const similarity = this.checkSimilarity(toolName, flatArgs);
    if (similarity.detected) return similarity;

    const consecutive = this.checkConsecutive(toolName);
    if (consecutive.detected) return consecutive;

    const oscillation = this.checkOscillation();
    if (oscillation.detected) return oscillation;

    return { detected: false };
  }

  /** Whether enough warnings have been issued to force a text-only response. */
  shouldForceTextResponse(): boolean {
    return this.warningCount >= this.config.maxWarnings;
  }

  /** Acknowledge that a warning was shown to the agent. */
  acknowledgeWarning(): void {
    this.warningCount++;
  }

  /** Reset all state. */
  reset(): void {
    this.history = [];
    this.warningCount = 0;
  }

  // ── Detection algorithms ─────────────────────────────────────────────────

  private checkConsecutive(toolName: string): LoopDetectionVerdict {
    if (this.config.exemptTools.has(toolName)) {
      return { detected: false };
    }

    const threshold = this.config.consecutiveThreshold;
    if (this.history.length < threshold) {
      return { detected: false };
    }

    // Check if the last N entries all have the same tool name
    const tail = this.history.slice(-threshold);
    const allSame = tail.every((r) => r.name === toolName);

    if (allSame) {
      return {
        detected: true,
        kind: 'consecutive',
        description: `Tool "${toolName}" called ${threshold} times consecutively. Step back and try a different approach.`,
      };
    }

    return { detected: false };
  }

  private checkOscillation(): LoopDetectionVerdict {
    const cycles = this.config.oscillationThreshold;
    const requiredLength = cycles * 2;

    if (this.history.length < requiredLength) {
      return { detected: false };
    }

    const tail = this.history.slice(-requiredLength);
    const toolA = tail[0]!.name;
    const toolB = tail[1]!.name;

    if (toolA === toolB) {
      return { detected: false };
    }

    // Check alternating pattern A,B,A,B,...
    const isOscillating = tail.every((r, i) =>
      i % 2 === 0 ? r.name === toolA : r.name === toolB
    );

    if (isOscillating) {
      return {
        detected: true,
        kind: 'oscillation',
        description: `Tools "${toolA}" and "${toolB}" oscillating for ${cycles} cycles. Break the pattern and try a different approach.`,
      };
    }

    return { detected: false };
  }

  private checkSimilarity(toolName: string, currentFlat: Set<string>): LoopDetectionVerdict {
    if (this.config.exemptTools.has(toolName)) {
      return { detected: false };
    }

    const threshold = this.config.consecutiveThreshold;
    if (this.history.length < threshold) {
      return { detected: false };
    }

    // Check if the last N entries (including current, which is already pushed) are the same tool
    const tail = this.history.slice(-threshold);
    if (!tail.every((r) => r.name === toolName)) {
      return { detected: false };
    }

    // Compare current args with args from the oldest entry in the window
    const oldest = tail[0]!;
    const similarity = jaccardSimilarity(oldest.flatArgs, currentFlat);

    if (similarity >= this.config.similarityThreshold) {
      return {
        detected: true,
        kind: 'similarity',
        description: `Tool "${toolName}" called ${threshold} times with ${(similarity * 100).toFixed(0)}% similar arguments. You appear to be in a loop — reconsider your approach.`,
      };
    }

    return { detected: false };
  }
}

// ── Helpers ──────────────────────────────────────────────────────────────────

/**
 * Flatten an object into a set of "key=value" strings for Jaccard comparison.
 * Nested objects are flattened with dot-path keys.
 */
export function flattenToSet(
  obj: Record<string, unknown>,
  prefix = '',
): Set<string> {
  const entries = new Set<string>();

  for (const [key, value] of Object.entries(obj)) {
    if (entries.size >= MAX_FLAT_ENTRIES) break;

    const fullKey = prefix ? `${prefix}.${key}` : key;

    if (value !== null && typeof value === 'object' && !Array.isArray(value)) {
      const nested = flattenToSet(value as Record<string, unknown>, fullKey);
      for (const entry of nested) {
        if (entries.size >= MAX_FLAT_ENTRIES) break;
        entries.add(entry);
      }
    } else {
      entries.add(`${fullKey}=${String(value)}`);
    }
  }

  return entries;
}

/**
 * Compute Jaccard similarity between two sets: |intersection| / |union|.
 * Returns 1.0 for two empty sets (both represent no arguments).
 */
export function jaccardSimilarity(a: Set<string>, b: Set<string>): number {
  if (a.size === 0 && b.size === 0) return 1.0;

  let intersection = 0;
  for (const item of a) {
    if (b.has(item)) intersection++;
  }

  const union = a.size + b.size - intersection;
  return union === 0 ? 1.0 : intersection / union;
}

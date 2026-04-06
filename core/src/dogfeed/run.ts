#!/usr/bin/env bun
/**
 * Standalone dogfeed cycle runner.
 *
 * Usage:
 *   bun run core/src/dogfeed/run.ts                    # run one cycle
 *   bun run core/src/dogfeed/run.ts --dry-run           # preview next task without executing
 *   bun run core/src/dogfeed/run.ts --no-push --no-merge # local only, no git push/merge
 *
 * Since sera-core runs on Rust (sera-core-rs), the TS dogfeed loop runs standalone.
 * Phase 1: port to Rust as native SERA orchestration.
 */

import { DogfeedLoop } from './loop.js';
import { DogfeedAnalyzer } from './analyzer.js';
import { createDefaultConfig } from './constants.js';

const args = process.argv.slice(2);
const dryRun = args.includes('--dry-run');
const noPush = args.includes('--no-push');
const noMerge = args.includes('--no-merge');

const config = createDefaultConfig({
  pushToRemote: !noPush,
  autoMerge: !noMerge,
});

if (dryRun) {
  const analyzer = new DogfeedAnalyzer(config.taskFile);
  const task = analyzer.pickNext();
  if (!task) {
    console.log('No ready tasks available.');
    process.exit(0);
  }
  console.log('Next task:');
  console.log(`  Priority: P${task.priority}`);
  console.log(`  Category: ${task.category}`);
  console.log(`  Description: ${task.description}`);
  if (task.filePath) console.log(`  File: ${task.filePath}${task.line ? `:${task.line}` : ''}`);

  // Show routing decision
  const { TRIVIAL_CATEGORIES } = await import('./types.js');
  const agent = TRIVIAL_CATEGORIES.has(task.category) ? 'pi-agent' : 'omc';
  console.log(`  Agent: ${agent}`);
  process.exit(0);
}

console.log('=== SERA Dogfeed Cycle ===');
console.log(`Push: ${config.pushToRemote}, Merge: ${config.autoMerge}`);
console.log('');

const loop = new DogfeedLoop({
  pushToRemote: !noPush,
  autoMerge: !noMerge,
});

try {
  const result = await loop.runCycle();

  console.log('');
  console.log('=== Cycle Result ===');
  console.log(`  Success: ${result.success}`);
  console.log(`  Task: ${result.task.description}`);
  console.log(`  Agent: ${result.agent}`);
  console.log(`  Branch: ${result.branch}`);
  console.log(`  CI Passed: ${result.ciPassed}`);
  console.log(`  Merged: ${result.merged}`);
  console.log(`  Duration: ${Math.round(result.durationMs / 1000)}s`);
  console.log(
    `  Files: ${result.filesChanged}, Lines: +${result.linesAdded}/-${result.linesRemoved}`
  );
  if (result.error) console.log(`  Error: ${result.error}`);

  process.exit(result.success ? 0 : 1);
} catch (err) {
  console.error('Fatal error:', err);
  process.exit(2);
}

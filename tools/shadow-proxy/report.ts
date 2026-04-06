import fs from 'node:fs';

const DIFF_LOG_PATH = process.env['DIFF_LOG_PATH'] ?? './parity-diffs.jsonl';

interface ParityResult {
  timestamp: string;
  method: string;
  path: string;
  statusMatch: boolean;
  primaryStatus: number;
  shadowStatus: number;
  bodyMatch: boolean;
  diff?: string;
  latencyPrimaryMs: number;
  latencyShadowMs: number;
}

interface EndpointStats {
  total: number;
  matching: number;
  diffs: string[];
}

function readResults(): ParityResult[] {
  if (!fs.existsSync(DIFF_LOG_PATH)) {
    console.error(`[report] Log file not found: ${DIFF_LOG_PATH}`);
    process.exit(1);
  }

  const lines = fs
    .readFileSync(DIFF_LOG_PATH, 'utf8')
    .split('\n')
    .filter((l) => l.trim().length > 0);

  const results: ParityResult[] = [];
  for (const line of lines) {
    try {
      results.push(JSON.parse(line) as ParityResult);
    } catch {
      // Skip malformed lines
    }
  }
  return results;
}

function endpointKey(result: ParityResult): string {
  // Normalize dynamic path segments (UUIDs, numeric IDs)
  const normalized = result.path
    .split('?')[0]!
    .replace(/[0-9a-f]{8}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{12}/gi, ':id')
    .replace(/\/\d+/g, '/:id');
  return `${result.method} ${normalized}`;
}

function main(): void {
  const results = readResults();

  if (results.length === 0) {
    console.log('No parity results found in log file.');
    return;
  }

  const totalRequests = results.length;
  const matching = results.filter((r) => r.statusMatch && r.bodyMatch).length;
  const mismatching = totalRequests - matching;
  const matchPct = ((matching / totalRequests) * 100).toFixed(1);
  const mismatchPct = ((mismatching / totalRequests) * 100).toFixed(1);

  console.log('=== Parity Report ===');
  console.log(`Total requests:  ${totalRequests}`);
  console.log(`Matching:        ${matching} (${matchPct}%)`);
  console.log(`Mismatching:     ${mismatching} (${mismatchPct}%)`);

  // Per-endpoint breakdown
  const endpoints = new Map<string, EndpointStats>();

  for (const result of results) {
    const key = endpointKey(result);
    let stats = endpoints.get(key);
    if (!stats) {
      stats = { total: 0, matching: 0, diffs: [] };
      endpoints.set(key, stats);
    }
    stats.total++;
    if (result.statusMatch && result.bodyMatch) {
      stats.matching++;
    } else {
      if (result.diff && !stats.diffs.includes(result.diff)) {
        stats.diffs.push(result.diff);
      }
    }
  }

  // Sort by endpoint key for consistent output
  const sorted = [...endpoints.entries()].sort(([a], [b]) => a.localeCompare(b));

  console.log('\nPer-endpoint breakdown:');
  for (const [key, stats] of sorted) {
    const pct = ((stats.matching / stats.total) * 100).toFixed(0);
    const mismatchCount = stats.total - stats.matching;
    const arrow =
      mismatchCount > 0 ? ` \u2190 ${mismatchCount} diff${mismatchCount === 1 ? '' : 's'}` : '';
    console.log(`  ${key.padEnd(40)} ${stats.matching}/${stats.total} (${pct}%)${arrow}`);

    // Show first unique diff for mismatching endpoints
    if (stats.diffs.length > 0) {
      const preview = stats.diffs[0]!.split('\n')[0]!;
      console.log(`    sample diff: ${preview}`);
    }
  }

  // Shadow connectivity summary
  const timeouts = results.filter((r) => r.diff === 'shadow_timeout').length;
  const connRefused = results.filter((r) => r.diff === 'shadow_connection_refused').length;

  if (timeouts > 0 || connRefused > 0) {
    console.log('\nConnectivity issues:');
    if (timeouts > 0) {
      console.log(`  Shadow timeouts:           ${timeouts}`);
    }
    if (connRefused > 0) {
      console.log(`  Shadow connection refused: ${connRefused}`);
    }
  }

  // Latency summary
  const validLatencies = results.filter(
    (r) =>
      r.latencyShadowMs > 0 && r.diff !== 'shadow_timeout' && r.diff !== 'shadow_connection_refused'
  );
  if (validLatencies.length > 0) {
    const avgPrimary =
      validLatencies.reduce((s, r) => s + r.latencyPrimaryMs, 0) / validLatencies.length;
    const avgShadow =
      validLatencies.reduce((s, r) => s + r.latencyShadowMs, 0) / validLatencies.length;
    console.log('\nLatency (avg):');
    console.log(`  Primary: ${avgPrimary.toFixed(0)}ms`);
    console.log(`  Shadow:  ${avgShadow.toFixed(0)}ms`);
  }
}

main();

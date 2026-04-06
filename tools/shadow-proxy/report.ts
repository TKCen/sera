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

interface TableRow {
  endpoint: string;
  method: string;
  statusMatch: string;
  bodyMatch: string;
  tsLatency: string;
  rustLatency: string;
}

function padR(s: string, n: number): string {
  return s.length >= n ? s : s + ' '.repeat(n - s.length);
}

function padL(s: string, n: number): string {
  return s.length >= n ? s : ' '.repeat(n - s.length) + s;
}

function renderTable(rows: TableRow[]): void {
  const cols: (keyof TableRow)[] = [
    'endpoint',
    'method',
    'statusMatch',
    'bodyMatch',
    'tsLatency',
    'rustLatency',
  ];
  const headers: Record<keyof TableRow, string> = {
    endpoint: 'Endpoint',
    method: 'Method',
    statusMatch: 'Status Match',
    bodyMatch: 'Body Match',
    tsLatency: 'TS Latency',
    rustLatency: 'Rust Latency',
  };

  const widths = cols.map((col) =>
    Math.max(headers[col].length, ...rows.map((r) => r[col].length))
  );

  const sep = '+' + widths.map((w) => '-'.repeat(w + 2)).join('+') + '+';
  const headerRow =
    '|' + cols.map((col, i) => ' ' + padR(headers[col], widths[i]!) + ' ').join('|') + '|';

  console.log(sep);
  console.log(headerRow);
  console.log(sep);
  for (const row of rows) {
    const line = '|' + cols.map((col, i) => ' ' + padR(row[col], widths[i]!) + ' ').join('|') + '|';
    console.log(line);
  }
  console.log(sep);
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
  console.log(`Overall parity:  ${matchPct}%`);

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

  // Latency averages per endpoint
  const latencyMap = new Map<string, { tsTotal: number; rustTotal: number; count: number }>();
  for (const result of results) {
    const key = endpointKey(result);
    const isConnError =
      result.diff === 'shadow_timeout' || result.diff === 'shadow_connection_refused';
    if (!isConnError) {
      let lat = latencyMap.get(key);
      if (!lat) {
        lat = { tsTotal: 0, rustTotal: 0, count: 0 };
        latencyMap.set(key, lat);
      }
      lat.tsTotal += result.latencyPrimaryMs;
      lat.rustTotal += result.latencyShadowMs;
      lat.count++;
    }
  }

  // Sort by endpoint key for consistent output
  const sorted = [...endpoints.entries()].sort(([a], [b]) => a.localeCompare(b));

  console.log('\nPer-endpoint summary table:');

  const rows: TableRow[] = sorted.map(([key, stats]) => {
    const [method = '', ...pathParts] = key.split(' ');
    const path = pathParts.join(' ');
    const statusMatchCount = results.filter((r) => endpointKey(r) === key && r.statusMatch).length;
    const bodyMatchCount = results.filter((r) => endpointKey(r) === key && r.bodyMatch).length;
    const lat = latencyMap.get(key);
    const tsLatency = lat && lat.count > 0 ? `${(lat.tsTotal / lat.count).toFixed(0)}ms` : 'n/a';
    const rustLatency =
      lat && lat.count > 0 ? `${(lat.rustTotal / lat.count).toFixed(0)}ms` : 'n/a';
    return {
      endpoint: path,
      method,
      statusMatch: `${statusMatchCount}/${stats.total}`,
      bodyMatch: `${bodyMatchCount}/${stats.total}`,
      tsLatency,
      rustLatency,
    };
  });

  renderTable(rows);

  // Sample diffs for mismatching endpoints
  const withDiffs = sorted.filter(([, stats]) => stats.diffs.length > 0);
  if (withDiffs.length > 0) {
    console.log('\nSample diffs (first unique diff per endpoint):');
    for (const [key, stats] of withDiffs) {
      const preview = stats.diffs[0]!.split('\n')[0]!;
      console.log(`  ${key}`);
      console.log(`    ${preview}`);
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

  // Overall latency summary
  const validLatencies = results.filter(
    (r) =>
      r.latencyShadowMs > 0 && r.diff !== 'shadow_timeout' && r.diff !== 'shadow_connection_refused'
  );
  if (validLatencies.length > 0) {
    const avgPrimary =
      validLatencies.reduce((s, r) => s + r.latencyPrimaryMs, 0) / validLatencies.length;
    const avgShadow =
      validLatencies.reduce((s, r) => s + r.latencyShadowMs, 0) / validLatencies.length;
    console.log('\nLatency averages (all endpoints):');
    console.log(`  TS (primary): ${avgPrimary.toFixed(0)}ms`);
    console.log(`  Rust (shadow): ${avgShadow.toFixed(0)}ms`);
    const delta = avgShadow - avgPrimary;
    const sign = delta >= 0 ? '+' : '';
    console.log(`  Delta: ${sign}${delta.toFixed(0)}ms`);
  }

  // Final parity line
  console.log(
    `\nOverall parity: ${padL(matchPct, 5)}%  (${matching}/${totalRequests} requests fully matching)`
  );
}

main();

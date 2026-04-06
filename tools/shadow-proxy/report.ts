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
  conventionMismatches: number;
  dataMismatches: number;
  conventionFields: string[];
  dataFields: string[];
}

interface EndpointStats {
  total: number;
  matching: number;
  conventionOnly: number;
  dataDiff: number;
  notImplemented: number;
  diffs: string[];
  conventionCount: number;
  dataCount: number;
}

interface FieldStats {
  path: string;
  conventionCount: number;
  dataCount: number;
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
      const parsed = JSON.parse(line) as ParityResult;
      // Back-compat: older records may not have the new fields
      parsed.conventionMismatches = parsed.conventionMismatches ?? 0;
      parsed.dataMismatches = parsed.dataMismatches ?? 0;
      parsed.conventionFields = parsed.conventionFields ?? [];
      parsed.dataFields = parsed.dataFields ?? [];
      results.push(parsed);
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

function normalizeFieldPath(path: string): string {
  // Normalize array indices to [] and UUIDs to :id
  return path
    .replace(/\[\d+\]/g, '[]')
    .replace(/[0-9a-f]{8}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{12}/gi, ':id');
}

function analyzeFields(results: ParityResult[]): FieldStats[] {
  const fieldMap = new Map<string, { convention: number; data: number }>();

  for (const r of results) {
    for (const f of r.conventionFields) {
      const normalized = normalizeFieldPath(f);
      const entry = fieldMap.get(normalized) ?? { convention: 0, data: 0 };
      entry.convention++;
      fieldMap.set(normalized, entry);
    }
    for (const f of r.dataFields) {
      const normalized = normalizeFieldPath(f);
      const entry = fieldMap.get(normalized) ?? { convention: 0, data: 0 };
      entry.data++;
      fieldMap.set(normalized, entry);
    }
  }

  return [...fieldMap.entries()]
    .map(([path, stats]) => ({ path, conventionCount: stats.convention, dataCount: stats.data }))
    .sort((a, b) => b.conventionCount + b.dataCount - (a.conventionCount + a.dataCount));
}

interface TableRow {
  endpoint: string;
  method: string;
  statusMatch: string;
  bodyMatch: string;
  convention: string;
  dataDiff: string;
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
    'convention',
    'dataDiff',
    'tsLatency',
    'rustLatency',
  ];
  const headers: Record<keyof TableRow, string> = {
    endpoint: 'Endpoint',
    method: 'Method',
    statusMatch: 'Status Match',
    bodyMatch: 'Body Match',
    convention: 'Convention',
    dataDiff: 'Data Diff',
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

function isNotImplemented(result: ParityResult): boolean {
  return result.shadowStatus === 405 || result.shadowStatus === 501 || result.shadowStatus === 404;
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

  // Convention-adjusted parity: matches + convention-only mismatches
  const conventionAdjusted = results.filter(
    (r) => r.statusMatch && (r.bodyMatch || (r.dataMismatches === 0 && r.conventionMismatches > 0))
  ).length;
  const conventionAdjustedPct = ((conventionAdjusted / totalRequests) * 100).toFixed(1);

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
      stats = {
        total: 0,
        matching: 0,
        conventionOnly: 0,
        dataDiff: 0,
        notImplemented: 0,
        diffs: [],
        conventionCount: 0,
        dataCount: 0,
      };
      endpoints.set(key, stats);
    }
    stats.total++;
    stats.conventionCount += result.conventionMismatches;
    stats.dataCount += result.dataMismatches;

    if (result.statusMatch && result.bodyMatch) {
      stats.matching++;
    } else if (isNotImplemented(result)) {
      stats.notImplemented++;
    } else if (
      result.statusMatch &&
      result.dataMismatches === 0 &&
      result.conventionMismatches > 0
    ) {
      stats.conventionOnly++;
    } else {
      stats.dataDiff++;
      if (result.diff && !stats.diffs.includes(result.diff)) {
        stats.diffs.push(result.diff);
      }
    }
  }

  // === Convention Analysis ===
  const allConventionFields = analyzeFields(results);
  const totalConventionMismatches = results.reduce((s, r) => s + r.conventionMismatches, 0);
  const endpointsWithConvention = [...endpoints.values()].filter(
    (s) => s.conventionCount > 0
  ).length;

  if (totalConventionMismatches > 0) {
    console.log('\n=== Convention Analysis ===');
    console.log(
      `Convention-only mismatches: ${totalConventionMismatches} (across ${endpointsWithConvention} endpoints)`
    );
    const topConvention = allConventionFields.filter((f) => f.conventionCount > 0).slice(0, 10);
    if (topConvention.length > 0) {
      console.log('Top convention fields:');
      for (const field of topConvention) {
        // Derive the camelCase counterpart for display
        const snake = field.path;
        const camel = snake.replace(/_([a-z])/g, (_, c: string) => c.toUpperCase());
        const label = snake === camel ? snake : `${snake} / ${camel}`;
        console.log(`  ${padR(label, 36)} — ${field.conventionCount} occurrences`);
      }
    }
  }

  // === Route Coverage ===
  const sorted = [...endpoints.entries()].sort(([a], [b]) => a.localeCompare(b));
  const totalRoutes = endpoints.size;
  let fullyMatching = 0;
  let conventionOnlyRoutes = 0;
  let dataDiffRoutes = 0;
  let notImplementedRoutes = 0;

  for (const [, stats] of sorted) {
    if (stats.matching === stats.total) {
      fullyMatching++;
    } else if (stats.notImplemented > 0 && stats.notImplemented === stats.total - stats.matching) {
      notImplementedRoutes++;
    } else if (stats.conventionOnly > 0 && stats.dataDiff === 0) {
      conventionOnlyRoutes++;
    } else {
      dataDiffRoutes++;
    }
  }

  console.log('\n=== Route Coverage ===');
  console.log(`Total unique routes tested: ${totalRoutes}`);
  console.log(
    `  Fully matching:    ${fullyMatching} (${((fullyMatching / totalRoutes) * 100).toFixed(1)}%)`
  );
  console.log(
    `  Convention only:   ${conventionOnlyRoutes} (${((conventionOnlyRoutes / totalRoutes) * 100).toFixed(1)}%)  <- would match with serde rename`
  );
  console.log(
    `  Data differences:  ${dataDiffRoutes} (${((dataDiffRoutes / totalRoutes) * 100).toFixed(1)}%)`
  );
  console.log(
    `  Not implemented:   ${notImplementedRoutes} (${((notImplementedRoutes / totalRoutes) * 100).toFixed(1)}%)  <- 405/501 responses`
  );

  // === Aggregate Parity ===
  console.log('\n=== Aggregate Parity ===');
  console.log(`Strict parity:       ${padL(matchPct, 5)}%  (matching status + body exactly)`);
  console.log(
    `Convention-adjusted: ${padL(conventionAdjustedPct, 5)}%  (ignoring camelCase/snake_case diffs)`
  );

  // Per-endpoint table
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
      convention: stats.conventionCount > 0 ? String(stats.conventionCount) : '-',
      dataDiff: stats.dataCount > 0 ? String(stats.dataCount) : '-',
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

  // === Top Mismatched Fields ===
  if (allConventionFields.length > 0) {
    console.log('\n=== Top Mismatched Fields ===');
    for (const field of allConventionFields.slice(0, 15)) {
      const parts: string[] = [];
      if (field.conventionCount > 0) parts.push(`convention (${field.conventionCount}x)`);
      if (field.dataCount > 0) parts.push(`data (${field.dataCount}x)`);
      console.log(`  ${padR(field.path, 40)} — ${parts.join(', ')}`);
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

import http from 'node:http';
import fs from 'node:fs';
import { URL } from 'node:url';

const PRIMARY_URL = process.env['PRIMARY_URL'] ?? 'http://localhost:3001';
const SHADOW_URL = process.env['SHADOW_URL'] ?? 'http://localhost:3002';
const DIFF_LOG_PATH = process.env['DIFF_LOG_PATH'] ?? './parity-diffs.jsonl';
const PORT = parseInt(process.env['PORT'] ?? '3000', 10);
const SHADOW_TIMEOUT_MS = 5000;

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
  /** Number of fields that differ only in naming convention (camelCase vs snake_case) */
  conventionMismatches: number;
  /** Number of fields with actual data/value differences */
  dataMismatches: number;
  /** List of field paths that are convention-only differences */
  conventionFields: string[];
  /** List of field paths that are real data differences */
  dataFields: string[];
}

interface DiffEntry {
  path: string;
  type: 'convention' | 'data' | 'missing' | 'type_mismatch';
  primary?: unknown;
  shadow?: unknown;
}

const IGNORED_FIELDS = new Set(['timestamp', 'created_at', 'updated_at', 'id']);

function toSnakeCase(s: string): string {
  return s.replace(/([a-z0-9])([A-Z])/g, '$1_$2').toLowerCase();
}

function toCamelCase(s: string): string {
  return s.replace(/_([a-z])/g, (_, c: string) => c.toUpperCase());
}

/** Check if two keys are the same field in different conventions */
function isConventionMismatch(key1: string, key2: string): boolean {
  if (key1 === key2) return false;
  return toSnakeCase(key1) === toSnakeCase(key2) || toCamelCase(key1) === toCamelCase(key2);
}

function normalizeBody(obj: unknown): unknown {
  if (Array.isArray(obj)) {
    return obj.map(normalizeBody);
  }
  if (obj !== null && typeof obj === 'object') {
    const result: Record<string, unknown> = {};
    for (const [key, value] of Object.entries(obj as Record<string, unknown>)) {
      if (!IGNORED_FIELDS.has(key)) {
        result[key] = normalizeBody(value);
      }
    }
    return result;
  }
  return obj;
}

function deepDiff(primary: unknown, shadow: unknown, path = ''): DiffEntry[] {
  if (primary === shadow) return [];

  if (Array.isArray(primary) && Array.isArray(shadow)) {
    const diffs: DiffEntry[] = [];
    const len = Math.max(primary.length, shadow.length);
    for (let i = 0; i < len; i++) {
      diffs.push(...deepDiff(primary[i], shadow[i], `${path}[${i}]`));
    }
    return diffs;
  }

  if (
    primary !== null &&
    shadow !== null &&
    typeof primary === 'object' &&
    typeof shadow === 'object' &&
    !Array.isArray(primary) &&
    !Array.isArray(shadow)
  ) {
    const p = primary as Record<string, unknown>;
    const s = shadow as Record<string, unknown>;
    const primaryKeys = Object.keys(p);
    const shadowKeys = Object.keys(s);
    const diffs: DiffEntry[] = [];

    // Track which shadow keys have been matched
    const matchedShadowKeys = new Set<string>();

    for (const pk of primaryKeys) {
      const fieldPath = path ? `${path}.${pk}` : pk;

      if (pk in s) {
        // Same key exists in both — recurse
        matchedShadowKeys.add(pk);
        diffs.push(...deepDiff(p[pk], s[pk], fieldPath));
      } else {
        // Look for a convention-mapped equivalent in shadow
        const conventionKey = shadowKeys.find(
          (sk) => !matchedShadowKeys.has(sk) && isConventionMismatch(pk, sk)
        );
        if (conventionKey !== undefined) {
          matchedShadowKeys.add(conventionKey);
          const shadowFieldPath = path ? `${path}.${conventionKey}` : conventionKey;
          if (p[pk] === s[conventionKey]) {
            // Same value, different naming convention
            diffs.push({
              path: fieldPath,
              type: 'convention',
              primary: pk,
              shadow: shadowFieldPath,
            });
          } else {
            // Different naming AND different value — treat as data diff
            diffs.push({
              path: fieldPath,
              type: 'data',
              primary: p[pk],
              shadow: s[conventionKey],
            });
          }
        } else {
          // Key exists only in primary
          diffs.push({ path: fieldPath, type: 'missing', primary: p[pk], shadow: undefined });
        }
      }
    }

    // Shadow keys not matched to any primary key
    for (const sk of shadowKeys) {
      if (!matchedShadowKeys.has(sk)) {
        const fieldPath = path ? `${path}.${sk}` : sk;
        diffs.push({ path: fieldPath, type: 'missing', primary: undefined, shadow: s[sk] });
      }
    }

    return diffs;
  }

  // Type mismatch (one is array/object, other is not) or primitive value difference
  const fieldPath = path || '(root)';
  if (typeof primary !== typeof shadow || Array.isArray(primary) !== Array.isArray(shadow)) {
    return [{ path: fieldPath, type: 'type_mismatch', primary, shadow }];
  }
  return [{ path: fieldPath, type: 'data', primary, shadow }];
}

function appendParityResult(result: ParityResult): void {
  try {
    fs.appendFileSync(DIFF_LOG_PATH, JSON.stringify(result) + '\n', 'utf8');
  } catch (err) {
    console.error('[shadow-proxy] Failed to write parity log:', err);
  }
}

function forwardRequest(
  targetBase: string,
  method: string,
  path: string,
  headers: http.IncomingHttpHeaders,
  body: Buffer
): Promise<{ status: number; body: Buffer; latencyMs: number }> {
  return new Promise((resolve, reject) => {
    const target = new URL(path, targetBase);
    const options: http.RequestOptions = {
      hostname: target.hostname,
      port: target.port || (target.protocol === 'https:' ? 443 : 80),
      path: target.pathname + target.search,
      method,
      headers: {
        ...headers,
        host: target.host,
      },
    };

    const start = Date.now();
    const req = http.request(options, (res) => {
      const chunks: Buffer[] = [];
      res.on('data', (chunk: Buffer) => chunks.push(chunk));
      res.on('end', () => {
        resolve({
          status: res.statusCode ?? 0,
          body: Buffer.concat(chunks),
          latencyMs: Date.now() - start,
        });
      });
      res.on('error', reject);
    });

    req.on('error', reject);

    if (body.length > 0) {
      req.write(body);
    }
    req.end();
  });
}

async function compareShadow(
  method: string,
  path: string,
  headers: http.IncomingHttpHeaders,
  body: Buffer,
  primaryStatus: number,
  primaryBody: Buffer,
  latencyPrimaryMs: number
): Promise<void> {
  const controller = new AbortController();
  const timeout = setTimeout(() => controller.abort(), SHADOW_TIMEOUT_MS);

  let shadowStatus = 0;
  let shadowBody = Buffer.alloc(0);
  let latencyShadowMs = 0;
  let timedOut = false;
  let connectionRefused = false;

  try {
    const result = await forwardRequest(SHADOW_URL, method, path, headers, body);
    shadowStatus = result.status;
    shadowBody = result.body;
    latencyShadowMs = result.latencyMs;
  } catch (err: unknown) {
    const error = err as NodeJS.ErrnoException;
    if (error.name === 'AbortError' || controller.signal.aborted) {
      timedOut = true;
      latencyShadowMs = SHADOW_TIMEOUT_MS;
    } else if (
      error.code === 'ECONNREFUSED' ||
      error.code === 'ENOTFOUND' ||
      error.code === 'ECONNRESET'
    ) {
      connectionRefused = true;
    } else {
      console.error('[shadow-proxy] Shadow request error:', error.message);
      return;
    }
  } finally {
    clearTimeout(timeout);
  }

  if (timedOut) {
    const result: ParityResult = {
      timestamp: new Date().toISOString(),
      method,
      path,
      statusMatch: false,
      primaryStatus,
      shadowStatus: 0,
      bodyMatch: false,
      diff: 'shadow_timeout',
      latencyPrimaryMs,
      latencyShadowMs,
      conventionMismatches: 0,
      dataMismatches: 0,
      conventionFields: [],
      dataFields: [],
    };
    appendParityResult(result);
    return;
  }

  if (connectionRefused) {
    // Shadow not running — log silently, don't spam console
    appendParityResult({
      timestamp: new Date().toISOString(),
      method,
      path,
      statusMatch: false,
      primaryStatus,
      shadowStatus: 0,
      bodyMatch: false,
      diff: 'shadow_connection_refused',
      latencyPrimaryMs,
      latencyShadowMs: 0,
      conventionMismatches: 0,
      dataMismatches: 0,
      conventionFields: [],
      dataFields: [],
    });
    return;
  }

  const statusMatch = primaryStatus === shadowStatus;

  let bodyMatch = false;
  let diff: string | undefined;
  let conventionMismatches = 0;
  let dataMismatches = 0;
  const conventionFields: string[] = [];
  const dataFields: string[] = [];

  try {
    const primaryJson = JSON.parse(primaryBody.toString('utf8'));
    const shadowJson = JSON.parse(shadowBody.toString('utf8'));
    const normalizedPrimary = normalizeBody(primaryJson);
    const normalizedShadow = normalizeBody(shadowJson);
    const diffs = deepDiff(normalizedPrimary, normalizedShadow);

    for (const entry of diffs) {
      if (entry.type === 'convention') {
        conventionMismatches++;
        conventionFields.push(entry.path);
      } else {
        dataMismatches++;
        dataFields.push(entry.path);
      }
    }

    // Body matches if there are no non-convention diffs
    bodyMatch =
      diffs.length === 0 || (dataMismatches === 0 && diffs.every((d) => d.type === 'convention'));

    if (!bodyMatch || diffs.length > 0) {
      const diffLines = diffs.slice(0, 20).map((d) => {
        if (d.type === 'convention') {
          return `${d.path}: convention (${String(d.primary)} vs ${String(d.shadow)})`;
        }
        return `${d.path} [${d.type}]: primary=${JSON.stringify(d.primary)} shadow=${JSON.stringify(d.shadow)}`;
      });
      if (diffLines.length > 0) {
        diff = diffLines.join('\n');
      }
    }
  } catch {
    // Non-JSON bodies — compare raw
    bodyMatch = primaryBody.toString('utf8') === shadowBody.toString('utf8');
    if (!bodyMatch) {
      diff = `non-json body mismatch: primary=${primaryBody.length}b shadow=${shadowBody.length}b`;
      dataMismatches = 1;
      dataFields.push('(body)');
    }
  }

  const result: ParityResult = {
    timestamp: new Date().toISOString(),
    method,
    path,
    statusMatch,
    primaryStatus,
    shadowStatus,
    bodyMatch,
    diff,
    latencyPrimaryMs,
    latencyShadowMs,
    conventionMismatches,
    dataMismatches,
    conventionFields,
    dataFields,
  };

  if (!statusMatch || !bodyMatch) {
    const parts = [`[parity] ${method} ${path}`];
    parts.push(`status=${primaryStatus}/${shadowStatus}`);
    if (conventionMismatches > 0) parts.push(`convention=${conventionMismatches}`);
    if (dataMismatches > 0) parts.push(`data_diff=${dataMismatches}`);
    console.log(parts.join(' | '));
  } else {
    console.log(`[parity] OK ${method} ${path} ${latencyPrimaryMs}ms/${latencyShadowMs}ms`);
  }

  appendParityResult(result);
}

const server = http.createServer((req, res) => {
  const chunks: Buffer[] = [];

  req.on('data', (chunk: Buffer) => chunks.push(chunk));
  req.on('end', () => {
    const body = Buffer.concat(chunks);
    const method = req.method ?? 'GET';
    const path = req.url ?? '/';

    const primaryStart = Date.now();

    forwardRequest(PRIMARY_URL, method, path, req.headers, body)
      .then((primary) => {
        // Return primary response to client immediately
        res.writeHead(primary.status);
        res.end(primary.body);

        const latencyPrimaryMs = Date.now() - primaryStart;

        // Fire-and-forget shadow comparison
        compareShadow(
          method,
          path,
          req.headers,
          body,
          primary.status,
          primary.body,
          latencyPrimaryMs
        ).catch((err) => {
          console.error('[shadow-proxy] Unexpected comparison error:', err);
        });
      })
      .catch((err: NodeJS.ErrnoException) => {
        console.error('[shadow-proxy] Primary request failed:', err.message);
        res.writeHead(502);
        res.end(JSON.stringify({ error: 'Primary upstream unavailable' }));
      });
  });

  req.on('error', (err) => {
    console.error('[shadow-proxy] Request error:', err);
    res.writeHead(400);
    res.end();
  });
});

server.listen(PORT, () => {
  console.log(`[shadow-proxy] Listening on :${PORT}`);
  console.log(`[shadow-proxy] Primary: ${PRIMARY_URL}`);
  console.log(`[shadow-proxy] Shadow:  ${SHADOW_URL}`);
  console.log(`[shadow-proxy] Diff log: ${DIFF_LOG_PATH}`);
});

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
}

const IGNORED_FIELDS = new Set(['timestamp', 'created_at', 'updated_at', 'id']);

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

function deepDiff(primary: unknown, shadow: unknown, path = ''): string[] {
  if (primary === shadow) return [];

  if (Array.isArray(primary) && Array.isArray(shadow)) {
    const diffs: string[] = [];
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
    typeof shadow === 'object'
  ) {
    const p = primary as Record<string, unknown>;
    const s = shadow as Record<string, unknown>;
    const keys = new Set([...Object.keys(p), ...Object.keys(s)]);
    const diffs: string[] = [];
    for (const key of keys) {
      diffs.push(...deepDiff(p[key], s[key], path ? `${path}.${key}` : key));
    }
    return diffs;
  }

  const fieldPath = path || '(root)';
  return [`${fieldPath}: primary=${JSON.stringify(primary)} shadow=${JSON.stringify(shadow)}`];
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
    });
    return;
  }

  const statusMatch = primaryStatus === shadowStatus;

  let bodyMatch = false;
  let diff: string | undefined;

  try {
    const primaryJson = JSON.parse(primaryBody.toString('utf8'));
    const shadowJson = JSON.parse(shadowBody.toString('utf8'));
    const normalizedPrimary = normalizeBody(primaryJson);
    const normalizedShadow = normalizeBody(shadowJson);
    const diffs = deepDiff(normalizedPrimary, normalizedShadow);
    bodyMatch = diffs.length === 0;
    if (!bodyMatch) {
      diff = diffs.slice(0, 20).join('\n');
    }
  } catch {
    // Non-JSON bodies — compare raw
    bodyMatch = primaryBody.toString('utf8') === shadowBody.toString('utf8');
    if (!bodyMatch) {
      diff = `non-json body mismatch: primary=${primaryBody.length}b shadow=${shadowBody.length}b`;
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
  };

  if (!statusMatch || !bodyMatch) {
    console.log(
      `[parity] MISMATCH ${method} ${path} status=${primaryStatus}/${shadowStatus} body=${bodyMatch ? 'ok' : 'diff'}`
    );
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

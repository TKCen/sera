import { ENV } from '../playwright.config.js';

/**
 * Polls a URL until it returns HTTP 200 or the timeout elapses.
 * Throws if the service never becomes healthy.
 */
export async function waitForUrl(
  url: string,
  { timeoutMs = 120_000, intervalMs = 2_000, label = url } = {},
): Promise<void> {
  const deadline = Date.now() + timeoutMs;
  let lastError: unknown;

  while (Date.now() < deadline) {
    try {
      const res = await fetch(url, { signal: AbortSignal.timeout(5_000) });
      if (res.ok) return;
      lastError = new Error(`HTTP ${res.status} from ${url}`);
    } catch (err) {
      lastError = err;
    }
    await sleep(intervalMs);
  }

  throw new Error(
    `Timed out waiting for ${label} to become healthy after ${timeoutMs}ms. Last error: ${lastError}`,
  );
}

function sleep(ms: number): Promise<void> {
  return new Promise((resolve) => setTimeout(resolve, ms));
}

/**
 * Waits for the full SERA stack to be healthy.
 * Call this in a globalSetup or a beforeAll hook before any browser tests.
 *
 * What "healthy" means per mode:
 *   - All modes:   sera-core GET /api/health → 200
 *   - dev:         sera-web  GET /          → 200  (Vite dev server)
 *   - api-key/oidc: sera-web GET /          → 200  (nginx)
 *   - oidc only:   Authentik GET /api/v3/root/ → 200
 */
export async function waitForStack(mode: 'dev' | 'api-key' | 'oidc' = ENV.mode): Promise<void> {
  const checks: Array<{ url: string; label: string }> = [
    { url: `${ENV.apiBaseUrl}/api/health`, label: 'sera-core /api/health' },
    { url: `${ENV.webBaseUrl}/`, label: 'sera-web /' },
  ];

  if (mode === 'oidc') {
    checks.push({
      url: 'http://localhost:9000/api/v3/root/',
      label: 'authentik /api/v3/root/',
    });
  }

  console.log(`\nWaiting for SERA stack (mode: ${mode})...`);
  await Promise.all(
    checks.map(({ url, label }) =>
      waitForUrl(url, { timeoutMs: 180_000, label }).then(() =>
        console.log(`  ✓ ${label}`),
      ),
    ),
  );
  console.log('Stack is healthy.\n');
}

/**
 * Convenience: POST to sera-core to verify the bootstrap API key works.
 * Useful to confirm the key configured in ENV.apiKey is actually valid.
 */
export async function verifyBootstrapApiKey(): Promise<void> {
  const res = await fetch(`${ENV.apiBaseUrl}/api/health`, {
    headers: { Authorization: `Bearer ${ENV.apiKey}` },
    signal: AbortSignal.timeout(10_000),
  });
  if (!res.ok) {
    throw new Error(
      `Bootstrap API key check failed: HTTP ${res.status}. ` +
      `Ensure SERA_API_KEY matches SERA_BOOTSTRAP_API_KEY in the core container.`,
    );
  }
}

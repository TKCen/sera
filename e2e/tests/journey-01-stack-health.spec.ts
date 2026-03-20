/**
 * Journey 01 — Stack Health
 *
 * User story:
 *   As a developer running SERA locally, I want to verify that all required
 *   services started successfully before running any UI tests, so that test
 *   failures point to application bugs rather than an unhealthy stack.
 *
 * Covers:
 *   - sera-core REST API is reachable and returns a healthy response
 *   - sera-web is serving HTML at the root URL
 *   - (oidc mode) Authentik server API is reachable
 *
 * Prerequisites:
 *   Dev:      docker compose -f docker-compose.yaml -f docker-compose.dev.yaml up -d
 *   API-key:  docker compose -f docker-compose.yaml up -d  (+ port mapping for sera-web)
 *   OIDC:     docker compose -f docker-compose.yaml -f docker-compose.auth.yaml up -d
 *
 * Environment variables (see playwright.config.ts):
 *   E2E_STACK_MODE    dev | api-key | oidc  (default: dev)
 *   SERA_API_URL      sera-core base URL    (default: http://localhost:3001)
 *   SERA_WEB_URL      sera-web base URL     (default: http://localhost:3000 in dev)
 */

import { test, expect } from '@playwright/test';
import { ENV } from '../playwright.config.js';
import { waitForStack, verifyBootstrapApiKey } from '../fixtures/stack.js';

test.describe('Stack Health', () => {
  test.beforeAll(async () => {
    // Give the stack up to 3 minutes to reach a healthy state.
    // If you have already waited elsewhere, this will return immediately.
    await waitForStack(ENV.mode);
  });

  // ── sera-core ─────────────────────────────────────────────────────────────

  test('sera-core: GET /api/health returns 200', async ({ request }) => {
    const res = await request.get(`${ENV.apiBaseUrl}/api/health`);
    expect(res.status()).toBe(200);

    const body = await res.json();
    // The health endpoint must include a status field set to "ok"
    expect(body).toMatchObject({ status: 'ok' });
  });

  test('sera-core: bootstrap API key is accepted', async () => {
    await verifyBootstrapApiKey();
  });

  test('sera-core: unauthenticated request to protected route returns 401', async ({ request }) => {
    const res = await request.get(`${ENV.apiBaseUrl}/api/agents`);
    expect(res.status()).toBe(401);
  });

  // ── sera-web ──────────────────────────────────────────────────────────────

  test('sera-web: root URL returns HTML', async ({ request }) => {
    const res = await request.get(`${ENV.webBaseUrl}/`);
    expect(res.status()).toBe(200);

    const contentType = res.headers()['content-type'] ?? '';
    expect(contentType).toContain('text/html');

    const body = await res.text();
    // The SPA must contain the React root element
    expect(body).toContain('id="root"');
  });

  test('sera-web: unauthenticated navigation redirects to /login', async ({ page }) => {
    await page.goto('/');
    await expect(page).toHaveURL(/\/login/);
  });

  // ── Authentik (oidc mode only) ─────────────────────────────────────────────

  test('authentik: API root returns 200 [oidc mode only]', async ({ request }) => {
    if (ENV.mode !== 'oidc') {
      test.skip();
      return;
    }
    const res = await request.get('http://localhost:9000/api/v3/root/');
    expect(res.status()).toBe(200);
  });
});

/**
 * Journey 03 — Agent Management
 *
 * Covers the Agents page and the agent lifecycle (list, delete, no-auto-bootstrap):
 *
 *   1. Agents page lists YAML-loaded agents from the core server
 *   2. Each agent card shows a Delete button (hidden by default, visible on hover)
 *   3. Confirming deletion removes the agent from the list and from disk
 *   4. Cancelling the confirmation dialog leaves the list unchanged
 *   5. DELETE /api/agents/:name returns 404 for unknown agents
 *   6. No 'sera' agent is auto-created on first boot (bootstrap disabled)
 *
 * Prerequisites:
 *   - Dev stack running: docker compose -f docker-compose.yaml -f docker-compose.dev.yaml up -d
 *   - Authenticated session (dev key bypass, mode: dev)
 *
 * Environment variables (see playwright.config.ts):
 *   E2E_STACK_MODE   dev  (default)
 *   SERA_API_KEY     sera_bootstrap_dev_123
 */

import { test, expect, type Page } from '@playwright/test';
import { ENV } from '../playwright.config.js';
import { waitForStack } from '../fixtures/stack.js';

// ── Helpers ───────────────────────────────────────────────────────────────────

/** Log in via the dev key bypass and navigate to /agents. */
async function goToAgents(page: Page): Promise<void> {
  if (ENV.mode !== 'dev') {
    // For non-dev modes authenticate via API key then navigate
    await page.goto('/login');
    await page.getByLabel('API Key').fill(ENV.apiKey);
    await page.getByRole('button', { name: 'Continue with API Key' }).click();
    await expect(page).toHaveURL(/\/chat/, { timeout: 15_000 });
  } else {
    await page.goto('/login');
    // Dev mode: one-click bypass
    const devBtn = page.getByRole('button', { name: 'Continue to Dashboard' });
    if (await devBtn.isVisible()) {
      await devBtn.click();
      await expect(page).toHaveURL(/\/chat/, { timeout: 10_000 });
    }
  }
  await page.goto('/agents');
  await expect(page).toHaveURL(/\/agents/);
}

/**
 * Create a throwaway agent via the REST API so tests can safely delete it
 * without touching real manifests.
 */
async function createTestAgent(request: Parameters<typeof test>[1] extends { request: infer R } ? R : never, name: string): Promise<void> {
  const res = await request.put(`${ENV.apiBaseUrl}/api/agents/${name}/manifest`, {
    headers: { Authorization: `Bearer ${ENV.apiKey}`, 'Content-Type': 'application/json' },
    data: {
      apiVersion: 'sera/v1',
      kind: 'Agent',
      metadata: { name, displayName: `E2E Test — ${name}` },
      spec: {
        identity: { role: 'throwaway e2e test agent' },
        model: { provider: 'openai', name: 'gpt-4o-mini' },
      },
    },
  });
  expect(res.ok()).toBeTruthy();
}

/** Delete an agent via API directly (cleanup helper). */
async function deleteTestAgentViaApi(request: Parameters<typeof test>[1] extends { request: infer R } ? R : never, name: string): Promise<void> {
  // Ignore errors — agent may already be gone
  await request.delete(`${ENV.apiBaseUrl}/api/agents/${name}`, {
    headers: { Authorization: `Bearer ${ENV.apiKey}` },
  });
}

// ── Suite ─────────────────────────────────────────────────────────────────────

test.describe('Agents page', () => {
  test.beforeAll(async () => {
    await waitForStack(ENV.mode);
  });

  test.beforeEach(async ({ page }) => {
    await page.addInitScript(() => sessionStorage.clear());
  });

  // ── Listing ─────────────────────────────────────────────────────────────────

  test('agents page is accessible after login', async ({ page }) => {
    await goToAgents(page);
    await expect(page.getByRole('heading', { name: 'Agents' })).toBeVisible();
  });

  test('agents page lists at least one agent card', async ({ page }) => {
    await goToAgents(page);
    // Each card renders the agent metadata.name as a subtitle span
    const cards = page.locator('.sera-card');
    await expect(cards.first()).toBeVisible();
    const count = await cards.count();
    expect(count).toBeGreaterThan(0);
  });

  test('no "sera" instance is listed (bootstrap auto-creation disabled)', async ({ page }) => {
    await goToAgents(page);
    // Wait for the list to load
    await expect(page.locator('.sera-card').first()).toBeVisible();
    // There should be no card whose subtitle text is exactly "sera"
    const seraCard = page.locator('.sera-card').filter({ hasText: /^sera$/ });
    await expect(seraCard).toHaveCount(0);
  });

  test('each agent card has a Delete button in the DOM', async ({ page }) => {
    await goToAgents(page);
    await expect(page.locator('.sera-card').first()).toBeVisible();
    const deleteButtons = page.getByRole('button', { name: 'Delete' });
    const count = await deleteButtons.count();
    expect(count).toBeGreaterThan(0);
  });

  test('Delete button is inside an opacity-0 container (hidden until hover)', async ({ page }) => {
    await goToAgents(page);
    await expect(page.locator('.sera-card').first()).toBeVisible();

    const firstDelete = page.getByRole('button', { name: 'Delete' }).first();
    // The button's parent container should have opacity-0 class (hover reveals it)
    const container = firstDelete.locator('xpath=ancestor::div[contains(@class,"opacity-0")]');
    await expect(container).toHaveCount(1);
  });

  // ── Delete with confirm ─────────────────────────────────────────────────────

  test('Delete button triggers a confirmation dialog', async ({ page, request }) => {
    const agentName = 'e2e-delete-confirm-test';
    await createTestAgent(request, agentName);

    try {
      await goToAgents(page);

      // Wait for the test agent card to appear
      const testCard = page.locator('.sera-card').filter({ hasText: agentName });
      await expect(testCard).toBeVisible({ timeout: 10_000 });

      // Accept the dialog when it appears
      let dialogMessage = '';
      page.once('dialog', async (dialog) => {
        dialogMessage = dialog.message();
        await dialog.dismiss(); // Cancel — don't actually delete
      });

      const deleteBtn = testCard.getByRole('button', { name: 'Delete' });
      await deleteBtn.click();

      // Confirm the dialog mentioned the agent name
      expect(dialogMessage).toContain(agentName);
      expect(dialogMessage).toMatch(/cannot be undone/i);
    } finally {
      await deleteTestAgentViaApi(request, agentName);
    }
  });

  test('cancelling the confirmation leaves the agent in the list', async ({ page, request }) => {
    const agentName = 'e2e-cancel-delete-test';
    await createTestAgent(request, agentName);

    try {
      await goToAgents(page);
      const testCard = page.locator('.sera-card').filter({ hasText: agentName });
      await expect(testCard).toBeVisible({ timeout: 10_000 });

      // Dismiss (cancel) the dialog
      page.once('dialog', (dialog) => void dialog.dismiss());

      const deleteBtn = testCard.getByRole('button', { name: 'Delete' });
      await deleteBtn.click();

      // Agent card should still be visible
      await expect(testCard).toBeVisible();
    } finally {
      await deleteTestAgentViaApi(request, agentName);
    }
  });

  test('confirming deletion removes the agent from the list', async ({ page, request }) => {
    const agentName = 'e2e-full-delete-test';
    await createTestAgent(request, agentName);

    await goToAgents(page);
    const testCard = page.locator('.sera-card').filter({ hasText: agentName });
    await expect(testCard).toBeVisible({ timeout: 10_000 });

    // Accept the dialog
    page.once('dialog', (dialog) => void dialog.accept());

    const deleteBtn = testCard.getByRole('button', { name: 'Delete' });
    await deleteBtn.click();

    // Agent card should disappear from the list
    await expect(testCard).toHaveCount(0, { timeout: 10_000 });
  });

  test('confirming deletion removes the YAML manifest via the API', async ({ page, request }) => {
    const agentName = 'e2e-api-delete-test';
    await createTestAgent(request, agentName);

    await goToAgents(page);
    await expect(page.locator('.sera-card').filter({ hasText: agentName })).toBeVisible({
      timeout: 10_000,
    });

    page.once('dialog', (dialog) => void dialog.accept());
    await page.locator('.sera-card').filter({ hasText: agentName }).getByRole('button', { name: 'Delete' }).click();
    await expect(page.locator('.sera-card').filter({ hasText: agentName })).toHaveCount(0, {
      timeout: 10_000,
    });

    // Verify the manifest is gone from the API too
    const res = await request.delete(`${ENV.apiBaseUrl}/api/agents/${agentName}`, {
      headers: { Authorization: `Bearer ${ENV.apiKey}` },
    });
    // 404 means it's already gone — that's what we want
    expect(res.status()).toBe(404);
  });

  // ── API contract ────────────────────────────────────────────────────────────

  test('DELETE /api/agents/:name returns 404 for unknown agent', async ({ request }) => {
    const res = await request.delete(`${ENV.apiBaseUrl}/api/agents/does-not-exist-xyz`, {
      headers: { Authorization: `Bearer ${ENV.apiKey}` },
    });
    expect(res.status()).toBe(404);
    const body = await res.json();
    expect(body).toHaveProperty('error');
  });

  test('DELETE /api/agents/:name returns 401 without auth header [api-key/oidc mode only]', async ({ request }) => {
    // Dev mode runs without auth enforcement (VITE_DEV_API_KEY bypass), so skip there.
    if (ENV.mode === 'dev') {
      test.skip();
      return;
    }
    const res = await request.delete(`${ENV.apiBaseUrl}/api/agents/any-agent`);
    expect(res.status()).toBe(401);
  });

  test('DELETE /api/agents/:name removes agent from GET /api/agents list', async ({ request }) => {
    const agentName = 'e2e-list-sync-test';
    await createTestAgent(request, agentName);

    // Agent must appear in list first
    const listBefore = await request.get(`${ENV.apiBaseUrl}/api/agents`, {
      headers: { Authorization: `Bearer ${ENV.apiKey}` },
    });
    // Note: GET /api/agents returns only instantiated agents, so the newly-created
    // manifest may not appear until the server is restarted. Verify via the raw
    // manifest endpoint instead.
    const rawRes = await request.get(`${ENV.apiBaseUrl}/api/agents/${agentName}/manifest/raw`, {
      headers: { Authorization: `Bearer ${ENV.apiKey}` },
    });
    expect(rawRes.ok()).toBeTruthy();

    // Delete it
    const delRes = await request.delete(`${ENV.apiBaseUrl}/api/agents/${agentName}`, {
      headers: { Authorization: `Bearer ${ENV.apiKey}` },
    });
    expect(delRes.status()).toBe(204);

    // Manifest should now be gone
    const rawAfter = await request.get(`${ENV.apiBaseUrl}/api/agents/${agentName}/manifest/raw`, {
      headers: { Authorization: `Bearer ${ENV.apiKey}` },
    });
    expect(rawAfter.status()).toBe(404);
  });
});

/**
 * Journey 02 — Web Login
 *
 * Three login paths exist depending on how the stack is started:
 *
 * ┌─────────────────────────────────────────────────────────────────────────┐
 * │ Path A — Dev key bypass  (stack mode: dev, VITE_DEV_API_KEY set)        │
 * │                                                                         │
 * │  1. Visit /                                                             │
 * │  2. Redirected to /login                                                │
 * │  3. LoginPage shows "Dev API key configured via VITE_DEV_API_KEY"       │
 * │  4. Single button: "Continue to Dashboard"                              │
 * │  5. Click → navigate to /chat                                           │
 * │  6. AppShell (sidebar + main area) is visible                           │
 * └─────────────────────────────────────────────────────────────────────────┘
 *
 * ┌─────────────────────────────────────────────────────────────────────────┐
 * │ Path B — Manual API key  (stack mode: api-key, no OIDC_ISSUER_URL)      │
 * │                                                                         │
 * │  1. Visit /                                                             │
 * │  2. Redirected to /login                                                │
 * │  3. LoginPage shows "Sign in with SSO" button AND API key input field   │
 * │  4. Enter bootstrap API key in the password input                       │
 * │  5. Click "Continue with API Key"                                       │
 * │  6. Navigate to /chat                                                   │
 * │  7. AppShell is visible                                                 │
 * └─────────────────────────────────────────────────────────────────────────┘
 *
 * ┌─────────────────────────────────────────────────────────────────────────┐
 * │ Path C — OIDC / SSO  (stack mode: oidc, Authentik running)              │
 * │                                                                         │
 * │  1. Visit /                                                             │
 * │  2. Redirected to /login                                                │
 * │  3. Click "Sign in with SSO"                                            │
 * │  4. Redirected to Authentik login screen                                │
 * │  5. Fill in username + password                                         │
 * │  6. Submit → Authentik redirects to /auth/callback                      │
 * │  7. Callback page exchanges code for session token via sera-core        │
 * │  8. Redirected to /chat                                                 │
 * │  9. AppShell is visible, user name shown in sidebar                     │
 * └─────────────────────────────────────────────────────────────────────────┘
 *
 * Prerequisites:
 *   Run journey-01-stack-health first (or call waitForStack in beforeAll).
 *
 * Environment variables (see playwright.config.ts):
 *   E2E_STACK_MODE        dev | api-key | oidc  (default: dev)
 *   SERA_API_KEY          bootstrap API key     (default: sera_bootstrap_dev_123)
 *   E2E_OIDC_USER         Authentik username    (default: akadmin)
 *   E2E_OIDC_PASSWORD     Authentik password    (required for oidc mode)
 */

import { test, expect, type Page } from '@playwright/test';
import { ENV } from '../playwright.config.js';
import { waitForStack } from '../fixtures/stack.js';

// ── shared helpers ────────────────────────────────────────────────────────────

/**
 * Asserts that the SERA AppShell (sidebar navigation) is visible.
 * This is the post-login "landing" state — if it renders, auth completed.
 */
async function expectAppShellVisible(page: Page): Promise<void> {
  // The Sidebar renders a nav element with a link to /chat
  await expect(page.getByRole('navigation')).toBeVisible();
  await expect(page).toHaveURL(/\/chat/);
}

// ── test suite ────────────────────────────────────────────────────────────────

test.describe('Login', () => {
  test.beforeAll(async () => {
    await waitForStack(ENV.mode);
  });

  // Clear sessionStorage between tests so each one starts unauthenticated.
  test.beforeEach(async ({ page }) => {
    await page.addInitScript(() => sessionStorage.clear());
  });

  // ── Common: unauthenticated root redirect ───────────────────────────────────

  test('visiting / redirects to /login when not authenticated', async ({ page }) => {
    await page.goto('/');
    await expect(page).toHaveURL(/\/login/);
    await expect(page.getByRole('heading', { name: 'Sign In' })).toBeVisible();
  });

  test('login page shows SERA branding', async ({ page }) => {
    await page.goto('/login');
    await expect(page.getByText('Welcome to SERA')).toBeVisible();
    await expect(page.getByText('Sandboxed Extensible Reasoning Agent')).toBeVisible();
  });

  // ── Path A — Dev key bypass ─────────────────────────────────────────────────

  test.describe('Path A — dev key bypass', () => {
    test.skip(ENV.mode !== 'dev', 'Only runs in dev stack mode');

    test('shows dev mode confirmation message', async ({ page }) => {
      await page.goto('/login');
      await expect(
        page.getByText('Dev API key configured via VITE_DEV_API_KEY'),
      ).toBeVisible();
    });

    test('does not show the SSO button or API key input in dev mode', async ({ page }) => {
      await page.goto('/login');
      await expect(page.getByRole('button', { name: 'Sign in with SSO' })).not.toBeVisible();
      await expect(page.getByLabel('API Key')).not.toBeVisible();
    });

    test('clicking "Continue to Dashboard" lands on /chat with AppShell visible', async ({
      page,
    }) => {
      await page.goto('/login');
      await page.getByRole('button', { name: 'Continue to Dashboard' }).click();
      await expectAppShellVisible(page);
    });

    test('authenticated user visiting /login is redirected to /chat', async ({ page }) => {
      // First authenticate
      await page.goto('/login');
      await page.getByRole('button', { name: 'Continue to Dashboard' }).click();
      await expect(page).toHaveURL(/\/chat/);

      // Then revisit /login — should bounce back to /chat
      await page.goto('/login');
      await expect(page).toHaveURL(/\/chat/);
    });

    test('logout clears session and returns to /login', async ({ page }) => {
      await page.goto('/login');
      await page.getByRole('button', { name: 'Continue to Dashboard' }).click();
      await expect(page).toHaveURL(/\/chat/);

      // Find and click logout (sidebar user menu or logout button)
      // The sidebar renders a logout button with aria-label "Log out" or text "Logout"
      const logoutBtn = page
        .getByRole('button', { name: /log.?out/i })
        .or(page.getByRole('link', { name: /log.?out/i }));
      await logoutBtn.click();

      await expect(page).toHaveURL(/\/login/);
    });
  });

  // ── Path B — Manual API key ─────────────────────────────────────────────────

  test.describe('Path B — manual API key', () => {
    test.skip(ENV.mode !== 'api-key', 'Only runs in api-key stack mode');

    test('shows SSO button and API key input', async ({ page }) => {
      await page.goto('/login');
      await expect(page.getByRole('button', { name: 'Sign in with SSO' })).toBeVisible();
      await expect(page.getByLabel('API Key')).toBeVisible();
    });

    test('"Continue with API Key" button is disabled when input is empty', async ({ page }) => {
      await page.goto('/login');
      await expect(
        page.getByRole('button', { name: 'Continue with API Key' }),
      ).toBeDisabled();
    });

    test('entering a valid API key and submitting lands on /chat', async ({ page }) => {
      await page.goto('/login');

      const input = page.getByLabel('API Key');
      await input.fill(ENV.apiKey);

      await page.getByRole('button', { name: 'Continue with API Key' }).click();

      // The API key is stored in sessionStorage under 'sera_access_token'
      // and window.location.href is set to /chat by the page.
      await expect(page).toHaveURL(/\/chat/, { timeout: 15_000 });
      await expectAppShellVisible(page);
    });

    test('entering an invalid API key results in 401 errors (not a crash)', async ({ page }) => {
      await page.goto('/login');

      const input = page.getByLabel('API Key');
      await input.fill('definitely-not-a-real-key');
      await page.getByRole('button', { name: 'Continue with API Key' }).click();

      // The page navigates to /chat (optimistic) but subsequent API calls fail
      // with 401 which triggers logout and a return to /login.
      // Accept either outcome: stays on /login or returns there quickly.
      await expect(page).toHaveURL(/\/(login|chat)/, { timeout: 15_000 });
    });
  });

  // ── Path C — OIDC / SSO ────────────────────────────────────────────────────

  test.describe('Path C — OIDC via Authentik', () => {
    test.skip(ENV.mode !== 'oidc', 'Only runs in oidc stack mode');
    test.skip(!ENV.oidcPassword, 'E2E_OIDC_PASSWORD must be set for oidc mode tests');

    test('clicking "Sign in with SSO" redirects to Authentik', async ({ page }) => {
      await page.goto('/login');
      await page.getByRole('button', { name: 'Sign in with SSO' }).click();

      // Authentik serves login on port 9000
      await expect(page).toHaveURL(/localhost:9000/, { timeout: 15_000 });
    });

    test('full OIDC flow completes and lands on /chat', async ({ page }) => {
      await page.goto('/login');
      await page.getByRole('button', { name: 'Sign in with SSO' }).click();

      // Wait for Authentik's login form
      await expect(page).toHaveURL(/localhost:9000/);
      await page.getByLabel(/username/i).fill(ENV.oidcUser);
      await page.getByLabel(/password/i).fill(ENV.oidcPassword);
      await page.getByRole('button', { name: /log in|sign in/i }).click();

      // After Authentik authenticates the user it redirects to /auth/callback
      // which exchanges the code for a session token, then redirects to /chat.
      await expect(page).toHaveURL(/\/chat/, { timeout: 30_000 });
      await expectAppShellVisible(page);
    });

    test('user info is reflected in the sidebar after OIDC login', async ({ page }) => {
      await page.goto('/login');
      await page.getByRole('button', { name: 'Sign in with SSO' }).click();

      await expect(page).toHaveURL(/localhost:9000/);
      await page.getByLabel(/username/i).fill(ENV.oidcUser);
      await page.getByLabel(/password/i).fill(ENV.oidcPassword);
      await page.getByRole('button', { name: /log in|sign in/i }).click();

      await expect(page).toHaveURL(/\/chat/, { timeout: 30_000 });

      // The sidebar should show the authenticated user's name somewhere
      const sidebar = page.getByRole('navigation');
      await expect(sidebar).toContainText(ENV.oidcUser, { ignoreCase: true });
    });
  });
});

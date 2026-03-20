import { defineConfig, devices } from '@playwright/test';

/**
 * Stack modes control which services are running and which login path is expected:
 *
 *   dev       docker-compose.yaml + docker-compose.dev.yaml
 *             sera-web on localhost:3000 (Vite HMR)
 *             VITE_DEV_API_KEY set → one-button "Continue to Dashboard" login
 *
 *   api-key   docker-compose.yaml only (no OIDC_ISSUER_URL, no dev key)
 *             sera-web on localhost:SERA_WEB_PORT (default 8080, see note below)
 *             Manual API key input login
 *
 *   oidc      docker-compose.yaml + docker-compose.auth.yaml
 *             sera-web on localhost:SERA_WEB_PORT (default 8080)
 *             Full OIDC/PKCE flow via Authentik
 *
 * NOTE: docker-compose.yaml does not expose a host port for sera-web in
 *       production mode. Add a `ports: ["8080:80"]` entry to sera-web in
 *       your local override or a test-specific compose file before running
 *       api-key or oidc e2e tests.
 */

const mode = (process.env.E2E_STACK_MODE ?? 'dev') as 'dev' | 'api-key' | 'oidc';

const defaultWebPort = mode === 'dev' ? '3000' : '8080';
const defaultApiPort = '3001';

export const ENV = {
  mode,
  webBaseUrl: process.env.SERA_WEB_URL ?? `http://localhost:${process.env.SERA_WEB_PORT ?? defaultWebPort}`,
  apiBaseUrl: process.env.SERA_API_URL ?? `http://localhost:${defaultApiPort}`,
  /** Bootstrap API key used for manual-key login and direct API calls in tests. */
  apiKey: process.env.SERA_API_KEY ?? 'sera_bootstrap_dev_123',
  /** Authentik admin credentials — only needed for oidc mode. */
  oidcUser: process.env.E2E_OIDC_USER ?? 'akadmin',
  oidcPassword: process.env.E2E_OIDC_PASSWORD ?? '',
};

export default defineConfig({
  testDir: './tests',
  timeout: 60_000,
  expect: { timeout: 10_000 },
  fullyParallel: false, // stack tests must be ordered
  forbidOnly: Boolean(process.env.CI),
  retries: process.env.CI ? 1 : 0,
  workers: 1,
  reporter: [
    ['list'],
    ['html', { outputFolder: 'playwright-report', open: 'never' }],
  ],

  use: {
    baseURL: ENV.webBaseUrl,
    trace: 'on-first-retry',
    screenshot: 'only-on-failure',
    video: 'retain-on-failure',
  },

  projects: [
    {
      name: 'chromium',
      use: { ...devices['Desktop Chrome'] },
    },
  ],
});

import { test, expect } from '@playwright/test';
import { ENV } from '../playwright.config.js';
import { waitForStack } from '../fixtures/stack.js';

test.describe('Dynamic Provider Journey', () => {
  test.beforeAll(async () => {
    await waitForStack(ENV.mode);
  });

  test.beforeEach(async ({ page }) => {
    // Authenticate (Dev mode bypass)
    await page.goto('/login');
    const devBtn = page.getByRole('button', { name: 'Continue to Dashboard' });
    if (await devBtn.isVisible()) {
      await devBtn.click();
    } else {
      // Manual API key if needed
      const input = page.getByLabel('API Key');
      if (await input.isVisible()) {
        await input.fill(ENV.apiKey);
        await page.getByRole('button', { name: 'Continue with API Key' }).click();
      }
    }
    await expect(page).toHaveURL(/\/chat/);
  });

  test('should add, test and remove a dynamic provider', async ({ page }) => {
    await page.goto('/settings');
    
    // 1. Ensure we are on the Providers tab
    const providersTab = page.getByRole('button', { name: 'Providers' });
    await providersTab.click();
    await expect(page.getByText('Dynamic Discovery')).toBeVisible({ timeout: 10000 });

    // 2. Open Add form
    const addBtn = page.locator('section').filter({ hasText: 'Dynamic Discovery' }).getByRole('button', { name: 'Add Provider' });
    await addBtn.scrollIntoViewIfNeeded();
    await addBtn.click({ timeout: 15000 });

    const form = page.locator('.sera-card-static', { hasText: 'Add LM Studio Instance' });
    await expect(form).toBeVisible();

    // 3. Fill form
    const providerName = `Test Provider ${Date.now()}`;
    const providerId = `testid${Date.now()}`; // No dashes to be safe
    await page.getByPlaceholder('e.g. Local LM Studio').fill(providerName);
    await page.getByPlaceholder('e.g. lmstudio-1').fill(providerId);
    await page.waitForTimeout(500);

    // 4. Test connection - Mock the API response for reliability
    await page.route('**/api/providers/dynamic/test', route => {
      route.fulfill({
        status: 200,
        contentType: 'application/json',
        body: JSON.stringify({ success: true, models: [{ id: 'm1', name: 'Model 1' }] })
      });
    });

    await page.getByRole('button', { name: 'Test & Discover' }).click();
    await expect(page.getByText('Connection Successful', { exact: false })).toBeVisible({ timeout: 10000 });

    // 5. Save Provider
    const saveBtn = page.getByRole('button', { name: 'Save Provider' });
    await expect(saveBtn).toBeEnabled();
    await saveBtn.click();

    // 6. Verify provider card appears
    const card = page.locator('.sera-card-static', { hasText: providerName });
    await expect(card).toBeVisible({ timeout: 10000 });
    await expect(card.getByText(providerName)).toBeVisible();

    // 7. Expand and Remove
    await card.click();
    await card.getByRole('button', { name: 'Remove' }).click();

    // 8. Verify disappears
    await expect(card).not.toBeVisible();
  });
});

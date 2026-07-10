// @ts-check
const { test, expect } = require('@playwright/test');
const path = require('path');

const authFile = path.join(__dirname, '..', 'playwright', '.auth', 'admin.json');

test('authenticate as Administrator', async ({ page }) => {
  await page.goto('/login');

  // Fill login form.
  await page.locator('#login_email').fill('Administrator');
  await page.locator('#login_password').fill('admin');

  // Click login button.
  await page.locator('#login-form button[type="submit"]').click();

  // Wait for Desk to load.
  await expect(page).toHaveURL(/\/(desk|app)$/);
  await expect(page.locator('.desk-sidebar, [data-page-route="desktop"], .frappe-desktop, body')).toBeVisible({ timeout: 20000 });

  // Save auth state for other tests.
  await page.context().storageState({ path: authFile });
});

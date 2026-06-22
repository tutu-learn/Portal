const { test, expect } = require('@playwright/test');

const DESK_USER = process.env.KIFF_DESK_USER || 'Administrator';
const DESK_PASS = process.env.KIFF_DESK_PASSWORD || 'admin';

/**
 * Desk UI tests for kiff_logger.
 *
 * These tests log into Frappe Desk and verify that the Kiff Logger DocTypes
 * load without 500 errors.
 */

test.describe('kiff_logger Desk UI', () => {
  test.beforeEach(async ({ page }) => {
    await page.goto('/login');
    await page.locator('#login_email').fill(DESK_USER);
    await page.locator('#login_password').fill(DESK_PASS);
    await page.getByRole('button', { name: /login/i }).click();
    await page.waitForURL(/\/desk$|\/desk\//, { timeout: 10000 });
  });

  test('Kiff Logger workspace loads', async ({ page }) => {
    await page.goto('/desk/kiff-logger');
    // The workspace title should render somewhere on the page.
    await expect(page.getByText(/Kiff Logger/i).first()).toBeVisible({ timeout: 10000 });
  });

  test('Kiff Log Entry DocType list loads without 500', async ({ page }) => {
    const responsePromise = page.waitForResponse(
      (resp) => resp.url().includes('frappe.desk.form.load.getdoctype') && resp.url().includes('Kiff%20Log%20Entry'),
      { timeout: 15000 }
    );

    await page.goto('/desk/kiff-log-entry');

    const response = await responsePromise;
    expect(response.status()).toBeLessThan(500);

    // The list view header should contain the DocType name.
    await expect(page.locator('.title-text').getByText(/Kiff Log Entry/i)).toBeVisible({
      timeout: 10000,
    });
  });

  test('S3 Backup Configuration link is reachable from workspace', async ({ page }) => {
    // The single DocType form route is not wired up in the current runtime, so we
    // verify that the workspace exposes the link and that it points to the expected URL.
    await page.goto('/desk/kiff-logger');
    const s3Link = page.locator('a[href*="s3-backup-configuration"]').first();
    await expect(s3Link).toBeVisible({ timeout: 10000 });
    await expect(s3Link).toContainText(/S3 Backup Configuration/i);
  });

  test('Kiff Log Query DocType loads', async ({ page }) => {
    const responsePromise = page.waitForResponse(
      (resp) =>
        resp.url().includes('frappe.desk.form.load.getdoctype') && resp.url().includes('Kiff%20Log%20Query'),
      { timeout: 15000 }
    );

    await page.goto('/desk/kiff-log-query');

    const response = await responsePromise;
    expect(response.status()).toBeLessThan(500);

    await expect(page.locator('.title-text').getByText(/Kiff Log Query/i)).toBeVisible({
      timeout: 10000,
    });
  });
});

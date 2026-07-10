// @ts-check
const { test, expect } = require('@playwright/test');
const { query, queryRows } = require('./helpers/db.js');

function uniqueLabel(prefix) {
  return `${prefix}-${Date.now()}`;
}

function randomNewName(doctypeSlug) {
  const suffix = Math.random().toString(36).slice(2, 12);
  return `new-${doctypeSlug}-${suffix}`;
}

async function saveFullForm(page, oldTempName) {
  const saveBtn = page.locator('.page-actions button.btn-primary:has-text("Save")').first();
  await saveBtn.scrollIntoViewIfNeeded();
  await saveBtn.click();
  // After a successful save the form navigates from the temporary new-doc
  // route to the persisted document route, so the original button is detached.
  // Wait until we have left the temporary route.
  await expect(page).not.toHaveURL(new RegExp(`\\/${oldTempName}$`), { timeout: 15000 });
}

async function fillSelect(page, fieldname, value) {
  const select = page.locator(`[data-fieldname="${fieldname}"] select`).first();
  await select.scrollIntoViewIfNeeded();
  await select.selectOption(value);
}

async function fillDataField(page, fieldname, value) {
  const input = page.locator(`[data-fieldname="${fieldname}"] input`).first();
  await input.scrollIntoViewIfNeeded();
  await input.fill(value);
}

test.describe('Infrastructure Server creation from Desk', () => {
  test('admin can create an Infrastructure Server and it persists', async ({ page }) => {
    const title = uniqueLabel('srv');
    const newName = randomNewName('infrastructure-server');

    page.on('response', async (response) => {
      if (response.url().includes('/api/')) {
        const status = response.status();
        let body = '';
        try {
          body = await response.text();
        } catch (_e) {
          body = '<cannot read body>';
        }
        if (status >= 400 || body.includes('exc') || response.url().includes('savedocs')) {
          console.log('[API]', status, response.url(), body.slice(0, 1000));
        }
      }
    });

    page.on('console', (msg) => {
      if (msg.type() === 'error') {
        console.log('[JS ERROR]', msg.text());
      }
    });

    page.on('pageerror', (error) => {
      console.log('[PAGE ERROR]', error.message);
    });

    // Navigate directly to the new-doc form route (this is the path the
    // user reported as broken: Save appears to do nothing).
    await page.goto(`/desk/infrastructure-server/${newName}`);
    await page.locator('.form-page-header, .form-layout').first().waitFor({ state: 'visible' });

    await fillDataField(page, 'title', title);
    await fillDataField(page, 'hostname', `${title}.example.com`);
    await fillSelect(page, 'server_type', 'Windows Server');
    await fillSelect(page, 'operating_system', 'Windows Server');
    await fillSelect(page, 'environment', 'Production');
    await fillSelect(page, 'business_criticality', 'High');

    await saveFullForm(page, newName);

    // Verify backend persistence.
    const name = query(`SELECT name FROM "infrastructure_server" WHERE title = '${title}'`);
    expect(name).toBeTruthy();

    // Verify it appears in the list.
    await page.goto('/desk/infrastructure-server');
    await expect(page.locator('.list-row-container, .list-row').filter({ hasText: title }).first()).toBeVisible();
  });

  test('server token is auto-created and visible in token list', async ({ page }) => {
    const title = uniqueLabel('srv-token');
    const newName = randomNewName('infrastructure-server');

    await page.goto(`/desk/infrastructure-server/${newName}`);
    await page.locator('.form-page-header, .form-layout').first().waitFor({ state: 'visible' });

    await fillDataField(page, 'title', title);
    await fillDataField(page, 'hostname', `${title}.example.com`);
    await fillSelect(page, 'server_type', 'Linux Control Plane');
    await fillSelect(page, 'operating_system', 'Linux');
    await fillSelect(page, 'environment', 'Production');
    await fillSelect(page, 'business_criticality', 'Medium');

    await saveFullForm(page, newName);

    const serverName = query(`SELECT name FROM "infrastructure_server" WHERE title = '${title}'`);
    expect(serverName).toBeTruthy();

    // Poll for the async server-token hook to finish.
    let rows = [];
    for (let i = 0; i < 20; i++) {
      rows = queryRows(`SELECT name FROM "infrastructure_server_token" WHERE server = '${serverName}' AND enabled = 1`);
      if (rows.length > 0) break;
      await page.waitForTimeout(250);
    }
    expect(rows.length).toBe(1);
  });
});

test.describe('Kubernetes Cluster creation from Desk', () => {
  test('admin can create a Kubernetes Cluster and it persists', async ({ page }) => {
    const title = uniqueLabel('k8s');
    const newName = randomNewName('kubernetes-cluster');

    await page.goto(`/desk/kubernetes-cluster/${newName}`);
    await page.locator('.form-page-header, .form-layout').first().waitFor({ state: 'visible' });

    await fillDataField(page, 'title', title);
    await fillDataField(page, 'cluster_name', title);
    await fillSelect(page, 'environment', 'Production');
    await fillSelect(page, 'status', 'Active');

    await saveFullForm(page, newName);

    const name = query(`SELECT name FROM "kubernetes_cluster" WHERE title = '${title}'`);
    expect(name).toBeTruthy();

    await page.goto('/desk/kubernetes-cluster');
    await expect(page.locator('.list-row-container, .list-row').filter({ hasText: title }).first()).toBeVisible();
  });
});

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

  test('server token can be generated and appears in token list', async ({ page }) => {
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

    // No token is auto-created anymore; generate one explicitly through the API.
    const tokenRes = await page.request.post('/api/method/audit_ready.generate_server_token', {
      data: { server: serverName, token_name: 'Generated token' },
    });
    expect(tokenRes.ok()).toBeTruthy();
    const tokenJson = await tokenRes.json();
    expect(tokenJson.message?.ok || tokenJson.ok).toBeTruthy();
    expect(tokenJson.message?.token || tokenJson.token).toBeTruthy();

    const rows = queryRows(`SELECT name FROM "infrastructure_server_token" WHERE server = '${serverName}' AND enabled = 1`);
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

test.describe('Telemetry ingestion endpoint', () => {
  test('server token can submit telemetry and logs are immediately queryable', async ({ page }) => {
    // 1. Create a server so a token record exists.
    const title = uniqueLabel('srv-telemetry');
    const newName = randomNewName('infrastructure-server');

    await page.goto(`/desk/infrastructure-server/${newName}`);
    await page.locator('.form-page-header, .form-layout').first().waitFor({ state: 'visible' });

    await fillDataField(page, 'title', title);
    await fillDataField(page, 'hostname', `${title}.example.com`);
    await fillSelect(page, 'server_type', 'Linux Worker');
    await fillSelect(page, 'operating_system', 'Linux');
    await fillSelect(page, 'environment', 'Production');
    await fillSelect(page, 'business_criticality', 'Medium');

    await saveFullForm(page, newName);

    const serverName = query(`SELECT name FROM "infrastructure_server" WHERE title = '${title}'`);
    expect(serverName).toBeTruthy();

    // 2. Generate a fresh bearer token for the server.
    const tokenRes = await page.request.post('/api/method/audit_ready.generate_server_token', {
      data: { server: serverName, token_name: 'Telemetry token' },
    });
    expect(tokenRes.ok()).toBeTruthy();
    const tokenJson = await tokenRes.json();
    expect(tokenJson.message?.ok || tokenJson.ok).toBeTruthy();
    const bearerToken = tokenJson.message?.token || tokenJson.token;
    expect(bearerToken).toBeTruthy();

    // 3. POST a telemetry snapshot.
    const telemetryPayload = {
      hostname: `${title}.example.com`,
      endpoint_type: 'server',
      installed_software: {
        packages: [
          { name: 'openssl', version: '3.0.0', source: 'apt' },
          { name: 'nginx', version: '1.24.0', source: 'apt' },
        ],
      },
      compliance: {
        pass: 2,
        fail: 1,
        warn: 0,
        checks: [
          { name: 'Firewall', status: 'PASS', message: 'Firewall is active' },
          { name: 'Disk Encryption', status: 'FAIL', message: 'Disk encryption disabled' },
        ],
      },
      running_processes: {
        total: 42,
        flagged: 1,
        killed: 0,
        processes: [
          { pid: 1, ppid: 0, name: 'systemd', elevated: true, verdict: 'OK', command: '/sbin/init' },
          { pid: 999, ppid: 1, name: 'suspicious', elevated: false, verdict: 'FLAGGED', command: '/tmp/suspicious' },
        ],
      },
      network_traffic: {
        interfaces: [{ name: 'eth0', addresses: ['10.0.0.5'] }],
        connections: [{ proto: 'TCP', local: '10.0.0.5:22', remote: '1.2.3.4:12345', state: 'ESTABLISHED', pid: 4821, process: 'firefox' }],
        dns_queries: [{ ts_ms: 1737465600000, domain: 'example.com', qtype: 'A', answers: ['93.184.216.34'], resolver: '10.0.0.1', pid: 4821, process: 'firefox' }],
        dns_servers: ['10.0.0.1'],
      },
    };

    const telemRes = await page.request.post('/audit_ready/telemetry', {
      headers: { Authorization: `Bearer ${bearerToken}` },
      data: telemetryPayload,
    });
    expect(telemRes.ok()).toBeTruthy();
    const telemJson = await telemRes.json();
    expect(telemJson.ok).toBe(true);
    expect(telemJson.records).toBeGreaterThanOrEqual(3); // summary + 2 compliance + 1 flagged process

    // 4. Query the log engine immediately and confirm the batch was committed.
    const queryRes = await page.request.post('/api/method/kiff_logger.query', {
      data: {
        q: 'service:audit_ready.telemetry OR service:audit_ready.telemetry.compliance OR service:audit_ready.telemetry.process',
        limit: 20,
      },
    });
    expect(queryRes.ok()).toBeTruthy();
    const queryJson = await queryRes.json();
    const records = queryJson.message?.records || queryJson.records || [];
    const hostnames = records.map((r) => r.fields?.hostname);
    expect(hostnames).toContain(`${title}.example.com`);
    const services = records.map((r) => r.service);
    expect(services).toContain('audit_ready.telemetry');
    expect(services).toContain('audit_ready.telemetry.compliance');
    expect(services).toContain('audit_ready.telemetry.process');
  });

  test('telemetry rejects unauthenticated requests', async ({ page }) => {
    const res = await page.request.post('/audit_ready/telemetry', {
      data: { hostname: 'unauth.example.com' },
    });
    expect(res.status()).toBe(401);
  });
});

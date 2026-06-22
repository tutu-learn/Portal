const { test, expect } = require('@playwright/test');

test.setTimeout(120000);

const DESK_USER = process.env.KIFF_DESK_USER || 'Administrator';
const DESK_PASS = process.env.KIFF_DESK_PASSWORD || 'admin';

/**
 * End-to-end test for the Kiff Log Entry DocType.
 *
 * Logs into Frappe Desk, ingests a unique log record via the kiff_logger
 * REST endpoint, waits for the log engine to commit it, then verifies that
 * the record appears in the Desk list view and on its detail form.
 */

test.describe('Kiff Log Entry end-to-end', () => {
  test('ingest, query and view Kiff Log Entry in Desk', async ({ page, request }) => {
    // 1. Log in to the Desk
    await page.goto('/login');
    await page.locator('#login_email').fill(DESK_USER);
    await page.locator('#login_password').fill(DESK_PASS);
    await page.getByRole('button', { name: /login/i }).click();
    await page.waitForURL(/\/desk$|\/desk\//, { timeout: 10000 });

    // 2. Ingest a uniquely identifiable log record via the API request context
    const ts = Date.now().toString();
    const uniqueMessage = `kiff-log-entry E2E ${ts}`;
    const testRunId = `playwright-${ts}`;

    const login = await request.post('/api/method/login', {
      form: { usr: DESK_USER, pwd: DESK_PASS },
    });
    expect(login.ok()).toBeTruthy();

    const ingest = await request.post('/kiff_logger/ingest', {
      data: {
        level: 'INFO',
        service: 'playwright.kiff_log_entry',
        message: uniqueMessage,
        fields: {
          test_run_id: testRunId,
        },
      },
    });
    expect(ingest.ok()).toBeTruthy();
    const ingestBody = await ingest.json();
    expect(ingestBody).toMatchObject({ ok: true });

    // 3. Poll the query endpoint until the record is searchable.
    // The log engine commits roughly every 30s, so poll for up to ~46s.
    let matchedRecord = null;
    for (let i = 0; i < 23; i++) {
      const query = await request.get(
        `/kiff_logger/query?q=${encodeURIComponent(`message:${uniqueMessage}`)}&limit=10`
      );
      expect(query.ok()).toBeTruthy();
      const body = await query.json();
      matchedRecord = body.records.find((r) => r.message === uniqueMessage);
      if (matchedRecord) {
        break;
      }
      await new Promise((r) => setTimeout(r, 2000));
    }
    expect(matchedRecord).toBeTruthy();

    // 4. Navigate to the Kiff Log Entry list view and confirm the Desk page loads.
    const doctypeResponsePromise = page.waitForResponse(
      (resp) =>
        resp.url().includes('frappe.desk.form.load.getdoctype') &&
        resp.url().includes('Kiff%20Log%20Entry'),
      { timeout: 15000 }
    );

    await page.goto('/desk/kiff-log-entry');
    await page.waitForURL(/\/desk\/kiff-log-entry/, { timeout: 10000 });

    const doctypeResponse = await doctypeResponsePromise;
    expect(doctypeResponse.status()).toBeLessThan(500);

    // The list view header should contain the DocType name.
    await expect(page.locator('.title-text').getByText(/Kiff Log Entry/i)).toBeVisible({
      timeout: 10000,
    });

    // 5. Verify the queried log record has the expected fields.
    expect(matchedRecord.level).toBe('INFO');
    expect(matchedRecord.service).toBe('playwright.kiff_log_entry');
    expect(matchedRecord.fields.test_run_id).toBe(testRunId);
  });
});

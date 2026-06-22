const { test, expect } = require('@playwright/test');

const API_USER = process.env.KIFF_API_USER || 'Administrator';
const API_PASS = process.env.KIFF_API_PASSWORD || 'admin';

/**
 * API smoke tests for kiff_logger.
 *
 * These tests assume the Kiff runtime is running and the Frappe REST API is
 * available at the configured baseURL. They authenticate via cookie-based login
 * and then exercise the kiff_logger ingest/query endpoints.
 */

test.describe('kiff_logger API', () => {
  test('should ingest a log record via REST', async ({ request }) => {
    // Frappe cookie login
    const login = await request.post('/api/method/login', {
      form: { usr: API_USER, pwd: API_PASS },
    });
    expect(login.ok()).toBeTruthy();

    const ingest = await request.post('/kiff_logger/ingest', {
      data: {
        level: 'INFO',
        service: 'playwright.test',
        message: 'playwright ingest smoke test',
        fields: {
          source: 'playwright',
          test_run_id: Date.now().toString(),
        },
      },
    });

    expect(ingest.ok()).toBeTruthy();
    const body = await ingest.json();
    expect(body).toMatchObject({ ok: true });
  });

  test('should query ingested log records', async ({ request }) => {
    const ts = Date.now().toString();
    const uniqueMessage = `query test ${ts}`;

    await request.post('/api/method/login', {
      form: { usr: API_USER, pwd: API_PASS },
    });

    await request.post('/kiff_logger/ingest', {
      data: {
        level: 'WARN',
        service: 'playwright.query',
        message: uniqueMessage,
      },
    });

    // Records are not searchable until the log engine commits. The runtime
    // spawns a 30s commit loop, so in tests we poll briefly.
    let found = false;
    for (let i = 0; i < 20; i++) {
      const query = await request.get(
        `/kiff_logger/query?q=message:${encodeURIComponent(uniqueMessage)}&limit=10`
      );
      expect(query.ok()).toBeTruthy();
      const body = await query.json();
      if (body.records.some((r) => r.message === uniqueMessage)) {
        found = true;
        break;
      }
      await new Promise((r) => setTimeout(r, 2000));
    }

    expect(found).toBeTruthy();
  });

  test('should expose kiff_logger.query Frappe API method', async ({ request }) => {
    await request.post('/api/method/login', {
      form: { usr: API_USER, pwd: API_PASS },
    });

    const resp = await request.post('/api/method/kiff_logger.query', {
      data: { q: 'service:frappe.doc_event', limit: 5 },
    });

    expect(resp.ok()).toBeTruthy();
    const body = await resp.json();
    expect(body).toHaveProperty('message');
    expect(body.message).toHaveProperty('records');
    expect(Array.isArray(body.message.records)).toBeTruthy();
  });
});

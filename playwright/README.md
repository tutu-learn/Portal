# Kiff Logger Playwright Tests

End-to-end and API tests for the `kiff_logger` app.

## Setup

```bash
cd playwright
npm install
npx playwright install chromium
```

## Run

Make sure the Kiff runtime is running (default `http://localhost:8000`):

```bash
npm test
```

Override credentials or base URL via environment variables:

```bash
KIFF_BASE_URL=http://localhost:8080 \
KIFF_DESK_USER=Administrator \
KIFF_DESK_PASSWORD=admin \
npm test
```

## What is tested

- `01_kiff_logger_api.spec.js`
  - `POST /kiff_logger/ingest`
  - `GET /kiff_logger/query`
  - `POST /api/method/kiff_logger.query`

- `02_kiff_logger_ui.spec.js`
  - Kiff Logger workspace loads
  - `Kiff Log Entry`, `Kiff Log Query`, and `S3 Backup Configuration` DocType list views load without 500 errors

## Notes

- The query test polls for up to 40 seconds because the log engine commits on a 30-second loop.
- UI selectors assume the standard Frappe Desk layout. If your theme or version differs, selectors may need adjustment.

// @ts-check
const { execSync } = require('child_process');
const fs = require('fs');
const path = require('path');

const PROJECT_ROOT = path.resolve(__dirname, '..', '..');
const LOCKFILE = path.join(PROJECT_ROOT, 'playwright', '.e2e-server.lock');

function getDbPath() {
  if (!fs.existsSync(LOCKFILE)) {
    throw new Error('e2e server lockfile not found');
  }
  const lock = JSON.parse(fs.readFileSync(LOCKFILE, 'utf8'));
  return path.join(lock.runtimeDir, 'sites', 'localhost', 'site.db');
}

/**
 * Run a SQLite query against the ephemeral e2e site database. Because the
 * runtime keeps long-lived WAL-mode connections, external sqlite3 processes
 * may not see un-checkpointed writes. Each helper invocation runs
 * `PRAGMA wal_checkpoint(FULL)` in the same sqlite3 session as the query so
 * the read is current.
 *
 * @param {string} sql
 * @returns {string}
 */
function query(sql) {
  const dbPath = getDbPath();
  const script = `.mode json
PRAGMA wal_checkpoint(FULL);
${sql};`;
  const out = execSync(`sqlite3 "${dbPath}"`, {
    encoding: 'utf8',
    input: script,
  }).trim();
  if (!out) return '';
  const lines = out.split('\n');
  // First line is the JSON array for the checkpoint result; remaining lines
  // are the query result.
  const queryJson = lines.slice(1).join('\n');
  if (!queryJson) return '';
  const rows = JSON.parse(queryJson);
  if (!Array.isArray(rows) || rows.length === 0) return '';
  const first = rows[0];
  const values = Object.values(first);
  return values.length > 0 ? String(values[0]) : '';
}

/**
 * Run a SQLite query that returns rows. Output is read via JSON mode so we
 * do not have to parse CSV.
 *
 * @param {string} sql
 * @returns {Record<string, any>[]}
 */
function queryRows(sql) {
  const dbPath = getDbPath();
  const script = `.mode json
PRAGMA wal_checkpoint(FULL);
${sql};`;
  const out = execSync(`sqlite3 "${dbPath}"`, {
    encoding: 'utf8',
    input: script,
  }).trim();
  if (!out) return [];
  const lines = out.split('\n');
  const queryJson = lines.slice(1).join('\n');
  if (!queryJson) return [];
  const rows = JSON.parse(queryJson);
  return Array.isArray(rows) ? rows : [];
}

module.exports = { query, queryRows };

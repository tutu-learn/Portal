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
 * Run a SQLite query against the ephemeral e2e site database.
 * @param {string} sql
 * @returns {string}
 */
function query(sql) {
  const dbPath = getDbPath();
  return execSync(`sqlite3 "${dbPath}" "${sql.replace(/"/g, '""')}"`, {
    encoding: 'utf8',
  }).trim();
}

/**
 * Quote an identifier for SQLite.
 * @param {string} name
 * @returns {string}
 */
function quoteIdent(name) {
  return `"${name.replace(/"/g, '""')}"`;
}

/**
 * Run a SQLite query that returns rows, parse the pipe-delimited output.
 * @param {string} sql
 * @returns {Record<string, string>[]}
 */
function queryRows(sql) {
  const dbPath = getDbPath();
  const out = execSync(`sqlite3 -header -csv "${dbPath}" "${sql.replace(/"/g, '""')}"`, {
    encoding: 'utf8',
  }).trim();
  if (!out) return [];
  const lines = out.split('\n');
  const headers = lines[0].split(',').map((h) => h.replace(/^"|"$/g, ''));
  return lines.slice(1).map((line) => {
    // Very simple CSV parse: assume values are not quoted with commas inside.
    const values = line.split(',').map((v) => v.replace(/^"|"$/g, ''));
    const row = {};
    headers.forEach((h, i) => (row[h] = values[i]));
    return row;
  });
}

module.exports = { query, queryRows };

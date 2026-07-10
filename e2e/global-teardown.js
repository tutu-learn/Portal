// @ts-check
const fs = require('fs');
const path = require('path');

const PROJECT_ROOT = path.resolve(__dirname, '..');
const LOCKFILE = path.join(PROJECT_ROOT, 'playwright', '.e2e-server.lock');

function log(msg) {
  console.log(`[e2e teardown] ${msg}`);
}

module.exports = async function globalTeardown() {
  if (!fs.existsSync(LOCKFILE)) {
    log('no lockfile found; nothing to tear down');
    return;
  }

  const lock = JSON.parse(fs.readFileSync(LOCKFILE, 'utf8'));

  if (lock.pid) {
    log(`killing runtime process ${lock.pid}`);
    try {
      process.kill(lock.pid, 'SIGTERM');
    } catch (e) {
      log(`process ${lock.pid} was already gone: ${e.message}`);
    }
  }

  if (lock.runtimeDir && fs.existsSync(lock.runtimeDir)) {
    log(`cleaning up temp runtime dir ${lock.runtimeDir}`);
    fs.rmSync(lock.runtimeDir, { recursive: true, force: true });
  }

  fs.rmSync(LOCKFILE, { force: true });
  log('teardown complete');
};

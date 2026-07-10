// @ts-check
const { execSync, spawn } = require('child_process');
const fs = require('fs');
const path = require('path');
const { setTimeout } = require('timers/promises');

const PROJECT_ROOT = path.resolve(__dirname, '..');
const RUNTIME_BINARY = path.join(PROJECT_ROOT, 'target', 'debug', 'kiff-runtime');
const LOCKFILE = path.join(PROJECT_ROOT, 'playwright', '.e2e-server.lock');
const E2E_BASE_PORT = 8765;

function log(msg) {
  console.log(`[e2e setup] ${msg}`);
}

function ensureRuntimeBuilt() {
  if (fs.existsSync(RUNTIME_BINARY)) {
    log(`runtime already built: ${RUNTIME_BINARY}`);
    return;
  }
  log('building kiff-runtime...');
  execSync('cargo build -p runtime', {
    cwd: PROJECT_ROOT,
    stdio: 'inherit',
  });
}

function makeTempDir() {
  const tmpBase = path.join(PROJECT_ROOT, 'sites-e2e');
  fs.mkdirSync(tmpBase, { recursive: true });
  const dir = fs.mkdtempSync(path.join(tmpBase, 'run-'));
  log(`temp runtime dir: ${dir}`);
  return dir;
}

function writeRuntimeToml(runtimeDir) {
  const toml = [
    '[runtime]',
    `frappe_path  = "${path.join(PROJECT_ROOT, 'apps', 'frappe').replace(/\\/g, '/')}"`,
    'erpnext_path = ""',
    `shim_path    = "${path.join(PROJECT_ROOT, 'python').replace(/\\/g, '/')}"`,
    `sites_path   = "${path.join(runtimeDir, 'sites').replace(/\\/g, '/')}"`,
    '',
    '[database]',
    'driver = "sqlite"',
    `url    = "${path.join(runtimeDir, 'sites', '{site}', 'site.db').replace(/\\/g, '/')}"`,
    '',
    '[server]',
    'host    = "127.0.0.1"',
    `port    = ${E2E_BASE_PORT}`,
    'workers = 1',
    '',
    '[queue]',
    'short_workers   = 1',
    'default_workers = 1',
    'long_workers    = 1',
  ].join('\n');
  fs.writeFileSync(path.join(runtimeDir, 'runtime.toml'), toml);
}

function createSite(runtimeDir) {
  const siteDir = path.join(runtimeDir, 'sites', 'localhost');
  fs.mkdirSync(path.join(siteDir, 'private', 'files'), { recursive: true });
  fs.mkdirSync(path.join(siteDir, 'private', 'backups'), { recursive: true });
  fs.mkdirSync(path.join(siteDir, 'public', 'files'), { recursive: true });

  const siteConfig = {
    db_driver: 'sqlite',
    db_url: path.join(siteDir, 'site.db').replace(/\\/g, '/'),
    encryption_key: '',
    secret_key: '',
    mail_server: '',
    mail_port: 587,
    mail_login: '',
    file_size_limit: 25,
  };
  fs.writeFileSync(
    path.join(siteDir, 'site_config.json'),
    JSON.stringify(siteConfig, null, 2)
  );
}

async function waitForServer(url, timeoutMs = 120000) {
  const start = Date.now();
  while (Date.now() - start < timeoutMs) {
    try {
      const res = await fetch(url);
      if (res.status === 200 || res.status === 302 || res.status === 404) {
        return;
      }
    } catch (_e) {
      // server not ready yet
    }
    await setTimeout(250);
  }
  throw new Error(`server did not become ready within ${timeoutMs}ms`);
}

module.exports = async function globalSetup() {
  if (fs.existsSync(LOCKFILE)) {
    log('lockfile exists; assuming server is already managed externally');
    return;
  }

  ensureRuntimeBuilt();
  const runtimeDir = makeTempDir();
  writeRuntimeToml(runtimeDir);
  createSite(runtimeDir);

  log('starting kiff-runtime...');
  const proc = spawn(RUNTIME_BINARY, [], {
    cwd: PROJECT_ROOT,
    stdio: 'pipe',
    detached: false,
    env: {
      ...process.env,
      KIFF_RUNTIME_CONFIG: path.join(runtimeDir, 'runtime.toml'),
    },
  });

  proc.stdout.on('data', (data) => process.stdout.write(data));
  proc.stderr.on('data', (data) => process.stderr.write(data));

  const lock = {
    pid: proc.pid,
    runtimeDir,
    startedAt: new Date().toISOString(),
  };
  fs.mkdirSync(path.dirname(LOCKFILE), { recursive: true });
  fs.writeFileSync(LOCKFILE, JSON.stringify(lock, null, 2));

  try {
    await waitForServer(`http://127.0.0.1:${E2E_BASE_PORT}/api/method/audit_ready.hello`);
    log('server ready');
  } catch (e) {
    log('server failed to start, killing process');
    proc.kill('SIGTERM');
    fs.rmSync(runtimeDir, { recursive: true, force: true });
    fs.rmSync(LOCKFILE, { force: true });
    throw e;
  }
};

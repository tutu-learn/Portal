// @ts-check
const http = require('http');

/**
 * Mock Audit Ready s2s server for e2e tests. Records every request and
 * answers with spec-shaped responses. Control behavior via `state`:
 *   - failWith: HTTP status POST /deployments should fail with (null = ok)
 *   - liveStatus: status reported by GET /deployments/:id live_status
 *   - liveOutput: console-output tail reported in live_status.output
 */
async function startMockAuditReady() {
  const state = {
    requests: [], // { method, path, token, operator, body }
    failWith: null,
    liveStatus: 'Running',
    liveOutput: '[mock] pulling artifacts...\n[mock] deploy complete',
  };
  const server = http.createServer((req, res) => {
    let chunks = [];
    req.on('data', (c) => chunks.push(c));
    req.on('end', () => {
      const body = chunks.length ? JSON.parse(Buffer.concat(chunks).toString()) : null;
      state.requests.push({
        method: req.method,
        path: req.url,
        token: req.headers.authorization || '',
        operator: req.headers['x-operator-email'] || '',
        body,
      });

      res.setHeader('Content-Type', 'application/json');
      if (req.method === 'POST' && req.url === '/audit_ready/s2s/deployments') {
        if (state.failWith) {
          res.statusCode = state.failWith;
          res.end(JSON.stringify({ ok: false, message: `mock failure ${state.failWith}` }));
          return;
        }
        const id = `dep-mock-${state.requests.length}`;
        res.end(JSON.stringify({ ok: true, deployment_id: id, ref_id: `ref-${id}`, status: 'Queued' }));
        return;
      }
      if (req.method === 'GET' && req.url.startsWith('/audit_ready/s2s/deployments/')) {
        res.end(
          JSON.stringify({
            id: req.url.split('/').pop(),
            live_status: { status: state.liveStatus, progress: 50, result: '', error: '', output: state.liveOutput },
          })
        );
        return;
      }
      // The mock tracks no ids — like GET, DELETE of any id succeeds.
      if (req.method === 'DELETE' && req.url.startsWith('/audit_ready/s2s/deployments/')) {
        res.end(JSON.stringify({ ok: true }));
        return;
      }
      if (req.method === 'GET' && req.url === '/audit_ready/s2s/servers') {
        res.end(
          JSON.stringify([
            { name: 'win-web-01', server_name: 'win-web-01', environment: 'dev', status: 'Active' },
            { name: 'win-web-02', server_name: 'win-web-02', environment: 'prod', status: 'Active' },
          ])
        );
        return;
      }
      if (req.method === 'GET' && req.url === '/audit_ready/s2s/clusters') {
        res.end(
          JSON.stringify([
            { name: 'aks-prod-01', cluster_name: 'aks-prod-01', environment: 'prod', status: 'Active' },
          ])
        );
        return;
      }
      res.statusCode = 404;
      res.end(JSON.stringify({ ok: false, message: 'not found' }));
    });
  });
  await new Promise((resolve) => server.listen(0, '127.0.0.1', resolve));
  const port = server.address().port;
  return {
    state,
    url: `http://127.0.0.1:${port}`,
    close: () => new Promise((resolve) => server.close(resolve)),
  };
}

module.exports = { startMockAuditReady };

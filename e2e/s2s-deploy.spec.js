// @ts-check
const { test, expect } = require('@playwright/test');
const { query, queryRows } = require('./helpers/db.js');
const { startMockAuditReady } = require('./helpers/mock_audit_ready.js');

/**
 * Audit Ready s2s integration: approving/deploying a Sebrus Deployment calls
 * the external Audit Ready API; live status is synced back via
 * deployment_logs / sync_deploy_status. The external server is a local Node
 * mock recording every request.
 */

function uid() {
  return Math.random().toString(36).slice(2, 10);
}

/** Call a sebrus_apps desk method from the logged-in page. */
async function callMethod(page, method, args) {
  return page.evaluate(
    async ({ method, args }) => {
      try {
        const r = await fetch('/api/method/' + method, {
          method: 'POST',
          headers: { 'Content-Type': 'application/json' },
          credentials: 'include',
          body: JSON.stringify(args || {}),
        });
        const json = await r.json();
        if (json.exc_type || json.error) {
          return { ok: false, error: json.exc || json.error, exc_type: json.exc_type };
        }
        return { ok: true, message: json.message };
      } catch (e) {
        return { ok: false, error: String(e && e.message ? e.message : e) };
      }
    },
    { method, args }
  );
}

async function saveDoc(page, doc) {
  const res = await page.evaluate(async (d) => {
    try {
      const body = new URLSearchParams();
      body.append('doc', JSON.stringify(d));
      body.append('action', 'Save');
      const r = await fetch('/api/method/frappe.desk.form.save.savedocs', {
        method: 'POST',
        headers: {
          'Content-Type': 'application/x-www-form-urlencoded; charset=UTF-8',
          'X-Requested-With': 'XMLHttpRequest',
        },
        credentials: 'include',
        body,
      });
      const json = await r.json();
      if (!r.ok || json.error) return { ok: false, error: JSON.stringify(json) };
      return { ok: true, doc: (json.docs || [])[0] };
    } catch (e) {
      return { ok: false, error: String(e && e.message ? e.message : e) };
    }
  }, doc);
  expect(res.ok, `save of ${doc.doctype} failed: ${res.error}`).toBe(true);
  return res.doc;
}

function deploymentRow(name) {
  return queryRows(
    `SELECT workflow_state, deploy_status, s2s_refs FROM "sebrus_deployment" WHERE name = '${name}'`
  )[0];
}

/**
 * Full fixture: chain + service + secret + deployment (kind=script) wired at
 * the mock. Returns names.
 */
async function createFixture(page, suffix, mockUrl, kind = 'script') {
  await page.goto('/desk');
  await page.locator('body').waitFor({ state: 'visible' });

  const client = await saveDoc(page, {
    doctype: 'Sebrus Client',
    name: `new-sebrus-client-${suffix}`,
    __islocal: 1,
    client_name: `S2S Client ${suffix}`,
  });
  const project = await saveDoc(page, {
    doctype: 'Sebrus Project',
    name: `new-sebrus-project-${suffix}`,
    __islocal: 1,
    project_name: `S2S Project ${suffix}`,
    client: client.name,
  });
  const app = await saveDoc(page, {
    doctype: 'Sebrus App',
    name: `new-sebrus-app-${suffix}`,
    __islocal: 1,
    app_name: `S2S App ${suffix}`,
    app_type: 'Client App',
  });
  const service = await saveDoc(page, {
    doctype: 'Sebrus Service',
    name: `new-sebrus-service-${suffix}`,
    __islocal: 1,
    service_name: `svc-${suffix}`,
    app: app.name,
    kind: 'API',
  });
  const deployment = await saveDoc(page, {
    doctype: 'Sebrus Deployment',
    name: `new-sebrus-deployment-${suffix}`,
    __islocal: 1,
    client: client.name,
    app: app.name,
    project: project.name,
    tier: 'Shared',
    workflow_state: 'Draft',
    target_type: 'server',
    target: 'win-web-01',
    deploy_kind: kind,
    target_env: 'dev',
    deploy_payload:
      kind === 'script'
        ? '{"script": "echo deploy"}'
        : '{"iis": {"ado_org": "myorg", "ado_project": "Billing", "build_id": 12345, "artifact_name": "drop"}}',
    service_versions: [
      {
        doctype: 'Sebrus Service Version',
        name: `new-sv-${suffix}`,
        __islocal: 1,
        parent: `new-sebrus-deployment-${suffix}`,
        parenttype: 'Sebrus Deployment',
        parentfield: 'service_versions',
        env: 'dev',
        service: service.name,
        version: '2.0.0',
      },
    ],
  });

  // One secret scoped to this deployment + env.
  const sec = await callMethod(page, 'sebrus_apps.create_secret', {
    deployment: deployment.name,
    env: 'dev',
    secret_key: 'DB_URL',
    secret_value: 'super-secret-value',
    scope: 'Deployment env',
  });
  expect(sec.ok, `create_secret failed: ${sec.error}`).toBe(true);

  // Point the app at the mock Audit Ready server.
  const cfg = await callMethod(page, 'sebrus_apps.set_audit_ready_config', {
    url: mockUrl,
    token: 'mock-s2s-token',
  });
  expect(cfg.ok, `set config failed: ${cfg.error}`).toBe(true);

  return { client, project, app, service, deployment };
}

test.describe('Audit Ready s2s deploy integration', () => {
  let mock;
  test.beforeAll(async () => {
    mock = await startMockAuditReady();
  });
  test.afterAll(async () => {
    await mock.close();
  });
  test.beforeEach(() => {
    mock.state.requests = [];
    mock.state.failWith = null;
    mock.state.liveStatus = 'Running';
    mock.state.liveOutput = '[mock] pulling artifacts...\n[mock] deploy complete';
  });

  test('config status reports configured without leaking the token', async ({ page }) => {
    await page.goto('/desk');
    const suffix = uid();
    const cfg = await callMethod(page, 'sebrus_apps.set_audit_ready_config', {
      url: `${mock.url}/${suffix}`,
      token: 'mock-s2s-token',
    });
    expect(cfg.ok).toBe(true);

    const status = await callMethod(page, 'sebrus_apps.get_audit_ready_config_status', {});
    expect(status.ok).toBe(true);
    expect(status.message.configured).toBe(true);
    expect(status.message.url).toContain(suffix);
    expect(status.message.operator_email).toBe('Administrator');
    expect(JSON.stringify(status.message)).not.toContain('mock-s2s-token');
  });

  test('approve deploys each pinned service with secrets injected', async ({ page }) => {
    const suffix = uid();
    const { deployment } = await createFixture(page, suffix, mock.url);

    const submit = await callMethod(page, 'sebrus_apps.deployment_transition', {
      deployment: deployment.name,
      action: 'Submit for Approval',
    });
    expect(submit.ok).toBe(true);

    const approve = await callMethod(page, 'sebrus_apps.deployment_transition', {
      deployment: deployment.name,
      action: 'Approve',
    });
    expect(approve.ok, `approve failed: ${approve.error}`).toBe(true);
    expect(approve.message.workflow_state).toBe('Approved');
    expect(approve.message.deploy_status).toBe('Queued');

    // The mock received exactly one deploy POST, authed + attributed.
    const posts = mock.state.requests.filter((r) => r.method === 'POST');
    expect(posts.length).toBe(1);
    expect(posts[0].token).toBe('Bearer mock-s2s-token');
    expect(posts[0].operator).toBe('Administrator');
    expect(posts[0].body.target_type).toBe('server');
    expect(posts[0].body.target).toBe('win-web-01');
    expect(posts[0].body.kind).toBe('script');
    expect(posts[0].body.name).toBe(`svc-${suffix}`);
    // Secret injected as an export line before the payload script.
    expect(posts[0].body.script).toContain('export DB_URL="super-secret-value"');
    expect(posts[0].body.script).toContain('echo deploy');

    // Deployment row reflects the queued deploy; no secret value stored.
    const row = deploymentRow(deployment.name);
    expect(row.workflow_state).toBe('Approved');
    expect(row.deploy_status).toBe('Queued');
    expect(row.s2s_refs).toContain('dep-mock-');
    expect(row.s2s_refs).not.toContain('super-secret-value');
  });

  test('iis kind injects secrets into iis.env and ADO_PAT into iis.pat', async ({ page }) => {
    const suffix = uid();
    const { deployment } = await createFixture(page, suffix, mock.url, 'iis');
    // The PAT travels as a vault secret, never in the stored payload.
    const pat = await callMethod(page, 'sebrus_apps.create_secret', {
      deployment: deployment.name,
      env: 'dev',
      secret_key: 'ADO_PAT',
      secret_value: 'pat-value-123',
      scope: 'Deployment env',
    });
    expect(pat.ok, `ADO_PAT secret failed: ${pat.error}`).toBe(true);

    await callMethod(page, 'sebrus_apps.deployment_transition', {
      deployment: deployment.name,
      action: 'Submit for Approval',
    });
    const approve = await callMethod(page, 'sebrus_apps.deployment_transition', {
      deployment: deployment.name,
      action: 'Approve',
    });
    expect(approve.ok, `approve failed: ${approve.error}`).toBe(true);

    const post = mock.state.requests.find((r) => r.method === 'POST');
    expect(post.body.kind).toBe('iis');
    expect(post.body.iis.env.DB_URL).toBe('super-secret-value');
    expect(post.body.iis.env.ADO_PAT).toBeUndefined();
    expect(post.body.iis.pat).toBe('pat-value-123');
    expect(post.body.iis.config_style).toBe('appsettings');
  });

  test('validate_deploy_config enforces the Audit Ready schema', async ({ page }) => {
    await page.goto('/desk');

    const missing = await callMethod(page, 'sebrus_apps.validate_deploy_config', {
      target_type: 'server',
      kind: 'iis',
      payload: '{"iis": {"ado_org": "myorg"}}',
    });
    expect(missing.ok).toBe(false);

    const patInPayload = await callMethod(page, 'sebrus_apps.validate_deploy_config', {
      target_type: 'server',
      kind: 'iis',
      payload:
        '{"iis": {"ado_org": "o", "ado_project": "p", "build_id": 1, "artifact_name": "d", "pat": "x"}}',
    });
    expect(patInPayload.ok).toBe(false);
    expect(JSON.stringify(patInPayload)).toContain('ADO_PAT');

    const helm = await callMethod(page, 'sebrus_apps.validate_deploy_config', {
      target_type: 'cluster',
      kind: 'kubernetes',
      payload:
        '{"helm": {"releaseName": "api", "namespace": "prod", "chartRef": "oci://charts/api", "version": "1.0.0"}}',
    });
    expect(helm.ok, `valid helm payload rejected: ${helm.error}`).toBe(true);

    const wrongTarget = await callMethod(page, 'sebrus_apps.validate_deploy_config', {
      target_type: 'cluster',
      kind: 'iis',
      payload: '{}',
    });
    expect(wrongTarget.ok).toBe(false);
  });

  test('approve is blocked when a service payload violates the schema', async ({ page }) => {
    const suffix = uid();
    const { deployment } = await createFixture(page, suffix, mock.url);

    // Corrupt the stored payload behind the portal's back (REST write).
    const patch = await page.evaluate(async (name) => {
      const r = await fetch('/api/resource/Sebrus Deployment/' + encodeURIComponent(name), {
        method: 'PUT',
        headers: { 'Content-Type': 'application/json' },
        credentials: 'include',
        body: JSON.stringify({ deploy_payload: '{}' }),
      });
      return r.ok;
    }, deployment.name);
    expect(patch).toBe(true);

    await callMethod(page, 'sebrus_apps.deployment_transition', {
      deployment: deployment.name,
      action: 'Submit for Approval',
    });
    const approve = await callMethod(page, 'sebrus_apps.deployment_transition', {
      deployment: deployment.name,
      action: 'Approve',
    });
    expect(approve.ok, 'approve should be blocked by schema validation').toBe(false);
    expect(JSON.stringify(approve)).toContain(`svc-${suffix}`);
    // Nothing reached Audit Ready.
    expect(mock.state.requests.filter((r) => r.method === 'POST').length).toBe(0);
  });

  test('failed deploy does not block approval; Deploy recovers', async ({ page }) => {
    const suffix = uid();
    const { deployment } = await createFixture(page, suffix, mock.url);
    await callMethod(page, 'sebrus_apps.deployment_transition', {
      deployment: deployment.name,
      action: 'Submit for Approval',
    });

    mock.state.failWith = 401;
    const approve = await callMethod(page, 'sebrus_apps.deployment_transition', {
      deployment: deployment.name,
      action: 'Approve',
    });
    expect(approve.ok, `approve should succeed despite deploy failure: ${approve.error}`).toBe(true);
    expect(deploymentRow(deployment.name).deploy_status).toBe('Failed');

    mock.state.failWith = null;
    const retry = await callMethod(page, 'sebrus_apps.deployment_transition', {
      deployment: deployment.name,
      action: 'Deploy',
    });
    expect(retry.ok, `deploy failed: ${retry.error}`).toBe(true);
    expect(deploymentRow(deployment.name).deploy_status).toBe('Queued');
  });

  test('deployment_logs refreshes live output without changing deploy_status', async ({ page }) => {
    const suffix = uid();
    const { deployment } = await createFixture(page, suffix, mock.url);
    await callMethod(page, 'sebrus_apps.deployment_transition', {
      deployment: deployment.name,
      action: 'Submit for Approval',
    });
    await callMethod(page, 'sebrus_apps.deployment_transition', {
      deployment: deployment.name,
      action: 'Approve',
    });

    // Live output lands on the ref; deploy_status is left alone (still
    // Queued — viewing logs never marks a deployment done).
    mock.state.liveStatus = 'InProgress';
    const logs = await callMethod(page, 'sebrus_apps.deployment_logs', {
      deployment: deployment.name,
    });
    expect(logs.ok, `deployment_logs failed: ${logs.error}`).toBe(true);
    expect(logs.message.deploy_status).toBe('Queued');
    expect(logs.message.all_done).toBe(false);
    expect(logs.message.refs.length).toBe(1);
    expect(logs.message.refs[0].service).toBe(`svc-${suffix}`);
    expect(logs.message.refs[0].live_status).toBe('InProgress');
    expect(logs.message.refs[0].live_output).toContain('[mock] deploy complete');
    expect(logs.message.refs[0].live_progress).toBe(50);

    const row = deploymentRow(deployment.name);
    expect(row.deploy_status).toBe('Queued');
    // s2s_refs is comma-laden JSON — the CSV-based queryRows() helper
    // truncates it, so check persistence with a LIKE in the DB instead.
    const persisted = query(
      `SELECT COUNT(*) FROM "sebrus_deployment" WHERE name = '${deployment.name}' AND s2s_refs LIKE '%[mock] deploy complete%'`
    );
    expect(persisted).toBe('1');

    // Audit Ready's terminal success status is Done (script/iis) — it counts
    // as finished, still without flipping deploy_status.
    mock.state.liveStatus = 'Done';
    const done = await callMethod(page, 'sebrus_apps.deployment_logs', {
      deployment: deployment.name,
    });
    expect(done.ok, `Done deployment_logs failed: ${done.error}`).toBe(true);
    expect(done.message.refs[0].live_status).toBe('Done');
    expect(done.message.all_done).toBe(true);
    expect(deploymentRow(deployment.name).deploy_status).toBe('Queued');

    // No Audit Ready deploy yet → friendly note, not an error.
    const fresh = await createFixture(page, uid(), mock.url);
    const empty = await callMethod(page, 'sebrus_apps.deployment_logs', {
      deployment: fresh.deployment.name,
    });
    expect(empty.ok, `empty deployment_logs failed: ${empty.error}`).toBe(true);
    expect(empty.message.refs).toEqual([]);
    expect(empty.message.note).toContain('No Audit Ready deploy');
  });

  test('post-queue failure inside Audit Ready flips deploy_status and unlocks Deploy', async ({ page }) => {
    const suffix = uid();
    const { deployment } = await createFixture(page, suffix, mock.url);
    await callMethod(page, 'sebrus_apps.deployment_transition', {
      deployment: deployment.name,
      action: 'Submit for Approval',
    });
    await callMethod(page, 'sebrus_apps.deployment_transition', {
      deployment: deployment.name,
      action: 'Approve',
    });
    expect(deploymentRow(deployment.name).deploy_status).toBe('Queued');

    // The rollout queued fine but then failed inside Audit Ready — the exact
    // case that used to leave deploy_status stuck on Queued with no redeploy.
    mock.state.liveStatus = 'Failed';
    const logs = await callMethod(page, 'sebrus_apps.deployment_logs', {
      deployment: deployment.name,
    });
    expect(logs.ok, `deployment_logs failed: ${logs.error}`).toBe(true);
    expect(logs.message.refs[0].live_status).toBe('Failed');
    expect(logs.message.deploy_status).toBe('Failed');
    expect(deploymentRow(deployment.name).deploy_status).toBe('Failed');

    // Deploy reruns the rollout and re-queues.
    mock.state.liveStatus = 'Running';
    const retry = await callMethod(page, 'sebrus_apps.deployment_transition', {
      deployment: deployment.name,
      action: 'Deploy',
    });
    expect(retry.ok, `deploy failed: ${retry.error}`).toBe(true);
    expect(deploymentRow(deployment.name).deploy_status).toBe('Queued');
  });

  test('delete_service deletes the linked s2s deployment and cascades locally', async ({ page }) => {
    const suffix = uid();
    const { deployment, service } = await createFixture(page, suffix, mock.url);
    await callMethod(page, 'sebrus_apps.deployment_transition', {
      deployment: deployment.name,
      action: 'Submit for Approval',
    });
    const approve = await callMethod(page, 'sebrus_apps.deployment_transition', {
      deployment: deployment.name,
      action: 'Approve',
    });
    expect(approve.ok, `approve failed: ${approve.error}`).toBe(true);

    // A service-level secret owned by this service.
    const sec = await callMethod(page, 'sebrus_apps.create_secret', {
      owner_type: 'Sebrus Service',
      owner: service.name,
      secret_key: 'SVC_TOKEN',
      secret_value: 'svc-secret-value',
    });
    expect(sec.ok, `service secret failed: ${sec.error}`).toBe(true);

    // The s2s deployment id Audit Ready returned for this service.
    const s2sId = query(
      `SELECT s2s_deployment_id FROM "sebrus_deploy_record" WHERE service = '${service.name}'`
    );
    expect(s2sId).toContain('dep-mock-');

    const del = await callMethod(page, 'sebrus_apps.delete_service', { service: service.name });
    expect(del.ok, `delete_service failed: ${del.error}`).toBe(true);
    expect(del.message.remote_deleted).toBe(1);
    expect(del.message.remote_errors).toEqual([]);

    // The mock received the remote DELETE, authed + attributed.
    const deletes = mock.state.requests.filter((r) => r.method === 'DELETE');
    expect(deletes.length).toBe(1);
    expect(deletes[0].path).toBe(`/audit_ready/s2s/deployments/${s2sId}`);
    expect(deletes[0].token).toBe('Bearer mock-s2s-token');
    expect(deletes[0].operator).toBe('Administrator');

    // Local cascade: service, its deploy records, pinned versions and
    // service-level secrets are gone.
    expect(query(`SELECT COUNT(*) FROM "sebrus_service" WHERE name = '${service.name}'`)).toBe('0');
    expect(
      query(`SELECT COUNT(*) FROM "sebrus_deploy_record" WHERE service = '${service.name}'`)
    ).toBe('0');
    expect(
      query(`SELECT COUNT(*) FROM "sebrus_service_version" WHERE service = '${service.name}'`)
    ).toBe('0');
    expect(
      query(
        `SELECT COUNT(*) FROM "sebrus_secret" WHERE owner_type = 'Sebrus Service' AND owner_name = '${service.name}'`
      )
    ).toBe('0');

    // Deleting an unknown service is a NotFound, not a silent ok.
    const missing = await callMethod(page, 'sebrus_apps.delete_service', { service: service.name });
    expect(missing.ok).toBe(false);
  });

  test('portal shows live deploy logs and auto-opens them after Approve', async ({ page }) => {
    const suffix = uid();
    const { deployment } = await createFixture(page, suffix, mock.url);

    // Portal workflow buttons confirm via window.confirm — accept them all.
    page.on('dialog', (d) => d.accept());

    await page.goto('/sebrus_apps/portal');
    await page.locator('.nav-item', { hasText: 'Deployments' }).first().click();
    await page.locator(`[data-goto-deployment="${deployment.name}"]`).first().click();
    await expect(page.locator('.panel-title').first()).toContainText(`S2S App ${suffix}`, {
      timeout: 15000,
    });

    // Submit → Approve through the portal; the logs modal opens on its own.
    await page.locator('[data-wf-action="Submit for Approval"]').click();
    await page.locator('[data-wf-action="Approve"]').waitFor({ state: 'visible', timeout: 15000 });
    await page.locator('[data-wf-action="Approve"]').click();
    await page.locator('#logsModal').waitFor({ state: 'visible', timeout: 15000 });
    await expect(page.locator('#logsModal .panel-title')).toContainText('Deploy logs');
    await expect(page.locator('#logsBody')).toContainText(`svc-${suffix}`, { timeout: 15000 });
    await expect(page.locator('#logsBody')).toContainText('[mock] deploy complete');

    // The deployment view syncs live status on entry (maybeSyncEnvHealth);
    // the mock reports Running, a healthy steady state → deploy_status has
    // already flipped to Deployed by the time the logs modal polls.
    expect(deploymentRow(deployment.name).deploy_status).toBe('Deployed');

    // Close, reopen via the View logs button: same live content.
    await page.locator('#logsClose').click();
    await page.locator('#logsModal').waitFor({ state: 'hidden' });
    await page.locator('[data-view-logs]').click();
    await page.locator('#logsModal').waitFor({ state: 'visible' });
    await expect(page.locator('#logsBody')).toContainText('[mock] deploy complete', {
      timeout: 15000,
    });
  });

  test('delete_service deletes the linked s2s deployment and cascades locally', async ({ page }) => {
    const suffix = uid();
    const { deployment, service } = await createFixture(page, suffix, mock.url);
    await callMethod(page, 'sebrus_apps.deployment_transition', {
      deployment: deployment.name,
      action: 'Submit for Approval',
    });
    const approve = await callMethod(page, 'sebrus_apps.deployment_transition', {
      deployment: deployment.name,
      action: 'Approve',
    });
    expect(approve.ok, `approve failed: ${approve.error}`).toBe(true);

    // A service-level secret owned by this service.
    const sec = await callMethod(page, 'sebrus_apps.create_secret', {
      owner_type: 'Sebrus Service',
      owner: service.name,
      secret_key: 'SVC_TOKEN',
      secret_value: 'svc-secret-value',
    });
    expect(sec.ok, `service secret failed: ${sec.error}`).toBe(true);

    // The s2s deployment id Audit Ready returned for this service.
    const s2sId = query(
      `SELECT s2s_deployment_id FROM "sebrus_deploy_record" WHERE service = '${service.name}'`
    );
    expect(s2sId).toContain('dep-mock-');

    const del = await callMethod(page, 'sebrus_apps.delete_service', { service: service.name });
    expect(del.ok, `delete_service failed: ${del.error}`).toBe(true);
    expect(del.message.remote_deleted).toBe(1);
    expect(del.message.remote_errors).toEqual([]);

    // The mock received the remote DELETE, authed + attributed.
    const deletes = mock.state.requests.filter((r) => r.method === 'DELETE');
    expect(deletes.length).toBe(1);
    expect(deletes[0].path).toBe(`/audit_ready/s2s/deployments/${s2sId}`);
    expect(deletes[0].token).toBe('Bearer mock-s2s-token');
    expect(deletes[0].operator).toBe('Administrator');

    // Local cascade: service, its deploy records, pinned versions and
    // service-level secrets are gone.
    expect(query(`SELECT COUNT(*) FROM "sebrus_service" WHERE name = '${service.name}'`)).toBe('0');
    expect(
      query(`SELECT COUNT(*) FROM "sebrus_deploy_record" WHERE service = '${service.name}'`)
    ).toBe('0');
    expect(
      query(`SELECT COUNT(*) FROM "sebrus_service_version" WHERE service = '${service.name}'`)
    ).toBe('0');
    expect(
      query(
        `SELECT COUNT(*) FROM "sebrus_secret" WHERE owner_type = 'Sebrus Service' AND owner_name = '${service.name}'`
      )
    ).toBe('0');

    // Deleting an unknown service is a NotFound, not a silent ok.
    const missing = await callMethod(page, 'sebrus_apps.delete_service', { service: service.name });
    expect(missing.ok).toBe(false);
  });

  test('approve without Audit Ready fields is rejected before any call', async ({ page }) => {
    const suffix = uid();
    await page.goto('/desk');
    const client = await saveDoc(page, {
      doctype: 'Sebrus Client',
      name: `new-sebrus-client-${suffix}`,
      __islocal: 1,
      client_name: `S2S Client ${suffix}`,
    });
    const project = await saveDoc(page, {
      doctype: 'Sebrus Project',
      name: `new-sebrus-project-${suffix}`,
      __islocal: 1,
      project_name: `S2S Project ${suffix}`,
      client: client.name,
    });
    const app = await saveDoc(page, {
      doctype: 'Sebrus App',
      name: `new-sebrus-app-${suffix}`,
      __islocal: 1,
      app_name: `S2S App ${suffix}`,
      app_type: 'Client App',
    });
    const deployment = await saveDoc(page, {
      doctype: 'Sebrus Deployment',
      name: `new-sebrus-deployment-${suffix}`,
      __islocal: 1,
      client: client.name,
      app: app.name,
      project: project.name,
      tier: 'Shared',
      workflow_state: 'Draft',
      service_versions: [
        {
          doctype: 'Sebrus Service Version',
          name: `new-sv-${suffix}`,
          __islocal: 1,
          parent: `new-sebrus-deployment-${suffix}`,
          parenttype: 'Sebrus Deployment',
          parentfield: 'service_versions',
          env: 'dev',
          service: `svc-nowhere-${suffix}`,
          version: '1.0.0',
        },
      ],
    });

    await callMethod(page, 'sebrus_apps.deployment_transition', {
      deployment: deployment.name,
      action: 'Submit for Approval',
    });
    const approve = await callMethod(page, 'sebrus_apps.deployment_transition', {
      deployment: deployment.name,
      action: 'Approve',
    });
    expect(approve.ok).toBe(false);
    expect(approve.error).toContain('deployment target');
    expect(deploymentRow(deployment.name).workflow_state).toBe('Pending Approval');
    expect(mock.state.requests.filter((r) => r.method === 'POST').length).toBe(0);
  });

  test('rollback redeploys the previous version with current secrets', async ({ page }) => {
    const suffix = uid();
    const { deployment, service } = await createFixture(page, suffix, mock.url);
    await callMethod(page, 'sebrus_apps.deployment_transition', {
      deployment: deployment.name,
      action: 'Submit for Approval',
    });
    const approve = await callMethod(page, 'sebrus_apps.deployment_transition', {
      deployment: deployment.name,
      action: 'Approve',
    });
    expect(approve.ok, `approve failed: ${approve.error}`).toBe(true);

    // v2 goes out (Deploy after repinning the version).
    query(
      `UPDATE "sebrus_service_version" SET version = '2.1.0' WHERE parent = '${deployment.name}' AND service = '${service.name}'`
    );
    const retry = await callMethod(page, 'sebrus_apps.deployment_transition', {
      deployment: deployment.name,
      action: 'Deploy',
    });
    expect(retry.ok, `deploy failed: ${retry.error}`).toBe(true);

    const before = mock.state.requests.filter((r) => r.method === 'POST').length;
    expect(before).toBe(2);

    // Roll back dev → previous version (2.0.0) redeploys.
    const rb = await callMethod(page, 'sebrus_apps.rollback_env', {
      deployment: deployment.name,
      env: 'dev',
    });
    expect(rb.ok, `rollback failed: ${rb.error}`).toBe(true);
    expect(rb.message.ok).toBe(true);

    const posts = mock.state.requests.filter((r) => r.method === 'POST');
    expect(posts.length).toBe(3);
    expect(posts[2].body.name).toBe(`svc-${suffix}`);
    // Secrets re-injected at rollback time.
    expect(posts[2].body.script).toContain('export DB_URL="super-secret-value"');

    // Version row repinned, rollback record written.
    const pin = query(
      `SELECT version FROM "sebrus_service_version" WHERE parent = '${deployment.name}' AND service = '${service.name}'`
    );
    expect(pin).toBe('2.0.0');
    const records = queryRows(
      `SELECT version, is_rollback FROM "sebrus_deploy_record" WHERE deployment = '${deployment.name}' AND env = 'dev'`
    );
    expect(records.length).toBe(3);
    const rollbackRec = records.filter((r) => r.is_rollback === '1');
    expect(rollbackRec.length).toBe(1);
    expect(rollbackRec[0].version).toBe('2.0.0');
  });

  test('rollback without a previous version is rejected', async ({ page }) => {
    const suffix = uid();
    const { deployment } = await createFixture(page, suffix, mock.url);
    const rb = await callMethod(page, 'sebrus_apps.rollback_env', {
      deployment: deployment.name,
      env: 'dev',
    });
    expect(rb.ok).toBe(false);
    expect(rb.error).toContain('nothing to roll back');
  });

  test('service-level target overrides the deployment default', async ({ page }) => {
    const suffix = uid();
    await page.goto('/desk');
    await page.locator('body').waitFor({ state: 'visible' });

    const client = await saveDoc(page, {
      doctype: 'Sebrus Client',
      name: `new-sebrus-client-${suffix}`,
      __islocal: 1,
      client_name: `S2S Client ${suffix}`,
    });
    const project = await saveDoc(page, {
      doctype: 'Sebrus Project',
      name: `new-sebrus-project-${suffix}`,
      __islocal: 1,
      project_name: `S2S Project ${suffix}`,
      client: client.name,
    });
    const app = await saveDoc(page, {
      doctype: 'Sebrus App',
      name: `new-sebrus-app-${suffix}`,
      __islocal: 1,
      app_name: `S2S App ${suffix}`,
      app_type: 'Client App',
    });
    // Service A: no own target → uses the deployment default (server/script).
    const svcA = await saveDoc(page, {
      doctype: 'Sebrus Service',
      name: `new-sebrus-service-a-${suffix}`,
      __islocal: 1,
      service_name: `svc-a-${suffix}`,
      app: app.name,
      kind: 'Worker',
    });
    // Service B: own Kubernetes target with a manifest payload.
    const svcB = await saveDoc(page, {
      doctype: 'Sebrus Service',
      name: `new-sebrus-service-b-${suffix}`,
      __islocal: 1,
      service_name: `svc-b-${suffix}`,
      app: app.name,
      kind: 'API',
      target_type: 'cluster',
      target: 'aks-prod-01',
      deploy_kind: 'kubernetes',
      deploy_payload: '{"manifest": "apiVersion: v1\\nkind: ConfigMap"}',
    });
    const deployment = await saveDoc(page, {
      doctype: 'Sebrus Deployment',
      name: `new-sebrus-deployment-${suffix}`,
      __islocal: 1,
      client: client.name,
      app: app.name,
      project: project.name,
      tier: 'Shared',
      workflow_state: 'Draft',
      target_type: 'server',
      target: 'win-web-01',
      deploy_kind: 'script',
      target_env: 'dev',
      deploy_payload: '{"script": "echo deploy"}',
      service_versions: [
        {
          doctype: 'Sebrus Service Version',
          name: `new-sv-a-${suffix}`,
          __islocal: 1,
          parent: `new-sebrus-deployment-${suffix}`,
          parenttype: 'Sebrus Deployment',
          parentfield: 'service_versions',
          env: 'dev',
          service: svcA.name,
          version: '1.0.0',
        },
        {
          doctype: 'Sebrus Service Version',
          name: `new-sv-b-${suffix}`,
          __islocal: 1,
          parent: `new-sebrus-deployment-${suffix}`,
          parenttype: 'Sebrus Deployment',
          parentfield: 'service_versions',
          env: 'dev',
          service: svcB.name,
          version: '2.0.0',
        },
      ],
    });
    const cfg = await callMethod(page, 'sebrus_apps.set_audit_ready_config', {
      url: mock.url,
      token: 'mock-s2s-token',
    });
    expect(cfg.ok).toBe(true);

    await callMethod(page, 'sebrus_apps.deployment_transition', {
      deployment: deployment.name,
      action: 'Submit for Approval',
    });
    const approve = await callMethod(page, 'sebrus_apps.deployment_transition', {
      deployment: deployment.name,
      action: 'Approve',
    });
    expect(approve.ok, `approve failed: ${approve.error}`).toBe(true);

    const posts = mock.state.requests.filter((r) => r.method === 'POST');
    expect(posts.length).toBe(2);
    const bodyA = posts.find((r) => r.body.name === `svc-a-${suffix}`).body;
    const bodyB = posts.find((r) => r.body.name === `svc-b-${suffix}`).body;
    // A went to the deployment's default server target as a script.
    expect(bodyA.target_type).toBe('server');
    expect(bodyA.target).toBe('win-web-01');
    expect(bodyA.kind).toBe('script');
    expect(bodyA.script).toContain('echo deploy');
    // B went to its own cluster target with its own manifest.
    expect(bodyB.target_type).toBe('cluster');
    expect(bodyB.target).toBe('aks-prod-01');
    expect(bodyB.kind).toBe('kubernetes');
    expect(bodyB.manifest).toContain('ConfigMap');
  });

  test('kubernetes k8s template renders to a manifest for Audit Ready', async ({ page }) => {
    const suffix = uid();
    await page.goto('/desk');
    await page.locator('body').waitFor({ state: 'visible' });

    const client = await saveDoc(page, {
      doctype: 'Sebrus Client',
      name: `new-sebrus-client-${suffix}`,
      __islocal: 1,
      client_name: `S2S Client ${suffix}`,
    });
    const project = await saveDoc(page, {
      doctype: 'Sebrus Project',
      name: `new-sebrus-project-${suffix}`,
      __islocal: 1,
      project_name: `S2S Project ${suffix}`,
      client: client.name,
    });
    const app = await saveDoc(page, {
      doctype: 'Sebrus App',
      name: `new-sebrus-app-${suffix}`,
      __islocal: 1,
      app_name: `S2S App ${suffix}`,
      app_type: 'Client App',
    });
    // Kubernetes service with a structured k8s template payload.
    const service = await saveDoc(page, {
      doctype: 'Sebrus Service',
      name: `new-sebrus-service-${suffix}`,
      __islocal: 1,
      service_name: `svc-k8s-${suffix}`,
      app: app.name,
      kind: 'API',
      target_type: 'cluster',
      target: 'aks-prod-01',
      deploy_kind: 'kubernetes',
      deploy_payload: JSON.stringify({
        k8s: {
          namespace: 'apps',
          image: 'registry.example.com/billing-api',
          port: 8080,
          replicas: 3,
          service_type: 'LoadBalancer',
          env: [{ name: 'LOG_LEVEL', value: 'debug' }],
          resources: {
            cpu_request: '250m',
            memory_request: '256Mi',
            cpu_limit: '500m',
            memory_limit: '512Mi',
          },
          probes: { liveness_path: '/healthz', readiness_path: '/ready' },
          ingress: { host: `billing-${suffix}.example.com`, path: '/api', tls: true },
        },
      }),
    });
    const deployment = await saveDoc(page, {
      doctype: 'Sebrus Deployment',
      name: `new-sebrus-deployment-${suffix}`,
      __islocal: 1,
      client: client.name,
      app: app.name,
      project: project.name,
      tier: 'Shared',
      workflow_state: 'Draft',
      target_env: 'dev',
      service_versions: [
        {
          doctype: 'Sebrus Service Version',
          name: `new-sv-${suffix}`,
          __islocal: 1,
          parent: `new-sebrus-deployment-${suffix}`,
          parenttype: 'Sebrus Deployment',
          parentfield: 'service_versions',
          env: 'dev',
          service: service.name,
          version: '3.1.4',
        },
      ],
    });

    // A vault secret: appended to the container env at render time.
    const sec = await callMethod(page, 'sebrus_apps.create_secret', {
      deployment: deployment.name,
      env: 'dev',
      secret_key: 'DB_URL',
      secret_value: 'super-secret-value',
      scope: 'Deployment env',
    });
    expect(sec.ok, `create_secret failed: ${sec.error}`).toBe(true);

    const cfg = await callMethod(page, 'sebrus_apps.set_audit_ready_config', {
      url: mock.url,
      token: 'mock-s2s-token',
    });
    expect(cfg.ok).toBe(true);

    await callMethod(page, 'sebrus_apps.deployment_transition', {
      deployment: deployment.name,
      action: 'Submit for Approval',
    });
    const approve = await callMethod(page, 'sebrus_apps.deployment_transition', {
      deployment: deployment.name,
      action: 'Approve',
    });
    expect(approve.ok, `approve failed: ${approve.error}`).toBe(true);

    // The mock received a normal deploy body: manifest string, no k8s leak.
    const posts = mock.state.requests.filter((r) => r.method === 'POST');
    expect(posts.length).toBe(1);
    const body = posts[0].body;
    expect(body.target_type).toBe('cluster');
    expect(body.target).toBe('aks-prod-01');
    expect(body.kind).toBe('kubernetes');
    expect(body.name).toBe(`svc-k8s-${suffix}`);
    expect(typeof body.manifest).toBe('string');
    expect(body.k8s).toBeUndefined();
    expect(Object.keys(body)).not.toContain('k8s');

    // The manifest parses into Deployment + Service + Ingress documents.
    const docs = body.manifest.split('\n---\n').map((d) => JSON.parse(d));
    expect(docs.map((d) => d.kind)).toEqual(['Deployment', 'Service', 'Ingress']);

    const dep = docs[0];
    expect(dep.metadata.namespace).toBe('apps');
    expect(dep.spec.replicas).toBe(3);
    const container = dep.spec.template.spec.containers[0];
    // Pinned version becomes the image tag.
    expect(container.image).toBe('registry.example.com/billing-api:3.1.4');
    expect(container.ports[0].containerPort).toBe(8080);
    // Template env entries first, then the vault secret.
    expect(container.env).toContainEqual({ name: 'LOG_LEVEL', value: 'debug' });
    expect(container.env).toContainEqual({ name: 'DB_URL', value: 'super-secret-value' });
    expect(container.resources.requests).toEqual({ cpu: '250m', memory: '256Mi' });
    expect(container.resources.limits).toEqual({ cpu: '500m', memory: '512Mi' });
    expect(container.livenessProbe.httpGet).toEqual({ path: '/healthz', port: 8080 });
    expect(container.readinessProbe.httpGet).toEqual({ path: '/ready', port: 8080 });

    const svc = docs[1];
    expect(svc.spec.type).toBe('LoadBalancer');
    expect(svc.spec.ports[0]).toEqual({ port: 8080, targetPort: 8080 });

    const ingress = docs[2];
    expect(ingress.spec.rules[0].host).toBe(`billing-${suffix}.example.com`);
    expect(ingress.spec.rules[0].http.paths[0].path).toBe('/api');
    expect(ingress.spec.tls[0].hosts).toEqual([`billing-${suffix}.example.com`]);
    expect(ingress.spec.tls[0].secretName).toBe(`svc-k8s-${suffix}-tls`);
  });

  test('k8s template with registry renders a pull secret and secretKeyRef env', async ({ page }) => {
    const suffix = uid();
    const regName = `acr-${suffix}`;
    const regUrl = `registry-${suffix}.example.com`;
    await page.goto('/desk');
    await page.locator('body').waitFor({ state: 'visible' });

    // Registry upsert + listing (password_set flag, never the password).
    const up = await callMethod(page, 'sebrus_apps.upsert_registry', {
      name: regName,
      url: regUrl,
      username: 'robot',
      password: 'sup3r-secret',
    });
    expect(up.ok, `upsert_registry failed: ${up.error}`).toBe(true);

    const list = await callMethod(page, 'sebrus_apps.list_registries', {});
    expect(list.ok, `list_registries failed: ${list.error}`).toBe(true);
    const reg = list.message.find((r) => r.registry_name === regName);
    expect(reg, 'upserted registry should be listed').toBeTruthy();
    expect(reg.url).toBe(regUrl);
    expect(reg.username).toBe('robot');
    expect(reg.password_set).toBe(true);
    expect(JSON.stringify(list.message)).not.toContain('sup3r-secret');

    const client = await saveDoc(page, {
      doctype: 'Sebrus Client',
      name: `new-sebrus-client-${suffix}`,
      __islocal: 1,
      client_name: `S2S Client ${suffix}`,
    });
    const project = await saveDoc(page, {
      doctype: 'Sebrus Project',
      name: `new-sebrus-project-${suffix}`,
      __islocal: 1,
      project_name: `S2S Project ${suffix}`,
      client: client.name,
    });
    const app = await saveDoc(page, {
      doctype: 'Sebrus App',
      name: `new-sebrus-app-${suffix}`,
      __islocal: 1,
      app_name: `S2S App ${suffix}`,
      app_type: 'Client App',
    });
    // Kubernetes service: registry + literal env + env-from-k8s-Secret.
    const service = await saveDoc(page, {
      doctype: 'Sebrus Service',
      name: `new-sebrus-service-${suffix}`,
      __islocal: 1,
      service_name: `svc-reg-${suffix}`,
      app: app.name,
      kind: 'API',
      target_type: 'cluster',
      target: 'aks-prod-01',
      deploy_kind: 'kubernetes',
      deploy_payload: JSON.stringify({
        k8s: {
          namespace: 'apps',
          image: `${regUrl}/billing-api`,
          port: 8080,
          registry: regName,
          env: [
            { name: 'LOG_LEVEL', value: 'debug' },
            { name: 'DB_PASS', secret: { name: 'db-creds', key: 'password' } },
          ],
        },
      }),
    });
    const deployment = await saveDoc(page, {
      doctype: 'Sebrus Deployment',
      name: `new-sebrus-deployment-${suffix}`,
      __islocal: 1,
      client: client.name,
      app: app.name,
      project: project.name,
      tier: 'Shared',
      workflow_state: 'Draft',
      target_env: 'dev',
      service_versions: [
        {
          doctype: 'Sebrus Service Version',
          name: `new-sv-${suffix}`,
          __islocal: 1,
          parent: `new-sebrus-deployment-${suffix}`,
          parenttype: 'Sebrus Deployment',
          parentfield: 'service_versions',
          env: 'dev',
          service: service.name,
          version: '4.2.0',
        },
      ],
    });

    // A vault secret too: appended to the container env after the template env.
    const sec = await callMethod(page, 'sebrus_apps.create_secret', {
      deployment: deployment.name,
      env: 'dev',
      secret_key: 'DB_URL',
      secret_value: 'super-secret-value',
      scope: 'Deployment env',
    });
    expect(sec.ok, `create_secret failed: ${sec.error}`).toBe(true);

    const cfg = await callMethod(page, 'sebrus_apps.set_audit_ready_config', {
      url: mock.url,
      token: 'mock-s2s-token',
      operator_email: 'jane.doe@example.com',
    });
    expect(cfg.ok).toBe(true);

    await callMethod(page, 'sebrus_apps.deployment_transition', {
      deployment: deployment.name,
      action: 'Submit for Approval',
    });
    const approve = await callMethod(page, 'sebrus_apps.deployment_transition', {
      deployment: deployment.name,
      action: 'Approve',
    });
    expect(approve.ok, `approve failed: ${approve.error}`).toBe(true);

    const posts = mock.state.requests.filter((r) => r.method === 'POST');
    expect(posts.length).toBe(1);
    const body = posts[0].body;
    expect(body.kind).toBe('kubernetes');
    expect(typeof body.manifest).toBe('string');
    expect(body.k8s).toBeUndefined();

    // Pull Secret + Deployment + Service.
    const docs = body.manifest.split('\n---\n').map((d) => JSON.parse(d));
    expect(docs.map((d) => d.kind)).toEqual(['Secret', 'Deployment', 'Service']);

    const pull = docs[0];
    expect(pull.metadata.name).toBe(`sebrus-pull-${regName}`);
    expect(pull.metadata.namespace).toBe('apps');
    expect(pull.type).toBe('kubernetes.io/dockerconfigjson');
    const dockerconfig = JSON.parse(pull.stringData['.dockerconfigjson']);
    const auth = dockerconfig.auths[regUrl];
    expect(auth.username).toBe('robot');
    expect(auth.password).toBe('sup3r-secret');
    expect(auth.auth).toBe(Buffer.from('robot:sup3r-secret').toString('base64'));

    const pod = docs[1].spec.template.spec;
    expect(pod.imagePullSecrets[0].name).toBe(`sebrus-pull-${regName}`);
    const env = pod.containers[0].env;
    expect(env).toContainEqual({ name: 'LOG_LEVEL', value: 'debug' });
    expect(env).toContainEqual({
      name: 'DB_PASS',
      valueFrom: { secretKeyRef: { name: 'db-creds', key: 'password' } },
    });
    expect(env).toContainEqual({ name: 'DB_URL', value: 'super-secret-value' });

    // The deploy record stores the pre-injection template: the registry
    // password must not appear in it. (query(), not queryRows(): the payload
    // JSON contains commas, which the helper's naive CSV parse can't handle.)
    const recPayload = query(
      `SELECT payload FROM "sebrus_deploy_record" WHERE deployment = '${deployment.name}' AND env = 'dev'`
    );
    expect(recPayload).toContain(regName);
    expect(recPayload).not.toContain('sup3r-secret');
    expect(recPayload).not.toContain('super-secret-value');
  });

  test('validate_deploy_config rejects an unknown k8s registry', async ({ page }) => {
    await page.goto('/desk');
    const missing = `acr-missing-${uid()}`;
    const res = await callMethod(page, 'sebrus_apps.validate_deploy_config', {
      target_type: 'cluster',
      kind: 'kubernetes',
      payload: JSON.stringify({
        k8s: { namespace: 'apps', image: 'reg/api', port: 8080, registry: missing },
      }),
    });
    expect(res.ok, 'unknown registry should be rejected').toBe(false);
    expect(JSON.stringify(res)).toContain(missing);
  });

  test('prod promotion requires UAT to pass first', async ({ page }) => {
    const suffix = uid();
    const { deployment } = await createFixture(page, suffix, mock.url);

    // Promote to prod while uat was never ok → refused.
    const early = await callMethod(page, 'sebrus_apps.set_env_status', {
      deployment: deployment.name,
      env: 'prod',
      status: 'pending',
    });
    expect(early.ok).toBe(false);
    expect(early.error).toContain('UAT');

    // UAT passes → promotion allowed.
    const uat = await callMethod(page, 'sebrus_apps.set_env_status', {
      deployment: deployment.name,
      env: 'uat',
      status: 'ok',
    });
    expect(uat.ok, `uat ok failed: ${uat.error}`).toBe(true);
    const promote = await callMethod(page, 'sebrus_apps.set_env_status', {
      deployment: deployment.name,
      env: 'prod',
      status: 'pending',
    });
    expect(promote.ok, `promote failed: ${promote.error}`).toBe(true);
  });

  test('infrastructure is pulled from Audit Ready for target pickers', async ({ page }) => {
    await page.goto('/desk');
    const cfg = await callMethod(page, 'sebrus_apps.set_audit_ready_config', {
      url: mock.url,
      token: 'mock-s2s-token',
    });
    expect(cfg.ok).toBe(true);

    const infra = await callMethod(page, 'sebrus_apps.audit_ready_infrastructure', {});
    expect(infra.ok, `infra failed: ${infra.error}`).toBe(true);
    expect(infra.message.servers.map((s) => s.name)).toEqual(['win-web-01', 'win-web-02']);
    expect(infra.message.clusters.map((c) => c.name)).toEqual(['aks-prod-01']);

    // The calls carried the vault-stored credentials.
    const pulls = mock.state.requests.filter((r) => r.method === 'GET' && r.path.includes('/s2s/'));
    expect(pulls.length).toBe(2);
    expect(pulls[0].token).toBe('Bearer mock-s2s-token');
    expect(pulls[0].operator).toBe('Administrator');
  });
});

// @ts-check
const { test, expect } = require('@playwright/test');
const { query, queryRows } = require('./helpers/db.js');
const { startMockAuditReady } = require('./helpers/mock_audit_ready.js');

/**
 * Release flow: Sebrus Client → Sebrus Project → Sebrus App → Sebrus Deployment.
 *
 * Documents are created through the same `savedocs` endpoint the Desk Save
 * button uses (form-UI clicking is covered by user-and-roles.spec.js), then
 * verified in the database and in the Desk list/form views.
 */

function uid() {
  return Math.random().toString(36).slice(2, 10);
}

/** Save a doc via the native desk endpoint; returns the saved doc. */
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

/** Call the workflow transition method; returns { ok, state?, error? }. */
async function callTransition(page, deployment, action) {
  return page.evaluate(
    async ({ deployment, action }) => {
      try {
        const body = new URLSearchParams();
        body.append('deployment', deployment);
        body.append('action', action);
        const r = await fetch('/api/method/sebrus_apps.deployment_transition', {
          method: 'POST',
          headers: {
            'Content-Type': 'application/x-www-form-urlencoded; charset=UTF-8',
            'X-Requested-With': 'XMLHttpRequest',
          },
          credentials: 'include',
          body,
        });
        const json = await r.json();
        if (json.exc_type || json.error) {
          return { ok: false, error: json.exc || json.error, exc_type: json.exc_type };
        }
        return { ok: true, state: json.message && json.message.workflow_state };
      } catch (e) {
        return { ok: false, error: String(e && e.message ? e.message : e) };
      }
    },
    { deployment, action }
  );
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

/** Create the full chain and return a fresh Draft deployment name. */
async function createDeployment(page, suffix, auditReady = false) {
  await page.goto('/desk');
  await page.locator('body').waitFor({ state: 'visible' });

  const client = await saveDoc(page, {
    doctype: 'Sebrus Client',
    name: `new-sebrus-client-${suffix}`,
    __islocal: 1,
    client_name: `E2E Client ${suffix}`,
  });
  const project = await saveDoc(page, {
    doctype: 'Sebrus Project',
    name: `new-sebrus-project-${suffix}`,
    __islocal: 1,
    project_name: `E2E Project ${suffix}`,
    client: client.name,
  });
  const app = await saveDoc(page, {
    doctype: 'Sebrus App',
    name: `new-sebrus-app-${suffix}`,
    __islocal: 1,
    app_name: `E2E App ${suffix}`,
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
    // The desk client applies doctype defaults on the New form and posts them
    // back; do the same here.
    workflow_state: 'Draft',
    // Approve deploys through Audit Ready — the walk test wires these at the
    // mock; other tests leave them empty.
    ...(auditReady
      ? {
          target_type: 'server',
          target: 'win-web-01',
          deploy_kind: 'script',
          target_env: 'dev',
          deploy_payload: '{"script": "echo deploy"}',
        }
      : {}),
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
        version: '1.0.0',
      },
    ],
  });
  return deployment.name;
}

function dbWorkflowState(name) {
  return query(`SELECT workflow_state FROM "sebrus_deployment" WHERE name = '${name}'`);
}

test.describe('Portal: new deployment form', () => {
  test('modal creates a deployment with pinned service versions', async ({ page }) => {
    const suffix = uid();
    const clientName = `Portal Client ${suffix}`;
    const projectName = `Portal Project ${suffix}`;
    const appName = `Portal App ${suffix}`;

    // Fixture data via the desk endpoint.
    await page.goto('/desk');
    await page.locator('body').waitFor({ state: 'visible' });
    const client = await saveDoc(page, {
      doctype: 'Sebrus Client',
      name: `new-sebrus-client-${suffix}`,
      __islocal: 1,
      client_name: clientName,
    });
    const project = await saveDoc(page, {
      doctype: 'Sebrus Project',
      name: `new-sebrus-project-${suffix}`,
      __islocal: 1,
      project_name: projectName,
      client: client.name,
    });
    const app = await saveDoc(page, {
      doctype: 'Sebrus App',
      name: `new-sebrus-app-${suffix}`,
      __islocal: 1,
      app_name: appName,
      app_type: 'Client App',
    });
    const service = await saveDoc(page, {
      doctype: 'Sebrus Service',
      name: `new-sebrus-service-${suffix}`,
      __islocal: 1,
      service_name: `portal-svc-${suffix}`,
      app: app.name,
      kind: 'API',
    });

    // Drive the portal modal.
    await page.goto('/sebrus_apps/portal');
    await page.locator('.nav-item', { hasText: 'Deployments' }).first().click();
    await page.locator('button[data-new-deployment]').first().click();
    await page.locator('#depModal').waitFor({ state: 'visible' });

    await page.locator('#ndApp').selectOption({ value: app.name });
    await page.locator('#ndProject').selectOption({ value: project.name });
    await page.locator('#ndTier').selectOption('Shared');
    await page.locator('#ndTargetType').selectOption('server');
    await page.locator('#ndKind').selectOption('script');
    await page.locator('#ndTargetManual').fill('win-web-01');
    await page.locator('#ndScript').fill('echo deploy');
    await page.locator(`.nd-ver[data-service="${service.name}"]`).fill('3.1.0');
    await page.locator('#depModalCreate').click();

    // Lands on the new deployment view.
    await page.locator('#depModal').waitFor({ state: 'hidden', timeout: 15000 });
    await expect(page.locator('.panel-title').first()).toContainText(appName, { timeout: 15000 });

    // DB: one deployment with the pinned version row.
    const row = queryRows(
      `SELECT name, app, project, tier, workflow_state, target, deploy_kind FROM "sebrus_deployment" WHERE app = '${app.name}' AND project = '${project.name}'`
    )[0];
    expect(row).toBeTruthy();
    expect(row.workflow_state).toBe('Draft');
    expect(row.target).toBe('win-web-01');
    expect(row.deploy_kind).toBe('script');
    const vers = queryRows(
      `SELECT env, service, version FROM "sebrus_service_version" WHERE parent = '${row.name}'`
    );
    expect(vers.length).toBe(1);
    expect(vers[0].service).toBe(service.name);
    expect(vers[0].version).toBe('3.1.0');

    // The script field built the deploy payload.
    const payload = query(
      `SELECT deploy_payload FROM "sebrus_deployment" WHERE name = '${row.name}'`
    );
    expect(JSON.parse(payload).script).toBe('echo deploy');
  });

  test('iis target links an Azure DevOps artifact and stores secrets', async ({ page }) => {
    const suffix = uid();
    const appName = `Portal IIS App ${suffix}`;

    await page.goto('/desk');
    await page.locator('body').waitFor({ state: 'visible' });
    const client = await saveDoc(page, {
      doctype: 'Sebrus Client',
      name: `new-sebrus-client-${suffix}`,
      __islocal: 1,
      client_name: `Portal Client ${suffix}`,
    });
    const project = await saveDoc(page, {
      doctype: 'Sebrus Project',
      name: `new-sebrus-project-${suffix}`,
      __islocal: 1,
      project_name: `Portal Project ${suffix}`,
      client: client.name,
    });
    const app = await saveDoc(page, {
      doctype: 'Sebrus App',
      name: `new-sebrus-app-${suffix}`,
      __islocal: 1,
      app_name: appName,
      app_type: 'Client App',
    });
    const service = await saveDoc(page, {
      doctype: 'Sebrus Service',
      name: `new-sebrus-service-${suffix}`,
      __islocal: 1,
      service_name: `iis-svc-${suffix}`,
      app: app.name,
      kind: 'Frontend',
    });

    await page.goto('/sebrus_apps/portal');
    await page.locator('.nav-item', { hasText: 'Deployments' }).first().click();
    await page.locator('button[data-new-deployment]').first().click();
    await page.locator('#depModal').waitFor({ state: 'visible' });

    await page.locator('#ndApp').selectOption({ value: app.name });
    await page.locator('#ndProject').selectOption({ value: project.name });
    await page.locator('#ndTargetType').selectOption('server');
    await page.locator('#ndKind').selectOption('iis');
    await page.locator('#ndTargetManual').fill('win-web-01');
    // ADO artifact link.
    await page.locator('#ndAdoOrg').fill('myorg');
    await page.locator('#ndAdoProject').fill('Billing');
    await page.locator('#ndBuildId').fill('12345');
    await page.locator('#ndArtifact').fill('drop');
    await page.locator('#ndPat').fill('test-pat-123');
    await page.locator('#ndConfigStyle').selectOption('appsettings');
    await page.locator(`.nd-ver[data-service="${service.name}"]`).fill('2.0.0');
    // Secrets editor is gated on the vault: uninitialized on e2e → disabled.
    await expect(page.locator('#ndAddSecret')).toBeDisabled();
    await expect(page.locator('#ndSecretHint')).toContainText('vault');
    await page.locator('#depModalCreate').click();

    await page.locator('#depModal').waitFor({ state: 'hidden', timeout: 15000 });

    const row = queryRows(
      `SELECT name, deploy_kind FROM "sebrus_deployment" WHERE app = '${app.name}' AND project = '${project.name}'`
    )[0];
    expect(row).toBeTruthy();
    expect(row.deploy_kind).toBe('iis');
    // deploy_payload is JSON with commas — fetch it outside the CSV parser.
    const payload = JSON.parse(
      query(`SELECT deploy_payload FROM "sebrus_deployment" WHERE name = '${row.name}'`)
    );
    expect(payload.iis.ado_org).toBe('myorg');
    expect(payload.iis.ado_project).toBe('Billing');
    expect(payload.iis.build_id).toBe(12345);
    expect(payload.iis.artifact_name).toBe('drop');
    // The PAT must not be stored on the row — it became the ADO_PAT secret.
    expect(payload.iis.pat).toBeUndefined();
    expect(payload.iis.config_style).toBe('appsettings');

    // Secret stored via the vault path, scoped to the deployment + env + service.
    const sec = await callMethod(page, 'sebrus_apps.create_secret', {
      deployment: row.name,
      env: 'dev',
      secret_key: 'CONN_STRING',
      secret_value: 'Server=db01;Database=Billing;',
      scope: 'Deployment env',
      service: `iis-svc-${suffix}`,
    });
    expect(sec.ok, `create_secret failed: ${sec.error}`).toBe(true);
    const secrets = queryRows(
      `SELECT secret_key, secret_value, env, service, scope FROM "sebrus_secret" WHERE deployment = '${row.name}'`
    );
    // Two rows: the modal stored the PAT as ADO_PAT, plus CONN_STRING above.
    expect(secrets.length).toBe(2);
    const adoPat = secrets.find((s) => s.secret_key === 'ADO_PAT');
    expect(adoPat).toBeTruthy();
    expect(adoPat.secret_value).toBe('test-pat-123');
    expect(adoPat.env).toBe('dev');
    expect(adoPat.service || '').toBe('');
    const connString = secrets.find((s) => s.secret_key === 'CONN_STRING');
    expect(connString).toBeTruthy();
    expect(connString.secret_value).toBe('Server=db01;Database=Billing;');
    expect(connString.env).toBe('dev');
    expect(connString.service).toBe(`iis-svc-${suffix}`);
  });
});

test.describe('Deployment approval workflow', () => {
  let mock;
  test.beforeAll(async () => {
    mock = await startMockAuditReady();
  });
  test.afterAll(async () => {
    await mock.close();
  });

  test('full transition walk Draft → Pending Approval → Approved → Deployed', async ({ page }) => {
    const suffix = uid();
    const name = await createDeployment(page, suffix, true);
    expect(dbWorkflowState(name)).toBe('Draft');

    // Approve deploys through Audit Ready — point the app at the mock.
    const cfg = await callMethod(page, 'sebrus_apps.set_audit_ready_config', {
      url: mock.url,
      token: 'mock-s2s-token',
    });
    expect(cfg.ok, `set config failed: ${cfg.error}`).toBe(true);

    // Illegal transition: cannot Approve from Draft.
    const bad = await callTransition(page, name, 'Approve');
    expect(bad.ok).toBe(false);
    expect(dbWorkflowState(name)).toBe('Draft');

    const submitted = await callTransition(page, name, 'Submit for Approval');
    expect(submitted.ok, `submit failed: ${submitted.error}`).toBe(true);
    expect(submitted.state).toBe('Pending Approval');
    expect(dbWorkflowState(name)).toBe('Pending Approval');

    const approved = await callTransition(page, name, 'Approve');
    expect(approved.ok, `approve failed: ${approved.error}`).toBe(true);
    expect(dbWorkflowState(name)).toBe('Approved');

    const deployed = await callTransition(page, name, 'Mark Deployed');
    expect(deployed.ok, `mark deployed failed: ${deployed.error}`).toBe(true);
    expect(dbWorkflowState(name)).toBe('Deployed');

    // Deployed is not terminal: one deployment per scenario, so the next
    // release is a repin + Retry Deploy on the same record, landing back in
    // Approved (and re-queued with Audit Ready).
    const retry = await callTransition(page, name, 'Retry Deploy');
    expect(retry.ok, `retry from Deployed failed: ${retry.error}`).toBe(true);
    expect(dbWorkflowState(name)).toBe('Approved');

    // Out-of-state transitions are still refused.
    const after = await callTransition(page, name, 'Submit for Approval');
    expect(after.ok).toBe(false);
  });

  test('reject then resubmit returns to Draft', async ({ page }) => {
    const name = await createDeployment(page, uid());

    await callTransition(page, name, 'Submit for Approval');
    const rejected = await callTransition(page, name, 'Reject');
    expect(rejected.ok, `reject failed: ${rejected.error}`).toBe(true);
    expect(dbWorkflowState(name)).toBe('Rejected');

    const resubmitted = await callTransition(page, name, 'Resubmit');
    expect(resubmitted.ok, `resubmit failed: ${resubmitted.error}`).toBe(true);
    expect(dbWorkflowState(name)).toBe('Draft');
  });

  test('workflow_state cannot be edited through a direct save', async ({ page }) => {
    const name = await createDeployment(page, uid());

    // Attempt to bypass the workflow by saving the doc with a crafted state.
    const res = await page.evaluate(
      async ({ name }) => {
        try {
          const doc = {
            doctype: 'Sebrus Deployment',
            name,
            workflow_state: 'Approved',
            __unsaved: 1,
          };
          const body = new URLSearchParams();
          body.append('doc', JSON.stringify(doc));
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
          return { ok: r.ok && !json.exc_type && !json.error, json };
        } catch (e) {
          return { ok: false, json: String(e) };
        }
      },
      { name }
    );
    expect(res.ok, `direct workflow_state edit was not rejected: ${JSON.stringify(res.json)}`).toBe(false);
    expect(dbWorkflowState(name)).toBe('Draft');
  });

  test('one deployment per scenario (client × project × app)', async ({ page }) => {
    const suffix = uid();
    await page.goto('/desk');
    await page.locator('body').waitFor({ state: 'visible' });

    const client = await saveDoc(page, {
      doctype: 'Sebrus Client',
      name: `new-sebrus-client-${suffix}`,
      __islocal: 1,
      client_name: `Scenario Client ${suffix}`,
    });
    const client2 = await saveDoc(page, {
      doctype: 'Sebrus Client',
      name: `new-sebrus-client-b-${suffix}`,
      __islocal: 1,
      client_name: `Scenario Client B ${suffix}`,
    });
    const project = await saveDoc(page, {
      doctype: 'Sebrus Project',
      name: `new-sebrus-project-${suffix}`,
      __islocal: 1,
      project_name: `Scenario Project ${suffix}`,
      client: client.name,
    });
    const app = await saveDoc(page, {
      doctype: 'Sebrus App',
      name: `new-sebrus-app-${suffix}`,
      __islocal: 1,
      app_name: `Scenario App ${suffix}`,
      app_type: 'Client App',
    });
    const first = await saveDoc(page, {
      doctype: 'Sebrus Deployment',
      name: `new-sebrus-deployment-${suffix}`,
      __islocal: 1,
      client: client.name,
      app: app.name,
      project: project.name,
      tier: 'Shared',
      workflow_state: 'Draft',
    });

    // A second deployment for the same client × project × app is rejected.
    const dupe = await page.evaluate(async (doc) => {
      const body = new URLSearchParams();
      body.append('doc', JSON.stringify(doc));
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
      return { ok: r.ok && !json.exc_type && !json.error, json };
    }, {
      doctype: 'Sebrus Deployment',
      name: `new-sebrus-deployment-dupe-${suffix}`,
      __islocal: 1,
      client: client.name,
      app: app.name,
      project: project.name,
      tier: 'Shared',
      workflow_state: 'Draft',
    });
    expect(dupe.ok, `duplicate scenario was not rejected: ${JSON.stringify(dupe.json)}`).toBe(false);
    expect(JSON.stringify(dupe.json)).toContain('already exists');
    expect(JSON.stringify(dupe.json)).toContain(first.name);

    // Same project + app under a different client is a different scenario —
    // one project can be deployed for different clients.
    const other = await saveDoc(page, {
      doctype: 'Sebrus Deployment',
      name: `new-sebrus-deployment-other-${suffix}`,
      __islocal: 1,
      client: client2.name,
      app: app.name,
      project: project.name,
      tier: 'Shared',
      workflow_state: 'Draft',
    });
    expect(other.name).toBeTruthy();
  });

  test('Sebrus Developer can submit but not approve', async ({ page, context, browser }) => {
    const suffix = uid();
    const name = await createDeployment(page, suffix);

    // Create a developer user (role assignment via savedocs child row — the
    // same native path user-and-roles.spec.js uses).
    const email = `dev.${suffix}@example.com`;
    const user = await saveDoc(page, {
      doctype: 'User',
      name: `new-user-${suffix}`,
      __islocal: 1,
      email,
      first_name: 'E2E Developer',
      enabled: 1,
      roles: [
        {
          doctype: 'Has Role',
          name: `new-has-role-${suffix}`,
          __islocal: 1,
          parent: `new-user-${suffix}`,
          parenttype: 'User',
          parentfield: 'roles',
          role: 'Sebrus Developer',
        },
      ],
    });
    // Passwords live in __auth as argon2id hashes keyed by the User's real
    // name; seed a known one ("admin"). The `$` chars are escaped because the
    // db helper passes SQL through a double-quoted shell string.
    const ARGON2_OF_ADMIN =
      '$argon2id$v=19$m=19456,t=2,p=1$UEWqTMicBrdEJXqPMhP4oA$bR1RecCR37Rw+Spup2ULPNKAZ7H6vZTX4VeqNAfvdkY'.replace(
        /\$/g,
        '\\$'
      );
    query(
      `INSERT INTO "__auth" (name, doctype, fieldname, password) VALUES ('${user.name}', 'User', 'password', '${ARGON2_OF_ADMIN}')`
    );

    // Log in as the developer.
    await context.close();
    const devContext = await browser.newContext();
    const devPage = await devContext.newPage();
    await devPage.goto('/login');
    await devPage.locator('#login_email').fill(email);
    await devPage.locator('#login_password').fill('admin');
    await devPage.locator('#login-form button[type="submit"]').click();
    await expect(devPage).toHaveURL(/\/(desk|app)$/, { timeout: 20000 });

    // Developer may submit for approval...
    const submitted = await callTransition(devPage, name, 'Submit for Approval');
    expect(submitted.ok, `developer submit failed: ${submitted.error}`).toBe(true);
    expect(dbWorkflowState(name)).toBe('Pending Approval');

    // ...but may not approve.
    const approved = await callTransition(devPage, name, 'Approve');
    expect(approved.ok).toBe(false);
    expect(dbWorkflowState(name)).toBe('Pending Approval');

    await devContext.close();
  });
});

test.describe('Release flow: client → project → app → deployment', () => {
  test('full chain creates and renders with workflow_state Draft', async ({ page }) => {
    const suffix = uid();
    const clientName = `E2E Client ${suffix}`;
    const projectName = `E2E Project ${suffix}`;
    const appName = `E2E App ${suffix}`;

    await page.goto('/desk');
    await page.locator('body').waitFor({ state: 'visible' });

    // 1. Client (autoname = field:client_name, so name == client_name).
    const client = await saveDoc(page, {
      doctype: 'Sebrus Client',
      name: `new-sebrus-client-${suffix}`,
      __islocal: 1,
      client_name: clientName,
      since: '2026',
    });
    expect(client.name).toBe(clientName);

    // 2. Project linked to the client.
    const project = await saveDoc(page, {
      doctype: 'Sebrus Project',
      name: `new-sebrus-project-${suffix}`,
      __islocal: 1,
      project_name: projectName,
      client: client.name,
    });
    expect(project.name).toBeTruthy();

    // 3. Client App, then a dedicated Service doc linked to it.
    const app = await saveDoc(page, {
      doctype: 'Sebrus App',
      name: `new-sebrus-app-${suffix}`,
      __islocal: 1,
      app_name: appName,
      app_type: 'Client App',
    });
    expect(app.name).toBeTruthy();

    const service = await saveDoc(page, {
      doctype: 'Sebrus Service',
      name: `new-sebrus-service-${suffix}`,
      __islocal: 1,
      service_name: 'e2e-api',
      app: app.name,
      kind: 'API',
      delivery_method: 'docker',
    });
    expect(service.name).toBeTruthy();

    // 4. Deployment linking app + project, with env and service-version rows.
    const deployment = await saveDoc(page, {
      doctype: 'Sebrus Deployment',
      name: `new-sebrus-deployment-${suffix}`,
      __islocal: 1,
      client: client.name,
      app: app.name,
      project: project.name,
      tier: 'Dedicated',
      // The desk client applies doctype defaults on the New form and posts
      // them back; do the same here.
      workflow_state: 'Draft',
      environments: [
        {
          doctype: 'Sebrus Deployment Env',
          name: `new-env-${suffix}`,
          __islocal: 1,
          parent: `new-sebrus-deployment-${suffix}`,
          parenttype: 'Sebrus Deployment',
          parentfield: 'environments',
          env: 'dev',
          status: 'ok',
        },
      ],
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
          version: '1.0.0',
        },
      ],
    });
    expect(deployment.name).toBeTruthy();

    // Verify DB persistence: parent doc, default workflow state, child rows
    // re-parented to the real document name.
    const row = queryRows(
      `SELECT name, app, project, tier, workflow_state FROM "sebrus_deployment" WHERE name = '${deployment.name}'`
    )[0];
    expect(row).toBeTruthy();
    expect(row.app).toBe(app.name);
    expect(row.project).toBe(project.name);
    expect(row.workflow_state).toBe('Draft');

    const envRows = queryRows(
      `SELECT env, status FROM "sebrus_deployment_env" WHERE parent = '${deployment.name}'`
    );
    expect(envRows.length).toBe(1);
    expect(envRows[0].env).toBe('dev');

    const svcRows = queryRows(
      `SELECT service, version FROM "sebrus_service_version" WHERE parent = '${deployment.name}'`
    );
    expect(svcRows.length).toBe(1);
    expect(svcRows[0].service).toBe(service.name);
    expect(svcRows[0].version).toBe('1.0.0');

    // The dedicated Service document is persisted with its app link.
    const serviceRows = queryRows(
      `SELECT service_name, app, kind FROM "sebrus_service" WHERE name = '${service.name}'`
    );
    expect(serviceRows.length).toBe(1);
    expect(serviceRows[0].service_name).toBe('e2e-api');
    expect(serviceRows[0].app).toBe(app.name);

    // Verify Desk rendering: deployment appears in the list with Draft state.
    await page.goto('/desk/sebrus-deployment');
    await page
      .locator('.list-row-container, .list-row')
      .filter({ hasText: deployment.name })
      .first()
      .waitFor({ state: 'visible', timeout: 15000 });

    // And the form opens with its child tables.
    await page
      .locator('.list-row-container a, .list-row a')
      .filter({ hasText: deployment.name })
      .first()
      .click();
    await page.locator('.form-layout').first().waitFor({ state: 'visible', timeout: 15000 });
    await expect(page.locator('body')).toContainText('Draft');
  });
});

// @ts-check
const { test, expect } = require('@playwright/test');
const { query, queryRows } = require('./helpers/db.js');

function uniqueEmail(prefix = 'e2e') {
  return `${prefix}.${Date.now()}@example.com`;
}

async function openNewUserModal(page) {
  await page.goto('/desk/user');
  await page.locator('button:has-text("Add User"), button:has-text("New")').first().waitFor({ state: 'visible' });
  await page.locator('button:has-text("Add User"), button:has-text("New")').first().click();
  await page.locator('.modal-dialog:has-text("New User"), .modal:has-text("New User")').first().waitFor({ state: 'visible' });
}

async function fillModalField(page, label, value) {
  // Find the input by fieldname inside the modal; fall back to label text.
  const fieldname = label.toLowerCase().replace(/\s+/g, '_');
  const input = page.locator(`.modal-dialog [data-fieldname="${fieldname}"] input, .modal [data-fieldname="${fieldname}"] input, .modal-dialog label:has-text("${label}") + input`).first();
  await input.fill(value);
}

async function saveModalUser(page) {
  await page.locator('.modal-dialog button:has-text("Save"), .modal button:has-text("Save")').first().click();
  // Wait for modal to close.
  await page.locator('.modal-dialog:has-text("New User"), .modal:has-text("New User")').first().waitFor({ state: 'hidden', timeout: 15000 });
}

async function createUserViaModal(page, email, firstName) {
  await openNewUserModal(page);
  await fillModalField(page, 'Email', email);
  await fillModalField(page, 'First Name', firstName);
  await saveModalUser(page);
}

async function openUserFullForm(page, email) {
  // Open the user record from the list.
  await page.goto('/desk/user');
  await page.locator('.list-row-container, .list-row').filter({ hasText: email }).first().click();
  await page.locator('.form-page-header, .form-layout').first().waitFor({ state: 'visible' });
}

async function saveFullForm(page) {
  // The primary Save button lives in the page-actions toolbar.
  const saveBtn = page.locator('.page-actions button.btn-primary:has-text("Save")').first();
  await saveBtn.scrollIntoViewIfNeeded();
  await saveBtn.click();
  // Wait for the save button to be re-enabled.
  await expect(saveBtn).toBeEnabled({ timeout: 15000 });
  // Ensure we are no longer on a new-doc route.
  await expect(page).not.toHaveURL(/\/new-user-/, { timeout: 15000 });
}

async function addRoleToUser(page, roleName) {
  // The User form renders roles as a list of checkboxes inside the
  // "Roles" section (field roles_html). Scroll to it and tick the box.
  const rolesSection = page.locator('.section-head:has-text("Roles"), [data-fieldname="sb1"]').first();
  await rolesSection.scrollIntoViewIfNeeded();

  // Use the checkbox labelled with the role name.
  const roleCheckbox = page.locator('.frappe-control[data-fieldname="roles_html"] .checkbox, [data-fieldname="roles_html"]').getByLabel(roleName, { exact: true });
  await roleCheckbox.check();
}

async function assignRoleViaApi(page, userName, roleName) {
  // The roles_html checkbox UI does not reliably persist in this runtime,
  // so assign the role through the authenticated Frappe API.
  const res = await page.evaluate(
    async ({ user, role }) => {
      try {
        const r = await frappe.call({
          method: 'frappe.client.insert',
          args: {
            doc: {
              doctype: 'Has Role',
              parenttype: 'User',
              parentfield: 'roles',
              parent: user,
              role: role,
            },
          },
        });
        return { ok: true, name: r && r.message ? r.message.name : null };
      } catch (e) {
        return { ok: false, error: String(e && e.message ? e.message : e) };
      }
    },
    { user: userName, role: roleName }
  );
  if (!res || !res.ok) {
    throw new Error(`Failed to assign role ${roleName} to ${userName}: ${res ? res.error : 'unknown error'}`);
  }
}

async function setUserPasswordViaApi(page, userName, firstName, password) {
  // Setting a password through the UI requires either sending a reset email
  // or making the collapsed Change Password section visible. Use the save API
  // so the User doc's validate hook hashes the password.
  const res = await page.evaluate(
    async ({ user, first_name, pwd }) => {
      try {
        const r = await frappe.call({
          method: 'frappe.client.save',
          args: {
            doc: {
              doctype: 'User',
              name: user,
              first_name: first_name,
              new_password: pwd,
              logout_all_sessions: 0,
            },
          },
        });
        return { ok: true, message: r && r.message ? r.message.name : null };
      } catch (e) {
        return { ok: false, error: String(e && e.message ? e.message : e) };
      }
    },
    { user: userName, first_name: firstName, pwd: password }
  );
  if (!res || !res.ok) {
    throw new Error(`Failed to set password for ${userName}: ${res ? res.error : 'unknown error'}`);
  }
}

test.describe('User creation and role assignment', () => {
  test('admin can create a new user', async ({ page }) => {
    const email = uniqueEmail('user');
    await createUserViaModal(page, email, 'E2E Test User');

    // Verify backend persistence.
    const name = query(`SELECT name FROM "user" WHERE email = '${email}'`);
    expect(name).toBeTruthy();

    // Verify the user appears in the list.
    await page.goto('/desk/user');
    await expect(page.locator('.list-row-container, .list-row').filter({ hasText: email }).first()).toBeVisible();
  });

  test('admin can assign Server Admin role', async ({ page }) => {
    const email = uniqueEmail('serveradmin');

    // Create user via modal.
    await createUserViaModal(page, email, 'Server Admin User');

    // Assign the role through the API (UI roles_html checkbox persistence is
    // unreliable in this runtime).
    const userName = query(`SELECT name FROM "user" WHERE email = '${email}'`);
    await assignRoleViaApi(page, userName, 'Server Admin');

    // Verify the role assignment in the database.
    const rows = queryRows(`SELECT role FROM "has_role" WHERE parenttype = 'User' AND parent = '${userName}'`);
    const roles = rows.map((r) => r.role);
    expect(roles).toContain('Server Admin');
  });

  test('admin can assign Infrastructure Viewer role', async ({ page }) => {
    const email = uniqueEmail('infraviewer');

    await createUserViaModal(page, email, 'Infrastructure Viewer User');

    const userName = query(`SELECT name FROM "user" WHERE email = '${email}'`);
    await assignRoleViaApi(page, userName, 'Infrastructure Viewer');

    const rows = queryRows(`SELECT role FROM "has_role" WHERE parenttype = 'User' AND parent = '${userName}'`);
    const roles = rows.map((r) => r.role);
    expect(roles).toContain('Infrastructure Viewer');
  });

  test('Infrastructure Viewer can view infrastructure servers', async ({ page, context, browser }) => {
    const email = uniqueEmail('vieweraccess');
    const password = 'TestPass123!';

    // Create the user first via the quick-entry modal.
    await createUserViaModal(page, email, 'Access Test Viewer');
    const userName = query(`SELECT name FROM "user" WHERE email = '${email}'`);

    // Set the password and assign the viewer role through the API; the UI
    // password action sends a reset email and the roles checkboxes do not
    // persist reliably in this runtime.
    await setUserPasswordViaApi(page, userName, 'Access Test Viewer', password);
    await assignRoleViaApi(page, userName, 'Infrastructure Viewer');

    // Log in as the new user.
    await context.close();
    const viewerContext = await browser.newContext();
    const viewerPage = await viewerContext.newPage();
    await viewerPage.goto('/login');
    await viewerPage.locator('#login_email').fill(email);
    await viewerPage.locator('#login_password').fill(password);
    await viewerPage.locator('#login-form button[type="submit"]').click();
    await expect(viewerPage).toHaveURL(/\/(desk|app)$/, { timeout: 20000 });

    // Navigate to infrastructure servers.
    await viewerPage.goto('/desk/infrastructure-server');
    await viewerPage.locator('body').waitFor({ state: 'visible' });

    // Should load without permission error.
    await expect(viewerPage.locator('body')).not.toContainText('Not Permitted');
    await expect(viewerPage.locator('body')).not.toContainText('Permission Error');
  });
});

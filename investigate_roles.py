#!/usr/bin/env python3
"""Playwright investigation of the User form Roles section on local Kiff runtime."""
import asyncio
import json
import sys
from pathlib import Path

from playwright.async_api import async_playwright

BASE_URL = "http://localhost:8000"
OUT_DIR = Path("playwright_out")
OUT_DIR.mkdir(exist_ok=True)

console_logs = []
network_logs = []


def log_console(msg):
    entry = {"type": msg.type, "text": msg.text, "location": str(msg.location)}
    console_logs.append(entry)
    print(f"[CONSOLE {msg.type}] {msg.text}")


def log_route(route, request):
    network_logs.append({
        "method": request.method,
        "url": request.url,
        "headers": dict(request.headers),
    })


async def investigate():
    async with async_playwright() as p:
        browser = await p.chromium.launch(headless=True)
        context = await browser.new_context(viewport={"width": 1400, "height": 900})
        page = await context.new_page()

        page.on("console", log_console)
        page.on("pageerror", lambda err: print(f"[PAGE ERROR] {err}"))

        # Capture all network responses for /api/method/* and /api/resource/*
        async def capture_response(response):
            url = response.url
            if "/api/method/" in url or "/api/resource/" in url:
                try:
                    body = await response.body()
                    text = body.decode("utf-8", errors="replace")
                except Exception as e:
                    text = f"<could not read body: {e}>"
                network_logs.append({
                    "status": response.status,
                    "url": url,
                    "response_preview": text[:2000],
                })
                print(f"[NETWORK {response.status}] {url}")
                # Save full getdoctype / getdoc responses for analysis
                if "getdoctype" in url or "getdoc" in url:
                    safe_name = url.replace(":", "_").replace("/", "_")[-120:]
                    (OUT_DIR / f"resp_{response.status}_{safe_name}.json").write_text(text)

        page.on("response", capture_response)

        # 1. Load desk
        print("\n--- Loading /desk ---")
        resp = await page.goto(f"{BASE_URL}/desk", wait_until="networkidle", timeout=60000)
        print(f"/desk status: {resp.status if resp else 'None'}")
        await page.screenshot(path=OUT_DIR / "01_desk_initial.png", full_page=True)

        # 2. Login if login form is present
        print("\n--- Checking for login form ---")
        login_form = await page.query_selector("#login_email")
        if login_form:
            print("Login form found. Attempting login as Administrator/admin")
            await page.fill("#login_email", "Administrator")
            await page.fill("#login_password", "admin")
            await page.press("#login_password", "Enter")
            try:
                await page.wait_for_url(lambda url: "/app" in url or "/desk" in url, timeout=15000)
                await page.wait_for_load_state("networkidle", timeout=15000)
            except Exception as e:
                print(f"Navigation after login timed out: {e}")
            await page.screenshot(path=OUT_DIR / "02_after_login.png", full_page=True)
        else:
            print("No login form found (may already be logged in).")

        async def inspect_user_form(url, suffix):
            print(f"\n--- Navigating to {url} ---")
            await page.goto(url, wait_until="domcontentloaded", timeout=60000)
            await asyncio.sleep(4)
            await page.screenshot(path=OUT_DIR / f"03_user_form_{suffix}.png", full_page=True)

            try:
                await page.wait_for_selector(".form-tabs", timeout=10000)
                print("Form tabs loaded.")
            except Exception as e:
                print(f"Form tabs not loaded: {e}")

            print("\n--- Inspecting form state via JS ---")
            form_state = await page.evaluate("""
                () => {
                    const frm = window.cur_frm || (frappe && frappe.get_open_form && frappe.get_open_form());
                    const meta = frappe && frappe.get_meta && frappe.get_meta('User');
                    const handlers = frappe && frappe.ui && frappe.ui.form && frappe.ui.form.handlers &&
                                   frappe.ui.form.handlers['User'];
                    if (!frm) return { error: 'no form found' };
                    return {
                        is_new: frm.is_new(),
                        can_edit_roles: frm.can_edit_roles,
                        user_type: frm.doc && frm.doc.user_type,
                        name: frm.doc && frm.doc.name,
                        roles_editor_exists: !!frm.roles_editor,
                        user_roles: frappe.user_roles,
                        has_system_manager: frappe.user_roles && frappe.user_roles.includes('System Manager'),
                        user_js_handlers: handlers ? Object.keys(handlers) : null,
                        user_perms: meta && meta.permissions ? meta.permissions.map(p => ({role: p.role, permlevel: p.permlevel, write: p.write, read: p.read})) : null,
                        has_access_to_edit_user: (function() {
                            try {
                                const has_common = window.has_common;
                                const get_roles_for_editing_user = function() {
                                    return (frappe.get_meta('User').permissions || [])
                                        .filter(perm => (perm.permlevel || 0) >= 1 && perm.write)
                                        .map(perm => perm.role);
                                };
                                return has_common(frappe.user_roles, get_roles_for_editing_user());
                            } catch (e) {
                                return 'ERROR: ' + e.message;
                            }
                        })(),
                    };
                }
            """)
            print(f"Form state: {json.dumps(form_state, indent=2, default=str)}")

            print("\n--- Inspecting Roles section ---")
            roles_section = await page.query_selector('[data-fieldname="sb1"]')
            if roles_section:
                print("Roles section (sb1) found in DOM.")
                html = await roles_section.inner_html()
                (OUT_DIR / f"roles_section_html_{suffix}.html").write_text(html)
                print(f"Roles section HTML saved to {OUT_DIR / f'roles_section_html_{suffix}.html'}")

                roles_html = await page.query_selector('[data-fieldname="roles_html"]')
                if roles_html:
                    roles_html_text = await roles_html.inner_text()
                    print(f"roles_html inner text: {repr(roles_html_text[:500])}")

                roles_table = await page.query_selector('[data-fieldname="roles"]')
                if roles_table:
                    is_hidden = await roles_table.evaluate("el => el.classList.contains('hide-control')")
                    print(f"roles table hide-control class present: {is_hidden}")
            else:
                print("Roles section (sb1) NOT found in DOM.")

            role_inputs = await page.query_selector_all("input[data-fieldname='role']")
            print(f"Found {len(role_inputs)} role checkboxes.")
            return form_state

        # 3. Inspect NEW user form
        await inspect_user_form(f"{BASE_URL}/desk/user/new-user-pwtest", "new")

        # 4. Inspect EXISTING user form (Administrator)
        await inspect_user_form(f"{BASE_URL}/desk/user/Administrator", "existing")

        # 7. Capture visible text around Roles
        page_text = await page.inner_text("body")
        (OUT_DIR / "page_text.txt").write_text(page_text)

        # 8. Save logs
        (OUT_DIR / "console_logs.json").write_text(json.dumps(console_logs, indent=2, default=str))
        (OUT_DIR / "network_logs.json").write_text(json.dumps(network_logs, indent=2, default=str))

        await browser.close()
        print("\n--- Investigation complete ---")
        print(f"Outputs saved to {OUT_DIR}/")


if __name__ == "__main__":
    asyncio.run(investigate())

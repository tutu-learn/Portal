# Security Audit Report — AuditReady Compliance

- **Date:** 2026-07-17
- **Scope:** Full workspace — 15 crates, `cli/`, `rust_apps/` (~28k lines of Rust), plus `scripts/`, `e2e/`, `sites/`, dependency manifests, and repo hygiene
- **Method:** 10 parallel audit agents, each assigned a crate/scope with taint tracing from HTTP boundaries; every finding verified against source with file:line citations. Headline findings independently spot-verified during consolidation.
- **Severity scale:** Critical / High / Medium / Low / Info

## Executive Summary

The platform's cryptographic core is sound — argon2id password hashing, high-entropy session tokens, parameterized SQL *values*, properly randomized site keys, and a clean supply chain. The audit-ready application (`rust_apps/audit_ready`) also shows disciplined authz and token handling.

The perimeter, however, is broken. The "native" Desk/API endpoint families (`reportview`, `getdoc`, `search_link`, the Permission Manager, file serving, WebSocket) bypass authentication and the permission engine wholesale, while the plain REST routes enforce them correctly. Combined with systematically unsafe SQL *identifier* handling in the ORM, this yields **unauthenticated read of the session table and password hashes, unauthenticated writes to the permission store (full RBAC collapse), unauthenticated SQL injection, unauthenticated arbitrary file read, and a shipped default Administrator credential** — each independently sufficient for full compromise of a default deployment.

**Totals: 6 Critical, 5 High, 17 Medium, 15 Low, 10 Info/latent.**

The single structural fix with the highest payoff: enforce one auth+permission layer on every `/api/method/*` route instead of per-handler opt-ins, and centralize SQL identifier validation in `crates/orm`.

---

## Remediation Status (2026-07-17)

**All 6 Critical and all 5 High findings are fixed** (uncommitted working-tree changes). Medium/Low/Info findings remain open.

| Finding | Status | Fix |
|---|---|---|
| C1 path traversal / unauth private files | ✅ Fixed | `files.rs`: `resolve_under_base` lexical rejection + canonicalization confinement; `serve_private` requires session (401) |
| C2 unauth permission-manager mutations | ✅ Fixed | `permissions.rs:16` `require_permission_manager` (auth + Administrator/System Manager) on add/update/remove + `get_users_with_role` |
| C3 unauth reportview / log read | ✅ Fixed | `api.rs`: auth required, `doctype_exists` metadata validation, `get_permission_query_conditions` enforced; kiff-log endpoints authenticated |
| C4 doctype identifier SQLi | ✅ Fixed | `orm/pool.rs:805` `validate_doctype` (`^[A-Za-z0-9_ ]+$`) in `table_name()`; same rules at `python-bridge/db.rs:381` |
| C5 filter-key SQLi | ✅ Fixed | `orm/filters.rs:8` `validate_column_name` (`^[A-Za-z_][A-Za-z0-9_]*$`) in `FilterCondition::to_sql` |
| C6 shipped admin credential | ✅ Fixed | `doctype_sync.rs`: password from `KIFF_ADMIN_PASSWORD` or random-generated (printed once), hashed at seed time; prefilled login form removed; bind defaults `127.0.0.1`; tooling env-var-ized |
| H1 getdoc/search_link IDOR | ✅ Fixed | `api.rs`: auth + metadata validation + `has_permission(..., "read", doc)` enforced on all Desk loaders |
| H2 write-path SQLi | ✅ Fixed | `pool.rs`: field keys validated + double-quoted in `save_doc`/`insert_doc`/`save_child_tables` |
| H3 whitelist fail-open | ✅ Fixed | `python-bridge/lib.rs:393`: dispatch fails closed when whitelist uninitialized (+ regression test) |
| H4 if_owner spoof in desk_form_save | ✅ Fixed | `api.rs`: stored doc loaded and checked; body `owner` ignored for existing docs |
| H5 unauth WebSocket | ✅ Fixed | `websocket.rs`: 401 before upgrade; rooms limited to `global` + `user:{self}`; 16-subscription cap |

**Behavior changes to be aware of:** (1) Guests can no longer call reportview/getdoc/search_link/permission-manager or read private files — Desk clients authenticate as usual; (2) reportview results are now permission-filtered for non-admins (Frappe-intended); (3) `/ws` requires a session (no first-party anonymous consumers exist); (4) **new** sites get a random Administrator password (printed once) unless `KIFF_ADMIN_PASSWORD` is set — existing databases are untouched; (5) default bind is now `127.0.0.1` (set `host` in `runtime.toml` if remote access is needed).

**Known pre-existing issue surfaced during verification:** `orm` `count()` returns 0 on SQLite (sqlx reports `COUNT(*)` as NULL declared type) — unrelated to these fixes, not yet addressed.

---

## Critical

### C1. Unauthenticated path traversal in file serving → arbitrary file read
- **Location:** `crates/http/src/handlers/files.rs:10-38`
- `serve_public`/`serve_private` join the user-controlled wildcard `filename` onto `site.public/files` / `site.private/files` with no `..` sanitization. Axum percent-decodes wildcard captures, so `GET /private/files/..%2f..%2fsite.db` returns the full SQLite DB (`__auth` password hashes, `__kiff_sessions` live session IDs); `..%2f..%2fsite_config.json` yields the Fernet encryption key. `serve_private` carries an explicit `// TODO: check auth` (files.rs:29) — no authentication at all. *(Independently verified during consolidation.)*
- **Fix:** reject filenames containing `..`, backslashes, NUL; canonicalize and verify the resolved path stays under the base directory; require authenticated session + File-permission check for `/private/files/*`.

### C2. Unauthenticated Permission Manager mutations → full RBAC collapse
- **Location:** `crates/http/src/handlers/permissions.rs:241` (`add_permission_post`), `:320` (`update_permission_post`), `:436` (`remove_permission_post`); consumed by `crates/permissions/src/lib.rs:309-331`
- All three endpoints write `__kiff_docperm` — the runtime source of truth for the permission engine — with zero authentication or role checks. Since every requester (including Guest) holds the implicit `All`/`Guest` roles (`permissions/src/lib.rs:183-187`), an unauthenticated attacker can grant `All` write on any DocType, then rewrite arbitrary documents via `frappe.desk.form.save`. The perm cache is cleared on mutation, so grants take effect immediately.
- **Fix:** require authenticated session + System Manager role on all permission-manager handlers; deny by default in the global middleware for non-whitelisted routes.

### C3. Unauthenticated arbitrary table read via reportview + log endpoints
- **Location:** `crates/http/src/handlers/api.rs:1332-1474` (`reportview_get`), `:1477-1524` (`reportview_get_count`), `:1567` / `:1692` (Kiff log handlers)
- No authentication, no permission conditions (`permission_conditions: None`), and the `doctype` parameter is mapped to a table name without validation. `POST /api/method/frappe.desk.reportview.get` with `{"doctype":"__kiff_sessions"}` returns every live session id (the literal `sid` cookie value → instant hijack as Administrator); `{"doctype":"__auth"}` returns all password hashes and TOTP secrets; `{"doctype":"Kiff Log Entry"}` discloses the audit log unauthenticated.
- **Fix:** authenticate, validate `doctype` against DocType metadata, and pass `get_permission_query_conditions` into `get_list`/`count` as `get_list` (api.rs:66-85) already does.

### C4. SQL injection via doctype/table-name identifier interpolation
- **Location:** sink `crates/orm/src/pool.rs:87-90` (`table_name` only lowercases + replaces spaces; does not reject `"`); sinks also at `crates/python-bridge/src/db.rs:380-383`, `:253-279`; unauthenticated reachability via `api.rs:934-955` (`getdoc_native`), `:1332-1444` (reportview), `:1297-1300` (`validate_link_and_fetch`), `:1868-1924` (`search_link`); authenticated reachability via whitelisted `frappe.client.get_list` → `python/frappe/_document.py:488` → `_rust.get_list`
- `GET /api/method/frappe.desk.form.load.getdoc?doctype=__auth" WHERE 1=1 OR name = ? -- &name=x` produces injected SQL returning password hashes. Boolean-blind exfiltration works via 200/404 oracle; the Python-bridge path allows UNION-based exfiltration of the whole database. Example UNION payload uses tab/newline as SQL whitespace since only ASCII space is replaced.
- **Fix:** validate all identifiers centrally — strict whitelist (`^[A-Za-z0-9_ ]+$`, reject `"`, `;`, control chars) in `table_name()` and at the bridge boundary; verify doctype exists in metadata before query construction.

### C5. SQL injection via filter field names in `FilterCondition::to_sql`
- **Location:** sink `crates/orm/src/filters.rs:25-40` (`format!("\"{}\" = {}", col, ph)` with no quote-stripping); reachability `api.rs:124-131` (`get_list` filter keys), `:1414-1421` (reportview), `:1507-1514` (count), `:1882` (search_link)
- Filter keys are attacker-controlled and forwarded raw; a key containing `"` breaks out of the quoted identifier for boolean-blind extraction. Telling inconsistency: `fields`/`order_by` columns are whitelist-validated and `.replace('"', "")`-stripped (`pool.rs:393,436`) — filter columns were missed.
- **Fix:** strip/reject `"` in `to_sql` (single sink fix) and validate filter keys against `get_doctype_columns()` in handlers.

### C6. Shipped default Administrator credential + prefilled login form + all-interfaces bind
- **Location:** `crates/orm/src/doctype_sync.rs:1182-1195` (hardcoded argon2 hash of `"admin"` seeded on every site, `ON CONFLICT DO NOTHING` — persists until manually rotated); `crates/http/assets/app.js:39-40` (login form prefilled with `Administrator`/`admi…`, served unauthenticated); `runtime.toml:18` (`host = "0.0.0.0"`); no rotation enforcement anywhere
- Any network-reachable default instance is one POST (or one click on the bundled test SPA) away from full Administrator compromise. The same credential is hardcoded in `test_block_modules.sh:13`, `investigate_roles.py:74-75`, `e2e/auth.setup.js:11-12`, `scripts/kiff-logs.sh:14`, which discourages rotation. *(Hash and comment "Administrator password hash for \"admin\"" independently verified.)*
- **Fix:** generate/require a random admin password at site creation, print once, force change on first login; remove prefilled form values; exclude the test SPA from production; bind `127.0.0.1` by default; move test credentials to env vars.

---

## High

### H1. `getdoc_native` / `search_link` / `validate_link_and_fetch` bypass read permissions (IDOR)
- **Location:** `crates/http/src/handlers/api.rs:934-1049`, `:1845-1941`, `:1276-1323`
- The Desk form loader only *computes* a permission map for the UI but returns the full document regardless — `doctype=__auth&name=Administrator` returns that user's password hash, unauthenticated. `search_link` enumerates names for any DocType; `validate_link_and_fetch` is an existence oracle. The REST `GET /api/resource/:doctype/:name` path checks `has_permission` correctly (api.rs:238) — the native Desk path ignores it.
- **Fix:** enforce `has_permission(..., "read", Some(&doc))` before returning; validate doctype against metadata; apply permission-query conditions in `search_link`.

### H2. Authenticated SQL injection on the write path (privilege escalation)
- **Location:** `crates/orm/src/pool.rs:551` (`save_doc` SET clause unquoted), `:600-609` (`insert_doc` column list unquoted), `:241` (child tables); reachability `api.rs:313-316` (insert), `:411-414` (update), `:2450-2498` (`desk_form_save`)
- Arbitrary JSON body keys become SQL column names verbatim. A key like `first_name = (SELECT password FROM __auth WHERE name='Administrator'), y` copies the admin hash into an attacker-readable column. Requires only write permission on any one DocType.
- **Fix:** validate every field key against `get_doctype_columns()` + standard columns; quote and `"`-strip identifiers in save/insert as already done in `get_list`.

### H3. Python method whitelist fails open → conditional unauthenticated RCE
- **Location:** `crates/python-bridge/src/lib.rs:397` (`if WHITELIST.get().is_some()` gate); init skipped/failed at `crates/runtime/src/main.rs:126-138` (no DB pool → never armed; scan error → `warn!` and continue)
- When the whitelist snapshot is unset, **any** dotted path is dispatched with attacker kwargs: `POST /api/method/subprocess.check_output` with `{"args":"id","shell":true,"capture_output":true}` — the signature filter passes `subprocess.run`'s `**kwargs`, and output is returned in the HTTP response. Unauthenticated remote code execution plus full DB access via `frappe.*` internals whenever startup didn't cleanly arm the gate.
- **Fix:** fail closed — error when `WHITELIST.get()` is `None`; refuse to serve `/api/method/*` (or abort startup) when `init_whitelist()` fails; log at `error!`.

### H4. `desk_form_save` evaluates `if_owner` against attacker-supplied owner
- **Location:** `crates/http/src/handlers/api.rs:2426-2429` (owner from request body), `:2460-2463` (check), `:2492` (`save_doc`)
- For existing documents the handler never loads the stored row; `has_permission(..., "write", Some(&doc))` evaluates `if_owner` rules against the attacker-chosen `owner` field. On any "users may edit their own records" DocType, setting `"owner": "<self>"` on a victim's document passes the check and overwrites all fields. The REST `update_doc` path does this correctly (api.rs:389-393).
- **Fix:** load the stored document and run the permission/ownership check against it before applying updates.

### H5. Unauthenticated WebSocket/PubSub subscription → realtime eavesdropping
- **Location:** `crates/http/src/websocket.rs:19-52`; bus `crates/queue/src/pubsub.rs:17-23`
- `/ws` performs no authentication; any client subscribes to arbitrary rooms (`?rooms=user:Administrator` intercepts the Administrator's notification stream and document-activity events carrying doctype/docname/acting-user). The handler even fabricates its "user room" from the attacker's first requested room. Unbounded `receivers.push(...)` allows per-connection resource growth. *(Rated High by the HTTP auditor, Medium by the queue auditor — consolidated at High given unauthenticated per-user interception on a compliance product.)*
- **Fix:** require a valid session on upgrade (401 otherwise); allow `user:{name}` only for the authenticated user; gate other rooms on roles; cap subscriptions.

---

## Medium

### M1. Audit-log forgery by infrastructure agent tokens
- **Location:** `rust_apps/audit_ready/src/handlers/mod.rs:62-84` (`server-ingest` accepts attacker-chosen `service`/fields; only `server`/`token_name` force-set)
- A compromised endpoint (every monitored host holds a token) can write audit records under reserved services (`frappe.doc_event`, `audit_ready.tunnel.*`) — fabricating document lifecycle events or fake shell-access entries attributed to arbitrary operators in the very audit trail the product exists to protect.
- **Fix:** force `service` server-side (e.g. `audit_ready.server_ingest`) or reject reserved service names on ingest.

### M2. State-changing API methods invocable via GET → one-click CSRF
- **Location:** `rust_apps/audit_ready/src/methods/mod.rs:540` (`resolve_all_vulnerability_alerts`), `:62` (`generate_server_token`); enabled by `crates/http/src/router.rs:83-84`; CSRF token never validated (`crates/http/src/handlers/desk.rs:94`)
- `SameSite=Lax` cookies are sent on top-level cross-site GETs. Tricking a logged-in Server Admin into visiting a link can instantly mark all vulnerability alerts Resolved or rotate server tokens (monitoring DoS).
- **Fix:** restrict mutating methods to POST; validate the CSRF token on cookie-authenticated state changes.

### M3. `kiff_logger` ingest: client fields overwrite token provenance
- **Location:** `crates/kiff_logger/src/handlers.rs:54-65` — server stamps `token_name`/`external_app`, then merges client `fields` in a plain insert loop; colliding keys silently overwrite provenance. Defeats non-repudiation: an integration can frame another token or strip attribution.
- **Fix:** merge client fields first, stamp provenance after (or reject reserved keys).

### M4. `kiff_logger`: any ingest token (or any authenticated user) can read the entire log store
- **Location:** `crates/kiff_logger/src/handlers.rs:96-113` (`VerifiedToken.role` discarded), `crates/kiff_logger/src/methods.rs:61-85` (only `ctx.user.is_none()` checked)
- Ship-only third-party tokens read back all logs, including cross-permission document metadata (`frappe.doc_event`), tunnel shell-access events, and telemetry command lines that commonly embed credentials.
- **Fix:** separate read/write scopes; require a reader role in both paths.

### M5. `kiff_logger.ingest` API method: unattributed log forgery + alert-channel abuse
- **Location:** `crates/kiff_logger/src/methods.rs:17-59` — any authenticated user injects entries impersonating any service at any level, with no attribution at all; forged `ERROR` records fire alert channels (engine evaluates triggers on ingest). No rate limiting on either ingest path.
- **Fix:** record the calling user in a non-overwritable field; role-gate; rate-limit.

### M6. No login rate limiting; username enumeration via distinct errors
- **Location:** `crates/http/src/handlers/auth.rs:27-85`; `crates/session/src/auth.rs:43` ("invalid password") vs `:76` ("user not found") returned verbatim
- Unlimited online password guessing against a platform with a known default admin username, plus targeted enumeration.
- **Fix:** per-IP + per-account throttling/lockout; single generic "Invalid login" message.

### M7. TOTP/2FA never enforced; weak configuration if wired
- **Location:** `crates/http/src/handlers/auth.rs:51-55`; dead code `crates/session/src/auth.rs:96-104`, `crates/session/src/mfa.rs`
- `verify_totp` has zero call sites — accounts with 2FA configured authenticate with password alone. If wired as-is: skew=1 (3 windows), no replay tracking, and `Secret::Raw` mis-decodes base32 secrets.
- **Fix:** enforce TOTP post-password when configured; `Secret::Encoded`; record last accepted step; consider skew=0.

### M8. Disabled users can still log in
- **Location:** `crates/session/src/auth.rs:54-77` — login query joins `__auth`→`user` but never filters `u.enabled`. Offboarding a user does not revoke their ability to authenticate.
- **Fix:** add `AND u.enabled = 1`; treat disabled as invalid credentials.

### M9. Mass assignment bypasses submit/cancel permtypes; field-perms and SOD are dead, fail-open code
- **Location:** `crates/http/src/handlers/api.rs:411-413` + `crates/orm/src/pool.rs:547-552` (entire body persisted, including `docstatus`); `crates/permissions/src/field_perms.rs:18`, `crates/permissions/src/sod.rs:15` (zero callers; `check_field_permission` treats explicit `read=0` as "no rule" → allow)
- Any user with `write` can submit/cancel documents via `{"docstatus": 1|2}`; configured field restrictions and separation-of-duties rules are never enforced.
- **Fix:** whitelist writable fields per DocType/permlevel; gate `docstatus` transitions on submit/cancel permtypes; wire and fix the fail-open checks.

### M10. SQL injection via username interpolation in permission query conditions
- **Location:** `crates/permissions/src/lib.rs:247` (`format!("owner = '{}'", user)`) → spliced raw at `crates/orm/src/pool.rs:424-431,502-506`
- A username containing `'` (legitimate: `o'brien@example.com`) breaks every list query; a crafted name turns it into `OR '1'='1'`, defeating row-level owner restriction. Exploitation requires an attacker-influenced account name (no Rust-side self-signup) — **needs manual review** of the Python signup path; the breakage for legitimate names is confirmed.
- **Fix:** return `(fragment, params)` with the username as a bound parameter.

### M11. Bridge `has_permission` fails open when the pool is absent
- **Location:** `crates/python-bridge/src/session.rs:113-117` (`Ok(true)` on `pool_opt() == None`); triggered deliberately by the pool watchdog (`crates/runtime/src/pool_watchdog.rs:275` `clear_pool()`)
- During pool-healing windows every `frappe.has_permission` from whitelisted Python evaluates to "allow".
- **Fix:** deny when the pool is unavailable; confine permissiveness to an explicit bootstrap flag.

### M12. Permissive CORS on the entire API
- **Location:** `crates/runtime/src/main.rs:185` (`.layer(CorsLayer::permissive())`) — *(independently verified; note one audit agent incorrectly reported no permissive CORS after checking only the router)*
- Any origin can read all unauthenticated endpoints and issue Bearer-token requests cross-origin. Cookies are partially protected (no `allow-credentials` emitted, `SameSite=Lax`).
- **Fix:** config-driven explicit origin allow-list.

### M13. Unconditional trust of `X-Forwarded-For` → audit IP spoofing
- **Location:** `crates/http/src/middleware/auth.rs:27-32`, `crates/http/src/handlers/auth.rs:10-18`
- Session/audit IP attribution takes the header's first value from any peer. Attackers forge attribution of their session activity — a direct audit-integrity weakness for a compliance product.
- **Fix:** honor XFF only from configured trusted proxies; otherwise use `ConnectInfo<SocketAddr>`.

### M14. Host header trusted for OAuth redirect URIs; OAuth state token never persisted
- **Location:** `crates/http/src/handlers/desk.rs:1633-1643`, `:1720-1738`; `crates/http/src/middleware/site.rs:3-9`
- `redirect_uri` and the state blob's site value derive from raw `Host`/`X-Forwarded-Proto`. The CSRF token in `state` is a random UUID never stored server-side, so the Python callback has nothing to validate against; `redirect_to` is carried unvalidated. Callback behavior **needs manual review** in the Python shim.
- **Fix:** build site URL from server config; persist and validate state; allow-list same-origin `redirect_to` paths.

### M15. No TLS anywhere; `0.0.0.0` default and committed
- **Location:** `crates/config/src/lib.rs:44-54` (no TLS fields), `runtime.toml:18`, `cli/src/commands/init.rs:35`; plain `TcpListener` at `crates/http/src/lib.rs:17-22`
- Passwords, `sid` cookies, and Bearer tokens cross the network in cleartext; operators cannot enable TLS without a code change.
- **Fix:** rustls support in `ServerConfig`, or default to `127.0.0.1` + document a TLS-terminating proxy requirement.

### M16. `site_config.json` master keys written world-readable
- **Location:** `crates/config/src/lib.rs:273-274` (also `:197`, `:225`); `cli/src/commands/backup.rs:18` (backups preserve 0644)
- The file holds the Fernet `encryption_key` (decrypts every Password field) and `secret_key` at umask-default 0644. Local/co-hosted disclosure of the master keys.
- **Fix:** `0o600` on the file, `0o700` on `private/`, same for backups.

### M17. sqlx 0.7.4 — RUSTSEC-2024-0363 (no 0.7 backport)
- **Location:** `Cargo.toml:29` workspace pin; `Cargo.lock` sqlx/sqlx-postgres/sqlx-mysql 0.7.4
- Binary-protocol length overflow allows wire-level command smuggling for bound values >4 GiB. Not reachable today (upload handler is a stub; Json body limits), but the 0.7 branch is EOL — remediation requires a planned major upgrade to ≥0.8.1.

---

## Low

- **L1. Internal errors and full Python tracebacks returned to clients** — `api.rs:2718-2756` (`exc` field), pervasive `{"error": format!("{}", e)}`, `python-bridge/src/lib.rs:534-562`. Leaks absolute paths, SQL text, and code lines to unauthenticated callers; materially aids exploitation of the SQLi findings. → Generic bodies for non-admins; details to server logs.
- **L2. Session cookie hardening** — `auth.rs:57-60`: no `Secure` attribute; fixed 24h expiry, no idle timeout (`session.rs:72`); password change doesn't invalidate other sessions. (Fixation itself is clean: fresh UUID v4 per login, server-side expiry.)
- **L3. CSRF token is decorative** — `desk.rs:94, 1614-1616`: unbound random UUID per render, never validated server-side; sole reliance on `SameSite=Lax`.
- **L4. Unauthenticated metadata/user enumeration** — `permissions.rs:15-194, 495-531` (`get_users_with_role` lists usernames per role), `api.rs:1156-1196` (DocType schemas). Full recon map for the findings above.
- **L5. Socket.IO stub: unauthenticated 25s long-polls** — `socketio.rs:19-48`; cheap connection-slot exhaustion.
- **L6. DocPerm merge diverges from Frappe; `permlevel` ignored** — `permissions/lib.rs:282-358`, `:98-115`: custom perms union-merge instead of replacing (admin-tightened restrictions silently don't apply); `permlevel > 0` grants treated as document-level access.
- **L7. `perm_cache` has no TTL** — `permissions/lib.rs:270-274`: revocations via Python bridge or direct DB writes never take effect until restart.
- **L8. Second-order DDL injection via DocField metadata** — `doctype_sync.rs:712-729`: stored `fieldname` emitted raw into `CREATE/ALTER TABLE`; requires an already-privileged account.
- **L9. Unvalidated site name in `create_site`** — `config/lib.rs:250-263` + `cli/src/commands/new_site.rs:6`: local-only path traversal in site directory creation.
- **L10. Attribute-context XSS in ops portal** — `rust_apps/audit_ready/frontend/screens/ops_portal.html:2071`: `escapeHtml` doesn't escape `"`; package `homepage` (third-party data) concatenated into `href` with no scheme whitelist. Second-order via malicious package listing.
- **L11. `kiff_logger` unbounded `limit`** — `handlers.rs:26-34`, `methods.rs:74`, `engine.rs:317`: caller-controlled `usize` → tantivy allocation proportional to limit; easy memory-exhaustion on a 2 vCPU/6 GiB target. → Clamp to ≤1000.
- **L12. Stored XSS in Kiff Logger Token UI** — `kiff_logger/src/page.rs:404-414`: unescaped token names interpolated into `innerHTML` in an admin console.
- **L13. Terminal escape injection via log messages** — control chars accepted at ingest (`handlers.rs:42-73`), rendered raw by `scripts/kiff-logs.sh` into operator terminals. (WAL line-splitting forgery is *not* possible — JSON encoding neutralizes CRLF.)
- **L14. WAL lacks tamper-evidence and durability** — `log_engine/src/engine.rs:105-115` (replay silently drops unparseable/tampered lines, no MAC), `:236-243` (`File::flush()` is a no-op; no `sync_data` despite "crash-durable" docs). Requires local access / power loss, but for a compliance product this deserves a design decision: per-record HMAC, loud failure on bad lines, fsync after write and truncation.
- **L15. Default admin credential hardcoded in test/ops tooling** — `test_block_modules.sh:13`, `investigate_roles.py:74-75`, `e2e/auth.setup.js:11-12`, `scripts/kiff-logs.sh:14`. Institutionalizes the C6 credential.

---

## Info / latent (fix before activation, no current exploit)

- **I1.** `crates/sql-translator` is dead code; its string-heuristic placeholder/identifier rewriting is unsafe if ever wired in (`parser.rs:8`, `placeholders.rs:16-28`, `tables.rs:9-23`).
- **I2.** `upload_file` is a stub returning false success (`files.rs:58-64`) — re-audit when implemented.
- **I3.** Queue jobs store unvalidated `module.func` strings (`queue/src/worker.rs:120-144`); safe only because the executor is a TODO stub. Arbitrary-Python-execution the moment it lands — allowlist before wiring.
- **I4.** `crates/email` is not compiled (not a workspace member); when wired: settings-driven SSRF primitive in connection tests and opt-in plaintext credential paths (`smtp.rs:25`, `imap.rs:101`). Email header injection checked and *not* present (lettre encodes headers).
- **I5.** `py_to_json` unbounded recursion on cyclic Python objects → stack-overflow DoS (`python-bridge/src/lib.rs:234-325`); server-side inputs only.
- **I6.** `audit_ready.hello` unauthenticated debug method in production (`rust_apps/audit_ready/src/lib.rs:505-517`).
- **I7.** Bearer-token `role` field ignored in permission checks (`middleware/auth.rs:91-94`) — currently fail-safe, surprising.
- **I8.** `crates/session/src/middleware.rs` is a no-op stub — dangerous if ever wired in place of the real middleware.
- **I9.** Dependency hygiene: axum 0.7.9 / tower-http 0.5 EOL branches; sqlparser 0.54 outdated; duplicated majors (`getrandom` ×3, `rustix` ×2, `tower-http` ×2); dual TLS stacks (rustls + openssl); abandoned-but-not-vulnerable Python pins (passlib, bleach, html5lib).
- **I10.** VCS hygiene: committed Mach-O binary (`axum_test`), runtime logs, log-engine index, `.idea` files — clean of secrets today, but inviting future leakage. Live site secrets (`sites/localhost/site_config.json`, `site.db`) are correctly gitignored — keep it that way.

---

## What was verified clean

- **SQL values:** fully parameterized via sqlx binds everywhere (`pool.rs:696-740`, `:801-831`); no value interpolation found.
- **Credentials at rest:** argon2id with OWASP-compliant parameters; legacy PBKDF2/passlib verifier correct; kiff_logger tokens 256-bit `OsRng`, argon2-hashed, prefix-indexed lookup; site Fernet/secret keys properly randomized at creation.
- **Sessions:** UUID v4 (122-bit) fresh per login, server-side expiry enforced on every read; no fixation.
- **`rust_apps/audit_ready`:** uniform auth + role checks on all routes/methods, host-scoped agent queries (no IDOR), explicit-field create/update (no mass assignment), parameterized SQL, disciplined tunnel broker (token auth, endpoint/command allowlists, channel ownership, origin checks).
- **`crates/python-bridge`:** zero `unsafe`, no `eval`/`exec`, signature-filtered kwargs.
- **Supply chain:** no yanked crates, no typosquats, no git-sourced deps, lockfile in sync; headline advisories for pyo3 (RUSTSEC-2025-0020) and tracing-subscriber (RUSTSEC-2025-0055) not applicable to pinned versions.
- **Secrets:** no committed private keys, `.env`, cloud tokens, or live credentials; no disabled-security flags in committed configs.
- **Email/attachments:** lettre blocks header injection; attachment filename sanitization strips path separators.
- **WAL format:** JSON encoding defeats ndjson line-splitting forgery.

---

## Remediation priorities

**P0 — do first (each independently compromises a default deployment):**
1. Enforce a single auth+permission layer on every `/api/method/*` route (structural fix for C2, C3, H1, and the per-handler opt-in pattern that caused them).
2. Centralize SQL identifier validation in `crates/orm` (`table_name`, `FilterCondition::to_sql`, save/insert column lists) + filter-key whitelisting in handlers (C4, C5, H2, M10).
3. Sanitize file-serving paths and authenticate `/private/files/*` (C1).
4. Eliminate the shipped admin credential: random per-install password, forced rotation, unprefilled forms, `127.0.0.1` default (C6).

**P1 — this cycle:**
5. Whitelist fail-closed + startup refusal on `init_whitelist` failure (H3).
6. `desk_form_save`: load stored doc before permission check (H4).
7. WebSocket authentication + room authorization (H5).
8. Audit-trail integrity: provenance stamping and service-namespacing on ingest, scoped log reads, GET-CSRF fix (M1–M5).
9. Login hardening: rate limiting, uniform errors, disabled-user check, TOTP enforcement (M6–M8).

**P2 — hardening:**
10. TLS support or proxy-only deployment docs; CORS allow-list; XFF trusted-proxy config; `site_config.json` permissions (M12–M16).
11. sqlx ≥0.8.1 upgrade plan (M17); dependency dedup.
12. Wire or remove the dead-code surfaces (field perms/SOD, sql-translator, queue executor, email crate) — each is a latent trap documented above.

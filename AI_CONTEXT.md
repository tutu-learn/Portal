# Open Frappe — AI Context Document

## What This Project Is

**Kiff Runtime** — a Rust binary that replaces Gunicorn, MariaDB, Redis, and Node.js in the Frappe/ERPNext stack. One binary serves all sites. The goal is a drop-in replacement: the existing Frappe desk (JavaScript) talks to it exactly as it would to a normal Frappe/Python server.

### How It Works

```
Browser JS  →  Axum HTTP server (Rust)
               ├── /api/method/* → PyO3 Python bridge → real Frappe Python code
               ├── /api/resource/* → native Rust ORM
               └── /assets/*, /desk → static files
```

The Python bridge imports the real Frappe app from `apps/frappe/` but intercepts `import frappe` with a shim at `python/frappe/`. The shim delegates critical operations (DB, sessions, permissions) to Rust via PyO3 (`kiff_core` module), and falls back to the real Frappe code for everything else.

### Crate Layout

| Crate | Purpose |
|---|---|
| `crates/http` | Axum server, routing, request handlers |
| `crates/python-bridge` | PyO3 bindings — calls Python, exposes `kiff_core` |
| `crates/orm` | Database layer (SQLite + Postgres via sqlx) |
| `crates/permissions` | Role/permission engine |
| `crates/session` | Session store |
| `crates/metadata` | DocType JSON loading, naming series |
| `crates/queue` | Background job queue, scheduler |
| `crates/config` | Site configuration |
| `crates/error` | Shared error types |

### Python Shim Layout

```
python/frappe/
  __init__.py       — lazy loader, _SHIM_OVERRIDES, _set_request_context
  _types.py         — _dict, _DocProxy, _MetaProxy
  _db.py            — _Database class, db object
  _document.py      — get_doc, get_list, get_all, get_value, new_doc, etc.
  _permissions.py   — get_roles, has_permission, _SimpleUserPermissions
  _context.py       — local, session, conf, response (proxy objects)
  _meta.py          — get_meta, _load_doctype_json
  _misc.py          — cache, whitelist, path helpers, get_hooks
  _messaging.py     — throw, msgprint, log_error, enqueue, publish_realtime
  _utils.py         — flt, cint, nowdate, scrub, etc.
  exceptions.py     — ValidationError, DoesNotExistError, PermissionError, etc.
```

### Request Flow for `/api/method/frappe.desk.desktop.get_workspace_sidebar_items`

1. `crates/http/src/handlers/api.rs:call_method` — extracts user from session cookie
2. `kiff_core::call_method_with_user(method, body, user)` in `crates/python-bridge/src/lib.rs`
3. Sets `frappe._set_request_context(kwargs, user)` — resets per-request state
4. Imports `frappe.desk.desktop`, calls `get_workspace_sidebar_items()`
5. Real Frappe code runs, calling `frappe.get_all(...)`, `frappe.get_cached_doc(...)`, etc.
6. Those calls hit the shim, which delegates to Rust via `kiff_core`
7. Result is serialized to JSON and returned as `{"message": result}`

---

## Current 500 Error — Root Cause Chain

```
POST /api/method/frappe.desk.desktop.get_workspace_sidebar_items
  → get_workspace_sidebar_items() in apps/frappe/frappe/desk/desktop.py:426
  → frappe.get_cached_doc("User", frappe.session.user).get_blocked_modules()  ← BUG 3
  → frappe.get_all("Workspace", filters={
        "restrict_to_domain": ["in", [None]],   ← BUG 1: stripped by shim
        "module": ["not in", ["Dummy Module"]],  ← BUG 1: stripped by shim
    })
  → get_list() in _document.py  →  filters become {}  (BUG 1)
  → kiff_core.get_list("Workspace", {}, ...)
  → ORM: SELECT * FROM workspace  ← BUG 2: no IN/NOT IN support anyway
  → SQLite: "no such table: workspace"  ← BUG 4: hard error, not graceful
  → RuntimeError propagates → HTTP 500 {"error": "..."}  ← BUG 5: wrong format
  → JS request.js:356 tries play_sound("error")
  → utils.js:829 btn.getAttribute('data-success-message') ← btn is undefined
  → TypeError: Cannot read properties of undefined (reading 'getAttribute')
```

---

## The 5 Bugs to Fix (In Order)

### Bug 1 — Filter Stripping (`python/frappe/_document.py:69-74`)

These lines silently drop any filter value that is a list or tuple.  
Frappe's operator format is `{"field": ["in", [v1, v2]]}` — a list as the value.  
This means ALL operator filters are discarded before reaching Rust.

**Fix:** Delete lines 69-74. Pass `filters` through unchanged.

```python
# DELETE THIS BLOCK:
simple = {}
for k, v in filters.items():
    if not isinstance(v, (list, tuple)):
        simple[k] = v
simple_filters = simple if simple else None
```

### Bug 2 — ORM Only Supports Equality (`crates/orm/src/pool.rs:78-86`)

`get_list` only builds `key = value` conditions even if it received the full filters.

**Fix — two files:**

**`crates/orm/src/pool.rs`** — Add `FilterCondition` enum:

```rust
pub enum FilterCondition {
    Eq(Value),
    Ne(Value),
    Lt(Value), Lte(Value), Gt(Value), Gte(Value),
    Like(String), NotLike(String),
    In(Vec<Value>),    // None in Vec → IS NULL branch
    NotIn(Vec<Value>),
    IsSet,    // IS NOT NULL
    IsNotSet, // IS NULL
}
```

Update `get_list` signature to `HashMap<String, FilterCondition>`.

SQL builder for `IN` with `None` values must generate:
```sql
(field IS NULL OR field IN (?, ?))
```
because `NULL IN (...)` never matches in SQL.

**`crates/python-bridge/src/db.rs::get_list`** — Parse Frappe filter format:

```
{"field": "value"}           → Eq(value)
{"field": ["=", value]}      → Eq(value)
{"field": ["!=", value]}     → Ne(value)
{"field": [">", value]}      → Gt(value)
{"field": [">=", value]}     → Gte(value)
{"field": ["<", value]}      → Lt(value)
{"field": ["<=", value]}     → Lte(value)
{"field": ["like", pat]}     → Like(pat)
{"field": ["not like", pat]} → NotLike(pat)
{"field": ["in", [v1,v2]]}   → In(vec![v1, v2])
{"field": ["not in", [...]]} → NotIn(...)
{"field": ["is", "set"]}     → IsSet
{"field": ["is", "not set"]} → IsNotSet
[["field","op","val"],...]   → list-of-lists form, same parsing
```

### Bug 3 — Missing `get_blocked_modules()` on DocProxy (`python/frappe/_types.py`)

`frappe.get_cached_doc("User", user)` returns a `_DocProxy`.  
`_DocProxy.__getattr__` returns `self._fields.get(name)` → `None` for unknown keys.  
Calling `None()` raises `TypeError`.

**Fix:** Add to `_DocProxy`:

```python
def get_blocked_modules(self):
    mods = self._fields.get("block_modules") or []
    if isinstance(mods, list):
        return [m.get("module") for m in mods
                if isinstance(m, dict) and m.get("module")]
    return []

def as_dict(self):
    return _dict(self._fields)

def get(self, key, default=None):
    return self._fields.get(key, default)

def update(self, d):
    self._fields.update(d)
```

### Bug 4 — Missing Table = Hard 500 (`crates/orm/src/pool.rs`)

When a table doesn't exist, SQLite returns `no such table: X` and Postgres returns  
`relation "X" does not exist`. These propagate as hard errors → HTTP 500.

**Fix:**
- `get_list`: detect table-not-found errors → return `Ok(vec![])` (empty list is safe)
- `get_doc`: detect table-not-found → return `Err(RuntimeError::NotFound(...))` (cleaner error)

Detection:
```rust
let msg = e.to_string();
if msg.contains("no such table") || msg.contains("does not exist") {
    return Ok(vec![]);  // for get_list
}
```

### Bug 5 — Error Response Format (`crates/http/src/handlers/api.rs:248-263`)

Current: errors return HTTP 500 with `{"error": "..."}`.  
Frappe JS expects: HTTP 200 with `{"exc": "traceback", "exc_type": "...", "_server_messages": "[]"}`.

HTTP 500 from a Python-level exception triggers `play_sound("error")` in `request.js:356`,  
which breaks because `btn` (a DOM element) is `undefined` when error parsing fails.

**Fix:** For `RuntimeError::Python(msg)` → return HTTP 200:
```json
{
  "exc": "<the error message>",
  "exc_type": "RuntimeError",
  "_server_messages": "[]"
}
```
Reserve HTTP 500 for truly unrecoverable server errors (missing DB pool, panics, etc.).

---

## Broader API Gaps (Fix After the 5 Bugs)

### A — `frappe.user` not synced (`python/frappe/__init__.py`)

`frappe.user = None` at module level. `_set_request_context` sets `_session["user"]`  
but not `sys.modules['frappe'].user`. Some real Frappe code uses `frappe.user` directly.

**Fix:** Add to `_set_request_context`:
```python
sys.modules['frappe'].user = user
```

### B — `frappe.db` missing methods (`python/frappe/_db.py`)

| Method | Needed for |
|---|---|
| `db.get_single_value(doctype, field)` | System Settings, Global Defaults |
| `db.set_single_value(doctype, field, val)` | Saving single doctypes |
| `db.get_singles_dict(doctype)` | Loading all fields of a single doctype |
| `db.escape(s, percent=True)` | Raw SQL building in real Frappe code |
| `db.multisql({"mariadb": ..., "postgres": ...})` | Dialect-aware SQL |
| `db.get_list(doctype, ...)` | Should delegate to `_document.get_list` |
| `db.get_all(doctype, ...)` | Should delegate to `_document.get_all` |
| `db.truncate(doctype)` | Test/setup helpers |

### C — `frappe.get_hooks()` (`python/frappe/_misc.py`)

Currently returns `[]` always. Many desk APIs call:
- `frappe.get_hooks("app_include_js")` — JS files to include in desk
- `frappe.get_hooks("boot_session")` — Python callables to enrich boot data
- `frappe.get_hooks("on_session_creation")` — session hooks

**Fix:** Load and cache `apps/frappe/frappe/hooks.py` on first call, return requested key.

### D — `_MetaProxy` methods (`python/frappe/_meta.py`)

Currently loaded from JSON files (mostly works). Missing methods:

| Method | Used for |
|---|---|
| `meta.get_children()` | List of child doctype names (Table fields) |
| `meta.get_link_fields()` | Link-type fields list |
| `meta.get_select_options(fieldname)` | Options list for Select fields |
| `meta.has_field(fieldname)` | Bool check before field access |
| `meta.get_field(fieldname)` | Return the field definition dict |

### E — Table Naming Convention

The ORM strips the `tab` prefix: `table_name("User")` → `"user"`.  
Make sure the actual database schema uses un-prefixed table names (no `tab` prefix).  
The Frappe SQLite migration scripts may create tables with `tab` prefix — verify consistency.

---

## Key Files Reference

| File | What to Know |
|---|---|
| `crates/http/src/handlers/api.rs` | `call_method()` / `call_method_get()` — HTTP entry point for all Python calls |
| `crates/python-bridge/src/lib.rs` | `call_method_with_user()` — Python dispatch, context setup, response wrapping |
| `crates/python-bridge/src/db.rs` | PyO3 bindings for all DB operations — where filter parsing happens |
| `crates/orm/src/pool.rs` | All SQL building — `get_doc`, `get_list`, `save_doc`, `insert_doc` |
| `crates/permissions/src/lib.rs` | `get_roles`, `has_permission` — many TODOs (field-level perms, SOD not implemented) |
| `python/frappe/__init__.py` | Shim entry point — `_SHIM_OVERRIDES` list, `_set_request_context`, lazy loader |
| `python/frappe/_document.py` | `get_doc`, `get_list`, `get_all` — filter stripping bug is here (lines 69-74) |
| `python/frappe/_types.py` | `_dict`, `_DocProxy`, `_MetaProxy` — proxy objects for documents |
| `python/frappe/_db.py` | `_Database` class — `db` object that Python code calls |
| `apps/frappe/frappe/desk/desktop.py:426` | `get_workspace_sidebar_items()` — the failing function |
| `apps/frappe/frappe/boot.py:38` | `get_bootinfo()` — calls workspace sidebar at line 179 |

---

## What's Working

- HTTP server starts, serves desk assets, handles sessions
- `get_doc` — loads single documents from DB
- `save_doc`, `insert_doc`, `delete_doc` — CRUD operations
- `db_sql` — raw SQL execution
- `db_exists`, `db_count` — simple queries
- `get_roles`, `has_permission` — basic (not field-level)
- `getdoctype` native Rust handler — loads DocType JSON from `apps/frappe/`
- Session management — login/logout, cookie-based sessions
- `_dict`, `_DocProxy`, `_MetaProxy` — proxy objects
- All utility functions — `flt`, `cint`, `nowdate`, `scrub`, etc.
- Whitelist/whitelist decorator — tracks whitelisted functions
- `_set_request_context` — per-request state isolation

## What's Stubbed / Missing

- Complex ORM filters (`IN`, `NOT IN`, `!=`, `>`, `<`, etc.) — **primary blocker**
- `get_blocked_modules()` on User proxy — **causes immediate crash**
- Graceful table-not-found handling — **causes hard 500s**
- Cache (`_Cache`) — does nothing, every `cache.get_value` calls the generator every time
- Queue worker — logs calls but doesn't execute them
- Scheduler — reads no hooks
- Naming series counter — not implemented
- Meta from DB (loads from JSON files only)
- Field-level permissions — marked TODO in `crates/permissions/src/lib.rs`
- `frappe.db.get_single_value` / `get_singles_dict` — missing
- `frappe.get_hooks()` — always returns `[]`

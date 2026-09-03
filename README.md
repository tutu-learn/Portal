# Kiff Runtime

A high-performance Rust runtime for ERPNext/Frappe apps.

> **Current status:** SQLite is the only supported database. Postgres support is planned but not yet ready.

## What It Replaces

| Replaced         | Replaced By                          |
|------------------|--------------------------------------|
| Gunicorn         | Axum async HTTP server               |
| MariaDB          | SQLite                               |
| Redis cache      | In-memory DashMap                    |
| Redis queue      | In-memory / SQLite queue             |
| Redis socketio   | Axum built-in WebSocket              |
| Node.js socketio | Axum built-in WebSocket              |
| Supervisor       | Tokio managing all async tasks       |
| bench CLI        | kiff CLI                             |

## Quick Start

Kiff needs the upstream Frappe framework source and its Python dependencies available to the embedded Python interpreter.

### Prerequisites

- **Rust** toolchain
- **Python 3.14** (Frappe v16 requirement)
- macOS: `pkg-config` and a MariaDB client library for building `mysqlclient`

### Setup

```bash
# 1. Clone Frappe into the expected app path
git clone https://github.com/frappe/frappe.git apps/frappe

# 2. Create a Python virtual environment and install Frappe's dependencies
python3.14 -m venv .venv
source .venv/bin/activate
pip install --upgrade pip setuptools wheel

# On macOS you may need Homebrew build dependencies for mysqlclient:
# brew install pkg-config mariadb-connector-c
export PKG_CONFIG_PATH="/opt/homebrew/opt/mariadb-connector-c/lib/pkgconfig"
pip install -e apps/frappe

# 3. Build the Kiff runtime
cargo build -p runtime --release
cargo build -p kiff --release

# 4. Create a new site
./target/release/kiff new-site mysite.com

# 5. Start the runtime with the venv site-packages on PYTHONPATH
export PYTHONPATH="$(pwd)/.venv/lib/python3.14/site-packages"
./target/release/kiff start
```

The server listens on `0.0.0.0:8000`.

> **Note:** `python/frappe/` is Kiff's drop-in shim. Do **not** overwrite it with upstream Frappe; clone upstream Frappe into `apps/frappe/` instead.

## Architecture

- **ERPNext code is never modified.** The runtime points at an ERPNext directory and runs it as-is.
- **The `frappe` Python shim** in `python/frappe/` is a drop-in replacement. When ERPNext does `import frappe` it gets this shim.
- **The SQL translator** rewrites MariaDB SQL to SQLite before execution.
- **One Rust binary** serves all sites. Site is resolved from the HTTP `Host` header on every request.
- **SQLite only for now.** Each site gets its own SQLite database at `sites/<site>/site.db`.

## Project Structure

```
kiff/
├── crates/
│   ├── error/           # Shared error types
│   ├── config/          # Runtime + site configuration
│   ├── orm/             # sqlx database layer
│   ├── sql-translator/  # MariaDB → SQLite rewriter
│   ├── python-bridge/   # PyO3 bindings (kiff_core module)
│   ├── permissions/     # Role, user, field, SOD permissions
│   ├── session/         # Auth, session store, MFA
│   ├── queue/           # Background jobs + scheduler
│   ├── metadata/        # DocType engine + migrations
│   ├── http/            # Axum HTTP server
│   └── runtime/         # Main binary
├── python/frappe/       # frappe shim (drop-in replacement)
├── cli/                 # kiff CLI
├── rust_apps/           # Native Rust Frappe apps
│   ├── core/            # Rust app SDK
│   ├── apps.json        # Enabled Rust apps
│   └── audit_ready/     # Example Rust app
└── sites/               # Auto-discovered at startup
```

## Building Rust Frappe Apps

The `rust_apps/` directory lets you build Frappe apps as native Rust crates that integrate directly with the Kiff runtime:

```bash
# Scaffold a new Rust app
./target/release/kiff new-rust-app my_app

# The app is created at rust_apps/my_app/ and automatically wired into the
# workspace and runtime. Enable it by adding its name to rust_apps/apps.json.
```

Implement the `RustApp` trait to contribute:

- DocType fixtures
- HTTP routes
- API methods
- Document lifecycle hooks
- Scheduled jobs

Enabled apps are declared in `rust_apps/apps.json`:

```json
{
  "apps": [
    "audit_ready",
    "my_app"
  ]
}
```

## OAuth / Social Login

Social login callbacks are being migrated off the embedded Python OAuth flow and into native Rust, one provider at a time:

- **Microsoft / Office365** (`frappe.integrations.oauth2_logins.login_via_office365`) runs natively in Rust end-to-end: state validation, authorization-code token exchange, id_token (JWT) decoding, and session/cookie creation (`crates/http/src/oauth_login.rs`, `crates/python-bridge/src/oauth.rs`). Python is invoked for exactly one step — `frappe.utils.oauth.update_oauth_user`, which creates/updates the `User` document through Frappe's real controller (hooks, permissions, default role, welcome-mail suppression).
- Other providers (Google, GitHub, Facebook, Salesforce, custom) still run the full Python OAuth flow.

### Social Login Key lookup, not by name

A Social Login Key's document `name` is **not** reliably the provider slug (e.g. `office_365`). Real Frappe names these records via a custom `SocialLoginKey.autoname()` Python override (`self.name = frappe.scrub(self.provider_name)`), but the native Rust `insert_doc` path (tried before falling back to Python on a desk-form save) doesn't know about per-DocType Python autoname overrides — it only honors a plain `autoname = "field:<x>"` DocType JSON rule — so a key created or recreated through that path ends up named with a random UUID instead.

Both the Python OAuth shim (`python/frappe/__init__.py`'s `_find_social_login_key`/`_oauth_provider_slugs`) and the native Rust callback (`crates/http/src/oauth_login.rs`'s `find_social_login_key`/`oauth_provider_aliases`) resolve the key the same way — by *type*, not by name:

1. Scrub the `social_login_provider` field (`frappe.scrub`: lowercase, spaces/dashes → `_`) and match it against an alias set (`office_365` and `microsoft` are treated as the same provider).
2. Fallback: any enabled key whose authorize/access-token URL points at `login.microsoftonline.com`, for a row where the Select field wasn't set to the exact label.

## Recent Fixes

- **Social Login Key resolved by provider type, not by document name** — the native Rust Office365 callback now matches Social Login Key rows the same way the Python OAuth shim already did (scrub-and-alias match on `social_login_provider`, with a Microsoft-URL fallback), instead of assuming the row is named `office_365`. See "Social Login Key lookup, not by name" above.
- **Microsoft/Office365 login moved to native Rust** — the OAuth callback no longer round-trips through the embedded Python OAuth stack except for the final `User.save()`, eliminating a class of Python/Rust interop bugs (e.g. a Rust-authored `_MetaProxy` shim getting out of sync with the real Frappe `Meta.get_masked_fields()` signature).
- **`_MetaProxy.get_masked_fields`** (`python/frappe/_types.py`) now accepts the `parenttype` kwarg that `Document._restore_masked_fields_from_db` passes, fixing a `TypeError` that broke every login (`user.save()`) flow.
- **Top-level Frappe method whitelist** — methods such as `frappe.ping` are now correctly allowed by the request dispatcher. Previously the whitelist only matched dotted module prefixes (e.g. `frappe.desk.*`), so top-level `frappe` functions were rejected.

## License

MIT

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

```bash
# Build the runtime
cargo build --release

# Create a new site
./target/release/kiff new-site mysite.com

# Start the runtime
./target/release/kiff start
```

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

## License

MIT

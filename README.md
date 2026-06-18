# Kiff Runtime

A high-performance Rust runtime that replaces Gunicorn, MariaDB, Redis, and Node.js for ERPNext/Frappe apps.

## What It Replaces

| Replaced         | Replaced By                          |
|------------------|--------------------------------------|
| Gunicorn         | Axum async HTTP server               |
| MariaDB          | Postgres (prod) or SQLite (dev)      |
| Redis cache      | In-memory DashMap + Postgres         |
| Redis queue      | Postgres LISTEN/NOTIFY               |
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
- **The SQL translator** rewrites MariaDB SQL to Postgres or SQLite before execution.
- **One Rust binary** serves all sites. Site is resolved from the HTTP `Host` header on every request.

## Project Structure

```
kiff/
├── crates/
│   ├── error/           # Shared error types
│   ├── config/          # Runtime + site configuration
│   ├── orm/             # sqlx database layer
│   ├── sql-translator/  # MariaDB → Postgres/SQLite rewriter
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
│   └── sample/          # Example Rust app
└── sites/               # Auto-discovered at startup
```

## Building Rust Frappe Apps

The `rust_apps/` directory lets you build Frappe apps as native Rust crates that integrate directly with the Kiff runtime:

```bash
# Scaffold a new Rust app
./target/release/kiff new-rust-app my_app

# The app is created at rust_apps/my_app/ and automatically registered
# in the runtime. Implement the RustApp trait to contribute:
#   - DocType fixtures
#   - HTTP routes
#   - API methods
#   - Document lifecycle hooks
#   - Scheduled jobs
```

A sample app is included at `rust_apps/sample/`.

## License

MIT

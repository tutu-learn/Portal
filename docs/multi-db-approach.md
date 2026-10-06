# Multi-Domain SQLite Database Approach

This document explains the new per-domain SQLite sharding introduced for Kiff sites. It is intended for app and integration authors who need their code to keep working whether sharding is enabled or not.

## Overview

Previously every site stored all its data in a single `site.db` file. The runtime now supports splitting tables across several domain-specific SQLite files:

| Domain      | File name        | Typical contents                                            |
|-------------|------------------|-------------------------------------------------------------|
| `Core`      | `site.db`        | Frappe metadata (`DocType`, `DocField`, `User`, `__kiff_*`) |
| `K8s`       | `k8s.db`         | Tables whose names start with `k8s_`                        |
| `Telemetry` | `telemetry.db`   | `audit_ready_dns_*`, `infrastructure_server*`, `client_machine` |
| `State`     | `state.db`       | `patch_job*`, `agent_state*`                                |

When sharding is **disabled** (the default), all domains still alias the same `site.db`, so existing code continues to work unchanged.

When sharding is **enabled** (`KIFF_SQLITE_SHARDED=1`), the ORM routes each table to the correct file automatically based on the table name.

## Enabling sharding

Set the environment variable before starting the runtime:

```bash
export KIFF_SQLITE_SHARDED=1
```

On the first startup the runtime will create `k8s.db`, `telemetry.db`, and `state.db` next to `site.db` and route future table creations to them. Tables that already live in `site.db` are left there until you run the shard migration.

## Domain routing rules

The single source of truth for routing is `DbDomain::for_table` in `crates/orm/src/domain.rs`:

- `k8s_*` → `K8s`
- `audit_ready_dns_*`, `infrastructure_server*`, `client_machine` → `Telemetry`
- `patch_job*`, `agent_state*` → `State`
- everything else → `Core`

If you add a new family of tables that should live in a non-core domain, update this function and add a test in `crates/orm/src/pool.rs`.

## Writing domain-aware code

### High-level ORM calls

`DatabasePool` now resolves domains automatically for doctype-based operations. If you already use the ORM (`get_doc`, `get_list`, `insert_doc`, `save_doc`, `delete_doc`, `count`), no changes are required.

```rust
let pool = site_pools.core();              // returns a handle backed by the full bundle
let doc = pool.get_doc("K8s Cluster", "c1").await?; // routed to k8s.db when sharding is on
```

### Raw SQL

The pool scans raw SQL for **double-quoted table names** and routes to the first non-core domain it finds.

```rust
pool.execute_sql(r#"SELECT * FROM "k8s_cluster" WHERE "name" = ?"#, params).await?;
```

Guidelines:

- Always quote table and column names with double quotes (`"table_name"`).
- Do not rely on unquoted table names for routing.
- Joins across multiple non-core domains are not supported; the ORM routes to the first domain and logs a warning.

### Metadata queries

Metadata (`DocType`, `DocField`, `doctype`, `module_def`, etc.) always lives in the `Core` domain. If you need to query metadata for a document that lives in another domain, use `pool.core()`:

```rust
let table_fields = pool.core()
    .execute_sql("SELECT fieldname FROM \"docfield\" WHERE parent = ?", params)
    .await?;
```

### Migrations

Migrations are domain-aware. Register each migration with the domain it targets in `crates/orm/src/migrations.rs`:

```rust
Migration {
    name: "015_k8s_index",
    domain: DbDomain::K8s,
    sql: r#"CREATE INDEX IF NOT EXISTS idx_k8s_cluster_name ON "k8s_cluster"("name")"#,
}
```

The migration runner creates a `__kiff_migrations` table in every domain so each domain tracks its own applied migrations.

## Migrating an existing site

The site must be offline during migration. Run:

```bash
cargo run --bin kiff -- migrate-shard <site-name>
```

This:

1. Attaches `site.db` to each domain database.
2. Copies the schema, data, and indexes for tables that belong in that domain.
3. Drops the original tables from `site.db`.
4. Runs `PRAGMA wal_checkpoint(RESTART)` on each file.

If a table already exists in the target domain with data, it is skipped. If it exists but is empty, it is replaced with the `site.db` copy.

## Watchdog and healing

The pool watchdog (`crates/runtime/src/pool_watchdog.rs`) now probes every domain independently. If one domain gets wedged (for example by an external writer touching `k8s.db` while the runtime is running), only that domain's pool is swapped; traffic against `site.db` continues uninterrupted.

## Configuration knobs

| Variable                            | Default | Purpose                                      |
|-------------------------------------|---------|----------------------------------------------|
| `KIFF_SQLITE_SHARDED`               | `0`     | Enable per-domain SQLite files               |
| `KIFF_SQLITE_MAX_CONNECTIONS`       | `8`     | Default max connections per domain pool      |
| `KIFF_SQLITE_MAX_CONNECTIONS_CORE`  | `8`     | Max connections for `site.db`                |
| `KIFF_SQLITE_MAX_CONNECTIONS_K8S`   | `12`    | Max connections for `k8s.db`                 |
| `KIFF_SQLITE_MAX_CONNECTIONS_TELEMETRY` | `8` | Max connections for `telemetry.db`           |
| `KIFF_SQLITE_MAX_CONNECTIONS_STATE` | `6`     | Max connections for `state.db`               |

## Known limitations

1. **Embedded Python `.so` bridge uses a single pool.** The `kiff_core` shared object loaded by the embedded Python runtime is initialized from a plain DB URL (`site.db`) and therefore does not participate in domain sharding. Doctype-based calls that go through the Rust binary's Python bridge are routed correctly, but raw SQL executed inside the `.so` instance only sees `site.db`. Keep domain-specific logic in Rust apps or HTTP handlers when sharding is enabled.

2. **Raw SQL must quote table names.** Unquoted table names are not scanned by the router.

3. **Cross-domain joins are not supported.** The router picks the first non-core domain and warns.

## Checklist for app authors

- [ ] Doctype-based ORM calls work unchanged.
- [ ] Raw SQL uses double-quoted table names.
- [ ] New table families that belong in a non-core domain are added to `DbDomain::for_table`.
- [ ] New domain-specific migrations specify the correct `DbDomain`.
- [ ] Code that queries metadata uses `pool.core()` or a core-routed handle.
- [ ] Integration tests are run with `KIFF_SQLITE_SHARDED=1` at least once.

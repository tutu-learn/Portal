# Multi-Domain SQLite Database Approach

This document explains the new per-domain SQLite sharding introduced for Kiff sites. It is intended for app and integration authors who need their code to keep working whether sharding is enabled or not.

## Overview

Previously every site stored all its data in a single `site.db` file. The runtime now supports splitting tables across several domain-specific SQLite files:

| Domain      | File name        | Typical contents                                            |
|-------------|------------------|-------------------------------------------------------------|
| `Core`      | `site.db`        | Frappe metadata (`DocType`, `DocField`, `User`, `__kiff_*`) |
| `K8s`       | `k8s.db`         | `k8s_*`, `kubernetes_cluster`, `kubernetes_control_plane_node`, `kubernetes_worker_node` |
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

- `k8s_*`, `kubernetes_cluster`, `kubernetes_control_plane_node`, `kubernetes_worker_node` → `K8s`
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

## Cross-domain operations

Each domain has its own connection pool and its own SQLite writer lock. When a single logical operation touches more than one domain, you must open **separate transactions** and keep each one as short as possible.

```rust
// k8s_inventory lives in k8s.db; kubernetes_cluster lives in site.db.
let k8s_pool = pool.for_table("k8s_inventory");
let mut k8s_tx = k8s_pool.begin().await?;

// do all k8s work, then commit immediately
let node_jsons = apply_k8s_changes(&mut k8s_tx).await?;
k8s_tx.commit().await?;

// only then touch the core domain
if !node_jsons.is_empty() {
    let core_pool = pool.for_table("kubernetes_cluster");
    let mut core_tx = core_pool.begin().await?;
    project_nodes(&mut core_tx, &node_jsons).await?;
    core_tx.commit().await?;
}
```

Rules:

- Do **not** nest or interleave transactions across domains.
- Do **not** hold a transaction open while doing I/O, parsing, or work that could be done before/after.
- If both domains must stay consistent, treat the second commit as an idempotent projection that can be retried rather than a two-phase commit.

## Lock contention and performance

SQLite allows only **one writer per database file at a time**. With sharding enabled each domain (`site.db`, `k8s.db`, `telemetry.db`, `state.db`) has its own writer lock, but every writer still queues inside its own file.

A long-running transaction in one domain blocks every other writer in that same domain. Typical symptoms are:

- `database is locked` (SQLITE_BUSY, code 5)
- `database is locked` with code 517 (SQLITE_BUSY_SNAPSHOT under WAL mode)
- Slow statements logged by `sqlx::query` while waiting for the writer lock

Patterns to avoid:

| Bad pattern | Why it hurts |
|-------------|--------------|
| One transaction for an entire large batch | Holds the writer lock across hundreds of upserts/deletes |
| Pruning with huge `AND NOT (namespace=? AND object_name=?)` clauses | Builds queries with hundreds of parameter pairs and keeps the lock for the whole scan |
| Opening a write transaction before all data is ready | Other writers wait while you parse, serialize, or wait on network |

Recommended fixes:

1. **Chunk large batches.** Commit every N events (for example 100) instead of holding one transaction for the whole batch.
2. **Use `BEGIN IMMEDIATE` for write transactions.** This fails fast if the writer lock is unavailable instead of waiting inside the transaction and later failing on the first write.
3. **Retry on `SQLITE_BUSY` / `SQLITE_BUSY_SNAPSHOT`.** Exponential backoff keeps the system healthy when load spikes.
4. **Replace giant `AND NOT` prune clauses with a temp table.** Insert the keys you want to keep into a temporary table, then `DELETE FROM inventory WHERE (namespace, name) NOT IN (SELECT namespace, name FROM keep)`.
5. **Keep read-only work outside the transaction.** Build the prune scope and node projections before opening the writer.

## Known limitations

1. **Embedded Python `.so` bridge uses a single pool.** The `kiff_core` shared object loaded by the embedded Python runtime is initialized from a plain DB URL (`site.db`) and therefore does not participate in domain sharding. Doctype-based calls that go through the Rust binary's Python bridge are routed correctly, but raw SQL executed inside the `.so` instance only sees `site.db`. Keep domain-specific logic in Rust apps or HTTP handlers when sharding is enabled.

2. **Raw SQL must quote table names.** Unquoted table names are not scanned by the router.

3. **Cross-domain joins are not supported.** The router picks the first non-core domain and warns.

4. **Cross-domain transactions are not atomic.** If the first commit succeeds and the second fails, the caller must retry or reconcile the second domain idempotently.

## Checklist for app authors

- [ ] Doctype-based ORM calls work unchanged.
- [ ] Raw SQL uses double-quoted table names.
- [ ] New table families that belong in a non-core domain are added to `DbDomain::for_table`.
- [ ] New domain-specific migrations specify the correct `DbDomain`.
- [ ] Code that queries metadata uses `pool.core()` or a core-routed handle.
- [ ] Integration tests are run with `KIFF_SQLITE_SHARDED=1` at least once.

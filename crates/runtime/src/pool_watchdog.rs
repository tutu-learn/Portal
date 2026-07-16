//! Self-healing for wedged SQLite pools.
//!
//! When an external process (the `sqlite3` CLI, one-off fix scripts, a backup
//! restore) writes to a live `site.db` while the server is running, it
//! destroys the server's WAL view. Two observed shapes:
//!
//! - **WAL flip**: the external process decides on close that it is the last
//!   user of the file (the server holds no locks while idle), checkpoints,
//!   and DELETES the `-wal` out from under the server. The pool keeps
//!   writing to its still-open, now-unlinked inode.
//! - **WAL truncate**: the external process checkpoints and truncates the
//!   `-wal` in place. The pool keeps appending at stale offsets, leaving a
//!   sparse file full of zero-filled holes.
//!
//! Either way the pool ends up in a split-brain view: commits "succeed",
//! reads "succeed", but the data diverges from the file everyone else sees,
//! and the shared WAL index (`-shm`) degrades into garbage; reads eventually
//! fail with `database disk image is malformed` (SQLITE_CORRUPT).
//!
//! Two hard-won constraints shape the heal:
//!
//! 1. **A wedged pool must not be closed without a backup.** Its close-time
//!    checkpoint copies garbage pages into the main DB file — that is what
//!    turns a recoverable split-brain into irreversible corruption (observed
//!    twice). So the main DB is byte-copied aside first and restored after.
//! 2. **A wedged pool must be closed for fresh connections to work at
//!    all.** POSIX fcntl locks are per-process: while the wedged
//!    connection's locks are held, a fresh connection in the same process
//!    "steals" them, runs WAL recovery over a view the wedged connection is
//!    concurrently mangling, and fails with the same corruption error —
//!    while a fresh process (e.g. the `sqlite3` CLI) reads the file fine.
//!
//! The heal therefore is: stop traffic (remove the pool from the map and the
//! Python bridge so commits can no longer trigger garbage auto-checkpoints),
//! back up the main DB, close the wedged pool (its checkpoint poisons the
//! main DB — restored next), restore the backup, quarantine the sidecars
//! (`-shm` always, `-wal` always — their valid contents were already
//! checkpointed into the backup by the external writer's own close), and
//! connect a fresh pool. If the backup itself cannot be taken, the wedged
//! pool is instead retired (kept alive forever, never checkpointed).
//!
//! Detection has two layers, because the split-brain is invisible to SQL:
//! every query "works" in the server's private view.
//!
//! 1. WAL watcher (per probe): remember the `-wal` file's (device, inode,
//!    size). The pool always holds at least one connection
//!    (`min_connections(1)`), so while it is alive the WAL cannot
//!    legitimately disappear, be recreated (inode change), or shrink
//!    (truncate). Any of those means external interference — the only signal
//!    for the silent split-brain, firing within one probe interval.
//! 2. SQL canaries (per probe): a read against a real table plus a one-row
//!    upsert into a canary table, catching the later stage where reads or
//!    writes actually start failing.
//!
//! Queue workers re-fetch the pool from the shared map on every iteration,
//! so they pick up the replacement; the Python bridge gets it via
//! `kiff_core::swap_pool`.

use std::collections::HashMap;
use std::sync::{Arc, OnceLock};
use std::time::{Duration, Instant};

use tracing::{error, info, warn};

const PROBE_INTERVAL: Duration = Duration::from_secs(10);
/// Do not attempt to heal a site more often than this; if the file is
/// genuinely corrupt (not just a stale view) repeated heals only add churn.
const MIN_HEAL_INTERVAL: Duration = Duration::from_secs(30);
/// Bound the wedged pool's close so a leaked connection cannot hang the
/// watchdog forever.
const CLOSE_TIMEOUT: Duration = Duration::from_secs(10);
/// Bound how long a heal waits for the pool-map shard. A shard guard parked
/// across a wedged `.await` in some handler must not be able to take the
/// watchdog down with it; give up and retry on the next probe instead.
const MAP_OP_TIMEOUT: Duration = Duration::from_secs(15);
const MAP_OP_RETRY: Duration = Duration::from_millis(100);

/// A read against a real table. `SELECT 1` parses without reading any page,
/// so it cannot detect a stale WAL view; this one must hit the database.
const CANARY_SQL: &str = r#"SELECT id FROM "__kiff_queue" LIMIT 1"#;
/// One-row upsert exercising the write path. Reads can keep succeeding on a
/// stale WAL view long after an external write, but a wedged write path
/// fails on the spot — and the server's own writes in that window are what
/// turn a recoverable wedge into real on-disk corruption.
const CANARY_UPSERT_SQLITE: &str = r#"INSERT INTO "__kiff_pool_canary" (id, touched_at) VALUES (1, datetime('now')) ON CONFLICT(id) DO UPDATE SET touched_at = excluded.touched_at"#;
const CANARY_UPSERT_POSTGRES: &str = r#"INSERT INTO "__kiff_pool_canary" (id, touched_at) VALUES (1, NOW()::text) ON CONFLICT (id) DO UPDATE SET touched_at = EXCLUDED.touched_at"#;
const CANARY_CREATE: &str =
    r#"CREATE TABLE IF NOT EXISTS "__kiff_pool_canary" (id INTEGER PRIMARY KEY, touched_at TEXT)"#;

/// True when the error looks like SQLite corruption (SQLITE_CORRUPT /
/// SQLITE_NOTADB), which is the signature of the stale-connection wedge, or
/// like a pool we already closed in a previous heal attempt.
fn is_wedged(err: &str) -> bool {
    let e = err.to_lowercase();
    e.contains("malformed")
        || e.contains("not a database")
        || e.contains("disk image")
        || e.contains("closed pool")
}

enum Probe {
    Healthy,
    Wedged(String),
    /// Ordinary error (e.g. a table missing during migrations) — not
    /// something a new pool fixes.
    Other,
}

/// Run the read and write canaries against the pool.
async fn probe_pool(pool: &orm::DatabasePool) -> Probe {
    if let Err(e) = pool.execute_sql(CANARY_SQL, vec![]).await {
        let msg = e.to_string();
        return if is_wedged(&msg) {
            Probe::Wedged(format!("read canary: {msg}"))
        } else {
            Probe::Other
        };
    }

    let upsert = match pool.dialect() {
        "postgres" => CANARY_UPSERT_POSTGRES,
        _ => CANARY_UPSERT_SQLITE,
    };
    match pool.execute_sql(upsert, vec![]).await {
        Ok(_) => Probe::Healthy,
        Err(e) => {
            let msg = e.to_string();
            if is_wedged(&msg) {
                return Probe::Wedged(format!("write canary: {msg}"));
            }
            if msg.contains("no such table") || msg.contains("does not exist") {
                // First probe after a fresh install: create the canary
                // table and retry the upsert once.
                if let Err(e) = pool.execute_sql(CANARY_CREATE, vec![]).await {
                    let msg = e.to_string();
                    return if is_wedged(&msg) {
                        Probe::Wedged(format!("canary create: {msg}"))
                    } else {
                        Probe::Other
                    };
                }
                return match pool.execute_sql(upsert, vec![]).await {
                    Ok(_) => Probe::Healthy,
                    Err(e) => {
                        let msg = e.to_string();
                        if is_wedged(&msg) {
                            Probe::Wedged(format!("write canary: {msg}"))
                        } else {
                            Probe::Other
                        }
                    }
                };
            }
            Probe::Other
        }
    }
}

/// Identity (device, inode, size) of the `-wal` file belonging to `db_path`,
/// or `None` when the file does not exist (no writes yet) or cannot be
/// statted.
#[cfg(unix)]
fn wal_identity(db_path: &str) -> Option<(u64, u64, u64)> {
    use std::os::unix::fs::MetadataExt;
    let meta = std::fs::metadata(format!("{db_path}-wal")).ok()?;
    Some((meta.dev(), meta.ino(), meta.len()))
}

#[cfg(not(unix))]
fn wal_identity(_db_path: &str) -> Option<(u64, u64, u64)> {
    None
}

/// Keep a pool alive forever so its destructor never runs a close-time WAL
/// checkpoint (see module docs). Used when the wedged pool must not be
/// closed because no backup of the main DB could be taken. Costs a few file
/// descriptors per incident; reclaimed on process exit.
fn retire_pool(pool: orm::DatabasePool) {
    static RETIRED: OnceLock<std::sync::Mutex<Vec<orm::DatabasePool>>> = OnceLock::new();
    let retired = RETIRED.get_or_init(|| std::sync::Mutex::new(Vec::new()));
    retired.lock().expect("retired pools lock poisoned").push(pool);
}

/// Move a stale `-shm`/`-wal` sidecar out of the way so a reconnect can
/// rebuild it from scratch. Rename, not delete, so nothing is truly lost.
fn quarantine(path: &str) {
    let ts = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0);
    let to = format!("{path}.quarantine-{ts}");
    match std::fs::rename(path, &to) {
        Ok(()) => warn!("quarantined stale {path} -> {to}"),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
        Err(e) => warn!("failed to quarantine {path}: {e}"),
    }
}

/// `DashMap::remove` with a timeout, built on the non-blocking `try_entry`
/// (DashMap 5.5 has no `try_remove`). Ok(Some(pool)) = removed, Ok(None) =
/// the site had no pool, Err(()) = the shard stayed locked for
/// MAP_OP_TIMEOUT (a guard is parked somewhere; retry on the next probe).
async fn remove_with_timeout(
    pools: &dashmap::DashMap<String, orm::DatabasePool>,
    site_name: &str,
) -> Result<Option<orm::DatabasePool>, ()> {
    let start = Instant::now();
    loop {
        match pools.try_entry(site_name.to_string()) {
            Some(dashmap::mapref::entry::Entry::Occupied(e)) => return Ok(Some(e.remove())),
            Some(dashmap::mapref::entry::Entry::Vacant(_)) => return Ok(None),
            None => {
                if start.elapsed() >= MAP_OP_TIMEOUT {
                    error!(
                        "pool map shard for site {} stayed locked for {:?}; aborting heal, will retry",
                        site_name, MAP_OP_TIMEOUT
                    );
                    return Err(());
                }
                tokio::time::sleep(MAP_OP_RETRY).await;
            }
        }
    }
}

/// `DashMap::insert` with a timeout; on Err the pool is handed back to the
/// caller.
async fn insert_with_timeout(
    pools: &dashmap::DashMap<String, orm::DatabasePool>,
    site_name: &str,
    pool: orm::DatabasePool,
) -> Result<(), orm::DatabasePool> {
    let start = Instant::now();
    let mut pool = pool;
    loop {
        match pools.try_entry(site_name.to_string()) {
            Some(mut e) => {
                e.insert(pool);
                return Ok(());
            }
            None => {
                if start.elapsed() >= MAP_OP_TIMEOUT {
                    error!(
                        "pool map shard for site {} stayed locked for {:?}; fresh pool not published, will retry",
                        site_name, MAP_OP_TIMEOUT
                    );
                    return Err(pool);
                }
                tokio::time::sleep(MAP_OP_RETRY).await;
            }
        }
    }
}

/// The heal described in the module docs. Returns true when a fresh pool is
/// in place.
async fn heal(
    pools: &Arc<dashmap::DashMap<String, orm::DatabasePool>>,
    site_name: &str,
    driver: &str,
    db_url: &str,
    reason: &str,
) -> bool {
    warn!("healing database pool for site {}: {}", site_name, reason);

    // 1. Stop all traffic through the wedged pool: commits into its
    //    split-brain view are what eventually poisons the main DB via
    //    auto-checkpoints. The remove is bounded: a shard guard parked
    //    across an `.await` elsewhere once blocked a plain `pools.remove`
    //    here forever and silently killed the watchdog.
    let old_map_pool = match remove_with_timeout(pools, site_name).await {
        Ok(p) => p,
        Err(()) => return false,
    };
    let old_bridge_pool = kiff_core::clear_pool();

    // Postgres has no WAL mechanics; a plain swap is enough.
    if driver == "postgres" {
        if let Some(p) = &old_map_pool {
            let _ = tokio::time::timeout(CLOSE_TIMEOUT, p.close()).await;
        }
        drop(old_bridge_pool);
        return match orm::DatabasePool::connect_postgres(db_url).await {
            Ok(new_pool) => {
                let _ = kiff_core::swap_pool(new_pool.clone());
                if insert_with_timeout(pools, site_name, new_pool).await.is_err() {
                    // The bridge already serves the fresh pool; the next
                    // probe sees the missing map entry and re-heals.
                    return false;
                }
                info!("swapped in a fresh database pool for site {}", site_name);
                true
            }
            Err(e) => {
                error!("failed to rebuild database pool for site {}: {}", site_name, e);
                false
            }
        };
    }

    // 2. Preserve the last checkpointed state. The wedged pool's close-time
    //    checkpoint (next step) writes garbage pages into the main DB, so a
    //    byte copy taken now is what we restore afterwards. For a large DB
    //    this copy takes a moment; heals are rare.
    let backup = format!("{db_url}.heal-backup");
    let backed_up = match std::fs::copy(db_url, &backup) {
        Ok(_) => true,
        Err(e) => {
            warn!("could not back up {db_url} before heal: {e}");
            false
        }
    };

    // 3. Release the poisoned per-process state (fcntl locks, stale shm
    //    mappings) by closing the wedged pool. Without a backup this close
    //    would be destructive, so then the pool is retired unclosed instead.
    if backed_up {
        if let Some(p) = &old_map_pool {
            if tokio::time::timeout(CLOSE_TIMEOUT, p.close())
                .await
                .is_err()
            {
                warn!("timed out closing wedged pool for site {}; reconnect may fail", site_name);
            }
        }
    } else if let Some(p) = old_map_pool.clone() {
        retire_pool(p);
    }
    // Same pool as the map handle (or None); already closed/retired above.
    drop(old_bridge_pool);

    // 4. Clean slate: restore the good main DB and move the sidecars aside.
    //    Everything valid in the `-wal` was already checkpointed into the
    //    main DB by the external writer's own close, so the backup carries
    //    it; the remaining sidecar contents are holes and garbage.
    if backed_up {
        if let Err(e) = std::fs::copy(&backup, db_url) {
            error!("failed to restore {backup} over {db_url}: {e}");
        }
    }
    quarantine(&format!("{db_url}-shm"));
    quarantine(&format!("{db_url}-wal"));

    // 5. Reconnect.
    match orm::DatabasePool::connect_sqlite(db_url).await {
        Ok(new_pool) => {
            let _ = kiff_core::swap_pool(new_pool.clone());
            if insert_with_timeout(pools, site_name, new_pool).await.is_err() {
                // The bridge already serves the fresh pool; the next probe
                // sees the missing map entry and re-heals.
                return false;
            }
            let _ = std::fs::remove_file(&backup);
            info!(
                "swapped in a fresh database pool for site {} after WAL wedge",
                site_name
            );
            true
        }
        Err(e) => {
            error!("failed to rebuild database pool for site {}: {}", site_name, e);
            false
        }
    }
}

pub fn spawn(
    pools: Arc<dashmap::DashMap<String, orm::DatabasePool>>,
    site_manager: Arc<config::SiteManager>,
) {
    tokio::spawn(async move {
        info!("database pool watchdog started (probe every {:?})", PROBE_INTERVAL);
        let mut last_attempt: HashMap<String, Instant> = HashMap::new();
        // Last seen WAL identity per site: (device, inode, size). Cleared
        // after each successful heal so the fresh pool's WAL is recorded
        // instead of flagged as "changed".
        let mut wal_ids: HashMap<String, (u64, u64, u64)> = HashMap::new();
        let mut ticker = tokio::time::interval(PROBE_INTERVAL);
        loop {
            ticker.tick().await;

            let site_names: Vec<String> = site_manager.sites().keys().cloned().collect();
            for site_name in site_names {
                let Some(site) = site_manager.sites().get(&site_name) else {
                    continue;
                };
                let driver = site.config.db_driver.clone();
                let is_sqlite = driver != "postgres";
                let db_url = site.db_url();

                // Non-blocking read: a shard guard parked across a wedged
                // `.await` in a handler must not stall the watchdog loop;
                // skip this site for this round instead.
                let pool = match pools.try_get(&site_name) {
                    dashmap::try_result::TryResult::Present(r) => Some(r.clone()),
                    dashmap::try_result::TryResult::Absent => None,
                    dashmap::try_result::TryResult::Locked => continue,
                };
                let reason: Option<String> = match &pool {
                    // Missing after a failed heal (or a failed startup
                    // connect): go straight to healing.
                    None => Some("pool missing after failed heal".to_string()),
                    Some(pool) => {
                        let mut reason: Option<String> = None;

                        // Detector 1 (sqlite only): the WAL watcher. The pool
                        // holds the WAL open continuously, so the file may
                        // only grow and must keep its inode; a shrink, a
                        // different inode, or a missing file means an
                        // external process interfered.
                        if is_sqlite {
                            let current = wal_identity(&db_url);
                            match (wal_ids.get(&site_name), current) {
                                (None, Some(c)) => {
                                    wal_ids.insert(site_name.clone(), c);
                                }
                                (Some(&r), Some(c)) => {
                                    if r.0 != c.0 || r.1 != c.1 {
                                        reason = Some(format!(
                                            "WAL file was replaced externally (inode {} -> {})",
                                            r.1, c.1
                                        ));
                                    } else if c.2 < r.2 {
                                        reason = Some(format!(
                                            "WAL file was truncated externally (size {} -> {})",
                                            r.2, c.2
                                        ));
                                    } else {
                                        wal_ids.insert(site_name.clone(), c);
                                    }
                                }
                                (Some(&r), None) => {
                                    reason = Some(format!(
                                        "WAL file was deleted externally (inode {} gone)",
                                        r.1
                                    ));
                                }
                                (None, None) => {}
                            }
                        }

                        // Detector 2: SQL canaries on the read and write path.
                        if reason.is_none() {
                            // Bound the probe: a wedged connection may stall
                            // inside SQLite instead of erroring, and an
                            // unbounded await would kill the watchdog loop.
                            let probe =
                                tokio::time::timeout(Duration::from_secs(5), probe_pool(pool))
                                    .await;
                            reason = match probe {
                                Ok(Probe::Healthy) | Ok(Probe::Other) => None,
                                Ok(Probe::Wedged(msg)) => Some(msg),
                                Err(_elapsed) => Some("probe timed out".to_string()),
                            };
                        }
                        reason
                    }
                };

                let Some(reason) = reason else { continue };

                let recently_attempted = last_attempt
                    .get(&site_name)
                    .map(|t| t.elapsed() < MIN_HEAL_INTERVAL)
                    .unwrap_or(false);
                if recently_attempted {
                    continue;
                }
                last_attempt.insert(site_name.clone(), Instant::now());

                if heal(&pools, &site_name, &driver, &db_url, &reason).await {
                    wal_ids.remove(&site_name);
                    last_attempt.remove(&site_name);
                }
            }
        }
    });
}

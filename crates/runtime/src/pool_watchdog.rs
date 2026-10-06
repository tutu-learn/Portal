//! Self-healing for wedged SQLite pools.
//!
//! When an external process (the `sqlite3` CLI, one-off fix scripts, a backup
//! restore) writes to a live SQLite file while the server is running, it
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
//!
//! With domain sharding, each physical SQLite file has its own WAL and is
//! healed independently. The watchdog probes every domain for every site and
//! swaps only the affected domain's pool, so a wedged `k8s.db` does not take
//! down reads against `site.db`.

use std::collections::HashMap;
use std::sync::{Arc, OnceLock};
use std::time::{Duration, Instant};

use orm::DbDomain;

use pyo3::types::PyAnyMethods;
use tracing::{error, info, warn};

const PROBE_INTERVAL: Duration = Duration::from_secs(10);
/// Do not attempt to heal a site more often than this; if the file is
/// genuinely corrupt (not just a stale view) repeated heals only add churn.
const MIN_HEAL_INTERVAL: Duration = Duration::from_secs(30);
/// Bound the wedged pool's close so a leaked connection cannot hang the
/// watchdog forever.
const CLOSE_TIMEOUT: Duration = Duration::from_secs(10);

/// A read against a real table. `SELECT 1` parses without reading any page,
/// so it cannot detect a stale WAL view; this one must hit the database.
const CANARY_SQL: &str = r#"SELECT id FROM "__kiff_queue" LIMIT 1"#;
/// One-row upsert exercising the write path. Reads can keep succeeding on a
/// stale WAL view long after an external write, but a wedged write path
/// fails on the spot — and the server's own writes in that window are what
/// turn a recoverable wedge into real on-disk corruption.
const CANARY_UPSERT: &str = r#"INSERT INTO "__kiff_pool_canary" (id, touched_at) VALUES (1, datetime('now')) ON CONFLICT(id) DO UPDATE SET touched_at = excluded.touched_at"#;
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

    match pool.execute_sql(CANARY_UPSERT, vec![]).await {
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
                return match pool.execute_sql(CANARY_UPSERT, vec![]).await {
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
    retired
        .lock()
        .expect("retired pools lock poisoned")
        .push(pool);
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

/// The embedded Python loads `kiff_core` as a separate cdylib with its own
/// POOL static (`init_from_url`), so swapping the binary's bridge pool is
/// not enough: the .so instance keeps file handles to the quarantined WAL
/// and stays wedged across heals, re-triggering the heal cycle after every
/// heal. Ask that instance — over the Python boundary, the only way to
/// reach its statics — to retire and reconnect its pool. Best-effort: if
/// the .so was never loaded, the binary bridge is still healed.
async fn reset_embedded_python_pool(db_url: &str) {
    let db_url = db_url.to_string();
    // spawn_blocking: `reset_pool_from_url` uses `Runtime::block_on`, which
    // panics when called from an async worker thread.
    let result = tokio::task::spawn_blocking(move || {
        pyo3::Python::with_gil(|py| -> Result<(), pyo3::PyErr> {
            let kc = py.import("kiff_core")?;
            kc.call_method1("reset_pool_from_url", ("sqlite", db_url))?;
            Ok(())
        })
    })
    .await;
    match result {
        Ok(Ok(())) => info!("reset embedded Python kiff_core pool"),
        Ok(Err(e)) => warn!("failed to reset embedded Python kiff_core pool: {e}"),
        Err(e) => warn!("embedded Python pool reset task failed: {e}"),
    }
}

/// Heal a single wedged domain pool for a site. Returns true when a fresh
/// pool for that domain is in place.
///
/// The `DomainPools` bundle stays in the shared map; only the affected domain
/// is swapped, so traffic against other domains is not interrupted.
async fn heal_domain(
    pools: &Arc<dashmap::DashMap<String, Arc<orm::DomainPools>>>,
    site_name: &str,
    site_path: &std::path::Path,
    domain: orm::DbDomain,
    reason: &str,
) -> bool {
    warn!(
        "healing {:?} database pool for site {}: {}",
        domain, site_name, reason
    );

    let Some(domain_pools) = pools.get(site_name).map(|e| e.clone()) else {
        // Bundle missing entirely (failed startup). Only core can be rebuilt
        // from a plain db_url; other domains have no bundle to swap into.
        if domain == orm::DbDomain::Core {
            let db_url = domain.file_path(site_path).to_string_lossy().to_string();
            return heal_missing_core_bundle(pools, site_name, &db_url, reason).await;
        }
        return false;
    };

    let db_path = domain.file_path(site_path);
    let db_url = db_path.to_string_lossy().to_string();
    let old_pool = domain_pools.pool_for(domain);

    // For the core domain, also refresh the Python bridge pool so the embedded
    // .so does not keep file handles to the quarantined WAL.
    let old_bridge_pool = if domain == orm::DbDomain::Core {
        kiff_core::clear_pool()
    } else {
        None
    };

    // Preserve the last checkpointed state before closing the wedged pool.
    let backup = format!("{db_url}.heal-backup");
    let backed_up = match std::fs::copy(&db_path, &backup) {
        Ok(_) => true,
        Err(e) => {
            warn!("could not back up {db_url} before heal: {e}");
            false
        }
    };

    if backed_up {
        if tokio::time::timeout(CLOSE_TIMEOUT, old_pool.close())
            .await
            .is_err()
        {
            warn!(
                "timed out closing wedged {:?} pool for site {}; reconnect may fail",
                domain, site_name
            );
        }
    } else {
        retire_pool(old_pool);
    }
    drop(old_bridge_pool);

    if backed_up {
        if let Err(e) = std::fs::copy(&backup, &db_path) {
            error!("failed to restore {backup} over {db_url}: {e}");
        }
    }
    quarantine(&format!("{db_url}-shm"));
    quarantine(&format!("{db_url}-wal"));

    match orm::DatabasePool::connect_sqlite_domain(site_path, domain).await {
        Ok(new_pool) => {
            domain_pools.swap_pool(domain, new_pool);
            if domain == orm::DbDomain::Core {
                let _ = kiff_core::swap_pool(domain_pools.core());
            }
            let _ = std::fs::remove_file(&backup);
            info!(
                "swapped in a fresh {:?} database pool for site {} after WAL wedge",
                domain, site_name
            );
            if domain == orm::DbDomain::Core {
                reset_embedded_python_pool(&db_url).await;
            }
            true
        }
        Err(e) => {
            error!(
                "failed to rebuild {:?} database pool for site {}: {}",
                domain, site_name, e
            );
            false
        }
    }
}

/// Fallback heal used when the whole `DomainPools` bundle has been removed
/// from the map (e.g. after a failed startup connect). Rebuilds a fresh
/// single-pool bundle for the core domain.
async fn heal_missing_core_bundle(
    pools: &Arc<dashmap::DashMap<String, Arc<orm::DomainPools>>>,
    site_name: &str,
    db_url: &str,
    reason: &str,
) -> bool {
    warn!(
        "healing missing core database pool bundle for site {}: {}",
        site_name, reason
    );

    let old_bridge_pool = kiff_core::clear_pool();

    let backup = format!("{db_url}.heal-backup");
    let backed_up = match std::fs::copy(db_url, &backup) {
        Ok(_) => true,
        Err(e) => {
            warn!("could not back up {db_url} before heal: {e}");
            false
        }
    };

    drop(old_bridge_pool);

    if backed_up {
        quarantine(&format!("{db_url}-shm"));
        quarantine(&format!("{db_url}-wal"));
    }

    match orm::DatabasePool::connect_sqlite(db_url).await {
        Ok(new_pool) => {
            pools.insert(
                site_name.to_string(),
                Arc::new(orm::DomainPools::from_core_pool(new_pool.clone())),
            );
            let _ = kiff_core::swap_pool(new_pool);
            let _ = std::fs::remove_file(&backup);
            info!(
                "rebuilt missing core database pool bundle for site {}",
                site_name
            );
            reset_embedded_python_pool(db_url).await;
            true
        }
        Err(e) => {
            error!(
                "failed to rebuild missing core database pool for site {}: {}",
                site_name, e
            );
            false
        }
    }
}

pub fn spawn(
    pools: Arc<dashmap::DashMap<String, Arc<orm::DomainPools>>>,
    site_manager: Arc<config::SiteManager>,
) {
    if std::env::var("KIFF_DISABLE_POOL_WATCHDOG").is_ok() {
        info!("database pool watchdog disabled by KIFF_DISABLE_POOL_WATCHDOG");
        return;
    }
    tokio::spawn(async move {
        info!(
            "database pool watchdog started (probe every {:?})",
            PROBE_INTERVAL
        );
        // Last seen WAL identity per (site, domain): (device, inode, size).
        let mut wal_ids: HashMap<(String, DbDomain), (u64, u64, u64)> = HashMap::new();
        let mut last_attempt: HashMap<(String, DbDomain), Instant> = HashMap::new();
        let mut ticker = tokio::time::interval(PROBE_INTERVAL);
        loop {
            ticker.tick().await;

            let site_names: Vec<String> = site_manager.sites().keys().cloned().collect();
            for site_name in site_names {
                let Some(site) = site_manager.sites().get(&site_name) else {
                    continue;
                };
                let site_path = site.path.clone();

                // Non-blocking read: a shard guard parked across a wedged
                // `.await` in a handler must not stall the watchdog loop;
                // skip this site for this round instead.
                let domain_pools = match pools.try_get(&site_name) {
                    dashmap::try_result::TryResult::Present(r) => Some(r.clone()),
                    dashmap::try_result::TryResult::Absent => None,
                    dashmap::try_result::TryResult::Locked => continue,
                };

                // Missing after a failed startup connect: only core can be
                // rebuilt from a plain path; heal it first and the rest of the
                // domains will be connected on the next startup.
                if domain_pools.is_none() {
                    let key = (site_name.clone(), DbDomain::Core);
                    let recently = last_attempt
                        .get(&key)
                        .map(|t| t.elapsed() < MIN_HEAL_INTERVAL)
                        .unwrap_or(false);
                    if !recently {
                        last_attempt.insert(key, Instant::now());
                        let core_path = DbDomain::Core.file_path(&site_path);
                        let db_url = core_path.to_string_lossy().to_string();
                        if heal_missing_core_bundle(&pools, &site_name, &db_url, "pool missing after failed startup").await {
                            wal_ids.retain(|(s, _), _| s != &site_name);
                            last_attempt.retain(|(s, _), _| s != &site_name);
                        }
                    }
                    continue;
                }
                let domain_pools = domain_pools.unwrap();

                for &domain in DbDomain::all() {
                    let key = (site_name.clone(), domain);
                    let db_path = domain.file_path(&site_path);
                    let db_url = db_path.to_string_lossy().to_string();
                    let pool = domain_pools.pool_for(domain);
                    let mut reason: Option<String> = None;

                    // Detector 1: WAL watcher.
                    let current = wal_identity(&db_url);
                    match (wal_ids.get(&key), current) {
                        (None, Some(c)) => {
                            wal_ids.insert(key.clone(), c);
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
                                wal_ids.insert(key.clone(), c);
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

                    // Detector 2: SQL canaries.
                    if reason.is_none() {
                        let probe =
                            tokio::time::timeout(Duration::from_secs(5), probe_pool(&pool))
                                .await;
                        reason = match probe {
                            Ok(Probe::Healthy) | Ok(Probe::Other) => None,
                            Ok(Probe::Wedged(msg)) => Some(msg),
                            Err(_elapsed) => Some("probe timed out".to_string()),
                        };
                    }

                    let Some(reason) = reason else { continue };

                    let recently_attempted = last_attempt
                        .get(&key)
                        .map(|t| t.elapsed() < MIN_HEAL_INTERVAL)
                        .unwrap_or(false);
                    if recently_attempted {
                        continue;
                    }
                    last_attempt.insert(key.clone(), Instant::now());

                    if heal_domain(&pools, &site_name, &site_path, domain, &reason).await {
                        wal_ids.remove(&key);
                        last_attempt.remove(&key);
                    }
                }
            }
        }
    });
}

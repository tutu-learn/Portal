use crate::domain::{DbDomain, sharding_enabled};
use crate::pool::DatabasePool;
use error::Result;
use std::path::Path;
use std::sync::{Arc, RwLock};

/// A bundle of SQLite pools, one per logical domain.
///
/// When sharding is disabled, `k8s`, `telemetry`, and `state` alias the core
/// pool, so code that requests any domain still works against the single
/// `site.db`. Each field is behind an `RwLock` so the runtime watchdog can
/// swap a wedged domain pool without disturbing the others.
#[derive(Debug, Clone)]
pub struct DomainPools {
    core: Arc<RwLock<sqlx::SqlitePool>>,
    k8s: Arc<RwLock<sqlx::SqlitePool>>,
    telemetry: Arc<RwLock<sqlx::SqlitePool>>,
    state: Arc<RwLock<sqlx::SqlitePool>>,
}

impl DomainPools {
    /// Build a DomainPools bundle where every domain aliases a single core
    /// pool. Useful for tests and for the Python bridge fallback.
    pub fn from_core_pool(pool: DatabasePool) -> Self {
        Self::from_core_pool_inner(pool.into_inner())
    }

    pub(crate) fn from_core_pool_inner(pool: sqlx::SqlitePool) -> Self {
        let shared = Arc::new(RwLock::new(pool));
        Self {
            core: shared.clone(),
            k8s: shared.clone(),
            telemetry: shared.clone(),
            state: shared,
        }
    }

    /// Connect all domain pools for a site.
    pub async fn connect(site_path: &Path) -> Result<Self> {
        let core = DatabasePool::connect_sqlite_domain(site_path, DbDomain::Core).await?;

        if !sharding_enabled() {
            // All domains alias the core pool so callers can ask for any domain
            // and transparently hit `site.db`.
            let shared = Arc::new(RwLock::new(core));
            return Ok(Self {
                core: shared.clone(),
                k8s: shared.clone(),
                telemetry: shared.clone(),
                state: shared,
            });
        }

        let k8s = DatabasePool::connect_sqlite_domain(site_path, DbDomain::K8s).await?;
        let telemetry = DatabasePool::connect_sqlite_domain(site_path, DbDomain::Telemetry).await?;
        let state = DatabasePool::connect_sqlite_domain(site_path, DbDomain::State).await?;

        Ok(Self {
            core: Arc::new(RwLock::new(core)),
            k8s: Arc::new(RwLock::new(k8s)),
            telemetry: Arc::new(RwLock::new(telemetry)),
            state: Arc::new(RwLock::new(state)),
        })
    }

    /// Returns a handle to the pool for a given domain. The handle routes
    /// doctype-based operations to the requested domain and stays valid after
    /// a watchdog swap because it resolves the actual `SqlitePool` lazily.
    pub fn pool_for(self: &Arc<Self>, domain: DbDomain) -> DatabasePool {
        DatabasePool::new(Arc::clone(self), domain)
    }

    /// Shorthand for a handle to the core (metadata) pool.
    pub fn core(self: &Arc<Self>) -> DatabasePool {
        self.pool_for(DbDomain::Core)
    }

    /// Returns the underlying `SqlitePool` currently installed for a domain.
    /// Most callers should use `pool_for` so they pick up watchdog swaps.
    pub(crate) fn raw_pool_for(&self, domain: DbDomain) -> sqlx::SqlitePool {
        match domain {
            DbDomain::Core => self.core.read().unwrap().clone(),
            DbDomain::K8s => self.k8s.read().unwrap().clone(),
            DbDomain::Telemetry => self.telemetry.read().unwrap().clone(),
            DbDomain::State => self.state.read().unwrap().clone(),
        }
    }

    /// Swap a single domain's underlying pool. Used by the watchdog after
    /// healing a wedged domain.
    pub fn swap_pool(&self, domain: DbDomain, new_pool: sqlx::SqlitePool) {
        match domain {
            DbDomain::Core => *self.core.write().unwrap() = new_pool,
            DbDomain::K8s => *self.k8s.write().unwrap() = new_pool,
            DbDomain::Telemetry => *self.telemetry.write().unwrap() = new_pool,
            DbDomain::State => *self.state.write().unwrap() = new_pool,
        }
    }

    /// Close every pool. Used by the watchdog before reconnecting, or during
    /// shutdown.
    pub async fn close(&self) {
        self.core.read().unwrap().close().await;
        if sharding_enabled() {
            self.k8s.read().unwrap().close().await;
            self.telemetry.read().unwrap().close().await;
            self.state.read().unwrap().close().await;
        }
    }
}

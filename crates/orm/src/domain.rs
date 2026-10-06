use std::path::{Path, PathBuf};

/// Logical database domain. Each domain maps to a separate SQLite file when
/// sharding is enabled; otherwise all domains alias the core `site.db`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum DbDomain {
    Core,
    K8s,
    Telemetry,
    State,
}

impl DbDomain {
    /// Returns the file name for this domain inside a site directory.
    pub fn file_name(&self) -> &'static str {
        match self {
            DbDomain::Core => "site.db",
            DbDomain::K8s => "k8s.db",
            DbDomain::Telemetry => "telemetry.db",
            DbDomain::State => "state.db",
        }
    }

    /// Returns the full path for this domain's SQLite file.
    pub fn file_path(&self, site_path: &Path) -> PathBuf {
        site_path.join(self.file_name())
    }

    /// Determine the domain for a physical table name.
    ///
    /// This is the single place in code that decides which table lives in
    /// which SQLite file when sharding is enabled.
    pub fn for_table(name: &str) -> Self {
        let lower = name.to_lowercase();
        if lower.starts_with("k8s_")
            || lower == "kubernetes_cluster"
            || lower.starts_with("kubernetes_control_plane_node")
            || lower.starts_with("kubernetes_worker_node")
        {
            DbDomain::K8s
        } else if lower.starts_with("audit_ready_dns_")
            || lower.starts_with("infrastructure_server")
            || lower == "client_machine"
        {
            DbDomain::Telemetry
        } else if lower.starts_with("patch_job") || lower.starts_with("agent_state") {
            DbDomain::State
        } else {
            DbDomain::Core
        }
    }

    /// All domains in their default order.
    pub fn all() -> &'static [DbDomain] {
        &[DbDomain::Core, DbDomain::K8s, DbDomain::Telemetry, DbDomain::State]
    }
}

/// Returns true when per-domain SQLite sharding is enabled.
///
/// Controlled by the `KIFF_SQLITE_SHARDED` environment variable. When disabled
/// (the default), all domains share the core `site.db` and the system behaves
/// like the original single-database architecture.
pub fn sharding_enabled() -> bool {
    std::env::var("KIFF_SQLITE_SHARDED")
        .ok()
        .map(|s| s == "1" || s.eq_ignore_ascii_case("true"))
        .unwrap_or(false)
}

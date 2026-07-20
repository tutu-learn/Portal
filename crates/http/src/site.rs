//! Site resolution helpers for HTTP requests.
//!
//! These helpers map incoming requests to a Frappe site and its cached database
//! pool. Pools are created once at runtime startup; the HTTP layer only looks
//! them up by site name.

use axum::http::HeaderMap;
use config::site::Site;
use rust_apps_core::AppState;

/// Resolve the site name for a request.
///
/// Priority:
/// 1. `X-Frappe-Site-Name` header
/// 2. `Host` header matched against a site's configured `host_name`
/// 3. The only configured site, if exactly one exists
pub fn resolve_site_name(state: &AppState, headers: &HeaderMap) -> Option<String> {
    if let Some(name) = headers
        .get("x-frappe-site-name")
        .and_then(|h| h.to_str().ok())
    {
        return Some(name.to_string());
    }

    if let Some(host) = headers.get("host").and_then(|h| h.to_str().ok()) {
        for site in state.site_manager.sites().values() {
            if site
                .config
                .host_name
                .as_deref()
                .map(|h| h.eq_ignore_ascii_case(host))
                .unwrap_or(false)
            {
                return Some(site.name.clone());
            }
        }
    }

    let sites = state.site_manager.sites();
    if sites.len() == 1 {
        return sites.values().next().map(|s| s.name.clone());
    }

    None
}

/// Resolve the site and its cached database pool for a request.
///
/// Returns `None` when no site can be resolved or no pool has been connected
/// for it (pools are established during runtime startup).
pub fn resolve_site_pool(
    state: &AppState,
    headers: &HeaderMap,
) -> Option<(Site, orm::DatabasePool)> {
    let name = resolve_site_name(state, headers)?;
    let site = state.site_manager.get(&name)?.clone();
    let pool = state.pools.get(&name)?.clone();
    Some((site, pool))
}

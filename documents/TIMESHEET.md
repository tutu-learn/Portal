# Timesheet & Overtime Report — RustRoverProjects

Generated 2026-08-17 from git commit history, all repos authored by johannnefdt.

**Scope:** SebrusApps (includes identical repo copies AuditReadyComplicnce, Portal, logger — counted once), AuditReady, Sebrus Compliance System. `Wenkem App` and `logs` contain no git history.

**Method:** commits within 2 h form a session; each commit is credited the minutes until the next commit, the last in a session gets 30 min. Hours are an estimate from commit timestamps, not clocked time.

**Overtime definition:** ordinary hours = Mon–Fri 08:00–17:00. Work committed after 17:00, before 08:00, on weekends, or on SA public holidays (Youth Day 16 Jun 2026) counts as overtime.

---

## Headline figures

- Period: **2026-05-31 → 2026-08-13** (10.6 weeks)
- Commits: **176** across 37 active days
- Total estimated time: **~98.3 h**
- Ordinary hours: **~51.0 h** (52%)
- **Overtime: ~47.4 h (48% of all work)**
  - Weekday evenings (after 17:00): ~24.4 h
  - Weekends / public holidays: ~23.0 h
  - Early mornings (before 08:00): ~0.0 h

## Per project

| Project | Ordinary h | Overtime h | Total h | Commits |
|---|---|---|---|---|
| SebrusApps | 37.9 | 38.4 | 76.2 | 122 |
| AuditReady | 10.1 | 6.0 | 16.1 | 39 |
| Sebrus Compliance System | 3.0 | 3.0 | 6.0 | 15 |
| **Total** | **51.0** | **47.4** | **98.3** | **176** |

## Per month

| Month | Ordinary h | Overtime h | Total h | OT % |
|---|---|---|---|---|
| 2026-05 | 0.0 | 0.5 | 0.5 | 100% |
| 2026-06 | 17.3 | 17.2 | 34.5 | 50% |
| 2026-07 | 29.5 | 27.6 | 57.2 | 48% |
| 2026-08 | 4.1 | 2.0 | 6.1 | 33% |

## Per week

| Week | Ordinary h | Overtime h | Total h |
|---|---|---|---|
| 2026-05 (wk 22) | 0.0 | 0.5 | 0.5 |
| 2026-06 (wk 24) | 0.0 | 0.5 | 0.5 |
| 2026-06 (wk 25) | 4.6 | 11.6 | 16.1 |
| 2026-06 (wk 26) | 12.8 | 5.1 | 17.9 |
| 2026-07 (wk 28) | 2.0 | 13.9 | 15.9 |
| 2026-07 (wk 29) | 18.1 | 10.4 | 28.5 |
| 2026-07 (wk 30) | 6.9 | 2.9 | 9.8 |
| 2026-07 (wk 31) | 2.5 | 0.5 | 3.0 |
| 2026-08 (wk 31) | 0.0 | 0.5 | 0.5 |
| 2026-08 (wk 32) | 3.6 | 1.0 | 4.6 |
| 2026-08 (wk 33) | 0.5 | 0.5 | 1.0 |

## Daily log

### Sun 2026-05-31 — AuditReady (~0.5 h; std 0.0 / OT 0.5)

- 13:58 `2ea9a75` Initial commit *[infra/config]* [OT]

### Fri 2026-06-12 — SebrusApps (~0.5 h; std 0.0 / OT 0.5)

- 20:25 `d9937b8` Initial implementation of CLI commands (`init`, `start`, `new_site`, `backup`, `shell`, and `migrate`) for managing the Kiff runtime and Frappe sites. Added IntelliJ project configurations. *[cli, crates/config, crates/error, crates/http, crates/metadata, crates/orm, crates/permissions, crates/python-bridge, crates/queue, crates/runtime, crates/session, crates/sql-translator, docs, frappe apps/config, infra/config, scripts, src]* [OT]

### Tue 2026-06-16 — Youth Day — SebrusApps (~0.5 h; std 0.0 / OT 0.5)

- 21:32 `85fdc7f` Remove deprecated Frappe model shim and unused JavaScript bundle files. *[cli, crates/http, crates/orm, crates/python-bridge, frappe apps/config, infra/config]* [OT]

### Thu 2026-06-18 — SebrusApps (~0.5 h; std 0.5 / OT 0.0)

- 15:52 `cbc02b2` Introduce `rust_apps` integration and extend Kiff runtime with hooks, permissions, and sample app support. *[cli, crates/http, crates/orm, crates/permissions, crates/runtime, docs, frappe apps/config, infra/config, rust_apps/core, rust_apps/sample, src]*

### Fri 2026-06-19 — Sebrus Compliance System (~1.5 h; std 0.5 / OT 1.0)

- 16:45 `34e0319` Add Dockerized Sebrus Portal runtime with Azure pipelines *[docs, infra/config]*
- 16:51 `6343025` Update Azure pipeline configuration to include build context and custom checkout path *[infra/config]*
- 17:13 `c41dadb` Update Azure pipeline configuration to include build context and custom checkout path *[docs, infra/config]* [OT]
- 23:26 `f964a98` Update Dockerfile for Kiff runtime to use PyO3 0.25 and adjust Python compatibility settings *[docs, infra/config]* [OT]

### Fri 2026-06-19 — SebrusApps (~6.2 h; std 3.6 / OT 2.7)

- 13:29 `0d7e246` Add 20 new ISO27001-compliant DocTypes for "audit_ready" module *[cli, crates/http, crates/orm, crates/runtime, docs, infra/config, rust_apps/core, rust_apps/sample]*
- 14:53 `1e8a5d5` Add SQLite metadata support and default site initialization for the "audit_ready" module. *[crates/config, crates/http, crates/orm, crates/runtime, frappe apps/config, infra/config, rust_apps (root)]*
- 16:00 `2d54030` Refactor workspace boot logic and standardize naming. *[crates/http]*
- 16:34 `eacb332` Upgrade PyO3 to 0.25 and refactor Python bindings for improved API compatibility and reliability. *[crates/python-bridge, crates/runtime, infra/config]*
- 20:49 `e8cfcd9` Enhance messaging, authentication, and UI components for improved compatibility and functionality: *[crates/http, crates/orm, crates/session, frappe apps/config]* [OT]
- 22:36 `4ddf339` Add social login support with OAuth2 integration and UI enhancements *[crates/http]* [OT]
- 22:58 `215f0bc` Serve node modules under `/assets/frappe/node_modules` in HTTP router. *[crates/http]* [OT]

### Sat 2026-06-20 — Sebrus Compliance System (~1.2 h; std 0.0 / OT 1.2)

- 00:20 `234a7e0` Update Dockerfile for Kiff runtime to use PyO3 0.25 and adjust Python compatibility settings *[docs, infra/config]* [OT]
- 21:32 `c049cbe` Add node_modules to runtime image for lazy-loaded desk control assets *[docs, infra/config]* [OT]
- 21:45 `53c47a8` Update Dockerfile to install Git for resolving Frappe dependencies from git+https URLs *[infra/config]* [OT]

### Sat 2026-06-20 — SebrusApps (~1.5 h; std 0.0 / OT 1.5)

- 00:01 `6c0df34` Sync workspace child tables and patch Frappe Workspace getters to handle None as empty lists for fixture compatibility. *[crates/orm, frappe apps/config]* [OT]
- 22:47 `2f531cb` Patch OAuth token exchange logic to include scope for Microsoft Entra ID compatibility. *[frappe apps/config]* [OT]
- 23:14 `a0e81a0` Default scope to "openid email profile" for Microsoft Entra ID in OAuth2 authorization URL and add unit test for verification. *[crates/http]* [OT]

### Sun 2026-06-21 — SebrusApps (~4.7 h; std 0.0 / OT 4.7)

- 13:27 `c5e733c` Add base64 and rand dependencies; implement Fernet key generation and validation *[crates/config, infra/config]* [OT]
- 15:39 `95eb237` Add Python-based social login key updater, enhance document deletion, and patch OAuth/password handling. *[crates/http, frappe apps/config, scripts]* [OT]
- 20:10 `6ab2064` Add missing `frappe` imports to resolve undefined reference issues in social login and password removal methods *[frappe apps/config]* [OT]
- 21:04 `a2ba4be` Patch `load_doc_before_save` to handle `None` child tables as empty lists in SQLite runtime. *[frappe apps/config]* [OT]
- 21:24 `2e54eff` Resolve and encrypt Social Login Key names dynamically, add support for targeting specific keys via CLI arguments. *[scripts]* [OT]
- 22:07 `f23eadd` Add support for overriding `encryption_key` via `FRAPPE_ENCRYPTION_KEY` and persist it in `site_config.json` *[crates/config, frappe apps/config, scripts]* [OT]
- 22:45 `35d2cdd` Patch `load_doc_before_save` to initialize `_doc_before_save` as `None` for consistency in new documents. *[frappe apps/config]* [OT]
- 23:24 `031b83d` Patch `load_doc_before_save` to initialize `_doc_before_save` as `None` for consistency in new documents. *[scripts]* [OT]

### Mon 2026-06-22 — SebrusApps (~4.2 h; std 3.2 / OT 1.1)

- 13:01 `2d8a3bf` Add session and login manager stubs, enhance context initialization with cookie and hash utilities. *[frappe apps/config]*
- 14:17 `2eef640` Introduce support for HTTP redirects in method responses and add session cookie handling for Python login flows. *[crates/http, crates/permissions, crates/python-bridge, frappe apps/config, infra/config]*
- 15:41 `202cff5` Remove `frappe` prefix from `get_doc` and `DoesNotExistError` to fix redundant references in OAuth user update logic. *[frappe apps/config]*
- 23:18 `34e9646` Remove obsolete kiff runtime log files *[crates/http, crates/log_engine, crates/orm, crates/python-bridge, crates/runtime, docs, e2e tests, frappe apps/config, infra/config, rust_apps/core, scripts]* [OT]
- 23:52 `63819e8` Add `audit_ready` app, update tests and dependencies *[infra/config]* [OT]

### Tue 2026-06-23 — Sebrus Compliance System (~1.0 h; std 1.0 / OT 0.0)

- 10:34 `db92edd` Update Dockerfile to include `kiff_logger` crate in the build context *[infra/config]*
- 11:03 `ca3cc7e` Remove `kiff_logger` crate from Dockerfile build context *[infra/config]*

### Tue 2026-06-23 — SebrusApps (~5.0 h; std 2.2 / OT 2.8)

- 10:52 `1c79aec` Add missing kiff_logger and audit_ready crates *[crates/kiff_logger, rust_apps (root)]*
- 13:14 `3449b5e` Add new helpers for error handling, field validation, and patches *[crates/http, frappe apps/config]*
- 14:13 `112f8d1` Patch `ModuleProfile.update_all_users` to handle SQLite dict row results and synchronize module-profile changes with linked users. *[frappe apps/config]*
- 14:13 `f0d5aee` Patch `ModuleProfile.update_all_users` to handle SQLite dict row results and synchronize module-profile changes with linked users. *[rust_apps (root)]*
- 14:25 `f39e42c` Patch `ModuleProfile.update_all_users` to handle SQLite dict row results and synchronize module-profile changes with linked users. *[infra/config]*
- 17:52 `b0c98bd` Enhance Frappe integration with fallback logic, blocked module filtering, and child table handling. Refactor ORM for module fixtures and Rust-Python compatibility. *[crates/http, crates/orm, crates/runtime, frappe apps/config, infra/config, rust_apps/core]* [OT]
- 18:31 `d6a8db1` Remove obsolete Playwright test files, dependencies, and related artifacts. *[docs, e2e tests, infra/config]* [OT]
- 21:52 `aefb843` Refactor code for better readability and formatting *[cli, crates/config, crates/http, crates/log_engine, crates/metadata, crates/orm, crates/permissions, crates/python-bridge, crates/queue, crates/runtime, crates/session, crates/sql-translator, infra/config, rust_apps (root), rust_apps/core, src]* [OT]
- 23:04 `03c9621` Add support for virtual DocTypes and Kiff Log Entry integration *[crates/http, crates/orm]* [OT]

### Wed 2026-06-24 — Sebrus Compliance System (~1.3 h; std 0.5 / OT 0.7)

- 16:41 `f0fe629` Remove `azure-pipelines.yml` and add `Cargo.lock` for `openfrappe`. *[docs, infra/config]*
- 16:51 `8cbfac9` Remove `azure-pipelines.yml` and add `Cargo.lock` for `openfrappe`. *[infra/config]*
- 17:14 `f9a9667` Patch Dockerfiles to trim unused app dependencies and make builds app-specific *[docs, infra/config]* [OT]
- 17:29 `9ac0fb2` Patch Dockerfiles to trim unused app dependencies and make builds app-specific *[infra/config]* [OT]

### Wed 2026-06-24 — SebrusApps (~2.4 h; std 1.9 / OT 0.5)

- 10:49 `8c0eee2` Add token-based auth middleware and Kiff Logger external ingest *[crates/http, crates/orm, crates/runtime, rust_apps/core]*
- 10:50 `0e30763` Update `call_method` usage in `rust_apps.rs` to include optional parameter *[infra/config]*
- 15:50 `9849af4` Add Sebrus Logger app and dashboard components *[crates/http, crates/log_engine, crates/orm, crates/runtime, infra/config, rust_apps (root)]*
- 17:13 `c64d40d` feat(kiff_logger): add bearer token auth and token management UI *[crates/kiff_logger]* [OT]

### Thu 2026-06-25 — SebrusApps (~4.0 h; std 4.0 / OT 0.0)

- 10:06 `2e59092` add scripts for managing social login keys *[scripts]*
- 11:47 `80c4bb3` Refactor and reformat code for improved readability and consistency *[crates/http]*
- 14:23 `cc5849a` Refactor `getdoctype_native` to leverage in-memory Rust app fixtures, update error handling for missing DocType JSON, and revise `load_doctype_metadata` for improved performance and flexibility. *[crates/http]*
- 14:58 `aa73808` Refactor page loading to support Rust app fixtures. *[crates/http, rust_apps/core]*
- 15:40 `eef000b` Register kiff_logger token UI page as in-memory fixture *[crates/kiff_logger]*

### Tue 2026-07-07 — SebrusApps (~1.0 h; std 0.5 / OT 0.5)

- 12:57 `d6f5427` Introduce metadata support for sessions and align with Frappe compatibility *[crates/http, crates/kiff_logger, crates/orm, crates/permissions, crates/python-bridge, crates/session, frappe apps/config, infra/config]*
- 12:57 `dcc548d` Add tests for desk boot info validation *[infra/config]*
- 22:15 `652cd93` Refactor and enhance core functionalities *[cli, crates/http, crates/orm, crates/permissions, crates/python-bridge, crates/runtime, frappe apps/config, infra/config, rust_apps (root), scripts]* [OT]

### Wed 2026-07-08 — SebrusApps (~1.0 h; std 0.5 / OT 0.5)

- 15:11 `1512bd3` Remove IntelliJ IDEA project configuration files *[crates/http, crates/python-bridge, docs, infra/config]*
- 19:38 `c8e103c` Sync apps.json with Cargo.toml manifests *[crates/runtime, infra/config, rust_apps (root)]* [OT]

### Thu 2026-07-09 — SebrusApps (~0.5 h; std 0.5 / OT 0.0)

- 09:48 `4b74bf5` Add email connectivity module and extend permission merging logic *[crates/email, crates/http, crates/permissions, infra/config]*

### Fri 2026-07-10 — SebrusApps (~2.0 h; std 0.5 / OT 1.5)

- 09:06 `b2e1fb0` Extend IMAP session for unseen-message filing and email attachment parsing *[crates/email, crates/error, crates/orm, infra/config]*
- 18:39 `c00464d` Add `audit_ready` app for server token management *[crates/http, crates/orm, crates/runtime, infra/config, rust_apps (root), rust_apps/core, scripts]* [OT]
- 20:46 `2dcd9b6` Add initial Playwright E2E test setup for AuditReady Compliance *[crates/http, crates/python-bridge, crates/runtime, e2e tests, infra/config, rust_apps/core]* [OT]
- 21:08 `505e9ff` Add initial Playwright E2E test setup for AuditReady Compliance *[infra/config]* [OT]
- 21:14 `844a3d5` Add initial Playwright E2E test setup for AuditReady Compliance *[crates/http, e2e tests]* [OT]
- 21:17 `705bbd4` Add Playwright E2E tests for Infrastructure Server and Kubernetes Cluster creation *[e2e tests, infra/config]* [OT]

### Sat 2026-07-11 — AuditReady (~1.0 h; std 0.0 / OT 1.0)

- 21:16 `a376cad` Add initial implementation of AuditReady tool *[infra/config, src]* [OT]
- 23:30 `0a83a25` Add initial implementation of AuditReady tool *[crates/agent, crates/protocol, infra/config, src]* [OT]

### Sat 2026-07-11 — SebrusApps (~0.5 h; std 0.0 / OT 0.5)

- 23:46 `92c17d5` Delete outdated server token codebase *[crates/protocol, e2e tests, infra/config]* [OT]

### Sun 2026-07-12 — AuditReady (~1.4 h; std 0.0 / OT 1.4)

- 21:04 `7cf8f24` Update module configuration to include agent and protocol source folders *[infra/config]* [OT]
- 21:13 `43b0153` Add installation script, Windows installer, and release CI workflow *[infra/config, scripts]* [OT]
- 21:31 `a7241f1` Add installation script, Windows installer, and release CI workflow *[crates/agent]* [OT]
- 21:33 `ff42898` chore(release): bump crate versions to 0.1.1 *[crates/agent, crates/protocol]* [OT]
- 21:33 `150b2d8` chore(release): v0.1.1 *[crates/agent, crates/protocol]* [OT]
- 21:34 `86a4edf` Merge remote-tracking branch 'origin/main' *[misc]* [OT]
- 21:58 `1e0c9cc` ci: fix cargo-wix package selection and update MSI version to 0.1.1 *[infra/config]* [OT]

### Sun 2026-07-12 — SebrusApps (~8.4 h; std 0.0 / OT 8.4)

- 15:08 `3bf97fa` fixed bugs *[frappe apps/config, infra/config]* [OT]
- 15:37 `4bd10df` fixed bugs *[frappe apps/config]* [OT]
- 16:02 `10d5a73` fixee *[frappe apps/config]* [OT]
- 16:54 `ac9e50e` Enhance OAuth handling with improved error logging and input validation *[frappe apps/config]* [OT]
- 17:36 `e009e7a` Handle missing `redirect_uri` for custom OAuth providers *[frappe apps/config]* [OT]
- 18:04 `05be00b` Improve OAuth `redirect_uri` handling for custom providers *[frappe apps/config]* [OT]
- 18:27 `c0f6c95` Refactor OAuth provider resolution and key mapping *[frappe apps/config]* [OT]
- 19:50 `e3b92d0` Improve logging and error handling for initialization paths *[frappe apps/config]* [OT]
- 20:17 `869b35d` Refactor OAuth `redirect_uri` logic for better request context handling *[frappe apps/config]* [OT]
- 20:37 `75c2217` Enhance OAuth `redirect_uri` handling with improved fallback logic and detailed debug logging *[frappe apps/config]* [OT]
- 21:03 `0dc0efd` Enhance OAuth `redirect_uri` handling with improved fallback logic and detailed debug logging *[frappe apps/config]* [OT]
- 21:53 `62d313c` Enhance OAuth `redirect_uri` handling with improved fallback logic and detailed debug logging *[frappe apps/config]* [OT]
- 22:40 `7bf5bd9` Refactor OAuth provider mapping and enhance Microsoft tenant configuration *[frappe apps/config]* [OT]
- 23:05 `663d89c` Refactor OAuth provider mapping and enhance Microsoft tenant configuration *[frappe apps/config, infra/config]* [OT]

### Mon 2026-07-13 — AuditReady (~1.5 h; std 0.0 / OT 1.5)

- 20:44 `d853021` Add installation script, Windows installer, and release CI workflow *[infra/config]* [OT]
- 20:44 `e6aaa2c` refactor(wix): relocate `main.wxs` to `crates/agent/wix` and update file paths *[crates/agent]* [OT]
- 21:26 `e2b325d` Bump version to 0.1.2 *[crates/agent, crates/protocol]* [OT]
- 21:46 `42423bd` ci(release): explicitly match and attach build artifacts *[infra/config]* [OT]

### Mon 2026-07-13 — SebrusApps (~6.9 h; std 5.0 / OT 1.9)

- 09:38 `45d0f70` Add enhanced logging and tenant-specific handling for OAuth providers *[frappe apps/config, infra/config]*
- 10:46 `b8070b3` Add detailed debug logging for OAuth provider resolution and Social Login Key matching steps *[frappe apps/config]*
- 12:03 `9c0fdd6` Add detailed debug logging for OAuth provider resolution and Social Login Key matching steps *[frappe apps/config]*
- 14:44 `c32d83f` Add `restrict_to_domain` field to Doctype and enhance workspace child-table handling *[crates/http, crates/orm, frappe apps/config]*
- 16:18 `aabe845` Add support for page fixtures and extend infrastructure UI *[crates/orm, crates/runtime, infra/config]*
- 20:10 `17d3d2e` Update log engine data files and index metadata *[infra/config]* [OT]
- 21:33 `d7d5ec4` Add workspace redirects and enhance fleet UI filtering *[crates/http, infra/config]* [OT]

### Tue 2026-07-14 — AuditReady (~1.2 h; std 1.2 / OT 0.0)

- 10:55 `4f6d9ee` ci(release): create dated nightly release on main pushes *[infra/config]*
- 11:09 `2744b0c` ci(release): quote date format strings in release name step *[infra/config]*
- 11:27 `3189c28` feat(install): interactive Linux installer with systemd service and tunnel cwd *[crates/agent, scripts]*
- 11:35 `4542c7f` chore(install): set default telemetry interval to 10 seconds *[scripts]*
- 11:36 `99e279b` chore(install): support wget, pipe, and env-var based installs *[scripts]*

### Tue 2026-07-14 — Sebrus Compliance System (~0.5 h; std 0.5 / OT 0.0)

- 08:45 `c8f9935` Update Dockerfile to swap `audit_ready` references with `sebrus_logger` in dependencies and workspace configuration *[infra/config]*

### Tue 2026-07-14 — SebrusApps (~1.0 h; std 0.0 / OT 1.0)

- 19:10 `1673212` Implement batch processing of logs and optimize vulnerability scans. *[crates/log_engine, crates/orm, crates/runtime, infra/config]* [OT]
- 22:22 `a1d72ef` Add in-memory caches for telemetry and job handling. *[crates/log_engine, crates/runtime]* [OT]

### Wed 2026-07-15 — SebrusApps (~8.1 h; std 7.6 / OT 0.5)

- 08:07 `9b645a3` Refactor and optimize batch processing in core components *[crates/log_engine, crates/orm, crates/runtime, infra/config, rust_apps/core]*
- 09:53 `fc8a14f` Refactor code for improved readability and formatting. *[crates/http, crates/log_engine, crates/orm, crates/permissions, crates/protocol, crates/python-bridge, crates/runtime, infra/config, rust_apps/core]*
- 11:27 `171574a` Add WebSocket idle timeout and improve agent token handling *[infra/config]*
- 13:07 `c2aeb8e` Add staging buffer thresholds and optimize memory usage *[crates/http, crates/log_engine, crates/orm, infra/config]*
- 15:42 `de4ac09` Add log retention with pruning of old records *[crates/log_engine, crates/runtime]*
- 16:44 `6279731` Add log retention with pruning of old records *[crates/log_engine, infra/config]*
- 17:46 `6bbe4ce` Enhance query parsing to support advanced search features *[crates/log_engine]* [OT]

### Thu 2026-07-16 — AuditReady (~1.1 h; std 0.5 / OT 0.6)

- 13:43 `cfb9bab` feat(scripts): add `update-token.sh` for token updates with backup and restart support *[scripts]*
- 17:05 `3fdf349` feat(install): add support for `restart.sh` helper script and update release packaging *[infra/config, scripts]* [OT]
- 17:10 `c91e111` feat(install): add support for `restart.sh` helper script and update release packaging *[scripts]* [OT]

### Thu 2026-07-16 — SebrusApps (~2.9 h; std 2.4 / OT 0.5)

- 12:39 `b74a20e` Replace `audit_ready` with `sebrus_logger` across workspace configuration *[crates/runtime, infra/config, scripts]*
- 13:03 `6b4c1f7` Optimize database pool management and enhance vulnerability data handling. *[crates/http, crates/orm, crates/python-bridge, crates/queue, crates/runtime, infra/config]*
- 14:33 `3b9da12` Remove auto-generated server token logic *[e2e tests, infra/config]*
- 21:53 `d700714` "Implement WebSocket keepalive ping for idle timeout prevention *[infra/config]* [OT]

### Fri 2026-07-17 — AuditReady (~2.0 h; std 1.0 / OT 1.0)

- 09:18 `1d351d5` feat(install): add support for `restart.sh` helper script and update release packaging *[docs]*
- 09:40 `8ebc5c8` feat(install): switch to static musl builds for Linux binaries, update installer and CI *[crates/agent, docs, infra/config, scripts]*
- 09:48 `a08ff07` chore(ci): fix indentation in release workflow script *[infra/config]*
- 20:02 `abd8d4f` feat(agent): add WebSocket ping intervals and dead connection detection *[crates/agent, docs, infra/config, scripts]* [OT]
- 22:13 `250515c` feat(agent): add `--print-dns` flag to capture and display live DNS traffic in JSON format *[crates/agent]* [OT]

### Fri 2026-07-17 — SebrusApps (~1.1 h; std 0.0 / OT 1.1)

- 17:08 `d073954` Add per-server activity cache and node-level network/process views *[crates/orm, e2e tests, infra/config]* [OT]
- 21:37 `0aca832` Add toggle for node processes visibility in ops portal *[infra/config]* [OT]
- 21:45 `898d59c` Set node processes block to be hidden by default *[infra/config]* [OT]

### Sun 2026-07-19 — SebrusApps (~2.2 h; std 0.0 / OT 2.2)

- 16:05 `06c58a5` ● Yes, there are a few behavioral changes that could break code relying on the old (incorrect) behavior: *[crates/http, crates/python-bridge, crates/runtime, frappe apps/config, infra/config, rust_apps (root)]* [OT]
- 16:10 `af726eb` Apply local changes *[cli, crates/config, crates/http, crates/orm, docs, infra/config, rust_apps (root)]* [OT]
- 16:26 `2741f83` ● Yes, there are a few behavioral changes that could break code relying on the old (incorrect) behavior: *[infra/config]* [OT]
- 16:27 `d400ba9` Merge remote-tracking branch 'origin/master' *[misc]* [OT]
- 17:49 `03ddd31` Remove Axum tests and improve metadata handling *[crates/config, crates/http, crates/metadata, crates/orm, crates/permissions, crates/queue, crates/runtime, infra/config, rust_apps (root)]* [OT]

### Mon 2026-07-20 — AuditReady (~2.7 h; std 2.7 / OT 0.0)

- 10:53 `751047c` feat(install): add cross-platform installation scripts and configuration support *[crates/agent, docs, infra/config, scripts]*
- 11:09 `a4a6728` feat(install): add interactive prompts for domain and token in Windows installer *[docs, scripts]*
- 13:05 `8e83429` feat(install): add interactive prompts for domain and token in Windows installer *[docs, scripts]*

### Mon 2026-07-20 — SebrusApps (~2.4 h; std 1.0 / OT 1.4)

- 10:35 `40a248a` Add dynamic app registration, runtime refactors, and test updates *[crates/config, crates/http, crates/runtime, docs, infra/config, rust_apps (root)]*
- 15:25 `4416e0e` Disable connection recycling to prevent WAL file conflicts and pool wedging *[crates/orm]*
- 20:42 `2109072` Replace `sebrus_logger` with `audit_ready` dependency in runtime workspace *[crates/runtime, infra/config]* [OT]
- 21:37 `1b1580d` Add Python pool reset to prevent WAL conflicts during runtime healing *[crates/python-bridge, crates/runtime, frappe apps/config]* [OT]

### Tue 2026-07-21 — AuditReady (~0.5 h; std 0.5 / OT 0.0)

- 10:30 `235e621` fix(windows): handle UTF-8 BOM in JSON writes and parsing *[crates/agent, scripts]*

### Tue 2026-07-21 — SebrusApps (~0.5 h; std 0.5 / OT 0.0)

- 09:11 `83baad2` Refactor flush loop and enhance permissions handling. *[crates/permissions, infra/config]*

### Wed 2026-07-22 — AuditReady (~2.2 h; std 2.2 / OT 0.0)

- 11:04 `626b6f2` feat(agent): add `--print-updates` flag and support for pending OS updates in telemetry *[crates/agent]*
- 11:06 `129aa24` feat(agent): add `--print-updates` flag and support for pending OS updates in telemetry *[crates/agent]*
- 12:47 `cc40d86` feat(agent): add `--print-updates` flag and support for pending OS updates in telemetry *[crates/agent, scripts]*
- 12:47 `948a0ac` feat(agent): add `--print-updates` flag and support for pending OS updates in telemetry *[scripts]*

### Thu 2026-07-23 — SebrusApps (~0.5 h; std 0.0 / OT 0.5)

- 21:50 `4547843` Remove unused `json_to_py` import in `queue.rs` *[crates/python-bridge]* [OT]

### Fri 2026-07-24 — SebrusApps (~0.5 h; std 0.0 / OT 0.5)

- 21:41 `9c81692` fixed tokens and added node synk *[crates/http, infra/config]* [OT]

### Sat 2026-07-25 — SebrusApps (~0.5 h; std 0.0 / OT 0.5)

- 20:52 `b8035e0` ● Yes, there are a few behavioral changes that could break code relying on the old (incorrect) behavior: *[crates/runtime]* [OT]
- 20:53 `9c4de36` Merge remote-tracking branch 'origin/master' *[misc]* [OT]

### Mon 2026-07-27 — AuditReady (~0.5 h; std 0.5 / OT 0.0)

- 09:52 `0aece00` Merge remote-tracking branch 'origin/main' *[misc]*

### Tue 2026-07-28 — AuditReady (~0.5 h; std 0.5 / OT 0.0)

- 15:48 `142b15e` feat(agent): add `--print-updates` flag and support for pending OS updates in telemetry *[crates/agent]*

### Tue 2026-07-28 — SebrusApps (~1.0 h; std 1.0 / OT 0.0)

- 12:12 `fec8f1e` Remove `infrastructure-server.spec.js` and unused `.idea/shelf` artifacts *[crates/orm, crates/runtime, crates/session, e2e tests, frappe apps/config, infra/config, rust_apps (root)]*
- 14:47 `6a3d4ec` Replace `strongroom` with `audit_ready` in runtime workspace dependencies *[crates/runtime, infra/config]*

### Thu 2026-07-30 — SebrusApps (~1.0 h; std 0.5 / OT 0.5)

- 16:07 `c9133d8` Remove IDE configuration files and add column validation in db.rs *[crates/runtime, infra/config, rust_apps (root)]*
- 20:23 `dfd1358` Add `.dockerignore` for build and runtime artifact exclusions *[rust_apps (root)]* [OT]

### Sat 2026-08-01 — SebrusApps (~0.5 h; std 0.0 / OT 0.5)

- 22:07 `1be4ef3` Add `create_oauth_state` function for Frappe compatibility and refactor OAuth state handling *[crates/http, crates/python-bridge, infra/config, rust_apps (root)]* [OT]

### Mon 2026-08-03 — SebrusApps (~2.6 h; std 2.1 / OT 0.5)

- 14:35 `60cd986` Add IntelliJ IDEA configuration files for Sebrus Apps module *[infra/config]*
- 14:35 `bbe7fb0` Merge remote-tracking branch 'origin/master' *[misc]*
- 15:32 `8c86801` Implement password persistence and encryption for Frappe compatibility *[crates/http, crates/orm, crates/runtime, infra/config]*
- 15:34 `58e4e55` Implement password persistence and encryption for Frappe compatibility *[infra/config]*
- 16:10 `3cd42c4` Update password masking to align with Frappe's field initialization logic *[crates/orm, infra/config]*
- 21:36 `4e84db7` Refactor password field handling in ORM and runtime *[crates/http, crates/orm, crates/runtime, infra/config]* [OT]

### Tue 2026-08-04 — AuditReady (~0.5 h; std 0.5 / OT 0.0)

- 15:18 `5650c60` feat(agent): add IIS telemetry and cross-platform script execution support *[crates/agent]*

### Tue 2026-08-04 — Sebrus Compliance System (~0.5 h; std 0.5 / OT 0.0)

- 15:40 `7f81fd3` Update Dockerfile to ensure `audit_ready` app is the sole active dependency and verify build consistency *[infra/config]*

### Tue 2026-08-04 — SebrusApps (~0.5 h; std 0.0 / OT 0.5)

- 19:28 `fb37246` Change fieldtype of token_hash from Password to Data *[crates/kiff_logger]* [OT]

### Thu 2026-08-06 — SebrusApps (~0.5 h; std 0.5 / OT 0.0)

- 15:46 `9f44bff` Add workflow and migration features for Sebrus Apps *[crates/http, crates/orm, crates/session, e2e tests, frappe apps/config, infra/config, rust_apps (root)]*

### Thu 2026-08-13 — AuditReady (~0.5 h; std 0.5 / OT 0.0)

- 13:10 `43e108c` feat(agent): add live output tail to progress pings during script execution *[crates/agent]*

### Thu 2026-08-13 — SebrusApps (~0.5 h; std 0.0 / OT 0.5)

- 23:23 `934befd` Ensure safe column addition for existing DocType tables *[crates/orm, docs]* [OT]

## Work-area breakdown (commit touches, not time-weighted)

- AuditReady → crates/agent: 18 commits
- AuditReady → crates/protocol: 4 commits
- AuditReady → docs: 6 commits
- AuditReady → infra/config: 15 commits
- AuditReady → scripts: 15 commits
- AuditReady → src: 2 commits
- Sebrus Compliance System → docs: 7 commits
- Sebrus Compliance System → infra/config: 15 commits
- SebrusApps → cli: 7 commits
- SebrusApps → crates/config: 8 commits
- SebrusApps → crates/email: 2 commits
- SebrusApps → crates/error: 2 commits
- SebrusApps → crates/http: 43 commits
- SebrusApps → crates/kiff_logger: 5 commits
- SebrusApps → crates/log_engine: 11 commits
- SebrusApps → crates/metadata: 3 commits
- SebrusApps → crates/orm: 34 commits
- SebrusApps → crates/permissions: 10 commits
- SebrusApps → crates/protocol: 2 commits
- SebrusApps → crates/python-bridge: 16 commits
- SebrusApps → crates/queue: 4 commits
- SebrusApps → crates/runtime: 33 commits
- SebrusApps → crates/session: 6 commits
- SebrusApps → crates/sql-translator: 2 commits
- SebrusApps → docs: 9 commits
- SebrusApps → e2e tests: 10 commits
- SebrusApps → frappe apps/config: 43 commits
- SebrusApps → infra/config: 65 commits
- SebrusApps → rust_apps (root): 17 commits
- SebrusApps → rust_apps/core: 11 commits
- SebrusApps → rust_apps/sample: 2 commits
- SebrusApps → scripts: 10 commits
- SebrusApps → src: 3 commits

use pyo3::prelude::*;
use std::sync::OnceLock;

pub mod db;
pub mod document;
pub mod queue;
pub mod realtime;
pub mod session;
pub mod utils;

static RUNTIME: OnceLock<tokio::runtime::Runtime> = OnceLock::new();
/// The bridge pool is swappable: the runtime watchdog replaces it after
/// external writes to a live site.db wedge the pooled connections.
static POOL: std::sync::RwLock<Option<orm::DatabasePool>> = std::sync::RwLock::new(None);
static PUBSUB: OnceLock<std::sync::Arc<::queue::PubSub>> = OnceLock::new();
static LOG_SERVICE: OnceLock<log_engine::LogService> = OnceLock::new();
static WHITELIST: OnceLock<MethodWhitelist> = OnceLock::new();

/// Snapshot of Frappe's `@frappe.whitelist()`-decorated functions.
#[derive(Debug, Clone, Default)]
pub struct MethodWhitelist {
    all: std::collections::HashSet<String>,
    allow_guest: std::collections::HashSet<String>,
    allowed_prefixes: Vec<String>,
}

impl MethodWhitelist {
    pub fn contains(&self, method_path: &str) -> bool {
        self.all.contains(method_path)
    }

    pub fn allows_guest(&self, method_path: &str) -> bool {
        self.allow_guest.contains(method_path)
    }

    pub fn is_allowed_module_path(&self, module_path: &str) -> bool {
        self.allowed_prefixes.iter().any(|prefix| {
            // Allow exact module names (e.g. "frappe" for frappe.ping) as well
            // as dotted prefixes (e.g. "frappe.desk" for frappe.desk.page).
            module_path == prefix.trim_end_matches('.') || module_path.starts_with(prefix)
        })
    }
}

pub fn init(runtime: tokio::runtime::Runtime, pool: orm::DatabasePool) {
    let _ = RUNTIME.set(runtime);
    let _ = swap_pool(pool);
}

/// Replace the pool used by the Python bridge and return the replaced one,
/// if any. Called by the runtime watchdog when the previous pool's
/// connections went stale (e.g. after external writes to the live SQLite
/// file). The caller decides the old pool's fate: close it (safe when its
/// WAL is intact) or keep it alive forever (when its WAL is split-brain
/// garbage — dropping the last handle would run a close-time checkpoint that
/// copies garbage pages into the main database file).
pub fn swap_pool(pool: orm::DatabasePool) -> Option<orm::DatabasePool> {
    POOL.write().expect("POOL lock poisoned").replace(pool)
}

/// Remove the bridge's pool without replacing it, returning the old one.
/// The runtime watchdog calls this when a pool goes wedged: requests then
/// fail fast (`pool()` callers see "pool not initialized") instead of
/// committing into the wedged pool's split-brain view, where every commit
/// risks an auto-checkpoint that poisons the main database file.
pub fn clear_pool() -> Option<orm::DatabasePool> {
    POOL.write().expect("POOL lock poisoned").take()
}

pub fn init_pubsub(pubsub: std::sync::Arc<::queue::PubSub>) {
    let _ = PUBSUB.set(pubsub);
}

pub fn init_log_service(service: log_engine::LogService) {
    let _ = LOG_SERVICE.set(service);
}

/// Build a Rust-side snapshot of every function decorated with
/// `@frappe.whitelist()` by walking the installed app package trees.
///
/// This is intentionally done once at startup so the dispatcher can reject
/// non-whitelisted methods before any Python import runs.
pub fn init_whitelist() -> error::Result<()> {
    let mut all = std::collections::HashSet::new();
    let mut allow_guest = std::collections::HashSet::new();
    let mut allowed_prefixes = vec!["frappe.".to_string()];

    Python::with_gil(|py| {
        let frappe = py.import("frappe")?;
        let pkgutil = py.import("pkgutil")?;

        // Discover installed apps so we also walk app-specific whitelisted methods.
        let installed_apps = frappe
            .getattr("get_installed_apps")
            .and_then(|f| f.call0())
            .and_then(|v| v.extract::<Vec<String>>())
            .unwrap_or_else(|_| vec!["frappe".to_string()]);

        // Walk and import every module under each installed app. This triggers
        // the `@frappe.whitelist()` decorator at import time and populates
        // Frappe's internal whitelisted/guest_methods collections.
        for app in &installed_apps {
            if app != "frappe" {
                allowed_prefixes.push(format!("{}.", app));
            }

            let app_mod = match py.import(app) {
                Ok(m) => m,
                Err(e) => {
                    tracing::debug!(app = %app, error = %e, "skipping unimportable app for whitelist");
                    continue;
                }
            };
            let app_path = match app_mod.getattr("__path__") {
                Ok(p) => p,
                Err(_) => continue,
            };
            let prefix = format!("{}.", app);
            let iterwalk = pkgutil.getattr("walk_packages")?.call1((app_path, prefix))?;
            for item in iterwalk.try_iter()? {
                let item = item?;
                let mod_name: String = item.getattr("name")?.extract()?;
                if let Err(e) = py.import(&mod_name) {
                    tracing::debug!(module = %mod_name, error = %e, "skipping unimportable module for whitelist");
                }
            }
        }

        // Frappe keeps whitelisted functions in two places:
        //   - `frappe.whitelisted` / `frappe.guest_methods` are the shim's lists
        //     (used when modules are imported while the shim is active).
        //   - `frappe._real_frappe.whitelisted` / `...guest_methods` are the real
        //     Frappe module's sets (used when modules were preloaded while the
        //     real frappe module was temporarily active during startup).
        // We collect from both so the snapshot is complete.
        let path_of = |obj: &Bound<'_, PyAny>| -> Option<String> {
            let module: String = obj.getattr("__module__").ok()?.extract().ok()?;
            let name: String = obj.getattr("__name__").ok()?.extract().ok()?;
            Some(format!("{}.{}", module, name))
        };

        let collect = |collection: &Bound<'_, PyAny>, target: &mut std::collections::HashSet<String>| {
            let Ok(iter) = collection.try_iter() else {
                return;
            };
            for item in iter.flatten() {
                if let Some(path) = path_of(&item) {
                    target.insert(path);
                }
            }
        };

        let mut shim_all = 0usize;
        let mut shim_guest = 0usize;
        let mut real_all = 0usize;
        let mut real_guest = 0usize;

        if let Ok(whitelisted) = frappe.getattr("whitelisted") {
            let before = all.len();
            collect(&whitelisted, &mut all);
            shim_all = all.len() - before;
        }
        if let Ok(guest_methods) = frappe.getattr("guest_methods") {
            let before = allow_guest.len();
            collect(&guest_methods, &mut allow_guest);
            shim_guest = allow_guest.len() - before;
        }

        if let Ok(real) = frappe.getattr("_real_frappe") {
            if !real.is_none() {
                if let Ok(whitelisted) = real.getattr("whitelisted") {
                    let before = all.len();
                    collect(&whitelisted, &mut all);
                    real_all = all.len() - before;
                }
                if let Ok(guest_methods) = real.getattr("guest_methods") {
                    let before = allow_guest.len();
                    collect(&guest_methods, &mut allow_guest);
                    real_guest = allow_guest.len() - before;
                }
            }
        }
        tracing::info!(
            shim_all = shim_all,
            shim_guest = shim_guest,
            real_all = real_all,
            real_guest = real_guest,
            "whitelist source counts"
        );

        Ok::<(), PyErr>(())
    })
    .map_err(|e| error::RuntimeError::Python(format!("whitelist scan: {}", e)))?;

    let all_count = all.len();
    let guest_count = allow_guest.len();
    let _ = WHITELIST.set(MethodWhitelist {
        all,
        allow_guest,
        allowed_prefixes,
    });
    tracing::info!(
        all = all_count,
        guest = guest_count,
        "loaded Python method whitelist"
    );
    Ok(())
}

fn whitelist() -> &'static MethodWhitelist {
    WHITELIST.get_or_init(MethodWhitelist::default)
}

pub(crate) fn log_service() -> Option<&'static log_engine::LogService> {
    LOG_SERVICE.get()
}

pub(crate) fn rt() -> &'static tokio::runtime::Runtime {
    RUNTIME.get().expect("runtime not initialized")
}

pub(crate) fn pool() -> orm::DatabasePool {
    pool_opt().expect("pool not initialized")
}

pub(crate) fn pool_opt() -> Option<orm::DatabasePool> {
    POOL.read().expect("POOL lock poisoned").clone()
}

pub(crate) fn pubsub() -> Option<&'static std::sync::Arc<::queue::PubSub>> {
    PUBSUB.get()
}

pub(crate) fn py_to_json(obj: &Bound<'_, PyAny>) -> PyResult<serde_json::Value> {
    use serde_json::Value;
    if obj.is_none() {
        return Ok(Value::Null);
    }

    // Shim _LocalProxy (python/frappe/_context.py) exposes _resolve().
    if let Ok(proxy) = obj.getattr("_resolve") {
        if proxy.is_callable() {
            if let Ok(inner) = proxy.call0() {
                return py_to_json(&inner);
            }
        }
    }

    // Werkzeug/Flask-style local proxy: resolve to the underlying object.
    if let Ok(proxy) = obj.getattr("_get_current_object") {
        if let Ok(inner) = proxy.call0() {
            return py_to_json(&inner);
        }
    }

    // Plain scalars.
    if let Ok(b) = obj.extract::<bool>() {
        return Ok(Value::Bool(b));
    }
    if let Ok(i) = obj.extract::<i64>() {
        return Ok(Value::Number(i.into()));
    }
    if let Ok(f) = obj.extract::<f64>() {
        return Ok(Value::Number(
            serde_json::Number::from_f64(f).unwrap_or(0.into()),
        ));
    }
    if let Ok(s) = obj.extract::<String>() {
        return Ok(Value::String(s));
    }

    // Collections.  Defensively treat any unconvertible item as null so one
    // bad value (e.g. a dict with None keys or broken __repr__) does not
    // fail the whole response.
    if let Ok(list) = obj.downcast::<pyo3::types::PyList>() {
        let mut arr = Vec::new();
        for item in list.iter() {
            arr.push(py_to_json(&item).unwrap_or(Value::Null));
        }
        return Ok(Value::Array(arr));
    }
    if let Ok(tuple) = obj.downcast::<pyo3::types::PyTuple>() {
        let mut arr = Vec::new();
        for item in tuple.iter() {
            arr.push(py_to_json(&item).unwrap_or(Value::Null));
        }
        return Ok(Value::Array(arr));
    }
    if let Ok(dict) = obj.downcast::<pyo3::types::PyDict>() {
        let mut map = serde_json::Map::new();
        for (k, v) in dict {
            if let Ok(key) = k.extract::<String>() {
                map.insert(key, py_to_json(&v).unwrap_or(Value::Null));
            }
        }
        return Ok(Value::Object(map));
    }

    // Datetime/date objects.
    if let Ok(dt) = obj.getattr("isoformat") {
        if let Ok(s) = dt.call0() {
            if let Ok(s) = s.extract::<String>() {
                return Ok(Value::String(s));
            }
        }
    }

    // Frappe Document / FormMeta objects expose ``as_dict``.  Use it so the
    // result is JSON-serializable instead of a Python object repr.
    if let Ok(as_dict) = obj.getattr("as_dict") {
        if let Ok(d) = as_dict.call0() {
            return py_to_json(&d);
        }
    }

    // Fallback: string representation. Some Python objects (e.g. cached
    // responses or objects with a broken __repr__) raise when converted to a
    // string, so fall back to null rather than failing the whole API response.
    if let Ok(s) = obj.str() {
        if let Ok(s) = s.extract::<String>() {
            return Ok(Value::String(s));
        }
    }
    Ok(Value::Null)
}

pub fn json_to_py(py: Python<'_>, val: &serde_json::Value) -> PyResult<PyObject> {
    match val {
        serde_json::Value::Null => Ok(py.None()),
        serde_json::Value::Bool(b) => Ok(b.into_pyobject(py)?.to_owned().unbind().into()),
        serde_json::Value::Number(n) if n.is_i64() => {
            Ok(n.as_i64().unwrap().into_pyobject(py)?.unbind().into())
        }
        serde_json::Value::Number(n) if n.is_f64() => {
            Ok(n.as_f64().unwrap().into_pyobject(py)?.unbind().into())
        }
        serde_json::Value::Number(n) => Ok(n.as_u64().unwrap().into_pyobject(py)?.unbind().into()),
        serde_json::Value::String(s) => {
            Ok(s.as_str().into_pyobject(py)?.to_owned().unbind().into())
        }
        serde_json::Value::Array(arr) => {
            let list = pyo3::types::PyList::empty(py);
            for item in arr {
                list.append(json_to_py(py, item)?)?;
            }
            Ok(list.unbind().into())
        }
        serde_json::Value::Object(obj) => {
            let dict = pyo3::types::PyDict::new(py);
            for (k, v) in obj {
                dict.set_item(k, json_to_py(py, v)?)?;
            }
            Ok(dict.unbind().into())
        }
    }
}

/// Dynamically call a Python method by dotted path (e.g. "frappe.desk.doctype.get_list")
/// with kwargs parsed from JSON.
pub fn call_method(
    method_path: &str,
    kwargs: &serde_json::Value,
) -> error::Result<serde_json::Value> {
    call_method_with_user(method_path, kwargs, None)
}

/// Return the user stored in the current Python request context.
///
/// This is useful after a Python login flow (e.g. OAuth) has run inside the
/// shim; the Rust layer can then create a persisted session for that user.
pub fn current_py_session_user() -> Option<String> {
    Python::with_gil(|py| {
        let session = py.import("frappe").ok()?.getattr("session").ok()?;
        session.getattr("user").ok()?.extract::<String>().ok()
    })
}

pub fn call_method_with_user(
    method_path: &str,
    kwargs: &serde_json::Value,
    user: Option<&str>,
) -> error::Result<serde_json::Value> {
    let parts: Vec<&str> = method_path.split('.').collect();
    if parts.len() < 2 {
        return Err(error::RuntimeError::Python("invalid method path".into()));
    }

    let func_name = parts.last().unwrap();
    let module_path = parts[..parts.len() - 1].join(".");

    // Enforce the Frappe whitelist before any Python module is imported.
    // Only paths under the allowed namespaces may be reached, and only
    // functions explicitly decorated with @frappe.whitelist() are callable.
    // When the whitelist snapshot has not been loaded (e.g. unit tests that
    // do not run the full runtime startup), enforcement is skipped so basic
    // functionality still works; production code must call init_whitelist().
    if WHITELIST.get().is_some() {
        let snap = whitelist();
        if !snap.is_allowed_module_path(&module_path) {
            return Err(error::RuntimeError::Python(format!(
                "method path not allowed: {}",
                method_path
            )));
        }
        if !snap.contains(method_path) {
            return Err(error::RuntimeError::Python(format!(
                "method not whitelisted: {}",
                method_path
            )));
        }
        let is_guest = user.map(|u| u == "Guest").unwrap_or(true);
        if is_guest && !snap.allows_guest(method_path) {
            return Err(error::RuntimeError::Python(format!(
                "method not available to guests: {}",
                method_path
            )));
        }
    }

    Python::with_gil(|py| {
        // Request-level params injected by the frappe JS client that are
        // never part of a Python function's signature — strip them so we
        // don't get "unexpected keyword argument" TypeErrors.
        const SKIP_KEYS: &[&str] = &["cmd", "_", "type", "freeze", "freeze_message"];

        // Build the kwargs dict that will be passed to the Python function.
        // We use Python inspect to filter to only accepted parameters so that
        // functions without **kwargs don't receive unknown keys.
        let py_kwargs = if let serde_json::Value::Object(map) = kwargs {
            let dict = pyo3::types::PyDict::new(py);
            for (k, v) in map {
                if SKIP_KEYS.contains(&k.as_str()) {
                    continue;
                }
                let py_val = json_to_py(py, v).map_err(|e| {
                    error::RuntimeError::Python(format!("arg {} convert: {}", k, e))
                })?;
                dict.set_item(k, py_val)
                    .map_err(|e| error::RuntimeError::Python(format!("arg {} set: {}", k, e)))?;
            }
            Some(dict)
        } else {
            None
        };

        // Reset frappe.response and populate frappe.local.form_dict / frappe.form_dict
        // before dispatching so Python functions see a clean per-request context.
        if let Ok(frappe_mod) = py.import("frappe") {
            if let Ok(set_ctx) = frappe_mod.getattr("_set_request_context") {
                let ctx_dict = py_kwargs
                    .as_ref()
                    .map(|d| d.as_any().clone())
                    .unwrap_or_else(|| pyo3::types::PyDict::new(py).into_any());
                let _ = set_ctx.call1((ctx_dict, user.unwrap_or("Guest")));
            }
        }

        let module = py
            .import(module_path.as_str())
            .map_err(|e| error::RuntimeError::Python(format!("import {}: {}", module_path, e)))?;

        let func = module
            .getattr(*func_name)
            .map_err(|e| error::RuntimeError::Python(format!("getattr {}: {}", func_name, e)))?;

        // Filter kwargs to only parameters the function actually accepts,
        // unless the function declares **kwargs (VAR_KEYWORD parameter).
        let filtered_kwargs = if let Some(ref kw) = py_kwargs {
            let inspect = py
                .import("inspect")
                .ok()
                .and_then(|m| m.getattr("signature").ok())
                .and_then(|sig_fn| sig_fn.call1((&func,)).ok());

            if let Some(sig) = inspect {
                let accepts_var_keyword = sig
                    .getattr("parameters")
                    .ok()
                    .and_then(|params| {
                        // Check if any parameter has kind == VAR_KEYWORD (4)
                        let values = params.call_method0("values").ok()?;
                        let iter = values.try_iter().ok()?;
                        for p in iter {
                            if let Ok(p) = p {
                                if let Ok(kind) = p.getattr("kind") {
                                    if kind.extract::<i32>().ok() == Some(4) {
                                        return Some(true);
                                    }
                                }
                            }
                        }
                        Some(false)
                    })
                    .unwrap_or(false);

                if accepts_var_keyword {
                    Some(kw.clone())
                } else {
                    // Build a filtered dict with only accepted parameter names.
                    let param_names: std::collections::HashSet<String> = sig
                        .getattr("parameters")
                        .ok()
                        .and_then(|params| params.call_method0("keys").ok())
                        .and_then(|keys| {
                            keys.try_iter().ok().map(|iter| {
                                iter.filter_map(|k| k.ok()?.extract::<String>().ok())
                                    .collect()
                            })
                        })
                        .unwrap_or_default();

                    let filtered = pyo3::types::PyDict::new(py);
                    for (k, v) in kw.iter() {
                        if let Ok(key) = k.extract::<String>() {
                            if param_names.contains(&key) {
                                let _ = filtered.set_item(k, v);
                            }
                        }
                    }
                    Some(filtered)
                }
            } else {
                Some(kw.clone())
            }
        } else {
            None
        };

        let result = if let Some(ref kw) = filtered_kwargs {
            func.call((), Some(kw))
        } else {
            func.call0()
        }
        .map_err(|e| {
            // Extract a full Python traceback so the JS console / logs show the
            // real failure point instead of just the exception message.
            let detail = Python::with_gil(|py| {
                let traceback = py
                    .import("traceback")
                    .ok()
                    .and_then(|tb| tb.getattr("format_exception").ok())
                    .and_then(|fmt| {
                        // format_exception(exception_type, exception_value, traceback)
                        let args = (e.get_type(py), e.clone_ref(py), e.traceback(py));
                        fmt.call1(args).ok()
                    })
                    .and_then(|lines| py_to_json(&lines).ok())
                    .and_then(|v| v.as_array().cloned())
                    .map(|arr| {
                        arr.iter()
                            .map(|v| v.as_str().unwrap_or("").to_string())
                            .collect::<String>()
                    })
                    .unwrap_or_default();
                if traceback.is_empty() {
                    e.to_string()
                } else {
                    traceback
                }
            });
            error::RuntimeError::Python(format!("call {}: {}", method_path, detail))
        })?;

        let result_json = py_to_json(&result)
            .map_err(|e| error::RuntimeError::Python(format!("convert result: {}", e)))?;

        // If the Python function populated frappe.response.docs, return the full
        // response object (Frappe getdoc/getdoctype pattern).  Otherwise wrap the
        // return value in {"message": ...} as standard Frappe API does.
        let response_json = py
            .import("frappe")
            .ok()
            .and_then(|frappe| frappe.getattr("response").ok())
            .and_then(|resp| py_to_json(&resp).ok());

        if let Some(serde_json::Value::Object(mut resp_map)) = response_json {
            let docs_populated = resp_map
                .get("docs")
                .and_then(|d| d.as_array())
                .map(|a| !a.is_empty())
                .unwrap_or(false);

            // Also treat it as a real response if caller added keys other than docs.
            let extra_keys = resp_map.keys().any(|k| k != "docs");

            if docs_populated || extra_keys {
                if !result_json.is_null() {
                    resp_map.insert("message".to_string(), result_json);
                }
                return Ok(serde_json::Value::Object(resp_map));
            }
        }

        // Standard case: wrap in {"message": <return_value>}
        Ok(serde_json::json!({ "message": result_json }))
    })
}

pub(crate) fn values_from_py(obj: Option<Bound<'_, PyAny>>) -> PyResult<Vec<serde_json::Value>> {
    match obj {
        None => Ok(vec![]),
        Some(o) if o.is_none() => Ok(vec![]),
        Some(o) => {
            if let Ok(list) = o.downcast::<pyo3::types::PyList>() {
                let mut vals = Vec::new();
                for item in list.iter() {
                    vals.push(py_to_json(&item)?);
                }
                Ok(vals)
            } else if let Ok(tuple) = o.downcast::<pyo3::types::PyTuple>() {
                let mut vals = Vec::new();
                for item in tuple.iter() {
                    vals.push(py_to_json(&item)?);
                }
                Ok(vals)
            } else {
                Ok(vec![py_to_json(&o)?])
            }
        }
    }
}

/// Query the log engine for Kiff Log Entry records.
///
/// This bypasses the SQL database and returns records directly from the
/// Tantivy-backed log engine so the Desk list view can display them.
#[pyfunction]
fn log_query(q: &str, limit: usize) -> PyResult<PyObject> {
    let Some(service) = log_service() else {
        return Err(pyo3::exceptions::PyRuntimeError::new_err(
            "log engine not initialized",
        ));
    };

    let records = rt()
        .block_on(async { service.query(q, limit).await })
        .map_err(|e| {
            pyo3::exceptions::PyRuntimeError::new_err(format!("log query failed: {}", e))
        })?;

    Python::with_gil(|py| {
        let list = pyo3::types::PyList::empty(py);
        for (idx, rec) in records.into_iter().enumerate() {
            let dict = pyo3::types::PyDict::new(py);
            let ts_secs = rec.timestamp / 1000;
            let dt = chrono::DateTime::from_timestamp(ts_secs, 0)
                .map(|d| d.to_rfc3339())
                .unwrap_or_else(|| rec.timestamp.to_string());
            let name = format!("KLE-{}-{}", rec.timestamp, idx);

            dict.set_item("name", name)?;
            dict.set_item("doctype", "Kiff Log Entry")?;
            dict.set_item("timestamp", dt)?;
            dict.set_item("level", rec.level)?;
            dict.set_item("service", rec.service)?;
            dict.set_item("message", rec.message)?;
            if let Some(doctype) = rec.fields.get("doctype").and_then(|v| v.as_str()) {
                dict.set_item("doctype_field", doctype)?;
            }
            if let Some(docname) = rec.fields.get("docname").and_then(|v| v.as_str()) {
                dict.set_item("docname", docname)?;
            }
            if let Some(event) = rec.fields.get("event").and_then(|v| v.as_str()) {
                dict.set_item("event", event)?;
            }
            if let Some(status) = rec.fields.get("status").and_then(|v| v.as_str()) {
                dict.set_item("status", status)?;
            }
            if let Some(severity) = rec.fields.get("severity").and_then(|v| v.as_str()) {
                dict.set_item("severity", severity)?;
            }
            if !rec.fields.is_empty() {
                let raw = serde_json::to_string(&rec.fields).unwrap_or_else(|_| "{}".to_string());
                dict.set_item("raw_fields", raw)?;
            }

            list.append(dict)?;
        }
        Ok(list.into_pyobject(py)?.to_owned().unbind().into())
    })
}

/// Python-callable init: creates a Tokio runtime and DB pool from a URL.
/// This is used by the embedded Python (.so) instance to initialize itself
/// independently of the binary's statically-linked instance.
#[pyfunction]
fn init_from_url(db_driver: &str, db_url: &str) -> PyResult<()> {
    use pyo3::exceptions::PyRuntimeError;
    if RUNTIME.get().is_some() {
        return Ok(()); // already initialized
    }
    let rt = tokio::runtime::Builder::new_multi_thread()
        .worker_threads(2)
        .enable_all()
        .build()
        .map_err(|e| PyRuntimeError::new_err(format!("tokio: {}", e)))?;

    let pool = rt
        .block_on(async {
            match db_driver {
                "postgres" => orm::DatabasePool::connect_postgres(db_url).await,
                _ => orm::DatabasePool::connect_sqlite(db_url).await,
            }
        })
        .map_err(|e| PyRuntimeError::new_err(format!("db: {}", e)))?;

    let _ = RUNTIME.set(rt);
    let _ = swap_pool(pool);
    Ok(())
}

/// Python-callable pool reset, invoked by the runtime's pool watchdog after
/// it heals a wedged site database. This .so instance has its own POOL
/// static, separate from the binary's statically-linked instance, so the
/// watchdog swapping the binary's pool is not enough: left alone, this
/// instance keeps file handles to the quarantined WAL and stays wedged
/// across heals, re-triggering the heal cycle after every heal.
///
/// The old pool is deliberately leaked, not closed: its close-time
/// checkpoint could copy garbage pages into the freshly restored main DB.
/// Costs a few file descriptors per heal; reclaimed on process exit.
#[pyfunction]
fn reset_pool_from_url(db_driver: &str, db_url: &str) -> PyResult<()> {
    use pyo3::exceptions::PyRuntimeError;

    // Take the current pool out of service immediately so in-flight Python
    // calls fail fast instead of writing into the wedged view. Leak it (see
    // docstring) rather than closing it.
    if let Some(old) = clear_pool() {
        std::mem::forget(old);
    }

    if RUNTIME.get().is_none() {
        // Never initialized (startup init failed): a fresh init creates both
        // the runtime and the pool.
        return init_from_url(db_driver, db_url);
    }

    let rt = RUNTIME.get().expect("RUNTIME checked above");
    let pool = rt
        .block_on(async {
            match db_driver {
                "postgres" => orm::DatabasePool::connect_postgres(db_url).await,
                _ => orm::DatabasePool::connect_sqlite(db_url).await,
            }
        })
        .map_err(|e| PyRuntimeError::new_err(format!("db: {}", e)))?;

    let _ = swap_pool(pool);
    Ok(())
}

#[pymodule]
fn kiff_core(m: &Bound<'_, PyModule>) -> PyResult<()> {
    m.add_function(wrap_pyfunction!(init_from_url, m)?)?;
    m.add_function(wrap_pyfunction!(reset_pool_from_url, m)?)?;
    m.add_function(wrap_pyfunction!(db::get_doc, m)?)?;
    m.add_function(wrap_pyfunction!(db::get_list, m)?)?;
    m.add_function(wrap_pyfunction!(db::get_value, m)?)?;
    m.add_function(wrap_pyfunction!(db::db_sql, m)?)?;
    m.add_function(wrap_pyfunction!(db::db_set_values, m)?)?;
    m.add_function(wrap_pyfunction!(db::db_exists, m)?)?;
    m.add_function(wrap_pyfunction!(db::db_count, m)?)?;
    m.add_function(wrap_pyfunction!(db::save_doc, m)?)?;
    m.add_function(wrap_pyfunction!(db::insert_doc, m)?)?;
    m.add_function(wrap_pyfunction!(db::delete_doc, m)?)?;
    m.add_function(wrap_pyfunction!(db::db_commit, m)?)?;
    m.add_function(wrap_pyfunction!(db::db_rollback, m)?)?;

    m.add_function(wrap_pyfunction!(session::get_roles, m)?)?;
    m.add_function(wrap_pyfunction!(session::has_permission, m)?)?;

    m.add_function(wrap_pyfunction!(queue::enqueue, m)?)?;
    m.add_function(wrap_pyfunction!(realtime::publish_realtime, m)?)?;

    m.add_function(wrap_pyfunction!(log_query, m)?)?;

    Ok(())
}

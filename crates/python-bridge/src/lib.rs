use pyo3::prelude::*;
use std::sync::OnceLock;

pub mod db;
pub mod document;
pub mod queue;
pub mod realtime;
pub mod session;
pub mod utils;

static RUNTIME: OnceLock<tokio::runtime::Runtime> = OnceLock::new();
static POOL: OnceLock<orm::DatabasePool> = OnceLock::new();
static PUBSUB: OnceLock<std::sync::Arc<::queue::PubSub>> = OnceLock::new();

pub fn init(runtime: tokio::runtime::Runtime, pool: orm::DatabasePool) {
    let _ = RUNTIME.set(runtime);
    let _ = POOL.set(pool);
}

pub fn init_pubsub(pubsub: std::sync::Arc<::queue::PubSub>) {
    let _ = PUBSUB.set(pubsub);
}

pub(crate) fn rt() -> &'static tokio::runtime::Runtime {
    RUNTIME.get().expect("runtime not initialized")
}

pub(crate) fn pool() -> &'static orm::DatabasePool {
    POOL.get().expect("pool not initialized")
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
        return Ok(Value::Number(serde_json::Number::from_f64(f).unwrap_or(0.into())));
    }
    if let Ok(s) = obj.extract::<String>() {
        return Ok(Value::String(s));
    }

    // Collections.
    if let Ok(list) = obj.downcast::<pyo3::types::PyList>() {
        let mut arr = Vec::new();
        for item in list.iter() {
            arr.push(py_to_json(&item)?);
        }
        return Ok(Value::Array(arr));
    }
    if let Ok(tuple) = obj.downcast::<pyo3::types::PyTuple>() {
        let mut arr = Vec::new();
        for item in tuple.iter() {
            arr.push(py_to_json(&item)?);
        }
        return Ok(Value::Array(arr));
    }
    if let Ok(dict) = obj.downcast::<pyo3::types::PyDict>() {
        let mut map = serde_json::Map::new();
        for (k, v) in dict {
            let key: String = k.extract()?;
            map.insert(key, py_to_json(&v)?);
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

    // Fallback: string representation.
    Ok(Value::String(obj.str()?.to_string()))
}

pub fn json_to_py(py: Python<'_>, val: &serde_json::Value) -> PyResult<PyObject> {
    match val {
        serde_json::Value::Null => Ok(py.None()),
        serde_json::Value::Bool(b) => Ok(b.into_pyobject(py)?.to_owned().unbind().into()),
        serde_json::Value::Number(n) if n.is_i64() => Ok(n.as_i64().unwrap().into_pyobject(py)?.unbind().into()),
        serde_json::Value::Number(n) if n.is_f64() => Ok(n.as_f64().unwrap().into_pyobject(py)?.unbind().into()),
        serde_json::Value::Number(n) => Ok(n.as_u64().unwrap().into_pyobject(py)?.unbind().into()),
        serde_json::Value::String(s) => Ok(s.as_str().into_pyobject(py)?.to_owned().unbind().into()),
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
pub fn call_method(method_path: &str, kwargs: &serde_json::Value) -> error::Result<serde_json::Value> {
    call_method_with_user(method_path, kwargs, None)
}

pub fn call_method_with_user(
    method_path: &str,
    kwargs: &serde_json::Value,
    user: Option<&str>,
) -> error::Result<serde_json::Value> {
    Python::with_gil(|py| {
        let parts: Vec<&str> = method_path.split('.').collect();
        if parts.len() < 2 {
            return Err(error::RuntimeError::Python("invalid method path".into()));
        }

        let func_name = parts.last().unwrap();
        let module_path = parts[..parts.len() - 1].join(".");

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
                let py_val = json_to_py(py, v)
                    .map_err(|e| error::RuntimeError::Python(format!("arg {} convert: {}", k, e)))?;
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
                let ctx_dict = py_kwargs.as_ref()
                    .map(|d| d.as_any().clone())
                    .unwrap_or_else(|| pyo3::types::PyDict::new(py).into_any());
                let _ = set_ctx.call1((ctx_dict, user.unwrap_or("Guest")));
            }
        }

        let module = py.import(module_path.as_str())
            .map_err(|e| error::RuntimeError::Python(format!("import {}: {}", module_path, e)))?;

        let func = module.getattr(*func_name)
            .map_err(|e| error::RuntimeError::Python(format!("getattr {}: {}", func_name, e)))?;

        // Filter kwargs to only parameters the function actually accepts,
        // unless the function declares **kwargs (VAR_KEYWORD parameter).
        let filtered_kwargs = if let Some(ref kw) = py_kwargs {
            let inspect = py.import("inspect")
                .ok()
                .and_then(|m| m.getattr("signature").ok())
                .and_then(|sig_fn| sig_fn.call1((&func,)).ok());

            if let Some(sig) = inspect {
                let accepts_var_keyword = sig
                    .getattr("parameters").ok()
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
                        .getattr("parameters").ok()
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
        }.map_err(|e| {
            // Extract a full Python traceback so the JS console / logs show the
            // real failure point instead of just the exception message.
            let detail = Python::with_gil(|py| {
                let traceback = py.import("traceback").ok()
                    .and_then(|tb| tb.getattr("format_exception").ok())
                    .and_then(|fmt| {
                        // format_exception(exception_type, exception_value, traceback)
                        let args = (
                            e.get_type(py),
                            e.clone_ref(py),
                            e.traceback(py),
                        );
                        fmt.call1(args).ok()
                    })
                    .and_then(|lines| py_to_json(&lines).ok())
                    .and_then(|v| v.as_array().cloned())
                    .map(|arr| arr.iter().map(|v| v.as_str().unwrap_or("").to_string()).collect::<String>())
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
        let response_json = py.import("frappe")
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

    let pool = rt.block_on(async {
        match db_driver {
            "postgres" => orm::DatabasePool::connect_postgres(db_url).await,
            _ => orm::DatabasePool::connect_sqlite(db_url).await,
        }
    }).map_err(|e| PyRuntimeError::new_err(format!("db: {}", e)))?;

    let _ = RUNTIME.set(rt);
    let _ = POOL.set(pool);
    Ok(())
}

#[pymodule]
fn kiff_core(m: &Bound<'_, PyModule>) -> PyResult<()> {
    m.add_function(wrap_pyfunction!(init_from_url, m)?)?;
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

    Ok(())
}

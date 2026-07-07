use crate::{json_to_py, py_to_json};
use pyo3::prelude::*;
use pyo3::types::PyDict;

/// Run a whitelisted DocType controller's ``onload`` method for an in-memory
/// document loaded by the native ORM.
///
/// The native ``getdoc`` path does not execute Python controller hooks.  For
/// framework DocTypes where the desk UI depends on ``__onload`` data (e.g.
/// ``User`` needs ``__onload.all_modules``), we instantiate the real Frappe
/// Document controller, call ``onload()``, and return the populated
/// ``__onload`` dict.
pub fn run_onload(
    doctype: &str,
    doc: &serde_json::Value,
    user: Option<&str>,
) -> error::Result<serde_json::Value> {
    Python::with_gil(|py| {
        let frappe = py
            .import("frappe")
            .map_err(|e| error::RuntimeError::Python(format!("import frappe: {}", e)))?;

        // Initialise a lightweight request context so controller code can read
        // frappe.session.user, frappe.db, etc.
        if let Ok(set_ctx) = frappe.getattr("_set_request_context") {
            let ctx = PyDict::new(py);
            let _ = set_ctx.call1((ctx, user.unwrap_or("Guest")));
        }

        let helper = frappe.getattr("_run_document_onload").map_err(|e| {
            error::RuntimeError::Python(format!("missing _run_document_onload: {}", e))
        })?;

        let py_doc = json_to_py(py, doc)
            .map_err(|e| error::RuntimeError::Python(format!("json_to_py: {}", e)))?;

        let kwargs = PyDict::new(py);
        kwargs
            .set_item("doctype", doctype)
            .map_err(|e| error::RuntimeError::Python(format!("set doctype: {}", e)))?;
        kwargs
            .set_item("doc", py_doc)
            .map_err(|e| error::RuntimeError::Python(format!("set doc: {}", e)))?;
        if let Some(u) = user {
            kwargs
                .set_item("user", u)
                .map_err(|e| error::RuntimeError::Python(format!("set user: {}", e)))?;
        }

        let result = helper
            .call((), Some(&kwargs))
            .map_err(|e| error::RuntimeError::Python(format!("run_onload {}: {}", doctype, e)))?;

        py_to_json(&result).map_err(|e| error::RuntimeError::Python(format!("py_to_json: {}", e)))
    })
}

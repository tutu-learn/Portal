use crate::{json_to_py, pool, py_to_json, rt};
use pyo3::prelude::*;
use pyo3::types::PyDict;

#[pyfunction]
pub fn enqueue(
    _py: Python<'_>,
    method: String,
    queue: String,
    kwargs: Bound<'_, PyAny>,
) -> PyResult<String> {
    let kwargs_map: std::collections::HashMap<String, serde_json::Value> =
        if let Ok(dict) = kwargs.downcast::<PyDict>() {
            let mut map = std::collections::HashMap::new();
            for (k, v) in dict {
                let key: String = k.extract()?;
                map.insert(key, py_to_json(&v)?);
            }
            map
        } else {
            std::collections::HashMap::new()
        };

    let job_id = uuid::Uuid::new_v4().to_string();
    let kwargs_json = serde_json::to_string(&kwargs_map)
        .map_err(|e| PyErr::new::<pyo3::exceptions::PyRuntimeError, _>(format!("{}", e)))?;

    let job_id_clone = job_id.clone();
    rt().block_on(async {
        let pool = pool();
        let sql = match pool.dialect() {
            "postgres" => r#"
                INSERT INTO __kiff_queue (id, method, queue, kwargs, status, site, created_at, updated_at)
                VALUES ($1, $2, $3, $4, 'queued', 'localhost', CURRENT_TIMESTAMP, CURRENT_TIMESTAMP)
            "#,
            _ => r#"
                INSERT INTO __kiff_queue (id, method, queue, kwargs, status, site, created_at, updated_at)
                VALUES (?, ?, ?, ?, 'queued', 'localhost', datetime('now'), datetime('now'))
            "#,
        };
        pool.execute_sql(sql, vec![
            serde_json::Value::String(job_id_clone.clone()),
            serde_json::Value::String(method),
            serde_json::Value::String(queue),
            serde_json::Value::String(kwargs_json),
        ]).await
            .map_err(|e| PyErr::new::<pyo3::exceptions::PyRuntimeError, _>(format!("{}", e)))?;
        Ok::<(), PyErr>(())
    }).map_err(|e: PyErr| e)?;

    Ok(job_id)
}

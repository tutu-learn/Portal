use crate::{json_to_py, pool, py_to_json, rt, values_from_py};
use orm::FilterCondition;
use pyo3::prelude::*;
use pyo3::types::{PyDict, PyList};
use std::collections::HashMap;

fn parse_filter_condition(val: serde_json::Value) -> FilterCondition {
    if let serde_json::Value::Array(ref arr) = val {
        if arr.len() == 2 {
            let op = arr[0].as_str().unwrap_or("").to_lowercase();
            let operand = arr[1].clone();
            return match op.as_str() {
                "="        => FilterCondition::Eq(operand),
                "!="       => FilterCondition::Ne(operand),
                ">"        => FilterCondition::Gt(operand),
                ">="       => FilterCondition::Gte(operand),
                "<"        => FilterCondition::Lt(operand),
                "<="       => FilterCondition::Lte(operand),
                "like"     => FilterCondition::Like(operand.as_str().unwrap_or("").to_string()),
                "not like" => FilterCondition::NotLike(operand.as_str().unwrap_or("").to_string()),
                "in" => {
                    let items = operand.as_array().cloned().unwrap_or_default();
                    FilterCondition::In(items)
                }
                "not in" => {
                    let items = operand.as_array().cloned().unwrap_or_default();
                    FilterCondition::NotIn(items)
                }
                "is" => match operand.as_str().unwrap_or("").to_lowercase().as_str() {
                    "set"     => FilterCondition::IsSet,
                    "not set" => FilterCondition::IsNotSet,
                    _         => FilterCondition::Eq(operand),
                },
                _ => FilterCondition::Eq(val),
            };
        }
    }
    FilterCondition::Eq(val)
}

#[pyfunction]
pub fn get_doc(py: Python<'_>, doctype: String, name: Option<String>) -> PyResult<PyObject> {
    let name = name.unwrap_or_default();
    let doc = rt()
        .block_on(async { pool().get_doc(&doctype, &name).await })
        .map_err(|e| PyErr::new::<pyo3::exceptions::PyRuntimeError, _>(format!("{}", e)))?;

    let dict = PyDict::new_bound(py);
    dict.set_item("doctype", doc.doctype)?;
    dict.set_item("name", doc.name)?;
    dict.set_item("owner", doc.owner)?;
    dict.set_item("creation", doc.creation.to_rfc3339())?;
    dict.set_item("modified", doc.modified.to_rfc3339())?;
    dict.set_item("docstatus", doc.docstatus)?;
    for (k, v) in &doc.fields {
        dict.set_item(k, json_to_py(py, v)?)?;
    }
    Ok(dict.into_py(py))
}

#[pyfunction]
pub fn get_list(
    py: Python<'_>,
    doctype: String,
    filters: Option<Bound<'_, PyAny>>,
    fields: Option<Bound<'_, PyAny>>,
    order_by: Option<String>,
    limit: Option<usize>,
) -> PyResult<PyObject> {
    let filters: Option<HashMap<String, FilterCondition>> = match filters {
        Some(f) if !f.is_none() => {
            let dict = f.downcast::<PyDict>()
                .map_err(|_| pyo3::exceptions::PyTypeError::new_err("filters must be a dict"))?;
            let mut map = HashMap::new();
            for (k, v) in dict {
                let key: String = k.extract()?;
                let json_val = py_to_json(&v)?;
                map.insert(key, parse_filter_condition(json_val));
            }
            Some(map)
        }
        _ => None,
    };

    let fields: Option<Vec<String>> = match fields {
        Some(f) if !f.is_none() => {
            if let Ok(list) = f.downcast::<PyList>() {
                let mut vec = Vec::new();
                for item in list.iter() {
                    vec.push(item.extract::<String>()?);
                }
                Some(vec)
            } else {
                Some(vec![f.extract::<String>()?])
            }
        }
        _ => None,
    };

    let docs = rt()
        .block_on(async {
            pool().get_list(&doctype, filters, fields, order_by.as_deref(), limit).await
        })
        .map_err(|e| PyErr::new::<pyo3::exceptions::PyRuntimeError, _>(format!("{}", e)))?;

    let list = PyList::empty_bound(py);
    for doc in docs {
        let dict = PyDict::new_bound(py);
        dict.set_item("doctype", doc.doctype)?;
        dict.set_item("name", doc.name)?;
        for (k, v) in &doc.fields {
            dict.set_item(k, json_to_py(py, v)?)?;
        }
        list.append(dict)?;
    }
    Ok(list.into_py(py))
}

#[pyfunction]
pub fn get_value(
    py: Python<'_>,
    doctype: String,
    filters: Bound<'_, PyAny>,
    fieldname: String,
) -> PyResult<PyObject> {
    let filters_map: HashMap<String, FilterCondition> = if let Ok(dict) = filters.downcast::<PyDict>() {
        let mut map = HashMap::new();
        for (k, v) in dict {
            let key: String = k.extract()?;
            map.insert(key, FilterCondition::Eq(py_to_json(&v)?));
        }
        map
    } else {
        let mut map = HashMap::new();
        let name: String = filters.extract()?;
        map.insert("name".into(), FilterCondition::Eq(serde_json::Value::String(name)));
        map
    };

    let docs = rt()
        .block_on(async {
            pool().get_list(&doctype, Some(filters_map), Some(vec![fieldname.clone()]), None, Some(1)).await
        })
        .map_err(|e| PyErr::new::<pyo3::exceptions::PyRuntimeError, _>(format!("{}", e)))?;

    if let Some(doc) = docs.into_iter().next() {
        if let Some(v) = doc.fields.get(&fieldname) {
            return json_to_py(py, v);
        }
    }
    Ok(py.None())
}

#[pyfunction]
pub fn db_sql(
    py: Python<'_>,
    query: String,
    values: Option<Bound<'_, PyAny>>,
) -> PyResult<PyObject> {
    let params = values_from_py(values)?;
    let rows = rt()
        .block_on(async { pool().execute_sql(&query, params).await })
        .map_err(|e| PyErr::new::<pyo3::exceptions::PyRuntimeError, _>(format!("{}", e)))?;

    let list = PyList::empty_bound(py);
    for row in rows {
        let dict = PyDict::new_bound(py);
        for (k, v) in row {
            dict.set_item(k, json_to_py(py, &v)?)?;
        }
        list.append(dict)?;
    }
    Ok(list.into_py(py))
}

#[pyfunction]
pub fn db_set_values(
    _py: Python<'_>,
    doctype: String,
    name: String,
    values: Bound<'_, PyDict>,
) -> PyResult<()> {
    let mut doc = rt()
        .block_on(async { pool().get_doc(&doctype, &name).await })
        .map_err(|e| PyErr::new::<pyo3::exceptions::PyRuntimeError, _>(format!("{}", e)))?;

    for (k, v) in values {
        let key: String = k.extract()?;
        let val = py_to_json(&v)?;
        doc.set_field(key, val);
    }

    rt()
        .block_on(async { pool().save_doc(&doc).await })
        .map_err(|e| PyErr::new::<pyo3::exceptions::PyRuntimeError, _>(format!("{}", e)))?;

    Ok(())
}

#[pyfunction]
pub fn db_exists(_py: Python<'_>, doctype: String, name: String) -> PyResult<bool> {
    rt()
        .block_on(async { pool().exists(&doctype, &name).await })
        .map_err(|e| PyErr::new::<pyo3::exceptions::PyRuntimeError, _>(format!("{}", e)))
}

#[pyfunction]
pub fn db_count(
    _py: Python<'_>,
    doctype: String,
    filters: Option<Bound<'_, PyAny>>,
) -> PyResult<usize> {
    let filters_map: Option<HashMap<String, serde_json::Value>> = match filters {
        Some(f) if !f.is_none() => {
            let dict = f.downcast::<PyDict>()
                .map_err(|_| pyo3::exceptions::PyTypeError::new_err("filters must be a dict"))?;
            let mut map = HashMap::new();
            for (k, v) in dict {
                let key: String = k.extract()?;
                map.insert(key, py_to_json(&v)?);
            }
            Some(map)
        }
        _ => None,
    };

    let tbl = table_name(&doctype);
    let dialect = pool().dialect().to_string();

    let (sql, params) = match filters_map {
        Some(ref fmap) if !fmap.is_empty() => {
            let mut conditions = Vec::new();
            let mut vals: Vec<serde_json::Value> = Vec::new();
            for (i, (k, v)) in fmap.iter().enumerate() {
                let ph = if dialect == "postgres" {
                    format!("${}", i + 1)
                } else {
                    "?".to_string()
                };
                conditions.push(format!("\"{}\" = {}", k, ph));
                vals.push(v.clone());
            }
            (
                format!("SELECT COUNT(*) as c FROM \"{}\" WHERE {}", tbl, conditions.join(" AND ")),
                vals,
            )
        }
        _ => (format!("SELECT COUNT(*) as c FROM \"{}\"", tbl), vec![]),
    };

    let rows = rt()
        .block_on(async { pool().execute_sql(&sql, params).await })
        .map_err(|e| PyErr::new::<pyo3::exceptions::PyRuntimeError, _>(format!("{}", e)))?;

    let count = rows
        .into_iter()
        .next()
        .and_then(|m| m.get("c").cloned())
        .and_then(|v| v.as_i64())
        .unwrap_or(0) as usize;

    Ok(count)
}

#[pyfunction]
pub fn save_doc(_py: Python<'_>, doctype: String, name: String, values: Bound<'_, PyDict>) -> PyResult<()> {
    let mut doc = rt()
        .block_on(async { pool().get_doc(&doctype, &name).await })
        .map_err(|e| PyErr::new::<pyo3::exceptions::PyRuntimeError, _>(format!("{}", e)))?;

    for (k, v) in values {
        let key: String = k.extract()?;
        let val = py_to_json(&v)?;
        doc.set_field(key, val);
    }

    rt()
        .block_on(async { pool().save_doc(&doc).await })
        .map_err(|e| PyErr::new::<pyo3::exceptions::PyRuntimeError, _>(format!("{}", e)))?;

    Ok(())
}

#[pyfunction]
pub fn insert_doc(_py: Python<'_>, doctype: String, values: Bound<'_, PyDict>) -> PyResult<String> {
    let mut doc = orm::Document::new(doctype, uuid::Uuid::new_v4().to_string());

    for (k, v) in values {
        let key: String = k.extract()?;
        let val = py_to_json(&v)?;
        doc.set_field(key, val);
    }

    let name = rt()
        .block_on(async { pool().insert_doc(&doc).await })
        .map_err(|e| PyErr::new::<pyo3::exceptions::PyRuntimeError, _>(format!("{}", e)))?;

    Ok(name)
}

#[pyfunction]
pub fn delete_doc(_py: Python<'_>, doctype: String, name: String) -> PyResult<()> {
    rt()
        .block_on(async { pool().delete_doc(&doctype, &name).await })
        .map_err(|e| PyErr::new::<pyo3::exceptions::PyRuntimeError, _>(format!("{}", e)))?;
    Ok(())
}

#[pyfunction]
pub fn db_commit(_py: Python<'_>) -> PyResult<()> {
    rt()
        .block_on(async { pool().commit().await })
        .map_err(|e| PyErr::new::<pyo3::exceptions::PyRuntimeError, _>(format!("{}", e)))?;
    Ok(())
}

#[pyfunction]
pub fn db_rollback(_py: Python<'_>) -> PyResult<()> {
    rt()
        .block_on(async { pool().rollback().await })
        .map_err(|e| PyErr::new::<pyo3::exceptions::PyRuntimeError, _>(format!("{}", e)))?;
    Ok(())
}

fn table_name(doctype: &str) -> String {
    let name = doctype.to_lowercase().replace(" ", "_");
    name.strip_prefix("tab").unwrap_or(&name).to_string()
}

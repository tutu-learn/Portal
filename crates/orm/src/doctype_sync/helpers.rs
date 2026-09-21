use serde_json::Value;

pub(crate) fn json_str(doc: &Value, key: &str) -> String {
    doc.get(key)
        .and_then(|v| v.as_str())
        .unwrap_or("")
        .to_string()
}

pub(crate) fn row_str(row: &std::collections::HashMap<String, Value>, key: &str) -> String {
    row.get(key)
        .and_then(|v| v.as_str())
        .unwrap_or("")
        .to_string()
}

pub(crate) fn json_str_or_null(doc: &Value, key: &str) -> Value {
    match doc.get(key) {
        Some(Value::String(s)) if !s.is_empty() => Value::String(s.clone()),
        _ => Value::Null,
    }
}

pub(crate) fn json_i64(doc: &Value, key: &str) -> i64 {
    doc.get(key)
        .and_then(|v| v.as_i64())
        .or_else(|| {
            doc.get(key)
                .and_then(|v| v.as_str())
                .and_then(|s| s.parse().ok())
        })
        .unwrap_or(0)
}

pub(crate) fn json_f64(doc: &Value, key: &str) -> f64 {
    doc.get(key)
        .and_then(|v| v.as_f64())
        .or_else(|| doc.get(key).and_then(|v| v.as_i64()).map(|i| i as f64))
        .or_else(|| {
            doc.get(key)
                .and_then(|v| v.as_str())
                .and_then(|s| s.parse().ok())
        })
        .unwrap_or(0.0)
}

pub(crate) fn val(s: String) -> Value {
    Value::String(s)
}

pub(crate) fn num(n: i64) -> Value {
    Value::Number(n.into())
}

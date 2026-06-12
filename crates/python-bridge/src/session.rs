use pyo3::prelude::*;

#[pyfunction]
pub fn get_roles(_py: Python<'_>, user: String) -> PyResult<Vec<String>> {
    // TODO: integrate with permissions crate
    if user == "Administrator" {
        Ok(vec!["Administrator".into(), "System Manager".into(), "All".into()])
    } else if user == "Guest" {
        Ok(vec!["Guest".into(), "All".into()])
    } else {
        Ok(vec!["All".into()])
    }
}

#[pyfunction]
pub fn has_permission(
    _py: Python<'_>,
    _doctype: String,
    _ptype: String,
    _doc: Option<Bound<'_, PyAny>>,
    _user: Option<String>,
) -> PyResult<bool> {
    // TODO: integrate with permissions crate
    Ok(true)
}

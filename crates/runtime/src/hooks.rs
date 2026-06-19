use pyo3::prelude::*;
use pyo3::types::PyDict;
use std::collections::HashMap;
use std::path::Path;
use tracing::{info, warn};

#[derive(Debug, Clone)]
pub struct HookRegistry {
    hooks: HashMap<String, Vec<String>>, // event -> list of "module.function"
}

impl HookRegistry {
    pub fn new() -> Self {
        Self {
            hooks: HashMap::new(),
        }
    }

    pub fn load_from_path(&mut self, app_path: &str) -> error::Result<()> {
        let hooks_path = Path::new(app_path).join("hooks.py");
        if !hooks_path.exists() {
            self.scan_for_hooks(app_path)?;
            return Ok(());
        }

        info!("loading hooks from {:?}", hooks_path);
        Python::with_gil(|py| -> error::Result<()> {
            let locals = PyDict::new(py);
            let code = std::fs::read_to_string(&hooks_path)
                .map_err(|e| error::RuntimeError::Io(e))?;
            let code_c = std::ffi::CString::new(code)
                .map_err(|e| error::RuntimeError::Python(format!("null byte in hooks.py: {}", e)))?;
            py.run(&code_c, None, Some(&locals))
                .map_err(|e| error::RuntimeError::Python(format!("{}", e)))?;

            let events = vec![
                "doc_events",
                "scheduler_events",
                "on_login",
                "on_logout",
                "on_session_creation",
            ];

            for event in events {
                if let Ok(value) = locals.get_item(event) {
                    if let Some(v) = value {
                        if let Ok(dict) = v.downcast::<PyDict>() {
                            for (k, val) in dict {
                                let key: String = k.extract()
                                    .map_err(|e| error::RuntimeError::Python(format!("{}", e)))?;
                                if let Ok(hook_list) = val.downcast::<pyo3::types::PyList>() {
                                    for item in hook_list.iter() {
                                        let hook: String = item.extract()
                                            .map_err(|e| error::RuntimeError::Python(format!("{}", e)))?;
                                        self.hooks.entry(format!("{}:{}", event, key))
                                            .or_default()
                                            .push(hook);
                                    }
                                } else if let Ok(hook) = val.extract::<String>() {
                                    self.hooks.entry(format!("{}:{}", event, key))
                                        .or_default()
                                        .push(hook);
                                }
                            }
                        }
                    }
                }
            }

            Ok(())
        })
    }

    fn scan_for_hooks(&mut self, base_path: &str) -> error::Result<()> {
        let base = Path::new(base_path);
        if !base.is_dir() {
            return Ok(());
        }

        for entry in std::fs::read_dir(base)? {
            let entry = entry?;
            let path = entry.path();
            if path.is_dir() {
                let hooks_file = path.join("hooks.py");
                if hooks_file.exists() {
                    if let Err(e) = self.load_from_path(path.to_str().unwrap_or("")) {
                        warn!("failed to load hooks from {:?}: {}", hooks_file, e);
                    }
                }
            }
        }
        Ok(())
    }

    pub fn get_hooks(&self, event: &str, doctype: Option<&str>) -> Vec<String> {
        let mut result = Vec::new();

        if let Some(dt) = doctype {
            let key = format!("{}:{}", event, dt);
            if let Some(hooks) = self.hooks.get(&key) {
                result.extend(hooks.clone());
            }

            let wildcard = format!("{}:*", event);
            if let Some(hooks) = self.hooks.get(&wildcard) {
                result.extend(hooks.clone());
            }
        } else {
            for (key, hooks) in &self.hooks {
                if key.starts_with(&format!("{}:", event)) {
                    result.extend(hooks.clone());
                }
            }
        }

        result
    }

    pub async fn run_hook(
        &self,
        event: &str,
        doctype: Option<&str>,
        doc: Option<&orm::Document>,
    ) -> error::Result<()> {
        let hooks = self.get_hooks(event, doctype);
        if hooks.is_empty() {
            return Ok(());
        }

        let doc_dict = doc.map(|d| {
            let mut map = std::collections::HashMap::new();
            map.insert("doctype".to_string(), serde_json::Value::String(d.doctype.clone()));
            map.insert("name".to_string(), serde_json::Value::String(d.name.clone()));
            for (k, v) in &d.fields {
                map.insert(k.clone(), v.clone());
            }
            map
        });

        for hook in hooks {
            let parts: Vec<&str> = hook.rsplitn(2, '.').collect();
            if parts.len() != 2 {
                tracing::warn!("invalid hook format: {}", hook);
                continue;
            }
            let func_name = parts[0].to_string();
            let module_name = parts[1].to_string();
            let hook_clone = hook.clone();

            info!("running hook {} for {}:{:?}", hook, event, doctype);

            let result: Result<Result<(), error::RuntimeError>, _> = tokio::task::spawn_blocking({
                let doc_dict = doc_dict.clone();
                move || {
                    Python::with_gil(|py| -> error::Result<()> {
                        let module = py.import(module_name.as_str())
                            .map_err(|e| error::RuntimeError::Python(format!("{}", e)))?;
                        let func = module.getattr(func_name.as_str())
                            .map_err(|e| error::RuntimeError::Python(format!("{}", e)))?;

                        if let Some(ref dict) = doc_dict {
                            let py_dict = pyo3::types::PyDict::new(py);
                            for (k, v) in dict {
                                let val = kiff_core::json_to_py(py, v)
                                    .map_err(|e| error::RuntimeError::Python(format!("{}", e)))?;
                                py_dict.set_item(k, val)
                                    .map_err(|e| error::RuntimeError::Python(format!("{}", e)))?;
                            }
                            func.call1((py_dict,))
                                .map_err(|e| error::RuntimeError::Python(format!("{}", e)))?;
                        } else {
                            func.call0()
                                .map_err(|e| error::RuntimeError::Python(format!("{}", e)))?;
                        }

                        Ok(())
                    })
                }
            }).await;

            match result {
                Ok(Ok(())) => {}
                Ok(Err(e)) => warn!("hook {} failed: {}", hook, e),
                Err(e) => warn!("hook {} panicked: {}", hook, e),
            }
        }

        Ok(())
    }
}

impl Default for HookRegistry {
    fn default() -> Self {
        Self::new()
    }
}

use error::Result;
use pyo3::prelude::*;
use pyo3::types::PyList;
use tracing::info;

/// Ask the embedded Python for its extension suffix (e.g. `.cpython-312-darwin.so`).
fn ext_suffix() -> String {
    Python::with_gil(|py| {
        py.import("sysconfig")
            .and_then(|m| m.call_method1("get_config_var", ("EXT_SUFFIX",)))
            .and_then(|v| v.extract::<String>())
            .unwrap_or_else(|_| ".cpython-312-darwin.so".to_string())
    })
}

fn find_kiff_core_dylib() -> Option<std::path::PathBuf> {
    use std::path::Path;

    let exe = std::env::current_exe().unwrap_or_default();
    let exe_dir = exe.parent().unwrap_or(Path::new("."));

    // Cargo places the cdylib artifact next to the executable in release builds,
    // but in dev builds it lives under target/<profile>/deps/. Check both.
    let candidates: Vec<std::path::PathBuf> = [
        "libkiff_core.dylib",
        "libkiff_core.so",
        "libkiff_core.dll",
    ]
    .iter()
    .flat_map(|name| [exe_dir.join(name), exe_dir.join("deps").join(name)])
    .collect();

    for candidate in candidates {
        if candidate.exists() {
            return Some(candidate);
        }
    }
    None
}

fn install_kiff_core_symlink(site_packages: &str) {
    use std::path::Path;

    let suffix = ext_suffix();
    let dest = Path::new(site_packages).join(format!("kiff_core{}", suffix));

    let Some(dylib) = find_kiff_core_dylib() else {
        let exe = std::env::current_exe().unwrap_or_default();
        info!(
            "kiff_core dylib not found next to executable ({}), skipping symlink",
            exe.display()
        );
        return;
    };

    let _ = std::fs::remove_file(&dest);
    match std::os::unix::fs::symlink(&dylib, &dest) {
        Ok(_) => info!("installed kiff_core symlink: {} → {}", dylib.display(), dest.display()),
        Err(e) => info!("failed to install kiff_core symlink: {}", e),
    }
}

pub fn setup_python_path(shim_path: &str, frappe_path: &str, erpnext_path: &str) -> Result<()> {
    setup_python_path_with_db(shim_path, frappe_path, erpnext_path, None, None)
}

pub fn setup_python_path_with_db(
    shim_path: &str,
    frappe_path: &str,
    erpnext_path: &str,
    db_driver: Option<&str>,
    db_url: Option<&str>,
) -> Result<()> {
    let shim = std::path::Path::new(shim_path)
        .canonicalize()
        .unwrap_or_else(|_| shim_path.into());
    let frappe = std::path::Path::new(frappe_path)
        .canonicalize()
        .unwrap_or_else(|_| frappe_path.into());

    info!(
        "setting up Python path: shim={}, frappe={}",
        shim.display(),
        frappe.display()
    );

    Python::with_gil(|py| {
        let sys = py.import("sys")?;
        let path_obj = sys.getattr("path")?;
        let path = path_obj.downcast::<PyList>()?;

        // Add venv site-packages via Python's own `site` module so we don't
        // hardcode the Python version number. Try VIRTUAL_ENV env var first,
        // then fall back to a `.venv` directory next to the working directory.
        let venv = std::env::var("VIRTUAL_ENV").ok().or_else(|| {
            let candidates = [
                std::path::PathBuf::from(".venv"),
                std::env::current_dir().unwrap_or_default().join(".venv"),
            ];
            candidates.iter()
                .find(|p| p.join("bin/python3").exists() || p.join("bin/python").exists())
                .and_then(|p| p.canonicalize().ok())
                .and_then(|p| p.to_str().map(|s| s.to_string()))
        });

        if let Some(venv_root) = venv {
            // Embedded Python (e.g. PyO3) is not started from the venv's python
            // wrapper, so site.getsitepackages() returns system paths. Compute
            // the venv site-packages directly from sys.version_info.
            let version_info: (i32, i32, i32, String, i32) =
                sys.getattr("version_info")?.extract()?;
            let direct_sp = std::path::Path::new(&venv_root)
                .join(format!("lib/python{}.{}", version_info.0, version_info.1))
                .join("site-packages");

            let added = if direct_sp.exists() {
                let sp = direct_sp.to_string_lossy();
                info!("adding venv site-packages: {}", sp);
                path.insert(0, sp.as_ref())?;
                install_kiff_core_symlink(&sp);
                true
            } else {
                // Fallback: try site.getsitepackages() in case we're running
                // from a venv-aware interpreter.
                let site_pkgs: Vec<String> = py.import("site")
                    .and_then(|m| m.call_method0("getsitepackages"))
                    .and_then(|v| v.extract())
                    .unwrap_or_default();

                let mut found = false;
                for sp in &site_pkgs {
                    if sp.starts_with(&venv_root) && std::path::Path::new(sp).exists() {
                        info!("adding venv site-packages: {}", sp);
                        path.insert(0, sp.as_str())?;
                        install_kiff_core_symlink(sp);
                        found = true;
                        break;
                    }
                }
                found
            };

            if !added {
                info!("venv site-packages not found in {}", venv_root);
            }
        }

        // frappe framework at position 0 so its submodules resolve
        path.insert(0, frappe.to_str().unwrap_or(frappe_path))?;
        // shim first so `import frappe` resolves to our kiff_core bridge
        path.insert(0, shim.to_str().unwrap_or(shim_path))?;

        // Only append erpnext_path when it's a non-empty path that actually exists
        if !erpnext_path.is_empty() {
            if let Ok(erpnext) = std::path::Path::new(erpnext_path).canonicalize() {
                info!("adding erpnext path: {}", erpnext.display());
                path.append(erpnext.to_str().unwrap_or(erpnext_path))?;
            } else {
                info!("erpnext_path '{}' does not exist, skipping", erpnext_path);
            }
        }

        // Initialize kiff_core .so instance with DB connection so Python can use it.
        if let (Some(driver), Some(url)) = (db_driver, db_url) {
            match py.import("kiff_core") {
                Ok(kc) => match kc.call_method1("init_from_url", (driver, url)) {
                    Ok(_) => info!("kiff_core .so instance initialized"),
                    Err(e) => info!("kiff_core init_from_url failed: {}", e),
                },
                Err(e) => info!(
                    "kiff_core not importable from Python ({}), DB calls will be stubbed",
                    e
                ),
            }
        }

        Ok(())
    })
    .map_err(|e: PyErr| error::RuntimeError::Python(format!("{}", e)))
}

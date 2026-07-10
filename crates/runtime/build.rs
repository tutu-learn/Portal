use std::env;
use std::fs;
use std::path::{Path, PathBuf};

/// Config read from `<workspace_root>/rust_apps/apps.json`.
#[derive(Debug, serde::Deserialize)]
struct AppsConfig {
    #[serde(default)]
    apps: Vec<String>,
}

fn main() {
    let manifest_dir = PathBuf::from(env::var("CARGO_MANIFEST_DIR").expect("CARGO_MANIFEST_DIR"));
    let workspace_root = manifest_dir
        .parent()
        .expect("manifest dir has a parent")
        .parent()
        .expect("manifest dir has a grandparent")
        .to_path_buf();

    let apps_json = workspace_root.join("rust_apps/apps.json");
    println!("cargo:rerun-if-changed={}", apps_json.display());

    // If the config is missing or malformed we fall back to an empty list.
    // `kiff_logger` is appended unconditionally by `rust_apps::load_registry`,
    // so the runtime still has its core logging app available.
    let apps = read_apps(&apps_json).unwrap_or_default();
    let generated = generate_registered_apps(&apps);

    let out_path = manifest_dir.join("src/registered_apps.rs");
    write_if_changed(&out_path, &generated);

    // Keep Cargo.toml manifests in sync with apps.json so adding/removing an
    // app only requires editing the JSON config. Directory names may use mixed
    // case on case-insensitive filesystems, so resolve the real name.
    let app_dirs = resolve_app_dirs(&workspace_root, &apps);
    let runtime_changed = sync_runtime_cargo_toml(&manifest_dir, &app_dirs);
    let workspace_changed = sync_workspace_cargo_toml(&workspace_root, &app_dirs);

    if runtime_changed || workspace_changed {
        println!(
            "cargo:warning=apps.json changed; Cargo.toml manifests were updated. Run cargo again to pick up the new app crates."
        );
    }
}

/// Map each app name from apps.json to the actual directory name under
/// `rust_apps/`. This avoids duplicate-path errors on case-insensitive
/// filesystems when the JSON name and directory casing differ.
fn resolve_app_dirs(workspace_root: &Path, apps: &[String]) -> Vec<(String, String)> {
    let rust_apps_dir = workspace_root.join("rust_apps");
    let entries: Vec<String> = fs::read_dir(&rust_apps_dir)
        .expect("read rust_apps directory")
        .filter_map(|entry| {
            let entry = entry.ok()?;
            let name = entry.file_name().into_string().ok()?;
            if entry.file_type().ok()?.is_dir() {
                Some(name)
            } else {
                None
            }
        })
        .collect();

    apps.iter()
        .map(|app| {
            let dir = entries
                .iter()
                .find(|entry| entry.to_lowercase() == app.to_lowercase())
                .cloned()
                .unwrap_or_else(|| app.clone());
            (app.clone(), dir)
        })
        .collect()
}

fn read_apps(path: &Path) -> Result<Vec<String>, Box<dyn std::error::Error>> {
    let content = fs::read_to_string(path)?;
    let config: AppsConfig = serde_json::from_str(&content)?;
    Ok(config.apps)
}

fn generate_registered_apps(apps: &[String]) -> String {
    let mut lines = Vec::new();
    lines.push("//! Static list of Rust Frappe apps to load at startup.".to_string());
    lines.push("//!".to_string());
    lines.push("//! This file is generated at build time by `crates/runtime/build.rs` from".to_string());
    lines.push("//! `rust_apps/apps.json`. Do not edit it by hand; update the JSON config".to_string());
    lines.push("//! instead.".to_string());
    lines.push("//!".to_string());
    lines.push("//! If `rust_apps/apps.json` is missing or malformed the generated list falls".to_string());
    lines.push("//! back to empty, so only `kiff_logger` (appended by `rust_apps.rs`) is loaded.".to_string());
    lines.push(String::new());
    lines.push("use rust_apps_core::RustApp;".to_string());
    lines.push(String::new());
    lines.push("pub fn registered_apps() -> Vec<Box<dyn RustApp>> {".to_string());
    lines.push("    vec![".to_string());

    for app in apps {
        let type_name = format!("{}App", to_pascal_case(app));
        lines.push(format!("        Box::new({app}::{type_name}),"));
    }

    lines.push("    ]".to_string());
    lines.push("}".to_string());
    lines.join("\n") + "\n"
}

/// Update `crates/runtime/Cargo.toml` so every app in `apps.json` has a path
/// dependency and stale app dependencies are removed. Returns `true` if the
/// file was modified.
fn sync_runtime_cargo_toml(manifest_dir: &Path, app_dirs: &[(String, String)]) -> bool {
    let path = manifest_dir.join("Cargo.toml");
    let content = fs::read_to_string(&path).expect("read runtime Cargo.toml");
    let mut doc = content
        .parse::<toml_edit::DocumentMut>()
        .expect("parse runtime Cargo.toml");

    let deps = doc["dependencies"]
        .as_table_mut()
        .expect("runtime Cargo.toml must have a [dependencies] table");

    let app_names: Vec<String> = app_dirs.iter().map(|(app, _)| app.clone()).collect();

    // Remove stale app dependencies (path points into rust_apps/<crate>).
    let to_remove: Vec<String> = deps
        .iter()
        .filter_map(|(key, value)| {
            if is_app_dependency(key, value) && !app_names.contains(&key.to_string()) {
                Some(key.to_string())
            } else {
                None
            }
        })
        .collect();
    for key in to_remove {
        deps.remove(&key);
    }

    // Add missing app dependencies using the actual directory casing.
    for (app, dir) in app_dirs {
        if !deps.contains_key(app) {
            let mut dep = toml_edit::InlineTable::new();
            dep.insert(
                "path",
                toml_edit::Value::String(toml_edit::Formatted::new(format!(
                    "../../rust_apps/{dir}"
                ))),
            );
            deps.insert(app, toml_edit::Item::Value(toml_edit::Value::InlineTable(dep)));
        }
    }

    write_if_changed(&path, &doc.to_string())
}

/// Update the root `Cargo.toml` workspace members so every app in `apps.json`
/// is listed under `rust_apps/<app>` and stale app members are removed. Returns
/// `true` if the file was modified.
fn sync_workspace_cargo_toml(workspace_root: &Path, app_dirs: &[(String, String)]) -> bool {
    let path = workspace_root.join("Cargo.toml");
    let content = fs::read_to_string(&path).expect("read workspace Cargo.toml");
    let mut doc = content
        .parse::<toml_edit::DocumentMut>()
        .expect("parse workspace Cargo.toml");

    let app_dirs_lookup: std::collections::HashSet<String> = app_dirs
        .iter()
        .map(|(_, dir)| format!("rust_apps/{dir}"))
        .collect();

    // Modify the array inside a block so the mutable borrow ends before we
    // render the document.
    let member_values: Vec<String> = {
        let members = doc["workspace"]["members"]
            .as_array_mut()
            .expect("workspace Cargo.toml must have a workspace.members array");

        // Remove stale app members. `rust_apps/core` is the shared core crate
        // and is managed manually, not via apps.json.
        let to_remove: Vec<String> = members
            .iter()
            .filter_map(|value| value.as_str().map(String::from))
            .filter(|entry| {
                entry.starts_with("rust_apps/")
                    && *entry != "rust_apps/core"
                    && !app_dirs_lookup.contains(entry)
            })
            .collect();
        for entry in to_remove {
            members.retain(|value| value.as_str() != Some(&entry));
        }

        // Add missing app members, preserving apps.json order after core.
        for (_, dir) in app_dirs {
            let entry = format!("rust_apps/{dir}");
            let exists = members.iter().any(|value| value.as_str() == Some(&entry));
            if !exists {
                members.push(entry);
            }
        }

        members
            .iter()
            .filter_map(|value| value.as_str().map(|s| format!("    \"{s}\",")))
            .collect()
    };

    let mut rendered = doc.to_string();
    let formatted = format!("members = [\n{}\n]", member_values.join("\n"));
    rendered = replace_table_array(&rendered, "members", &formatted);

    write_if_changed(&path, &rendered)
}

/// Replace the serialized form of `key = [ ... ]` in `text` with `replacement`,
/// preserving everything else in the TOML document.
fn replace_table_array(text: &str, key: &str, replacement: &str) -> String {
    let mut result = String::new();
    let mut in_target = false;
    let mut bracket_depth = 0;
    let mut started = false;

    for line in text.lines() {
        if !started && line.trim_start().starts_with(&format!("{key} = [")) {
            result.push_str(replacement);
            result.push('\n');
            in_target = true;
            started = true;
            bracket_depth = 1;
            continue;
        }

        if in_target {
            bracket_depth += line.chars().filter(|&c| c == '[').count() as i32;
            bracket_depth -= line.chars().filter(|&c| c == ']').count() as i32;
            if bracket_depth <= 0 {
                in_target = false;
            }
            continue;
        }

        result.push_str(line);
        result.push('\n');
    }

    result
}

fn is_app_dependency(key: &str, value: &toml_edit::Item) -> bool {
    if let Some(table) = value.as_inline_table() {
        if let Some(path) = table.get("path").and_then(|v| v.as_str()) {
            if let Some(dir) = path.strip_prefix("../../rust_apps/") {
                return dir.to_lowercase() == key.to_lowercase();
            }
        }
    }
    false
}

fn write_if_changed(path: &Path, contents: &str) -> bool {
    if let Ok(existing) = fs::read_to_string(path) {
        if existing == contents {
            return false;
        }
    }
    fs::write(path, contents).expect("failed to write file");
    true
}

fn to_pascal_case(s: &str) -> String {
    s.split('_')
        .map(|word| {
            let mut chars = word.chars();
            match chars.next() {
                Some(first) => {
                    first.to_uppercase().collect::<String>() + &chars.as_str().to_lowercase()
                }
                None => String::new(),
            }
        })
        .collect()
}

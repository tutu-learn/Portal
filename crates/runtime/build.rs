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

    // Keep Cargo.toml manifests in sync with apps.json so adding/removing an
    // app only requires editing the JSON config. Directory names may use mixed
    // case on case-insensitive filesystems, so resolve the real name.
    // Apps listed in apps.json but not present under rust_apps/ are skipped so
    // the main repo can build independently of any specific app checkout.
    let app_dirs = resolve_app_dirs(&workspace_root, &apps);
    for app in apps.iter().filter(|a| !app_dirs.iter().any(|(name, _)| name == *a)) {
        println!(
            "cargo:warning=rust_apps/{} is configured but not checked out; skipping",
            app
        );
    }

    // Only register apps whose path dependency is already present in the
    // runtime manifest for the current cargo invocation. Missing dependencies
    // are added below, but cargo will not pick them up until the next build.
    // This lets a fresh checkout compile successfully on the first run.
    let registered_apps: Vec<String> = app_dirs
        .iter()
        .filter(|(app, _dir)| runtime_has_app_dependency(&manifest_dir, app))
        .map(|(app, _dir)| app.clone())
        .collect();
    let generated = generate_registered_apps(&registered_apps);

    let out_dir = PathBuf::from(env::var("OUT_DIR").expect("OUT_DIR"));
    let out_path = out_dir.join("registered_apps.rs");
    write_if_changed(&out_path, &generated);

    let app_members = resolve_app_workspace_members(&workspace_root, &apps);
    let runtime_changed = sync_runtime_cargo_toml(&manifest_dir, &app_dirs);
    let workspace_changed = sync_workspace_cargo_toml(&workspace_root, &app_members);

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
        .filter_map(|app| {
            entries
                .iter()
                .find(|entry| entry.to_lowercase() == app.to_lowercase())
                .cloned()
                .map(|dir| (app.clone(), dir))
        })
        .collect()
}

/// Return all workspace member paths for configured apps that are actually
/// checked out. This includes the app crate itself plus any nested crate
/// directories directly underneath it that contain a `Cargo.toml` (e.g.
/// `rust_apps/strongroom/tauri`).
fn resolve_app_workspace_members(workspace_root: &Path, apps: &[String]) -> Vec<String> {
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

    let mut members = Vec::new();
    for app in apps {
        let dir = match entries
            .iter()
            .find(|entry| entry.to_lowercase() == app.to_lowercase())
        {
            Some(dir) => dir,
            None => continue,
        };

        members.push(format!("rust_apps/{dir}"));

        let app_dir = rust_apps_dir.join(dir);
        let nested: Vec<String> = match fs::read_dir(&app_dir) {
            Ok(it) => it
                .filter_map(|entry| {
                    let entry = entry.ok()?;
                    if !entry.file_type().ok()?.is_dir() {
                        return None;
                    }
                    let name = entry.file_name().into_string().ok()?;
                    let cargo_toml = entry.path().join("Cargo.toml");
                    if !cargo_toml.is_file() {
                        return None;
                    }
                    // Skip nested crates that declare their own `[workspace]`;
                    // they are built independently and must not be pulled into
                    // the main monorepo workspace.
                    if let Ok(content) = fs::read_to_string(&cargo_toml) {
                        if let Ok(doc) = content.parse::<toml_edit::DocumentMut>() {
                            if doc.get("workspace").is_some() {
                                return None;
                            }
                        }
                    }
                    Some(format!("rust_apps/{dir}/{name}"))
                })
                .collect(),
            Err(_) => continue,
        };
        members.extend(nested);
    }
    members
}

fn read_apps(path: &Path) -> Result<Vec<String>, Box<dyn std::error::Error>> {
    let content = fs::read_to_string(path)?;
    let config: AppsConfig = serde_json::from_str(&content)?;
    Ok(config.apps)
}

fn generate_registered_apps(apps: &[String]) -> String {
    let mut lines = Vec::new();
    lines.push("// Static list of Rust Frappe apps to load at startup.".to_string());
    lines.push("//".to_string());
    lines.push(
        "// This file is generated at build time by `crates/runtime/build.rs` from".to_string(),
    );
    lines.push(
        "// `rust_apps/apps.json`. Do not edit it by hand; update the JSON config".to_string(),
    );
    lines.push("// instead.".to_string());
    lines.push("//".to_string());
    lines.push(
        "// If `rust_apps/apps.json` is missing or malformed the generated list falls".to_string(),
    );
    lines.push(
        "// back to empty, so only `kiff_logger` (appended by `rust_apps.rs`) is loaded."
            .to_string(),
    );
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
            deps.insert(
                app,
                toml_edit::Item::Value(toml_edit::Value::InlineTable(dep)),
            );
        }
    }

    write_if_changed(&path, &doc.to_string())
}

/// Update the root `Cargo.toml` workspace members so every configured app (and
/// any nested crates it contains) is listed, and stale app members are removed.
/// Returns `true` if the file was modified.
fn sync_workspace_cargo_toml(workspace_root: &Path, app_members: &[String]) -> bool {
    let path = workspace_root.join("Cargo.toml");
    let content = fs::read_to_string(&path).expect("read workspace Cargo.toml");
    let mut doc = content
        .parse::<toml_edit::DocumentMut>()
        .expect("parse workspace Cargo.toml");

    let app_members_lookup: std::collections::HashSet<String> =
        app_members.iter().cloned().collect();

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
                    && !app_members_lookup.contains(entry)
            })
            .collect();
        for entry in to_remove {
            members.retain(|value| value.as_str() != Some(&entry));
        }

        // Add missing app members, preserving apps.json order after core.
        for entry in app_members {
            let exists = members.iter().any(|value| value.as_str() == Some(entry));
            if !exists {
                members.push(entry.clone());
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

/// Returns true if `crates/runtime/Cargo.toml` already declares `app` as a path
/// dependency. We use this to decide whether it is safe to register the app in
/// the generated code for the current cargo invocation.
fn runtime_has_app_dependency(manifest_dir: &Path, app: &str) -> bool {
    let path = manifest_dir.join("Cargo.toml");
    let content = fs::read_to_string(&path).expect("read runtime Cargo.toml");
    let doc = content
        .parse::<toml_edit::DocumentMut>()
        .expect("parse runtime Cargo.toml");
    if let Some(deps) = doc["dependencies"].as_table() {
        if let Some(value) = deps.get(app) {
            return is_app_dependency(app, value);
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

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
    lines.push("//! If `rust_apps/apps.json` is missing or malformed, the generated list falls".to_string());
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

fn write_if_changed(path: &Path, contents: &str) {
    if let Ok(existing) = fs::read_to_string(path) {
        if existing == contents {
            return;
        }
    }
    fs::write(path, contents).expect("failed to write registered_apps.rs");
}

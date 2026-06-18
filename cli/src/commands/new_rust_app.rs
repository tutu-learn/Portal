use std::fs;
use std::io::Write;
use std::path::Path;
use tracing::info;

const CARGO_TOML_TEMPLATE: &str = r#"[package]
name = "{name}"
version = "0.1.0"
edition = "2021"

[dependencies]
rust_apps_core = { path = "../core" }
error          = { workspace = true }
serde          = { workspace = true }
serde_json     = { workspace = true }
axum           = { workspace = true }
tokio          = { workspace = true }
tracing        = { workspace = true }
async-trait    = "0.1"
"#;

const LIB_RS_TEMPLATE: &str = r#"//! Rust Frappe app: {pascal_name}

use axum::{extract::State, routing::get, Json, Router};
use rust_apps_core::{ApiMethod, AppContext, DoctypeFixture, RustApp};
use serde_json::Value;

pub struct {pascal_name}App;

#[async_trait::async_trait]
impl RustApp for {pascal_name}App {
    fn name(&self) -> &'static str {
        "{name}"
    }

    fn version(&self) -> &'static str {
        "0.1.0"
    }

    fn doctypes(&self) -> Vec<DoctypeFixture> {
        vec![DoctypeFixture::new(
            "{pascal_name}",
            "{pascal_name} DocType",
            include_str!("doctypes/{name}/{name}.json"),
        )]
    }

    fn routes(
        &self,
        _ctx: &AppContext,
        router: Router<rust_apps_core::AppState>,
    ) -> Router<rust_apps_core::AppState> {
        router.route("/{name}/health", get(health_handler))
    }

    fn api_methods(&self) -> Vec<ApiMethod> {
        vec![ApiMethod::new("{name}.hello", |ctx, params| async move {
            let name = params
                .get("name")
                .and_then(|v| v.as_str())
                .unwrap_or("world");
            Ok(Value::Object(serde_json::Map::from_iter([(
                "message".to_string(),
                Value::String(format!("Hello from {}: {}", ctx.app_name, name)),
            )])))
        })]
    }

    async fn on_startup(&self, ctx: &AppContext) -> error::Result<()> {
        tracing::info!("{} v{} starting up", ctx.app_name, self.version());
        Ok(())
    }
}

async fn health_handler(State(_state): State<rust_apps_core::AppState>) -> Json<Value> {
    Json(serde_json::json!({
        "status": "ok",
        "app": "{name}"
    }))
}
"#;

const DOCTYPE_JSON_TEMPLATE: &str = r#"{
    "name": "{pascal_name} DocType",
    "doctype": "DocType",
    "module": "{pascal_name}",
    "istable": 0,
    "issingle": 0,
    "is_submittable": 0,
    "track_changes": 1,
    "fields": [
        {
            "fieldname": "title",
            "fieldtype": "Data",
            "label": "Title",
            "reqd": 1,
            "in_list_view": 1
        }
    ],
    "permissions": [
        {
            "role": "System Manager",
            "read": 1,
            "write": 1,
            "create": 1,
            "delete": 1
        }
    ]
}
"#;

pub async fn run(name: &str) -> error::Result<()> {
    let pascal_name = to_pascal_case(name);
    let app_dir = format!("rust_apps/{}", name);

    if Path::new(&app_dir).exists() {
        return Err(error::RuntimeError::Io(std::io::Error::new(
            std::io::ErrorKind::AlreadyExists,
            format!("app directory {} already exists", app_dir),
        )));
    }

    fs::create_dir_all(format!("{}/src/doctypes/{}", app_dir, name))?;

    let cargo_toml = CARGO_TOML_TEMPLATE.replace("{name}", name);
    write_file(&format!("{}/Cargo.toml", app_dir), &cargo_toml)?;

    let lib_rs = LIB_RS_TEMPLATE
        .replace("{name}", name)
        .replace("{pascal_name}", &pascal_name);
    write_file(&format!("{}/src/lib.rs", app_dir), &lib_rs)?;

    let doctype_json = DOCTYPE_JSON_TEMPLATE.replace("{pascal_name}", &pascal_name);
    write_file(
        &format!("{}/src/doctypes/{}/{}.json", app_dir, name, name),
        &doctype_json,
    )?;

    add_to_workspace(name)?;
    add_to_registry(name, &pascal_name)?;

    info!("created rust app '{}' at {}", name, app_dir);
    println!(
        "Rust app '{}' created. Run `cargo check -p {}` to verify.",
        name, name
    );
    Ok(())
}

fn write_file(path: &str, contents: &str) -> error::Result<()> {
    let mut file = fs::File::create(path)?;
    file.write_all(contents.as_bytes())?;
    Ok(())
}

fn add_to_workspace(name: &str) -> error::Result<()> {
    let path = "Cargo.toml";
    let contents = fs::read_to_string(path)?;
    let member_line = format!("    \"rust_apps/{name}\",");

    if contents.contains(&member_line) {
        return Ok(());
    }

    let new_contents = if contents.contains("    \"rust_apps/sample\",\n") {
        contents.replacen(
            "    \"rust_apps/sample\",\n",
            &format!("    \"rust_apps/sample\",\n{member_line}\n"),
            1,
        )
    } else {
        contents.replacen(
            "    \"rust_apps/core\",\n",
            &format!("    \"rust_apps/core\",\n{member_line}\n"),
            1,
        )
    };
    fs::write(path, new_contents)?;
    Ok(())
}

fn add_to_registry(name: &str, pascal_name: &str) -> error::Result<()> {
    let path = "crates/runtime/src/registered_apps.rs";
    let contents = fs::read_to_string(path)?;
    let app_line = format!("        Box::new({name}::{pascal_name}App),");

    if contents.contains(&app_line) {
        return Ok(());
    }

    let new_contents = if contents.contains("        Box::new(sample::SampleApp),\n") {
        contents.replacen(
            "        Box::new(sample::SampleApp),\n",
            &format!("        Box::new(sample::SampleApp),\n{app_line}\n"),
            1,
        )
    } else {
        contents.replacen(
            "    vec![\n",
            &format!("    vec![\n{app_line}\n"),
            1,
        )
    };
    fs::write(path, new_contents)?;
    Ok(())
}

fn to_pascal_case(s: &str) -> String {
    s.split('_')
        .map(|word| {
            let mut chars = word.chars();
            match chars.next() {
                Some(first) => first.to_uppercase().collect::<String>() + &chars.as_str().to_lowercase(),
                None => String::new(),
            }
        })
        .collect()
}

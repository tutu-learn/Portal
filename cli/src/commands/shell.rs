use tracing::info;

pub async fn run() -> error::Result<()> {
    info!("starting Python REPL with frappe shim");
    let py = std::process::Command::new("python3")
        .args([
            "-c",
            r#"
import sys
sys.path.insert(0, './python')
import frappe
print("Frappe shim loaded. frappe.local.site =", frappe.local.site)
"#,
        ])
        .status()?;
    if !py.success() {
        return Err(error::RuntimeError::Io(std::io::Error::new(
            std::io::ErrorKind::Other,
            "python shell failed",
        )));
    }
    Ok(())
}

use std::io::{self, Write};

pub async fn run() -> error::Result<()> {
    println!("Welcome to Kiff! Let's create your runtime.toml.");

    print!("ERPNext path [/home/user/frappe-bench/apps/erpnext]: ");
    io::stdout().flush()?;
    let mut erpnext = String::new();
    io::stdin().read_line(&mut erpnext)?;
    let erpnext = erpnext.trim();
    let erpnext = if erpnext.is_empty() {
        "/home/user/frappe-bench/apps/erpnext"
    } else {
        erpnext
    };

    print!("Sites path [./sites]: ");
    io::stdout().flush()?;
    let mut sites = String::new();
    io::stdin().read_line(&mut sites)?;
    let sites = sites.trim();
    let sites = if sites.is_empty() { "./sites" } else { sites };

    let config = format!(
        r#"[runtime]
erpnext_path = "{}"
shim_path    = "./python"
sites_path   = "{}"

[database]
driver = "sqlite"
url    = "./sites/{{site}}/site.db"

[server]
host    = "0.0.0.0"
port    = 8000
workers = 4

[queue]
short_workers   = 2
default_workers = 2
long_workers    = 1
"#,
        erpnext, sites
    );

    std::fs::write("runtime.toml", config)?;
    println!("Created runtime.toml");
    Ok(())
}

use tracing::info;

pub async fn run() -> error::Result<()> {
    info!("Starting anri runtime...");
    // The real runtime binary is anri-runtime; this is a convenience wrapper.
    let status = tokio::process::Command::new("cargo")
        .args([
            "run",
            "--package",
            "runtime",
            "--bin",
            "anri-runtime",
            "--release",
        ])
        .status()
        .await?;
    if !status.success() {
        return Err(error::RuntimeError::Io(std::io::Error::new(
            std::io::ErrorKind::Other,
            "runtime exited with error",
        )));
    }
    Ok(())
}

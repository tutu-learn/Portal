use tracing::info;

pub async fn run() -> error::Result<()> {
    info!("Starting kiff runtime...");
    // The real runtime binary is kiff-runtime; this is a convenience wrapper.
    let status = tokio::process::Command::new("cargo")
        .args(["run", "--bin", "kiff-runtime", "--release"])
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

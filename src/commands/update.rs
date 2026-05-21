#![allow(clippy::missing_errors_doc)]

use anyhow::Result;

pub fn update() -> Result<()> {
    let status = std::process::Command::new("cargo")
        .args(["install", "stmo-cli"])
        .status()?;

    if status.success() {
        println!("stmo-cli updated successfully.");
        Ok(())
    } else {
        anyhow::bail!("cargo install stmo-cli failed");
    }
}

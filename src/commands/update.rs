#![allow(clippy::missing_errors_doc)]

use anyhow::Result;

pub fn update() -> Result<()> {
    let status = std::process::Command::new("cargo")
        .args(["binstall", "--no-confirm", "stmo-cli"])
        .status();

    match status {
        Ok(s) if s.success() => {
            println!("stmo-cli updated successfully.");
            return Ok(());
        }
        _ => eprintln!("cargo binstall not available, falling back to cargo install"),
    }

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

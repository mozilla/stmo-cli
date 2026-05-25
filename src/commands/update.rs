#![allow(clippy::missing_errors_doc)]

use anyhow::Result;
use std::fs;
use std::path::{Path, PathBuf};

fn cargo_home() -> Option<PathBuf> {
    std::env::var("CARGO_HOME")
        .ok()
        .map(PathBuf::from)
        .or_else(|| {
            std::env::var("HOME")
                .ok()
                .map(|h| PathBuf::from(h).join(".cargo"))
        })
}

fn dir_writable(dir: &Path) -> bool {
    let test_path = dir.join(".stmo-cli-write-test");
    match fs::write(&test_path, b"") {
        Ok(()) => {
            let _ = fs::remove_file(&test_path);
            true
        }
        Err(e) => e.kind() != std::io::ErrorKind::PermissionDenied,
    }
}

fn cargo_writable() -> bool {
    let Some(home) = cargo_home() else {
        return true;
    };
    if !home.exists() {
        return true;
    }
    dir_writable(&home)
}

pub fn update() -> Result<()> {
    if !cargo_writable() {
        anyhow::bail!(
            "Cannot update stmo-cli in the current environment \
             (write access to ~/.cargo/ is restricted).\n\
             Run this command outside the sandbox:\n  cargo binstall stmo-cli"
        );
    }

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

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    #[test]
    fn test_dir_writable_on_writable_dir() {
        let temp = TempDir::new().unwrap();
        assert!(dir_writable(temp.path()));
    }

    #[cfg(unix)]
    #[test]
    fn test_dir_writable_on_readonly_dir() {
        use std::os::unix::fs::PermissionsExt;
        let temp = TempDir::new().unwrap();
        fs::set_permissions(temp.path(), fs::Permissions::from_mode(0o555)).unwrap();
        let result = dir_writable(temp.path());
        fs::set_permissions(temp.path(), fs::Permissions::from_mode(0o755)).unwrap();
        assert!(!result);
    }
}

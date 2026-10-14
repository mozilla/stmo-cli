#![allow(clippy::missing_errors_doc)]

use anyhow::Result;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

const BINSTALL_ARGS: &[&str] = &["binstall", "--no-confirm", "stmo-cli"];
const CARGO_INSTALL_ARGS: &[&str] = &["install", "stmo-cli", "--locked"];

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

fn cargo_binstall_available() -> bool {
    Command::new("cargo")
        .args(["binstall", "--version"])
        .output()
        .is_ok_and(|output| output.status.success())
}

fn install_plan(binstall_available: bool) -> Vec<&'static [&'static str]> {
    if binstall_available {
        vec![BINSTALL_ARGS, CARGO_INSTALL_ARGS]
    } else {
        vec![CARGO_INSTALL_ARGS]
    }
}

fn run_cargo(args: &[&str]) -> std::io::Result<bool> {
    eprintln!("Running: cargo {}", args.join(" "));
    Command::new("cargo")
        .args(args)
        .status()
        .map(|status| status.success())
}

fn run_install_plan(plan: &[&[&str]]) -> Result<()> {
    for args in plan {
        if run_cargo(args)? {
            println!("stmo-cli updated successfully.");
            return Ok(());
        }
        eprintln!("cargo {} failed, trying the next installer", args.join(" "));
    }
    anyhow::bail!("Could not update stmo-cli: every installer failed");
}

pub fn update() -> Result<()> {
    if !cargo_writable() {
        anyhow::bail!(
            "Cannot update stmo-cli in the current environment \
             (write access to ~/.cargo/ is restricted).\n\
             Run this command outside the sandbox:\n  stmo-cli update"
        );
    }

    run_install_plan(&install_plan(cargo_binstall_available()))
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    #[test]
    fn test_install_plan_tries_binstall_before_locked_install() {
        assert_eq!(install_plan(true), vec![BINSTALL_ARGS, CARGO_INSTALL_ARGS]);
    }

    #[test]
    fn test_install_plan_without_binstall_uses_locked_install() {
        assert_eq!(install_plan(false), vec![CARGO_INSTALL_ARGS]);
    }

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

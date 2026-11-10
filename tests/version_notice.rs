#![cfg(unix)]

use std::process::Command;
use std::time::{SystemTime, UNIX_EPOCH};
use tempfile::TempDir;

#[test]
fn update_notice_points_at_stmo_cli_update() {
    let home = TempDir::new().unwrap();
    let mozbuild = home.path().join(".mozbuild");
    std::fs::create_dir_all(&mozbuild).unwrap();
    let last_check = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_secs();
    std::fs::write(
        mozbuild.join("tool-versions.json"),
        format!(r#"{{"stmo-cli": {{"last_check": {last_check}, "latest": "99.0.0"}}}}"#),
    )
    .unwrap();

    let output = Command::new(env!("CARGO_BIN_EXE_stmo-cli"))
        .arg("--version")
        .env("HOME", home.path())
        .env_remove("REDASH_API_KEY")
        .output()
        .unwrap();

    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(stderr.contains("Run: stmo-cli update"), "stderr: {stderr}");
    assert!(
        stderr.contains("ACTION REQUIRED: stmo-cli is out of date"),
        "stderr: {stderr}"
    );
    assert!(!stderr.contains("cargo binstall"), "stderr: {stderr}");
}

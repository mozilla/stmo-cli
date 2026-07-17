#![cfg(target_os = "macos")]
#![allow(clippy::missing_panics_doc)]

use std::process::Command;
use tempfile::TempDir;
use wiremock::matchers::{header, method, path};
use wiremock::{Mock, MockServer, ResponseTemplate};

// Drives the whole main.rs -> config::resolve_api_key -> keychain read -> RedashClient
// -> HTTP path through the real compiled binary, with REDASH_API_KEY unset. Uses a
// throwaway keychain file (never the user's login keychain), so it never prompts, never
// pops the "Always Allow" dialog, and never touches a real secret. wiremock stands in
// for sql.telemetry.mozilla.org.
#[tokio::test]
async fn discover_reads_api_key_from_keychain_file() {
    let tmp = TempDir::new().unwrap();
    let keychain_path = tmp.path().join("e2e.keychain-db");
    let keychain_path = keychain_path.to_str().unwrap();

    assert!(
        Command::new("security")
            .args(["create-keychain", "-p", "", keychain_path])
            .status()
            .unwrap()
            .success()
    );
    // `-A` grants every app silent read access to this throwaway item, so the test
    // never triggers the "Always Allow" dialog.
    assert!(
        Command::new("security")
            .args([
                "add-generic-password",
                "-A",
                "-a",
                "e2e",
                "-s",
                "stmo-cli",
                "-w",
                "test-key-abc",
                keychain_path,
            ])
            .status()
            .unwrap()
            .success()
    );

    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/api/queries/my"))
        // Proves the keychain value actually reached the Authorization header.
        .and(header("Authorization", "Key test-key-abc"))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
            "results": [],
            "count": 0,
            "page": 1,
            "page_size": 25
        })))
        .mount(&server)
        .await;

    let output = Command::new(env!("CARGO_BIN_EXE_stmo-cli"))
        .arg("discover")
        .env_remove("REDASH_API_KEY")
        .env("STMO_KEYCHAIN_PATH", keychain_path)
        .env("REDASH_URL", server.uri())
        .output()
        .unwrap();

    let _ = Command::new("security")
        .args(["delete-keychain", keychain_path])
        .status();

    assert!(
        output.status.success(),
        "stdout: {}\nstderr: {}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
}

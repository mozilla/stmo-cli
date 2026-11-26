#![allow(clippy::missing_panics_doc)]

use serde_json::json;
use std::process::{Command, Output};
use stmo_cli::api::RedashClient;
use wiremock::matchers::{header, method, path};
use wiremock::{Mock, MockServer, ResponseTemplate};

const FIXTURE_KEY: &str = "fixture-key-abc";

async fn mock_query_endpoint(status: u16, body: serde_json::Value) -> MockServer {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/api/queries/900000001"))
        .and(header("Authorization", "Key test-key"))
        .respond_with(ResponseTemplate::new(status).set_body_json(body))
        .mount(&server)
        .await;
    server
}

#[tokio::test]
async fn get_query_api_key_returns_the_query_key() {
    let server = mock_query_endpoint(200, json!({"id": 900_000_001, "api_key": FIXTURE_KEY})).await;
    let client = RedashClient::new(server.uri(), "test-key").unwrap();

    let api_key = client.get_query_api_key(900_000_001).await.unwrap();

    assert_eq!(api_key, FIXTURE_KEY);
}

#[tokio::test]
async fn get_query_api_key_surfaces_api_error_status() {
    let server = mock_query_endpoint(404, json!({"message": "Not found."})).await;
    let client = RedashClient::new(server.uri(), "test-key").unwrap();

    let error = client.get_query_api_key(900_000_001).await.unwrap_err();

    assert!(error.to_string().contains("API error 404"), "{error}");
}

#[tokio::test]
async fn get_query_api_key_fails_when_response_has_no_api_key() {
    let server = mock_query_endpoint(200, json!({"id": 900_000_001})).await;
    let client = RedashClient::new(server.uri(), "test-key").unwrap();

    let error = client.get_query_api_key(900_000_001).await.unwrap_err();

    assert!(error.to_string().contains("Failed to parse"), "{error}");
}

fn run_query_api_key(base_url: &str) -> Output {
    Command::new(env!("CARGO_BIN_EXE_stmo-cli"))
        .args(["query-api-key", "900000001"])
        .env("REDASH_API_KEY", "test-key")
        .env("REDASH_URL", base_url)
        .output()
        .unwrap()
}

#[tokio::test]
async fn query_api_key_prints_only_the_key_to_stdout() {
    let server = mock_query_endpoint(200, json!({"id": 900_000_001, "api_key": FIXTURE_KEY})).await;

    let output = run_query_api_key(&server.uri());

    assert!(
        output.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert_eq!(String::from_utf8_lossy(&output.stdout), "fixture-key-abc\n");
}

#[tokio::test]
async fn query_api_key_failure_exits_nonzero_with_empty_stdout() {
    let server = mock_query_endpoint(404, json!({"message": "Not found."})).await;

    let output = run_query_api_key(&server.uri());

    assert!(!output.status.success());
    assert_eq!(String::from_utf8_lossy(&output.stdout), "");
    assert!(String::from_utf8_lossy(&output.stderr).contains("API error 404"));
}

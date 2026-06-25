#![allow(clippy::missing_errors_doc)]
#![allow(clippy::missing_panics_doc)]

mod common;

use common::*;
use std::env;
use std::fs;
use std::sync::OnceLock;
use stmo_cli::api::RedashClient;
use stmo_cli::commands::OutputFormat;
use stmo_cli::commands::execute::{ExecuteArgs, execute};
use tempfile::TempDir;
use tokio::sync::Mutex;
use wiremock::matchers::{body_partial_json, method, path};
use wiremock::{Mock, MockServer, ResponseTemplate};

static TEST_MUTEX: OnceLock<Mutex<()>> = OnceLock::new();

fn get_test_lock() -> &'static Mutex<()> {
    TEST_MUTEX.get_or_init(|| Mutex::new(()))
}

struct TempWorkDir {
    _temp_dir: TempDir,
    original_dir: std::path::PathBuf,
}

impl TempWorkDir {
    fn new() -> Self {
        let temp_dir = TempDir::new().unwrap();
        let original_dir = env::current_dir().unwrap();
        env::set_current_dir(temp_dir.path()).unwrap();
        Self {
            _temp_dir: temp_dir,
            original_dir,
        }
    }
}

impl Drop for TempWorkDir {
    fn drop(&mut self) {
        env::set_current_dir(&self.original_dir).ok();
    }
}

const QUERY_YAML: &str = r"
id: 123
name: Test Query
description: null
data_source_id: 63
options:
  parameters: []
visualizations: []
tags: null
";

fn write_tracked_query(sql: &str) {
    fs::create_dir_all("queries").unwrap();
    fs::write("queries/123-test-query.sql", sql).unwrap();
    fs::write("queries/123-test-query.yaml", QUERY_YAML).unwrap();
}

fn default_args(query_id: Option<u64>) -> ExecuteArgs {
    ExecuteArgs {
        query_id,
        file: None,
        data_source: None,
        param_args: vec![],
        format: OutputFormat::Json,
        interactive: false,
        timeout_secs: 10,
        limit_rows: None,
        remote: false,
    }
}

#[tokio::test]
async fn test_execute_default_runs_local_sql_adhoc() {
    let _guard = get_test_lock().lock().await;
    let _cwd = TempWorkDir::new();
    write_tracked_query("SELECT 42 AS local_col");

    let mock_server = MockServer::start().await;

    // Default execute must POST the LOCAL sql to the ad-hoc endpoint,
    // NOT the stored /api/queries/{id}/results endpoint.
    Mock::given(method("POST"))
        .and(path("/api/query_results"))
        .and(body_partial_json(serde_json::json!({
            "query": "SELECT 42 AS local_col",
            "data_source_id": 63
        })))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
            "job": {"id": "adhoc-job", "status": 1, "query_result_id": null, "error": null}
        })))
        .expect(1)
        .mount(&mock_server)
        .await;
    mock_poll_job_success("adhoc-job", 789)
        .mount(&mock_server)
        .await;
    mock_get_adhoc_query_result(789).mount(&mock_server).await;

    let client = RedashClient::new(mock_server.uri(), "test-key").unwrap();
    execute(&client, default_args(Some(123))).await.unwrap();
}

#[tokio::test]
async fn test_execute_remote_runs_stored_query() {
    let _guard = get_test_lock().lock().await;
    let _cwd = TempWorkDir::new();
    write_tracked_query("SELECT 42 AS local_col");

    let mock_server = MockServer::start().await;

    // --remote must hit the stored per-query results endpoint.
    mock_refresh_query(123, "stored-job")
        .expect(1)
        .mount(&mock_server)
        .await;
    mock_poll_job_success("stored-job", 456)
        .mount(&mock_server)
        .await;
    mock_get_query_result(123, 456).mount(&mock_server).await;

    let client = RedashClient::new(mock_server.uri(), "test-key").unwrap();
    let mut args = default_args(Some(123));
    args.remote = true;
    execute(&client, args).await.unwrap();
}

#[tokio::test]
async fn test_execute_file_runs_adhoc_without_tracked_query() {
    let _guard = get_test_lock().lock().await;
    let _cwd = TempWorkDir::new();
    // Deliberately no queries/ directory.
    fs::write("scratch.sql", "SELECT 7 AS scratch").unwrap();

    let mock_server = MockServer::start().await;

    Mock::given(method("POST"))
        .and(path("/api/query_results"))
        .and(body_partial_json(serde_json::json!({
            "query": "SELECT 7 AS scratch",
            "data_source_id": 63
        })))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
            "job": {"id": "adhoc-job", "status": 1, "query_result_id": null, "error": null}
        })))
        .expect(1)
        .mount(&mock_server)
        .await;
    mock_poll_job_success("adhoc-job", 789)
        .mount(&mock_server)
        .await;
    mock_get_adhoc_query_result(789).mount(&mock_server).await;

    let client = RedashClient::new(mock_server.uri(), "test-key").unwrap();
    let mut args = default_args(None);
    args.file = Some("scratch.sql".to_string());
    args.data_source = Some(63);
    execute(&client, args).await.unwrap();
}

#[tokio::test]
async fn test_execute_file_without_data_source_errors() {
    let _guard = get_test_lock().lock().await;
    let _cwd = TempWorkDir::new();
    fs::write("scratch.sql", "SELECT 1").unwrap();

    let client = RedashClient::new("http://localhost:1".to_string(), "test-key").unwrap();
    let mut args = default_args(None);
    args.file = Some("scratch.sql".to_string());
    let err = execute(&client, args).await.unwrap_err();
    assert!(err.to_string().contains("--data-source"));
}

#[tokio::test]
async fn test_execute_no_query_and_no_file_errors() {
    let _guard = get_test_lock().lock().await;
    let _cwd = TempWorkDir::new();

    let client = RedashClient::new("http://localhost:1".to_string(), "test-key").unwrap();
    let err = execute(&client, default_args(None)).await.unwrap_err();
    assert!(err.to_string().contains("No query specified"));
}

#[tokio::test]
async fn test_execute_query_id_and_file_conflict_errors() {
    let _guard = get_test_lock().lock().await;
    let _cwd = TempWorkDir::new();
    fs::write("scratch.sql", "SELECT 1").unwrap();

    let client = RedashClient::new("http://localhost:1".to_string(), "test-key").unwrap();
    let mut args = default_args(Some(123));
    args.file = Some("scratch.sql".to_string());
    let err = execute(&client, args).await.unwrap_err();
    assert!(err.to_string().contains("Cannot combine"));
}

#[tokio::test]
async fn test_execute_file_and_remote_conflict_errors() {
    let _guard = get_test_lock().lock().await;
    let _cwd = TempWorkDir::new();
    fs::write("scratch.sql", "SELECT 1").unwrap();

    let client = RedashClient::new("http://localhost:1".to_string(), "test-key").unwrap();
    let mut args = default_args(None);
    args.file = Some("scratch.sql".to_string());
    args.remote = true;
    let err = execute(&client, args).await.unwrap_err();
    assert!(
        err.to_string()
            .contains("--remote cannot be combined with --file")
    );
}

#[tokio::test]
async fn test_execute_query_id_and_data_source_conflict_errors() {
    let _guard = get_test_lock().lock().await;
    let _cwd = TempWorkDir::new();

    let client = RedashClient::new("http://localhost:1".to_string(), "test-key").unwrap();
    let mut args = default_args(Some(123));
    args.data_source = Some(63);
    let err = execute(&client, args).await.unwrap_err();
    assert!(
        err.to_string()
            .contains("--data-source cannot be combined with a query ID")
    );
}

#[tokio::test]
async fn test_execute_remote_without_query_id_errors() {
    let _guard = get_test_lock().lock().await;
    let _cwd = TempWorkDir::new();

    let client = RedashClient::new("http://localhost:1".to_string(), "test-key").unwrap();
    let mut args = default_args(None);
    args.remote = true;
    let err = execute(&client, args).await.unwrap_err();
    assert!(err.to_string().contains("--remote runs a tracked query"));
}

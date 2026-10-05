#![allow(clippy::missing_errors_doc)]
#![allow(clippy::missing_panics_doc)]

mod common;

use common::*;
use std::env;
use std::sync::OnceLock;
use stmo_cli::api::RedashClient;
use stmo_cli::models::{CreateQuerySnippet, QuerySnippet};
use tempfile::TempDir;
use tokio::sync::Mutex;

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

#[tokio::test]
async fn test_list_query_snippets() {
    let mock_server = wiremock::MockServer::start().await;

    mock_list_query_snippets(&serde_json::json!([
        {
            "id": 900_000_002,
            "trigger": "sample_hll_count",
            "description": "Example HLL snippet",
            "snippet": "cardinality(merge(cast(${FIELD_NAME} AS HLL)))",
            "user": null,
            "updated_at": "2026-01-01T00:00:00Z",
            "created_at": "2026-01-01T00:00:00Z"
        },
        {
            "id": 900_000_001,
            "trigger": "sample_task_cte",
            "description": "Example task CTE",
            "snippet": "sample_tasks AS (\n    SELECT 1\n)",
            "user": null,
            "updated_at": "2026-01-01T00:00:00Z",
            "created_at": "2026-01-01T00:00:00Z"
        }
    ]))
    .mount(&mock_server)
    .await;

    let client = RedashClient::new(mock_server.uri(), "test-key").unwrap();
    let snippets = client.list_query_snippets().await.unwrap();

    assert_eq!(snippets.len(), 2);
    assert_eq!(snippets[0].trigger, "sample_hll_count");
    assert_eq!(snippets[1].id, 900_000_001);
    assert_eq!(snippets[1].trigger, "sample_task_cte");
}

#[tokio::test]
async fn test_get_query_snippet() {
    let mock_server = wiremock::MockServer::start().await;

    mock_get_query_snippet(
        900_000_001,
        "sample_task_cte",
        "sample_tasks AS (\n    SELECT 1\n)",
    )
    .mount(&mock_server)
    .await;

    let client = RedashClient::new(mock_server.uri(), "test-key").unwrap();
    let snippet = client.get_query_snippet(900_000_001).await.unwrap();

    assert_eq!(snippet.id, 900_000_001);
    assert_eq!(snippet.trigger, "sample_task_cte");
    assert!(snippet.snippet.contains("sample_tasks AS"));
}

#[tokio::test]
async fn test_create_query_snippet() {
    let mock_server = wiremock::MockServer::start().await;

    mock_create_query_snippet(42, "stmo_cli_selftest", "SELECT 1")
        .mount(&mock_server)
        .await;

    let client = RedashClient::new(mock_server.uri(), "test-key").unwrap();
    let create = CreateQuerySnippet {
        trigger: "stmo_cli_selftest".to_string(),
        description: Some("Self-test snippet".to_string()),
        snippet: "SELECT 1".to_string(),
    };
    let snippet = client.create_query_snippet(&create).await.unwrap();

    assert_eq!(snippet.id, 42);
    assert_eq!(snippet.trigger, "stmo_cli_selftest");
}

#[tokio::test]
async fn test_update_query_snippet() {
    let mock_server = wiremock::MockServer::start().await;

    mock_update_query_snippet(
        900_000_001,
        "sample_task_cte",
        "sample_tasks AS (\n    SELECT 2\n)",
    )
    .mount(&mock_server)
    .await;

    let client = RedashClient::new(mock_server.uri(), "test-key").unwrap();
    let existing = QuerySnippet {
        id: 900_000_001,
        trigger: "sample_task_cte".to_string(),
        description: Some("Example task CTE".to_string()),
        snippet: "sample_tasks AS (\n    SELECT 2\n)".to_string(),
        user: None,
        updated_at: "2026-01-01T00:00:00Z".to_string(),
        created_at: "2026-01-01T00:00:00Z".to_string(),
    };
    let updated = client.update_query_snippet(&existing).await.unwrap();

    assert_eq!(updated.id, 900_000_001);
    assert!(updated.snippet.contains("SELECT 2"));
}

#[tokio::test]
async fn test_delete_query_snippet() {
    let mock_server = wiremock::MockServer::start().await;

    mock_delete_query_snippet(42).mount(&mock_server).await;

    let client = RedashClient::new(mock_server.uri(), "test-key").unwrap();
    let result = client.delete_query_snippet(42).await;

    assert!(result.is_ok());
}

#[tokio::test]
async fn test_delete_query_snippet_not_found() {
    let mock_server = wiremock::MockServer::start().await;

    mock_delete_query_snippet_not_found(999)
        .mount(&mock_server)
        .await;

    let client = RedashClient::new(mock_server.uri(), "test-key").unwrap();
    let result = client.delete_query_snippet(999).await;

    assert!(result.is_err());
}

#[tokio::test]
async fn test_fetch_command_writes_snippet_files() {
    let _guard = get_test_lock().lock().await;
    let _temp_dir = TempWorkDir::new();
    let mock_server = wiremock::MockServer::start().await;

    mock_get_query_snippet(
        900_000_001,
        "sample_task_cte",
        "sample_tasks AS (\n    SELECT 1\n)",
    )
    .mount(&mock_server)
    .await;

    let client = RedashClient::new(mock_server.uri(), "test-key").unwrap();
    let result = stmo_cli::commands::snippets::fetch(&client, vec![900_000_001], false).await;

    assert!(result.is_ok());
    let sql = std::fs::read_to_string("snippets/900000001-sample_task_cte.sql").unwrap();
    assert!(sql.contains("sample_tasks AS"));
    let yaml = std::fs::read_to_string("snippets/900000001-sample_task_cte.yaml").unwrap();
    assert!(yaml.contains("id: 900000001"));
    assert!(yaml.contains("trigger: sample_task_cte"));
}

#[tokio::test]
async fn test_fetch_command_handles_trigger_with_spaces_and_quotes() {
    // Triggers may contain spaces and quotes. Unlike query names and dashboard
    // slugs, they are used unchanged in filenames, so this checks that the value
    // round-trips correctly in both the file path and YAML.
    let _guard = get_test_lock().lock().await;
    let _temp_dir = TempWorkDir::new();
    let mock_server = wiremock::MockServer::start().await;

    mock_get_query_snippet(900_000_003, "example's snippet", "parse_date(foo, bar)")
        .mount(&mock_server)
        .await;

    let client = RedashClient::new(mock_server.uri(), "test-key").unwrap();
    let result = stmo_cli::commands::snippets::fetch(&client, vec![900_000_003], false).await;
    assert!(result.is_ok());

    let sql_path = std::path::Path::new("snippets/900000003-example's snippet.sql");
    let yaml_path = std::path::Path::new("snippets/900000003-example's snippet.yaml");
    assert!(sql_path.exists(), "expected {}", sql_path.display());
    assert!(yaml_path.exists(), "expected {}", yaml_path.display());

    let sql = std::fs::read_to_string(sql_path).unwrap();
    assert_eq!(sql, "parse_date(foo, bar)");

    let yaml = std::fs::read_to_string(yaml_path).unwrap();
    let metadata: stmo_cli::models::SnippetMetadata = serde_yaml::from_str(&yaml).unwrap();
    assert_eq!(metadata.id, 900_000_003);
    assert_eq!(metadata.trigger, "example's snippet");

    // A second fetch --all must rediscover this file by id via directory scanning
    // (extract_snippet_ids_from_directory), proving the weird filename doesn't
    // break the id-prefix parsing that other snippets.rs functions rely on.
    let result = stmo_cli::commands::snippets::fetch(&client, vec![], true).await;
    assert!(result.is_ok());
}

#[tokio::test]
async fn test_deploy_new_snippet_with_id_zero() {
    let _guard = get_test_lock().lock().await;
    let _temp_dir = TempWorkDir::new();
    let mock_server = wiremock::MockServer::start().await;

    mock_create_query_snippet(42, "stmo_cli_selftest", "SELECT 1")
        .mount(&mock_server)
        .await;

    let client = RedashClient::new(mock_server.uri(), "test-key").unwrap();

    std::fs::create_dir_all("snippets").unwrap();
    std::fs::write("snippets/0-stmo_cli_selftest.sql", "SELECT 1").unwrap();
    std::fs::write(
        "snippets/0-stmo_cli_selftest.yaml",
        "id: 0\ntrigger: stmo_cli_selftest\ndescription: Self-test snippet\n",
    )
    .unwrap();

    let result = stmo_cli::commands::snippets::deploy(&client, vec![0], false).await;

    assert!(result.is_ok());
    assert!(!std::path::Path::new("snippets/0-stmo_cli_selftest.sql").exists());
    assert!(!std::path::Path::new("snippets/0-stmo_cli_selftest.yaml").exists());
    assert!(std::path::Path::new("snippets/42-stmo_cli_selftest.sql").exists());
    assert!(std::path::Path::new("snippets/42-stmo_cli_selftest.yaml").exists());
}

#[tokio::test]
async fn test_deploy_creates_several_new_snippets_in_one_run() {
    let _guard = get_test_lock().lock().await;
    let _temp_dir = TempWorkDir::new();
    let mock_server = wiremock::MockServer::start().await;

    mock_create_query_snippet_with_trigger(42, "first_trigger", "SELECT 1")
        .expect(1)
        .mount(&mock_server)
        .await;
    mock_create_query_snippet_with_trigger(43, "second_trigger", "SELECT 2")
        .expect(1)
        .mount(&mock_server)
        .await;
    mock_list_query_snippets(&serde_json::json!([]))
        .mount(&mock_server)
        .await;

    let client = RedashClient::new(mock_server.uri(), "test-key").unwrap();

    std::fs::create_dir_all("snippets").unwrap();
    std::fs::write("snippets/0-first_trigger.sql", "SELECT 1").unwrap();
    std::fs::write(
        "snippets/0-first_trigger.yaml",
        "id: 0\ntrigger: first_trigger\ndescription: First\n",
    )
    .unwrap();
    std::fs::write("snippets/0-second_trigger.sql", "SELECT 2").unwrap();
    std::fs::write(
        "snippets/0-second_trigger.yaml",
        "id: 0\ntrigger: second_trigger\ndescription: Second\n",
    )
    .unwrap();

    let result = stmo_cli::commands::snippets::deploy(&client, vec![], false).await;

    assert!(result.is_ok(), "{:?}", result.unwrap_err());
    assert!(!std::path::Path::new("snippets/0-first_trigger.yaml").exists());
    assert!(!std::path::Path::new("snippets/0-second_trigger.yaml").exists());
    assert!(std::path::Path::new("snippets/42-first_trigger.sql").exists());
    assert!(std::path::Path::new("snippets/42-first_trigger.yaml").exists());
    assert!(std::path::Path::new("snippets/43-second_trigger.sql").exists());
    assert!(std::path::Path::new("snippets/43-second_trigger.yaml").exists());

    mock_server.verify().await;
}

#[tokio::test]
async fn test_deploy_existing_snippet_hits_update_path() {
    let _guard = get_test_lock().lock().await;
    let _temp_dir = TempWorkDir::new();
    let mock_server = wiremock::MockServer::start().await;

    mock_update_query_snippet(
        900_000_001,
        "sample_task_cte",
        "sample_tasks AS (\n    SELECT 2\n)",
    )
    .mount(&mock_server)
    .await;

    let client = RedashClient::new(mock_server.uri(), "test-key").unwrap();

    std::fs::create_dir_all("snippets").unwrap();
    std::fs::write(
        "snippets/900000001-sample_task_cte.sql",
        "sample_tasks AS (\n    SELECT 2\n)",
    )
    .unwrap();
    std::fs::write(
        "snippets/900000001-sample_task_cte.yaml",
        "id: 900000001\ntrigger: sample_task_cte\ndescription: Example task CTE\n",
    )
    .unwrap();

    let result = stmo_cli::commands::snippets::deploy(&client, vec![900_000_001], false).await;

    assert!(result.is_ok());
    let sql = std::fs::read_to_string("snippets/900000001-sample_task_cte.sql").unwrap();
    assert!(sql.contains("SELECT 2"));
}

#[tokio::test]
async fn test_delete_command_removes_remote_and_local_files() {
    let _guard = get_test_lock().lock().await;
    let _temp_dir = TempWorkDir::new();
    let mock_server = wiremock::MockServer::start().await;

    mock_delete_query_snippet(42).mount(&mock_server).await;

    let client = RedashClient::new(mock_server.uri(), "test-key").unwrap();

    std::fs::create_dir_all("snippets").unwrap();
    std::fs::write("snippets/42-stmo_cli_selftest.sql", "SELECT 1").unwrap();
    std::fs::write(
        "snippets/42-stmo_cli_selftest.yaml",
        "id: 42\ntrigger: stmo_cli_selftest\ndescription: Self-test snippet\n",
    )
    .unwrap();

    let result = stmo_cli::commands::snippets::delete(&client, vec![42]).await;

    assert!(result.is_ok());
    assert!(!std::path::Path::new("snippets/42-stmo_cli_selftest.sql").exists());
    assert!(!std::path::Path::new("snippets/42-stmo_cli_selftest.yaml").exists());
}

#[tokio::test]
async fn test_list_command_succeeds_with_results() {
    let mock_server = wiremock::MockServer::start().await;

    mock_list_query_snippets(&serde_json::json!([
        {
            "id": 900_000_001,
            "trigger": "sample_task_cte",
            "description": "Example task CTE",
            "snippet": "sample_tasks AS (\n    SELECT 1\n)",
            "user": null,
            "updated_at": "2026-01-01T00:00:00Z",
            "created_at": "2026-01-01T00:00:00Z"
        },
        {
            "id": 900_000_002,
            "trigger": "sample_hll_count",
            "description": null,
            "snippet": "cardinality(merge(cast(${FIELD_NAME} AS HLL)))",
            "user": null,
            "updated_at": "2026-01-01T00:00:00Z",
            "created_at": "2026-01-01T00:00:00Z"
        }
    ]))
    .mount(&mock_server)
    .await;

    let client = RedashClient::new(mock_server.uri(), "test-key").unwrap();
    let result = stmo_cli::commands::snippets::list(&client).await;

    assert!(result.is_ok());
}

#[tokio::test]
async fn test_list_command_succeeds_with_empty_results() {
    let mock_server = wiremock::MockServer::start().await;

    mock_list_query_snippets(&serde_json::json!([]))
        .mount(&mock_server)
        .await;

    let client = RedashClient::new(mock_server.uri(), "test-key").unwrap();
    let result = stmo_cli::commands::snippets::list(&client).await;

    assert!(result.is_ok());
}

#[tokio::test]
async fn test_fetch_bails_when_all_and_no_local_snippets() {
    let _guard = get_test_lock().lock().await;
    let _temp_dir = TempWorkDir::new();
    let mock_server = wiremock::MockServer::start().await;

    let client = RedashClient::new(mock_server.uri(), "test-key").unwrap();
    let result = stmo_cli::commands::snippets::fetch(&client, vec![], true).await;

    assert!(result.is_err());
    assert!(
        result
            .unwrap_err()
            .to_string()
            .contains("No snippets found in snippets/ directory")
    );
}

#[tokio::test]
async fn test_fetch_bails_when_no_ids_and_no_all() {
    let _guard = get_test_lock().lock().await;
    let _temp_dir = TempWorkDir::new();
    let mock_server = wiremock::MockServer::start().await;

    let client = RedashClient::new(mock_server.uri(), "test-key").unwrap();
    let result = stmo_cli::commands::snippets::fetch(&client, vec![], false).await;

    assert!(result.is_err());
    assert!(
        result
            .unwrap_err()
            .to_string()
            .contains("No snippet IDs specified")
    );
}

#[tokio::test]
async fn test_fetch_partial_failure_writes_successful_and_warns() {
    let _guard = get_test_lock().lock().await;
    let _temp_dir = TempWorkDir::new();
    let mock_server = wiremock::MockServer::start().await;

    mock_get_query_snippet_not_found(999)
        .mount(&mock_server)
        .await;
    mock_get_query_snippet(
        900_000_001,
        "sample_task_cte",
        "sample_tasks AS (\n    SELECT 1\n)",
    )
    .mount(&mock_server)
    .await;

    let client = RedashClient::new(mock_server.uri(), "test-key").unwrap();
    let result = stmo_cli::commands::snippets::fetch(&client, vec![999, 900_000_001], false).await;

    // Matches queries' fetch.rs convention: partial failures are warned about via
    // stderr and skipped, not surfaced as an overall error (unlike dashboards::fetch).
    assert!(result.is_ok());
    assert!(std::path::Path::new("snippets/900000001-sample_task_cte.sql").exists());
    assert!(
        !std::path::Path::new("snippets")
            .read_dir()
            .unwrap()
            .any(|e| e.unwrap().file_name().to_string_lossy().starts_with("999-"))
    );
}

#[tokio::test]
async fn test_deploy_bails_when_no_matching_ids() {
    let _guard = get_test_lock().lock().await;
    let _temp_dir = TempWorkDir::new();
    let mock_server = wiremock::MockServer::start().await;

    let client = RedashClient::new(mock_server.uri(), "test-key").unwrap();

    std::fs::create_dir_all("snippets").unwrap();
    std::fs::write("snippets/900000001-sample_task_cte.sql", "SELECT 1").unwrap();
    std::fs::write(
        "snippets/900000001-sample_task_cte.yaml",
        "id: 900000001\ntrigger: sample_task_cte\ndescription: null\n",
    )
    .unwrap();

    let result = stmo_cli::commands::snippets::deploy(&client, vec![999], false).await;

    assert!(result.is_err());
    assert!(
        result
            .unwrap_err()
            .to_string()
            .contains("None of the specified snippet IDs were found")
    );
}

// Bare `snippets deploy` (no explicit IDs, no --all) decides what to push by
// comparing each tracked snippet's local content against the single
// `list_query_snippets` response, instead of asking git what changed on disk.

#[tokio::test]
async fn test_deploy_bare_skips_snippet_unchanged_from_server() {
    let _guard = get_test_lock().lock().await;
    let _temp_dir = TempWorkDir::new();
    let mock_server = wiremock::MockServer::start().await;

    mock_list_query_snippets(&serde_json::json!([
        {
            "id": 900_000_001,
            "trigger": "sample_task_cte",
            "description": null,
            "snippet": "SELECT 1",
            "user": null,
            "updated_at": "2026-01-21T10:00:00Z",
            "created_at": "2026-01-21T10:00:00Z"
        }
    ]))
    .mount(&mock_server)
    .await;

    let client = RedashClient::new(mock_server.uri(), "test-key").unwrap();
    std::fs::create_dir_all("snippets").unwrap();
    std::fs::write("snippets/900000001-sample_task_cte.sql", "SELECT 1").unwrap();
    std::fs::write(
        "snippets/900000001-sample_task_cte.yaml",
        "id: 900000001\ntrigger: sample_task_cte\ndescription: null\n",
    )
    .unwrap();

    // No update mock is registered — if `deploy` wrongly tried to push this
    // unchanged snippet, the request would hit no matching mock and fail.
    let result = stmo_cli::commands::snippets::deploy(&client, vec![], false).await;
    assert!(result.is_ok(), "Deploy failed: {:?}", result.err());
}

#[tokio::test]
async fn test_deploy_bare_second_run_deploys_nothing() {
    let _guard = get_test_lock().lock().await;
    let _temp_dir = TempWorkDir::new();
    let mock_server = wiremock::MockServer::start().await;

    // Simulate the server's content changing once the snippet is deployed:
    // the first list (consumed by the comparison before deploying) still has
    // the old body; every list after that (the second `deploy` call) has the
    // new body that was just pushed.
    mock_list_query_snippets(&serde_json::json!([
        {
            "id": 900_000_001,
            "trigger": "sample_task_cte",
            "description": null,
            "snippet": "SELECT 1",
            "user": null,
            "updated_at": "2026-01-21T10:00:00Z",
            "created_at": "2026-01-21T10:00:00Z"
        }
    ]))
    .up_to_n_times(1)
    .with_priority(1)
    .mount(&mock_server)
    .await;
    mock_list_query_snippets(&serde_json::json!([
        {
            "id": 900_000_001,
            "trigger": "sample_task_cte",
            // Matches what mock_update_query_snippet below returns — after the
            // first deploy, write_snippet_files() rewrites the local yaml from
            // exactly that response, so the second comparison must see the
            // same description here to correctly detect no diff.
            "description": "Test snippet",
            "snippet": "SELECT 2",
            "user": null,
            "updated_at": "2026-01-21T10:00:00Z",
            "created_at": "2026-01-21T10:00:00Z"
        }
    ]))
    .with_priority(2)
    .mount(&mock_server)
    .await;
    mock_update_query_snippet(900_000_001, "sample_task_cte", "SELECT 2")
        .expect(1)
        .mount(&mock_server)
        .await;

    let client = RedashClient::new(mock_server.uri(), "test-key").unwrap();
    std::fs::create_dir_all("snippets").unwrap();
    std::fs::write("snippets/900000001-sample_task_cte.sql", "SELECT 2").unwrap();
    std::fs::write(
        "snippets/900000001-sample_task_cte.yaml",
        "id: 900000001\ntrigger: sample_task_cte\ndescription: null\n",
    )
    .unwrap();

    let first = stmo_cli::commands::snippets::deploy(&client, vec![], false).await;
    assert!(first.is_ok(), "First deploy failed: {:?}", first.err());

    let second = stmo_cli::commands::snippets::deploy(&client, vec![], false).await;
    assert!(second.is_ok(), "Second deploy failed: {:?}", second.err());

    // The update mock has `.expect(1)` — verify() fails if it was hit twice.
    mock_server.verify().await;
}

#[tokio::test]
async fn test_deploy_one_missing_sql_file_bails() {
    let _guard = get_test_lock().lock().await;
    let _temp_dir = TempWorkDir::new();
    let mock_server = wiremock::MockServer::start().await;

    let client = RedashClient::new(mock_server.uri(), "test-key").unwrap();

    std::fs::create_dir_all("snippets").unwrap();
    std::fs::write(
        "snippets/900000001-sample_task_cte.yaml",
        "id: 900000001\ntrigger: sample_task_cte\ndescription: null\n",
    )
    .unwrap();

    let result =
        stmo_cli::commands::snippets::deploy_one(&client, 900_000_001, "sample_task_cte").await;

    assert!(result.is_err());
    assert!(
        result
            .unwrap_err()
            .to_string()
            .contains("Snippet SQL file not found")
    );
}

#[tokio::test]
async fn test_deploy_one_missing_yaml_file_bails() {
    let _guard = get_test_lock().lock().await;
    let _temp_dir = TempWorkDir::new();
    let mock_server = wiremock::MockServer::start().await;

    let client = RedashClient::new(mock_server.uri(), "test-key").unwrap();

    std::fs::create_dir_all("snippets").unwrap();
    std::fs::write("snippets/900000001-sample_task_cte.sql", "SELECT 1").unwrap();

    let result =
        stmo_cli::commands::snippets::deploy_one(&client, 900_000_001, "sample_task_cte").await;

    assert!(result.is_err());
    assert!(
        result
            .unwrap_err()
            .to_string()
            .contains("Snippet metadata file not found")
    );
}

#[tokio::test]
async fn test_deploy_one_malformed_yaml_bails() {
    let _guard = get_test_lock().lock().await;
    let _temp_dir = TempWorkDir::new();
    let mock_server = wiremock::MockServer::start().await;

    let client = RedashClient::new(mock_server.uri(), "test-key").unwrap();

    std::fs::create_dir_all("snippets").unwrap();
    std::fs::write("snippets/900000001-sample_task_cte.sql", "SELECT 1").unwrap();
    std::fs::write(
        "snippets/900000001-sample_task_cte.yaml",
        "description: missing required fields\n",
    )
    .unwrap();

    let result =
        stmo_cli::commands::snippets::deploy_one(&client, 900_000_001, "sample_task_cte").await;

    assert!(result.is_err());
    assert!(result.unwrap_err().to_string().contains("Failed to parse"));
}

#[tokio::test]
async fn test_delete_partial_failure_bails_but_removes_successful_local_files() {
    let _guard = get_test_lock().lock().await;
    let _temp_dir = TempWorkDir::new();
    let mock_server = wiremock::MockServer::start().await;

    mock_delete_query_snippet(42).mount(&mock_server).await;
    mock_delete_query_snippet_not_found(999)
        .mount(&mock_server)
        .await;

    let client = RedashClient::new(mock_server.uri(), "test-key").unwrap();

    std::fs::create_dir_all("snippets").unwrap();
    std::fs::write("snippets/42-stmo_cli_selftest.sql", "SELECT 1").unwrap();
    std::fs::write("snippets/42-stmo_cli_selftest.yaml", "id: 42").unwrap();
    std::fs::write("snippets/999-doomed.sql", "SELECT 1").unwrap();
    std::fs::write("snippets/999-doomed.yaml", "id: 999").unwrap();

    let result = stmo_cli::commands::snippets::delete(&client, vec![42, 999]).await;

    assert!(result.is_err());
    assert!(!std::path::Path::new("snippets/42-stmo_cli_selftest.sql").exists());
    assert!(!std::path::Path::new("snippets/42-stmo_cli_selftest.yaml").exists());
    assert!(std::path::Path::new("snippets/999-doomed.sql").exists());
    assert!(std::path::Path::new("snippets/999-doomed.yaml").exists());
}

#[tokio::test]
async fn test_delete_succeeds_when_no_local_files_found() {
    let _guard = get_test_lock().lock().await;
    let _temp_dir = TempWorkDir::new();
    let mock_server = wiremock::MockServer::start().await;

    mock_delete_query_snippet(42).mount(&mock_server).await;

    let client = RedashClient::new(mock_server.uri(), "test-key").unwrap();

    let result = stmo_cli::commands::snippets::delete(&client, vec![42]).await;

    assert!(result.is_ok());
}

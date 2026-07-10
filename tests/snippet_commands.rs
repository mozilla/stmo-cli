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

// Mirrors init.rs's clean_git_cmd(): strips inherited GIT_DIR/GIT_WORK_TREE/etc so
// git commands in a fresh temp dir aren't redirected at a parent worktree's repo.
fn clean_git_cmd() -> std::process::Command {
    let mut cmd = std::process::Command::new("git");
    cmd.env_remove("GIT_DIR")
        .env_remove("GIT_WORK_TREE")
        .env_remove("GIT_COMMON_DIR")
        .env_remove("GIT_INDEX_FILE");
    cmd
}

fn git_init_and_commit_all(dir: &std::path::Path) {
    clean_git_cmd()
        .arg("init")
        .current_dir(dir)
        .status()
        .unwrap();
    clean_git_cmd()
        .args(["config", "user.name", "Test"])
        .current_dir(dir)
        .status()
        .unwrap();
    clean_git_cmd()
        .args(["config", "user.email", "test@test"])
        .current_dir(dir)
        .status()
        .unwrap();
    clean_git_cmd()
        .args(["add", "."])
        .current_dir(dir)
        .status()
        .unwrap();
    clean_git_cmd()
        .args(["commit", "-m", "Initial commit"])
        .current_dir(dir)
        .status()
        .unwrap();
}

#[tokio::test]
async fn test_list_query_snippets() {
    let mock_server = wiremock::MockServer::start().await;

    mock_list_query_snippets(&serde_json::json!([
        {
            "id": 9,
            "trigger": "hll_convert",
            "description": "Snippet to display hyperloglog field as a numeric count value",
            "snippet": "cardinality(merge(cast(${FIELD_NAME} AS HLL)))",
            "user": null,
            "updated_at": "2018-03-08T02:16:37.962Z",
            "created_at": "2018-03-08T02:16:37.962Z"
        },
        {
            "id": 31,
            "trigger": "reviewbot_e2e_action_ctcs",
            "description": "Action-task gap compression CTEs",
            "snippet": "action_tasks AS (\n    SELECT 1\n)",
            "user": null,
            "updated_at": "2026-06-24T09:13:26.873Z",
            "created_at": "2026-06-24T09:13:26.873Z"
        }
    ]))
    .mount(&mock_server)
    .await;

    let client = RedashClient::new(mock_server.uri(), "test-key").unwrap();
    let snippets = client.list_query_snippets().await.unwrap();

    assert_eq!(snippets.len(), 2);
    assert_eq!(snippets[0].trigger, "hll_convert");
    assert_eq!(snippets[1].id, 31);
    assert_eq!(snippets[1].trigger, "reviewbot_e2e_action_ctcs");
}

#[tokio::test]
async fn test_get_query_snippet() {
    let mock_server = wiremock::MockServer::start().await;

    mock_get_query_snippet(
        31,
        "reviewbot_e2e_action_ctcs",
        "action_tasks AS (\n    SELECT 1\n)",
    )
    .mount(&mock_server)
    .await;

    let client = RedashClient::new(mock_server.uri(), "test-key").unwrap();
    let snippet = client.get_query_snippet(31).await.unwrap();

    assert_eq!(snippet.id, 31);
    assert_eq!(snippet.trigger, "reviewbot_e2e_action_ctcs");
    assert!(snippet.snippet.contains("action_tasks AS"));
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
        31,
        "reviewbot_e2e_action_ctcs",
        "action_tasks AS (\n    SELECT 2\n)",
    )
    .mount(&mock_server)
    .await;

    let client = RedashClient::new(mock_server.uri(), "test-key").unwrap();
    let existing = QuerySnippet {
        id: 31,
        trigger: "reviewbot_e2e_action_ctcs".to_string(),
        description: Some("Action-task gap compression CTEs".to_string()),
        snippet: "action_tasks AS (\n    SELECT 2\n)".to_string(),
        user: None,
        updated_at: "2026-06-24T09:13:26.873Z".to_string(),
        created_at: "2026-06-24T09:13:26.873Z".to_string(),
    };
    let updated = client.update_query_snippet(&existing).await.unwrap();

    assert_eq!(updated.id, 31);
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
        31,
        "reviewbot_e2e_action_ctcs",
        "action_tasks AS (\n    SELECT 1\n)",
    )
    .mount(&mock_server)
    .await;

    let client = RedashClient::new(mock_server.uri(), "test-key").unwrap();
    let result = stmo_cli::commands::snippets::fetch(&client, vec![31], false).await;

    assert!(result.is_ok());
    let sql = std::fs::read_to_string("snippets/31-reviewbot_e2e_action_ctcs.sql").unwrap();
    assert!(sql.contains("action_tasks AS"));
    let yaml = std::fs::read_to_string("snippets/31-reviewbot_e2e_action_ctcs.yaml").unwrap();
    assert!(yaml.contains("id: 31"));
    assert!(yaml.contains("trigger: reviewbot_e2e_action_ctcs"));
}

#[tokio::test]
async fn test_fetch_command_handles_trigger_with_spaces_and_quotes() {
    // Real triggers on sql.telemetry.mozilla.org include e.g. "nan's snippet" and
    // "stefan's date formatter" -- unlike query names/dashboard slugs, triggers are
    // never slugified before being used in a filename here, so this proves that
    // round-trips correctly rather than corrupting the file or the YAML.
    let _guard = get_test_lock().lock().await;
    let _temp_dir = TempWorkDir::new();
    let mock_server = wiremock::MockServer::start().await;

    mock_get_query_snippet(16, "nan's snippet", "parse_date(foo, bar)")
        .mount(&mock_server)
        .await;

    let client = RedashClient::new(mock_server.uri(), "test-key").unwrap();
    let result = stmo_cli::commands::snippets::fetch(&client, vec![16], false).await;
    assert!(result.is_ok());

    let sql_path = std::path::Path::new("snippets/16-nan's snippet.sql");
    let yaml_path = std::path::Path::new("snippets/16-nan's snippet.yaml");
    assert!(sql_path.exists(), "expected {}", sql_path.display());
    assert!(yaml_path.exists(), "expected {}", yaml_path.display());

    let sql = std::fs::read_to_string(sql_path).unwrap();
    assert_eq!(sql, "parse_date(foo, bar)");

    let yaml = std::fs::read_to_string(yaml_path).unwrap();
    let metadata: stmo_cli::models::SnippetMetadata = serde_yaml::from_str(&yaml).unwrap();
    assert_eq!(metadata.id, 16);
    assert_eq!(metadata.trigger, "nan's snippet");

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
async fn test_deploy_existing_snippet_hits_update_path() {
    let _guard = get_test_lock().lock().await;
    let _temp_dir = TempWorkDir::new();
    let mock_server = wiremock::MockServer::start().await;

    mock_update_query_snippet(
        31,
        "reviewbot_e2e_action_ctcs",
        "action_tasks AS (\n    SELECT 2\n)",
    )
    .mount(&mock_server)
    .await;

    let client = RedashClient::new(mock_server.uri(), "test-key").unwrap();

    std::fs::create_dir_all("snippets").unwrap();
    std::fs::write(
        "snippets/31-reviewbot_e2e_action_ctcs.sql",
        "action_tasks AS (\n    SELECT 2\n)",
    )
    .unwrap();
    std::fs::write(
        "snippets/31-reviewbot_e2e_action_ctcs.yaml",
        "id: 31\ntrigger: reviewbot_e2e_action_ctcs\ndescription: Action-task gap compression CTEs\n",
    )
    .unwrap();

    let result = stmo_cli::commands::snippets::deploy(&client, vec![31], false).await;

    assert!(result.is_ok());
    let sql = std::fs::read_to_string("snippets/31-reviewbot_e2e_action_ctcs.sql").unwrap();
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
            "id": 31,
            "trigger": "reviewbot_e2e_action_ctcs",
            "description": "Action-task gap compression CTEs",
            "snippet": "action_tasks AS (\n    SELECT 1\n)",
            "user": null,
            "updated_at": "2026-06-24T09:13:26.873Z",
            "created_at": "2026-06-24T09:13:26.873Z"
        },
        {
            "id": 9,
            "trigger": "hll_convert",
            "description": null,
            "snippet": "cardinality(merge(cast(${FIELD_NAME} AS HLL)))",
            "user": null,
            "updated_at": "2018-03-08T02:16:37.962Z",
            "created_at": "2018-03-08T02:16:37.962Z"
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
        31,
        "reviewbot_e2e_action_ctcs",
        "action_tasks AS (\n    SELECT 1\n)",
    )
    .mount(&mock_server)
    .await;

    let client = RedashClient::new(mock_server.uri(), "test-key").unwrap();
    let result = stmo_cli::commands::snippets::fetch(&client, vec![999, 31], false).await;

    // Matches queries' fetch.rs convention: partial failures are warned about via
    // stderr and skipped, not surfaced as an overall error (unlike dashboards::fetch).
    assert!(result.is_ok());
    assert!(std::path::Path::new("snippets/31-reviewbot_e2e_action_ctcs.sql").exists());
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
    std::fs::write("snippets/31-reviewbot_e2e_action_ctcs.sql", "SELECT 1").unwrap();
    std::fs::write(
        "snippets/31-reviewbot_e2e_action_ctcs.yaml",
        "id: 31\ntrigger: reviewbot_e2e_action_ctcs\ndescription: null\n",
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

#[tokio::test]
async fn test_deploy_returns_ok_when_not_a_git_repo() {
    let _guard = get_test_lock().lock().await;
    let _temp_dir = TempWorkDir::new();
    let mock_server = wiremock::MockServer::start().await;

    let client = RedashClient::new(mock_server.uri(), "test-key").unwrap();

    std::fs::create_dir_all("snippets").unwrap();
    std::fs::write("snippets/31-reviewbot_e2e_action_ctcs.sql", "SELECT 1").unwrap();
    std::fs::write(
        "snippets/31-reviewbot_e2e_action_ctcs.yaml",
        "id: 31\ntrigger: reviewbot_e2e_action_ctcs\ndescription: null\n",
    )
    .unwrap();

    // No git init here: get_changed_snippet_ids() must return None, and deploy()
    // must return Ok(()) without attempting any network call (no mocks mounted).
    let result = stmo_cli::commands::snippets::deploy(&client, vec![], false).await;

    assert!(result.is_ok());
}

#[tokio::test]
async fn test_deploy_returns_ok_when_git_repo_with_no_changes() {
    let _guard = get_test_lock().lock().await;
    let _temp_dir = TempWorkDir::new();
    let mock_server = wiremock::MockServer::start().await;

    let client = RedashClient::new(mock_server.uri(), "test-key").unwrap();

    std::fs::create_dir_all("snippets").unwrap();
    std::fs::write("snippets/31-reviewbot_e2e_action_ctcs.sql", "SELECT 1").unwrap();
    std::fs::write(
        "snippets/31-reviewbot_e2e_action_ctcs.yaml",
        "id: 31\ntrigger: reviewbot_e2e_action_ctcs\ndescription: null\n",
    )
    .unwrap();

    git_init_and_commit_all(&env::current_dir().unwrap());

    // Everything is committed, so git status --porcelain is empty and deploy()
    // must return Ok(()) without attempting any network call (no mocks mounted).
    let result = stmo_cli::commands::snippets::deploy(&client, vec![], false).await;

    assert!(result.is_ok());
}

#[tokio::test]
async fn test_deploy_one_missing_sql_file_bails() {
    let _guard = get_test_lock().lock().await;
    let _temp_dir = TempWorkDir::new();
    let mock_server = wiremock::MockServer::start().await;

    let client = RedashClient::new(mock_server.uri(), "test-key").unwrap();

    std::fs::create_dir_all("snippets").unwrap();
    std::fs::write(
        "snippets/31-reviewbot_e2e_action_ctcs.yaml",
        "id: 31\ntrigger: reviewbot_e2e_action_ctcs\ndescription: null\n",
    )
    .unwrap();

    let result =
        stmo_cli::commands::snippets::deploy_one(&client, 31, "reviewbot_e2e_action_ctcs").await;

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
    std::fs::write("snippets/31-reviewbot_e2e_action_ctcs.sql", "SELECT 1").unwrap();

    let result =
        stmo_cli::commands::snippets::deploy_one(&client, 31, "reviewbot_e2e_action_ctcs").await;

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
    std::fs::write("snippets/31-reviewbot_e2e_action_ctcs.sql", "SELECT 1").unwrap();
    std::fs::write(
        "snippets/31-reviewbot_e2e_action_ctcs.yaml",
        "description: missing required fields\n",
    )
    .unwrap();

    let result =
        stmo_cli::commands::snippets::deploy_one(&client, 31, "reviewbot_e2e_action_ctcs").await;

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

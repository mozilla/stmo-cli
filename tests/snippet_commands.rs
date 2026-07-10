#![allow(clippy::missing_errors_doc)]
#![allow(clippy::missing_panics_doc)]

mod common;

use common::*;
use stmo_cli::api::RedashClient;
use stmo_cli::models::{CreateQuerySnippet, QuerySnippet};

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

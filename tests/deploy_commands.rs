#![allow(clippy::missing_errors_doc)]
#![allow(clippy::missing_panics_doc)]

mod common;

use common::*;
use std::env;
use std::sync::OnceLock;
use stmo_cli::api::RedashClient;
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
async fn test_deploy_new_query_with_id_zero() {
    let _guard = get_test_lock().lock().await;
    let _temp_dir = TempWorkDir::new();
    let mock_server = wiremock::MockServer::start().await;

    mock_create_query(42, "Test Query")
        .mount(&mock_server)
        .await;

    mock_get_query_with_table_viz(42, "Test Query")
        .mount(&mock_server)
        .await;

    let client = RedashClient::new(mock_server.uri(), "test-key").unwrap();

    std::fs::create_dir_all("queries").unwrap();
    std::fs::write("queries/0-test-query.sql", "SELECT 1").unwrap();
    std::fs::write(
        "queries/0-test-query.yaml",
        "id: 0\nname: Test Query\ndescription: null\ndata_source_id: 63\nschedule: null\noptions:\n  parameters: []\nvisualizations: []\ntags: null\n",
    )
    .unwrap();

    let result = stmo_cli::commands::deploy::deploy(&client, vec![0], false).await;

    assert!(result.is_ok(), "Deploy failed: {:?}", result.err());

    assert!(
        !std::path::Path::new("queries/0-test-query.sql").exists(),
        "Old 0-*.sql file should be removed after creation"
    );
    assert!(
        !std::path::Path::new("queries/0-test-query.yaml").exists(),
        "Old 0-*.yaml file should be removed after creation"
    );

    assert!(
        std::path::Path::new("queries/42-test-query.sql").exists(),
        "New SQL file with server ID should be created"
    );
    assert!(
        std::path::Path::new("queries/42-test-query.yaml").exists(),
        "New YAML file with server ID should be created"
    );

    let yaml_content = std::fs::read_to_string("queries/42-test-query.yaml").unwrap();
    assert!(
        yaml_content.contains("id: 42"),
        "YAML should contain the new ID"
    );
}

#[tokio::test]
async fn test_deploy_creates_several_new_queries_in_one_run() {
    let _guard = get_test_lock().lock().await;
    let _temp_dir = TempWorkDir::new();
    let mock_server = wiremock::MockServer::start().await;

    mock_create_query_named(42, "First Query")
        .expect(1)
        .mount(&mock_server)
        .await;
    mock_create_query_named(43, "Second Query")
        .expect(1)
        .mount(&mock_server)
        .await;
    mock_get_query(42, "First Query", false)
        .mount(&mock_server)
        .await;
    mock_get_query(43, "Second Query", false)
        .mount(&mock_server)
        .await;

    let client = RedashClient::new(mock_server.uri(), "test-key").unwrap();

    std::fs::create_dir_all("queries").unwrap();
    write_query_files(0, "first-query", "SELECT 1", "First Query");
    write_query_files(0, "second-query", "SELECT 2", "Second Query");

    let result = stmo_cli::commands::deploy::deploy(&client, vec![], false).await;

    assert!(result.is_ok(), "{:?}", result.unwrap_err());
    assert!(!std::path::Path::new("queries/0-first-query.yaml").exists());
    assert!(!std::path::Path::new("queries/0-second-query.yaml").exists());
    assert!(std::path::Path::new("queries/42-first-query.sql").exists());
    assert!(std::path::Path::new("queries/42-first-query.yaml").exists());
    assert!(std::path::Path::new("queries/43-second-query.sql").exists());
    assert!(std::path::Path::new("queries/43-second-query.yaml").exists());

    mock_server.verify().await;
}

#[tokio::test]
async fn test_deploy_bare_always_includes_id_zero() {
    let _guard = get_test_lock().lock().await;
    let _temp_dir = TempWorkDir::new();
    let mock_server = wiremock::MockServer::start().await;

    // id 0 has nothing on the server yet, so it's always deployed without a
    // comparison GET — no GET mock for it is registered at all.
    mock_create_query(42, "New Query").mount(&mock_server).await;
    mock_get_query_with_table_viz(42, "New Query")
        .mount(&mock_server)
        .await;

    // An unrelated, unchanged tracked query must not be deployed.
    mock_get_query(43, "Unchanged Query", false)
        .mount(&mock_server)
        .await;

    let client = RedashClient::new(mock_server.uri(), "test-key").unwrap();
    std::fs::create_dir_all("queries").unwrap();
    write_query_files(0, "new-query", "SELECT 1", "New Query");
    write_query_files(43, "unchanged-query", "SELECT 1", "Unchanged Query");

    let result = stmo_cli::commands::deploy::deploy(&client, vec![], false).await;
    assert!(result.is_ok(), "Deploy failed: {:?}", result.err());

    assert!(
        std::path::Path::new("queries/42-new-query.sql").exists(),
        "New query should be created and renamed to its server ID"
    );
}

#[tokio::test]
async fn test_deploy_new_query_does_not_duplicate_auto_created_table() {
    let _guard = get_test_lock().lock().await;
    let _temp_dir = TempWorkDir::new();
    let mock_server = wiremock::MockServer::start().await;

    mock_create_query(42, "Test Query")
        .mount(&mock_server)
        .await;

    mock_get_query_with_table_viz(42, "Test Query")
        .mount(&mock_server)
        .await;

    mock_update_visualization(99999)
        .expect(1)
        .mount(&mock_server)
        .await;

    let client = RedashClient::new(mock_server.uri(), "test-key").unwrap();

    std::fs::create_dir_all("queries").unwrap();
    std::fs::write("queries/0-test-query.sql", "SELECT 1").unwrap();
    std::fs::write(
        "queries/0-test-query.yaml",
        "id: 0\nname: Test Query\ndescription: null\ndata_source_id: 63\nschedule: null\noptions:\n  parameters: []\nvisualizations:\n  - id: 0\n    name: Table\n    type: TABLE\n    options: {}\n    description: null\ntags: null\n",
    )
    .unwrap();

    let result = stmo_cli::commands::deploy::deploy(&client, vec![0], false).await;

    assert!(result.is_ok(), "Deploy failed: {:?}", result.err());

    mock_server.verify().await;
}

#[tokio::test]
async fn test_deploy_new_viz_does_not_overwrite_existing() {
    let _guard = get_test_lock().lock().await;
    let _temp_dir = TempWorkDir::new();
    let mock_server = wiremock::MockServer::start().await;

    let vizs = serde_json::json!([
        {"id": 200, "name": "Existing Chart", "type": "CHART", "options": {}, "description": null}
    ]);

    mock_update_query_with_vizs(42, "Test Query", &vizs)
        .mount(&mock_server)
        .await;

    mock_get_query_with_vizs(42, "Test Query", &vizs)
        .mount(&mock_server)
        .await;

    mock_update_visualization(200)
        .expect(1)
        .mount(&mock_server)
        .await;

    mock_create_visualization(300, "New Chart")
        .expect(1)
        .mount(&mock_server)
        .await;

    let client = RedashClient::new(mock_server.uri(), "test-key").unwrap();

    std::fs::create_dir_all("queries").unwrap();
    std::fs::write("queries/42-test-query.sql", "SELECT 1").unwrap();
    std::fs::write(
        "queries/42-test-query.yaml",
        "id: 42\nname: Test Query\ndescription: null\ndata_source_id: 63\nschedule: null\noptions:\n  parameters: []\nvisualizations:\n  - id: 200\n    name: Existing Chart\n    type: CHART\n    options: {}\n    description: null\n  - name: New Chart\n    type: CHART\n    options: {}\n    description: null\ntags: null\n",
    )
    .unwrap();

    let result = stmo_cli::commands::deploy::deploy(&client, vec![42], false).await;
    assert!(result.is_ok(), "Deploy failed: {:?}", result.err());

    mock_server.verify().await;
}

#[tokio::test]
async fn test_deploy_bare_second_run_deploys_nothing() {
    let _guard = get_test_lock().lock().await;
    let _temp_dir = TempWorkDir::new();
    let mock_server = wiremock::MockServer::start().await;

    // Simulate the server's content changing once the query is deployed:
    // the first GET (consumed by the comparison before deploying) still has
    // the old SQL; every GET after that (the refetch inside `deploy_one`,
    // then the comparison on the second `deploy` call) has the new SQL that
    // was just pushed.
    mock_get_query_with_sql(42, "Test Query", "SELECT 1", false)
        .up_to_n_times(1)
        .with_priority(1)
        .mount(&mock_server)
        .await;
    mock_get_query_with_sql(42, "Test Query", "SELECT 2", false)
        .with_priority(2)
        .mount(&mock_server)
        .await;
    mock_update_query_with_vizs(42, "Test Query", &serde_json::json!([]))
        .expect(1)
        .mount(&mock_server)
        .await;

    let client = RedashClient::new(mock_server.uri(), "test-key").unwrap();
    std::fs::create_dir_all("queries").unwrap();
    write_query_files(42, "test-query", "SELECT 2", "Test Query");

    let first = stmo_cli::commands::deploy::deploy(&client, vec![], false).await;
    assert!(first.is_ok(), "First deploy failed: {:?}", first.err());

    let second = stmo_cli::commands::deploy::deploy(&client, vec![], false).await;
    assert!(second.is_ok(), "Second deploy failed: {:?}", second.err());

    // The POST mock has `.expect(1)` — verify() fails if it was hit twice.
    mock_server.verify().await;
}

#[tokio::test]
async fn test_deploy_bare_skips_query_that_404s_without_aborting() {
    let _guard = get_test_lock().lock().await;
    let _temp_dir = TempWorkDir::new();
    let mock_server = wiremock::MockServer::start().await;

    // Query 42 was deleted or archived server-side — comparison fails, but
    // the run must warn and continue rather than aborting.
    mock_get_query_not_found(42).mount(&mock_server).await;

    // Query 43 is genuinely changed and must still be deployed.
    mock_get_query(43, "Changed Query", false)
        .mount(&mock_server)
        .await;
    mock_update_query_with_vizs(43, "Changed Query", &serde_json::json!([]))
        .expect(1)
        .mount(&mock_server)
        .await;

    let client = RedashClient::new(mock_server.uri(), "test-key").unwrap();
    std::fs::create_dir_all("queries").unwrap();
    write_query_files(42, "gone-query", "SELECT 1", "Gone Query");
    write_query_files(43, "changed-query", "SELECT 2", "Changed Query");

    let result = stmo_cli::commands::deploy::deploy(&client, vec![], false).await;
    assert!(result.is_ok(), "Deploy failed: {:?}", result.err());

    mock_server.verify().await;
}

fn write_query_files(id: u64, slug: &str, sql: &str, name: &str) {
    std::fs::write(format!("queries/{id}-{slug}.sql"), sql).unwrap();
    std::fs::write(
        format!("queries/{id}-{slug}.yaml"),
        format!(
            "id: {id}\nname: {name}\ndescription: null\ndata_source_id: 63\nschedule: null\noptions:\n  parameters: []\nvisualizations: []\ntags: null\n"
        ),
    )
    .unwrap();
}

// Bare `deploy` (no explicit IDs, no --all) now decides what to push by
// comparing each tracked query's local content against its server copy
// (see `find_changed_queries` / `tracked_query_differs` in deploy.rs),
// instead of asking git what changed on disk.

#[tokio::test]
async fn test_deploy_bare_skips_query_unchanged_from_server() {
    let _guard = get_test_lock().lock().await;
    let _temp_dir = TempWorkDir::new();
    let mock_server = wiremock::MockServer::start().await;

    // Matches exactly what mock_get_query(42, "Test Query", false) returns —
    // sql "SELECT 1", no description/schedule/tags/visualizations.
    mock_get_query(42, "Test Query", false)
        .mount(&mock_server)
        .await;

    let client = RedashClient::new(mock_server.uri(), "test-key").unwrap();
    std::fs::create_dir_all("queries").unwrap();
    write_query_files(42, "test-query", "SELECT 1", "Test Query");

    // No POST mock is registered for /api/queries/42 — if `deploy` wrongly
    // tried to push this unchanged query, the request would hit no matching
    // mock and the whole call would fail.
    let result = stmo_cli::commands::deploy::deploy(&client, vec![], false).await;
    assert!(result.is_ok(), "Deploy failed: {:?}", result.err());
}

#[tokio::test]
async fn test_deploy_bare_deploys_only_the_changed_query() {
    let _guard = get_test_lock().lock().await;
    let _temp_dir = TempWorkDir::new();
    let mock_server = wiremock::MockServer::start().await;

    // Query 42 is untouched — must not be deployed.
    mock_get_query(42, "Unchanged Query", false)
        .mount(&mock_server)
        .await;

    // Query 43's local SQL differs from the server's — must be deployed.
    mock_get_query(43, "Changed Query", false)
        .mount(&mock_server)
        .await;
    mock_update_query_with_vizs(43, "Changed Query", &serde_json::json!([]))
        .expect(1)
        .mount(&mock_server)
        .await;

    let client = RedashClient::new(mock_server.uri(), "test-key").unwrap();
    std::fs::create_dir_all("queries").unwrap();
    write_query_files(42, "unchanged-query", "SELECT 1", "Unchanged Query");
    write_query_files(43, "changed-query", "SELECT 2", "Changed Query");

    let result = stmo_cli::commands::deploy::deploy(&client, vec![], false).await;
    assert!(result.is_ok(), "Deploy failed: {:?}", result.err());

    mock_server.verify().await;
}

#[tokio::test]
async fn test_deploy_all_flag_deploys_regardless_of_diff() {
    let _guard = get_test_lock().lock().await;
    let _temp_dir = TempWorkDir::new();
    let mock_server = wiremock::MockServer::start().await;

    // Local content matches the server exactly, but `--all` must deploy it
    // anyway — explicit intent wins over comparison.
    mock_get_query(42, "Test Query", false)
        .mount(&mock_server)
        .await;
    mock_update_query_with_vizs(42, "Test Query", &serde_json::json!([]))
        .expect(1)
        .mount(&mock_server)
        .await;

    let client = RedashClient::new(mock_server.uri(), "test-key").unwrap();
    std::fs::create_dir_all("queries").unwrap();
    write_query_files(42, "test-query", "SELECT 1", "Test Query");

    let result = stmo_cli::commands::deploy::deploy(&client, vec![], true).await;
    assert!(result.is_ok(), "Deploy failed: {:?}", result.err());

    mock_server.verify().await;
}

#[tokio::test]
async fn test_deploy_explicit_ids_deploy_regardless_of_diff() {
    let _guard = get_test_lock().lock().await;
    let _temp_dir = TempWorkDir::new();
    let mock_server = wiremock::MockServer::start().await;

    // Same "unchanged" local content as the skip test above, but an explicit
    // ID must deploy it anyway.
    mock_get_query(42, "Test Query", false)
        .mount(&mock_server)
        .await;
    mock_update_query_with_vizs(42, "Test Query", &serde_json::json!([]))
        .expect(1)
        .mount(&mock_server)
        .await;

    let client = RedashClient::new(mock_server.uri(), "test-key").unwrap();
    std::fs::create_dir_all("queries").unwrap();
    write_query_files(42, "test-query", "SELECT 1", "Test Query");

    let result = stmo_cli::commands::deploy::deploy(&client, vec![42], false).await;
    assert!(result.is_ok(), "Deploy failed: {:?}", result.err());

    mock_server.verify().await;
}

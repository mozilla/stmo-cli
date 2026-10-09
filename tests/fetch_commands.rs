#![allow(clippy::missing_errors_doc)]
#![allow(clippy::missing_panics_doc)]

mod common;

use common::*;
use std::env;
use std::sync::OnceLock;
use stmo_cli::api::RedashClient;
use tempfile::TempDir;

static TEST_MUTEX: OnceLock<tokio::sync::Mutex<()>> = OnceLock::new();

fn test_lock() -> &'static tokio::sync::Mutex<()> {
    TEST_MUTEX.get_or_init(|| tokio::sync::Mutex::new(()))
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
        std::fs::create_dir("queries").unwrap();
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

fn write_query_pair(stem: &str, name: &str, sql: &str) {
    std::fs::write(format!("queries/{stem}.sql"), sql).unwrap();
    std::fs::write(
        format!("queries/{stem}.yaml"),
        format!(
            "id: {SAMPLE_QUERY_ID}\nname: {name}\ndescription: null\ndata_source_id: {SAMPLE_DATA_SOURCE_ID}\nschedule: null\noptions:\n  parameters: []\nvisualizations: []\ntags: null\n"
        ),
    )
    .unwrap();
}

#[tokio::test]
async fn fetch_collapses_clean_legacy_pairs_after_a_title_change() {
    let _guard = test_lock().lock().await;
    let _temp_dir = TempWorkDir::new();
    let mock_server = wiremock::MockServer::start().await;

    write_query_pair("1200000001-old-name", "Old Name", "SELECT 1");
    write_query_pair("1200000001-new-name", "New Name", "SELECT 1");
    mock_get_query_with_sql(SAMPLE_QUERY_ID, "New Name", "SELECT 1", false)
        .expect(1)
        .mount(&mock_server)
        .await;

    let client = RedashClient::new(mock_server.uri(), "test-key").unwrap();
    stmo_cli::commands::fetch::fetch(&client, vec![SAMPLE_QUERY_ID], false)
        .await
        .unwrap();

    assert!(std::path::Path::new("queries/1200000001.sql").exists());
    assert!(std::path::Path::new("queries/1200000001.yaml").exists());
    assert!(!std::path::Path::new("queries/1200000001-old-name.sql").exists());
    assert!(!std::path::Path::new("queries/1200000001-old-name.yaml").exists());
    assert!(!std::path::Path::new("queries/1200000001-new-name.sql").exists());
    assert!(!std::path::Path::new("queries/1200000001-new-name.yaml").exists());
}

#[tokio::test]
async fn fetch_leaves_divergent_legacy_files_untouched() {
    let _guard = test_lock().lock().await;
    let _temp_dir = TempWorkDir::new();
    let mock_server = wiremock::MockServer::start().await;

    write_query_pair("1200000001-old-name", "Old Name", "SELECT 2");
    mock_get_query_with_sql(SAMPLE_QUERY_ID, "New Name", "SELECT 1", false)
        .expect(1)
        .mount(&mock_server)
        .await;

    let client = RedashClient::new(mock_server.uri(), "test-key").unwrap();
    let result = stmo_cli::commands::fetch::fetch(&client, vec![SAMPLE_QUERY_ID], false).await;

    assert!(result.is_err());
    assert_eq!(
        std::fs::read_to_string("queries/1200000001-old-name.sql").unwrap(),
        "SELECT 2"
    );
    assert!(std::path::Path::new("queries/1200000001-old-name.yaml").exists());
    assert!(!std::path::Path::new("queries/1200000001.sql").exists());
    assert!(!std::path::Path::new("queries/1200000001.yaml").exists());
}

#[tokio::test]
async fn fetch_updates_name_in_an_id_only_pair() {
    let _guard = test_lock().lock().await;
    let _temp_dir = TempWorkDir::new();
    let mock_server = wiremock::MockServer::start().await;

    write_query_pair("1200000001", "Old Name", "SELECT 1");
    mock_get_query_with_sql(SAMPLE_QUERY_ID, "New Name", "SELECT 1", false)
        .expect(1)
        .mount(&mock_server)
        .await;

    let client = RedashClient::new(mock_server.uri(), "test-key").unwrap();
    stmo_cli::commands::fetch::fetch(&client, vec![SAMPLE_QUERY_ID], false)
        .await
        .unwrap();

    let yaml = std::fs::read_to_string("queries/1200000001.yaml").unwrap();
    assert!(yaml.contains("name: New Name"));
    assert!(std::path::Path::new("queries/1200000001.sql").exists());
    assert_eq!(std::fs::read_dir("queries").unwrap().count(), 2);
}

#[tokio::test]
async fn fetch_all_discovers_id_only_filenames() {
    let _guard = test_lock().lock().await;
    let _temp_dir = TempWorkDir::new();
    let mock_server = wiremock::MockServer::start().await;

    write_query_pair("1200000001", "Example Query", "SELECT 1");
    mock_get_query_with_sql(SAMPLE_QUERY_ID, "Example Query", "SELECT 1", false)
        .expect(1)
        .mount(&mock_server)
        .await;

    let client = RedashClient::new(mock_server.uri(), "test-key").unwrap();
    stmo_cli::commands::fetch::fetch(&client, vec![], true)
        .await
        .unwrap();

    assert!(std::path::Path::new("queries/1200000001.sql").exists());
    assert!(std::path::Path::new("queries/1200000001.yaml").exists());
    mock_server.verify().await;
}

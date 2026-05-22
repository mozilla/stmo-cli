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
async fn test_fetch_with_all_failures_returns_error() {
    let _guard = get_test_lock().lock().await;
    let _temp_dir = TempWorkDir::new();
    let mock_server = wiremock::MockServer::start().await;

    mock_get_dashboard_not_found("firefox-desktop-on-steamos")
        .mount(&mock_server)
        .await;

    mock_get_dashboard_not_found("test-dashboard")
        .mount(&mock_server)
        .await;

    let client = RedashClient::new(mock_server.uri(), "test-key").unwrap();

    let result = stmo_cli::commands::dashboards::fetch(
        &client,
        vec![
            "firefox-desktop-on-steamos".to_string(),
            "test-dashboard".to_string(),
        ],
    )
    .await;

    assert!(result.is_err());
    let error = result.unwrap_err();
    assert!(error.to_string().contains("2 dashboard(s) failed to fetch"));
    assert!(error.to_string().contains("firefox-desktop-on-steamos"));
    assert!(error.to_string().contains("test-dashboard"));
}

#[tokio::test]
async fn test_fetch_with_partial_failures_returns_error() {
    let _guard = get_test_lock().lock().await;
    let _temp_dir = TempWorkDir::new();
    let mock_server = wiremock::MockServer::start().await;

    mock_get_dashboard(2570, "Firefox Desktop on SteamOS", false)
        .mount(&mock_server)
        .await;

    mock_get_dashboard_not_found("test-dashboard")
        .mount(&mock_server)
        .await;

    let client = RedashClient::new(mock_server.uri(), "test-key").unwrap();

    let result = stmo_cli::commands::dashboards::fetch(
        &client,
        vec![
            "firefox-desktop-on-steamos".to_string(),
            "test-dashboard".to_string(),
        ],
    )
    .await;

    assert!(result.is_err());
    let error = result.unwrap_err();
    let error_msg = error.to_string();
    assert!(
        error_msg.contains("dashboard(s) failed to fetch"),
        "Error was: {error_msg}"
    );
    assert!(
        error_msg.contains("test-dashboard"),
        "Error was: {error_msg}"
    );
}

#[tokio::test]
async fn test_fetch_with_all_success_returns_ok() {
    let _guard = get_test_lock().lock().await;
    let _temp_dir = TempWorkDir::new();
    let mock_server = wiremock::MockServer::start().await;

    mock_get_dashboard(2570, "Firefox Desktop on SteamOS", false)
        .mount(&mock_server)
        .await;

    mock_get_dashboard(2558, "Test Dashboard", false)
        .mount(&mock_server)
        .await;

    let client = RedashClient::new(mock_server.uri(), "test-key").unwrap();

    let result = stmo_cli::commands::dashboards::fetch(
        &client,
        vec![
            "firefox-desktop-on-steamos".to_string(),
            "test-dashboard".to_string(),
        ],
    )
    .await;

    assert!(result.is_ok());

    let dashboards_dir = std::path::Path::new("dashboards");
    assert!(dashboards_dir.exists());

    let files: Vec<_> = std::fs::read_dir(dashboards_dir)
        .unwrap()
        .filter_map(std::result::Result::ok)
        .collect();

    assert_eq!(files.len(), 2);

    let yaml_files: Vec<_> = files
        .iter()
        .filter(|e| e.path().extension().is_some_and(|ext| ext == "yaml"))
        .collect();

    assert_eq!(yaml_files.len(), 2);
}

#[tokio::test]
async fn test_archive_with_all_failures_returns_error() {
    let _guard = get_test_lock().lock().await;
    let _temp_dir = TempWorkDir::new();
    let mock_server = wiremock::MockServer::start().await;

    mock_get_dashboard_not_found("firefox-desktop-on-steamos")
        .mount(&mock_server)
        .await;

    mock_get_dashboard_not_found("test-dashboard")
        .mount(&mock_server)
        .await;

    let client = RedashClient::new(mock_server.uri(), "test-key").unwrap();

    let result = stmo_cli::commands::dashboards::archive(
        &client,
        vec![
            "firefox-desktop-on-steamos".to_string(),
            "test-dashboard".to_string(),
        ],
    )
    .await;

    assert!(result.is_err());
    let error = result.unwrap_err();
    assert!(error.to_string().contains("2 dashboard(s) failed to"));
}

#[tokio::test]
async fn test_unarchive_with_failures_returns_error() {
    let _guard = get_test_lock().lock().await;
    let _temp_dir = TempWorkDir::new();
    let mock_server = wiremock::MockServer::start().await;

    mock_get_dashboard(2570, "Firefox Desktop on SteamOS", true)
        .mount(&mock_server)
        .await;

    mock_unarchive_dashboard_forbidden(2570)
        .mount(&mock_server)
        .await;

    mock_get_dashboard(2558, "Test Dashboard", true)
        .mount(&mock_server)
        .await;

    mock_unarchive_dashboard(2558, "Test Dashboard")
        .mount(&mock_server)
        .await;

    let client = RedashClient::new(mock_server.uri(), "test-key").unwrap();

    let result = stmo_cli::commands::dashboards::unarchive(
        &client,
        vec![
            "firefox-desktop-on-steamos".to_string(),
            "test-dashboard".to_string(),
        ],
    )
    .await;

    assert!(result.is_err());
    let error = result.unwrap_err();
    assert!(
        error
            .to_string()
            .contains("1 dashboard(s) failed to unarchive")
    );
    assert!(error.to_string().contains("firefox-desktop-on-steamos"));
}

#[tokio::test]
async fn test_fetch_with_triple_dash_slug() {
    let _guard = get_test_lock().lock().await;
    let _temp_dir = TempWorkDir::new();
    let mock_server = wiremock::MockServer::start().await;

    mock_get_dashboard_with_slug(
        2_006_698,
        "Bug 2006698 - ccov build regression",
        "bug-2006698---ccov-build-regression",
        false,
    )
    .mount(&mock_server)
    .await;

    let client = RedashClient::new(mock_server.uri(), "test-key").unwrap();

    let result = stmo_cli::commands::dashboards::fetch(
        &client,
        vec!["bug-2006698---ccov-build-regression".to_string()],
    )
    .await;

    assert!(result.is_ok());

    let dashboards_dir = std::path::Path::new("dashboards");
    assert!(dashboards_dir.exists());

    let expected_file = dashboards_dir.join("2006698-bug-2006698---ccov-build-regression.yaml");
    assert!(
        expected_file.exists(),
        "Expected file {expected_file:?} to exist"
    );

    let yaml_content = std::fs::read_to_string(&expected_file).unwrap();
    assert!(yaml_content.contains("slug: bug-2006698---ccov-build-regression"));
    assert!(yaml_content.contains("Bug 2006698 - ccov build regression"));
}

#[tokio::test]
async fn test_deploy_with_triple_dash_slug() {
    let _guard = get_test_lock().lock().await;
    let _temp_dir = TempWorkDir::new();
    let mock_server = wiremock::MockServer::start().await;

    mock_get_dashboard_with_slug(
        2_006_698,
        "Bug 2006698 - ccov build regression",
        "bug-2006698---ccov-build-regression",
        false,
    )
    .mount(&mock_server)
    .await;

    mock_update_dashboard(2_006_698, "Bug 2006698 - ccov build regression")
        .mount(&mock_server)
        .await;

    // Re-fetch uses the original slug
    mock_get_dashboard_with_slug(
        2_006_698,
        "Bug 2006698 - ccov build regression",
        "bug-2006698---ccov-build-regression",
        false,
    )
    .mount(&mock_server)
    .await;

    let client = RedashClient::new(mock_server.uri(), "test-key").unwrap();

    std::fs::create_dir_all("dashboards").unwrap();

    let yaml_content = r"id: 2006698
name: Bug 2006698 - ccov build regression
slug: bug-2006698---ccov-build-regression
user_id: 530
is_draft: false
is_archived: false
dashboard_filters_enabled: false
tags: []
widgets: []
";
    std::fs::write(
        "dashboards/2006698-bug-2006698---ccov-build-regression.yaml",
        yaml_content,
    )
    .unwrap();

    let result = stmo_cli::commands::dashboards::deploy(
        &client,
        vec!["bug-2006698---ccov-build-regression".to_string()],
        false,
    )
    .await;

    assert!(result.is_ok(), "Deploy failed: {:?}", result.err());
}

#[tokio::test]
async fn test_archive_with_triple_dash_slug() {
    let _guard = get_test_lock().lock().await;
    let _temp_dir = TempWorkDir::new();
    let mock_server = wiremock::MockServer::start().await;

    mock_get_dashboard_with_slug(
        2_006_698,
        "Bug 2006698 - ccov build regression",
        "bug-2006698---ccov-build-regression",
        false,
    )
    .mount(&mock_server)
    .await;

    mock_archive_dashboard(2_006_698).mount(&mock_server).await;

    let client = RedashClient::new(mock_server.uri(), "test-key").unwrap();

    std::fs::create_dir_all("dashboards").unwrap();
    let yaml_file = "dashboards/2006698-bug-2006698---ccov-build-regression.yaml";
    std::fs::write(yaml_file, "test content").unwrap();

    assert!(std::path::Path::new(yaml_file).exists());

    let result = stmo_cli::commands::dashboards::archive(
        &client,
        vec!["bug-2006698---ccov-build-regression".to_string()],
    )
    .await;

    assert!(result.is_ok());
    assert!(
        !std::path::Path::new(yaml_file).exists(),
        "File should be deleted after archiving"
    );
}

#[tokio::test]
async fn test_deploy_new_dashboard_with_id_zero() {
    let _guard = get_test_lock().lock().await;
    let _temp_dir = TempWorkDir::new();
    let mock_server = wiremock::MockServer::start().await;

    mock_create_dashboard(2621, "My New Dashboard", "my-new-dashboard")
        .mount(&mock_server)
        .await;

    mock_favorite_dashboard("my-new-dashboard")
        .mount(&mock_server)
        .await;

    mock_update_dashboard(2621, "My New Dashboard")
        .mount(&mock_server)
        .await;

    // Re-fetch uses the slug returned by the create response
    mock_get_dashboard_with_slug(2621, "My New Dashboard", "my-new-dashboard", false)
        .mount(&mock_server)
        .await;

    let client = RedashClient::new(mock_server.uri(), "test-key").unwrap();

    std::fs::create_dir_all("dashboards").unwrap();

    let yaml_content = "id: 0
name: My New Dashboard
slug: my-new-dashboard
user_id: 0
is_draft: true
is_archived: false
dashboard_filters_enabled: false
tags: []
widgets: []
";
    std::fs::write("dashboards/0-my-new-dashboard.yaml", yaml_content).unwrap();

    let result = stmo_cli::commands::dashboards::deploy(
        &client,
        vec!["my-new-dashboard".to_string()],
        false,
    )
    .await;

    assert!(result.is_ok(), "Deploy failed: {:?}", result.err());

    // Old file should be deleted
    assert!(
        !std::path::Path::new("dashboards/0-my-new-dashboard.yaml").exists(),
        "Old 0-*.yaml file should be removed after creation"
    );

    // New file with server-assigned ID should exist
    assert!(
        std::path::Path::new("dashboards/2621-my-new-dashboard.yaml").exists(),
        "New file with server ID should be created"
    );
}

#[tokio::test]
async fn test_deploy_auto_populates_parameter_mappings() {
    let _guard = get_test_lock().lock().await;
    let _temp_dir = TempWorkDir::new();
    let mock_server = wiremock::MockServer::start().await;

    let dashboard_id = 2570_u64;
    let query_id = 12345_u64;
    let slug = "my-parameterized-dashboard";

    mock_get_dashboard_with_slug(dashboard_id, "My Parameterized Dashboard", slug, false)
        .mount(&mock_server)
        .await;

    mock_get_query_with_parameters(
        query_id,
        "My Query",
        &[("channel", "enum"), ("date", "date")],
    )
    .mount(&mock_server)
    .await;

    mock_create_widget(dashboard_id, 99001)
        .mount(&mock_server)
        .await;

    mock_update_dashboard(dashboard_id, "My Parameterized Dashboard")
        .mount(&mock_server)
        .await;

    mock_get_dashboard_with_slug(dashboard_id, "My Parameterized Dashboard", slug, false)
        .mount(&mock_server)
        .await;

    let client = RedashClient::new(mock_server.uri(), "test-key").unwrap();

    std::fs::create_dir_all("dashboards").unwrap();

    let yaml_content = format!(
        "id: {dashboard_id}
name: My Parameterized Dashboard
slug: {slug}
user_id: 530
is_draft: false
is_archived: false
dashboard_filters_enabled: false
tags: []
widgets:
  - id: 0
    visualization_id: 279588
    query_id: {query_id}
    visualization_name: My Viz
    text: ''
    options:
      position:
        col: 0
        row: 0
        sizeX: 3
        sizeY: 8
"
    );
    std::fs::write(
        format!("dashboards/{dashboard_id}-{slug}.yaml"),
        yaml_content,
    )
    .unwrap();

    let result =
        stmo_cli::commands::dashboards::deploy(&client, vec![slug.to_string()], false).await;

    assert!(result.is_ok(), "Deploy failed: {:?}", result.err());

    let received = mock_server.received_requests().await.unwrap();

    let widget_create_req = received
        .iter()
        .find(|r| r.method.as_str() == "POST" && r.url.path() == "/api/widgets")
        .expect("Expected widget create request");

    let body: serde_json::Value = serde_json::from_slice(&widget_create_req.body).unwrap();
    let param_mappings = &body["options"]["parameterMappings"];

    assert!(
        param_mappings.is_object(),
        "parameterMappings should be an object, got: {param_mappings}"
    );
    assert_eq!(param_mappings["channel"]["type"], "dashboard-level");
    assert_eq!(param_mappings["channel"]["mapTo"], "channel");
    assert_eq!(param_mappings["date"]["type"], "dashboard-level");
    assert_eq!(param_mappings["date"]["mapTo"], "date");

    let dashboard_update_req = received
        .iter()
        .find(|r| {
            r.method.as_str() == "POST" && r.url.path() == format!("/api/dashboards/{dashboard_id}")
        })
        .expect("Expected dashboard update request");

    let update_body: serde_json::Value =
        serde_json::from_slice(&dashboard_update_req.body).unwrap();
    assert_eq!(
        update_body["dashboard_filters_enabled"], true,
        "dashboard_filters_enabled should be true when widgets have parameters"
    );
}

#[tokio::test]
async fn test_deploy_resolves_visualization_id_from_query_and_name() {
    let _guard = get_test_lock().lock().await;
    let _temp_dir = TempWorkDir::new();
    let mock_server = wiremock::MockServer::start().await;

    let dashboard_id = 2570_u64;
    let query_id = 12345_u64;
    let slug = "my-dashboard";
    let vizs = serde_json::json!([
        {"id": 55555, "name": "My Chart", "type": "CHART", "options": {}, "description": null},
        {"id": 55556, "name": "Table", "type": "TABLE", "options": {}, "description": null}
    ]);

    mock_get_dashboard_with_slug(dashboard_id, "My Dashboard", slug, false)
        .mount(&mock_server)
        .await;

    mock_get_query_with_vizs(query_id, "My Query", &vizs)
        .mount(&mock_server)
        .await;

    mock_create_widget(dashboard_id, 99001)
        .mount(&mock_server)
        .await;

    mock_update_dashboard(dashboard_id, "My Dashboard")
        .mount(&mock_server)
        .await;

    mock_get_dashboard_with_slug(dashboard_id, "My Dashboard", slug, false)
        .mount(&mock_server)
        .await;

    let client = RedashClient::new(mock_server.uri(), "test-key").unwrap();

    std::fs::create_dir_all("dashboards").unwrap();

    let yaml_content = format!(
        "id: {dashboard_id}
name: My Dashboard
slug: {slug}
user_id: 530
is_draft: false
is_archived: false
dashboard_filters_enabled: false
tags: []
widgets:
  - id: 0
    query_id: {query_id}
    visualization_name: My Chart
    options:
      position:
        col: 0
        row: 0
        sizeX: 3
        sizeY: 8
"
    );
    std::fs::write(
        format!("dashboards/{dashboard_id}-{slug}.yaml"),
        yaml_content,
    )
    .unwrap();

    let result =
        stmo_cli::commands::dashboards::deploy(&client, vec![slug.to_string()], false).await;

    assert!(result.is_ok(), "Deploy failed: {:?}", result.err());

    let received = mock_server.received_requests().await.unwrap();

    let widget_create_req = received
        .iter()
        .find(|r| r.method.as_str() == "POST" && r.url.path() == "/api/widgets")
        .expect("Expected widget create request");

    let body: serde_json::Value = serde_json::from_slice(&widget_create_req.body).unwrap();
    assert_eq!(
        body["visualization_id"], 55555,
        "visualization_id should be resolved from query_id + visualization_name, got: {}",
        body["visualization_id"]
    );
}

#[tokio::test]
async fn test_deploy_fails_when_visualization_name_not_found() {
    let _guard = get_test_lock().lock().await;
    let _temp_dir = TempWorkDir::new();
    let mock_server = wiremock::MockServer::start().await;

    let dashboard_id = 2570_u64;
    let query_id = 12345_u64;
    let slug = "my-dashboard";
    let vizs = serde_json::json!([
        {"id": 99999, "name": "Table", "type": "TABLE", "options": {}, "description": null}
    ]);

    mock_get_dashboard_with_slug(dashboard_id, "My Dashboard", slug, false)
        .mount(&mock_server)
        .await;

    mock_get_query_with_vizs(query_id, "My Query", &vizs)
        .mount(&mock_server)
        .await;

    let client = RedashClient::new(mock_server.uri(), "test-key").unwrap();

    std::fs::create_dir_all("dashboards").unwrap();

    let yaml_content = format!(
        "id: {dashboard_id}
name: My Dashboard
slug: {slug}
user_id: 530
is_draft: false
is_archived: false
dashboard_filters_enabled: false
tags: []
widgets:
  - id: 0
    query_id: {query_id}
    visualization_name: Nonexistent
    options:
      position:
        col: 0
        row: 0
        sizeX: 3
        sizeY: 8
"
    );
    std::fs::write(
        format!("dashboards/{dashboard_id}-{slug}.yaml"),
        yaml_content,
    )
    .unwrap();

    let result =
        stmo_cli::commands::dashboards::deploy(&client, vec![slug.to_string()], false).await;

    assert!(result.is_err(), "Expected deploy to fail");
    let err = result.unwrap_err().to_string();
    assert!(err.contains("failed to deploy"), "Unexpected error: {err}");
    assert!(err.contains("my-dashboard"), "Unexpected error: {err}");
}

#[tokio::test]
async fn test_deploy_updates_existing_widgets() {
    let _guard = get_test_lock().lock().await;
    let _temp_dir = TempWorkDir::new();
    let mock_server = wiremock::MockServer::start().await;

    let dashboard_id = 2570_u64;
    let query_id = 12345_u64;
    let widget_id = 75035_u64;
    let slug = "my-dashboard";
    let vizs = serde_json::json!([
        {"id": 55557, "name": "Updated Chart", "type": "CHART", "options": {}, "description": null},
        {"id": 55556, "name": "Table", "type": "TABLE", "options": {}, "description": null}
    ]);

    // First GET: server dashboard already has the existing widget
    wiremock::Mock::given(wiremock::matchers::method("GET"))
        .and(wiremock::matchers::path(format!("/api/dashboards/{slug}")))
        .respond_with(wiremock::ResponseTemplate::new(200).set_body_json(
            serde_json::json!({
                "id": dashboard_id,
                "name": "My Dashboard",
                "slug": slug,
                "user_id": 530,
                "is_archived": false,
                "is_draft": false,
                "dashboard_filters_enabled": false,
                "tags": [],
                "widgets": [{
                    "id": widget_id,
                    "dashboard_id": dashboard_id,
                    "width": 1,
                    "visualization_id": 55556,
                    "visualization": {"id": 55556, "name": "Table", "query": {"id": query_id, "name": "My Query"}},
                    "text": "",
                    "options": {"position": {"col": 0, "row": 0, "sizeX": 3, "sizeY": 2}}
                }]
            })
        ))
        .mount(&mock_server)
        .await;

    mock_get_query_with_vizs(query_id, "My Query", &vizs)
        .mount(&mock_server)
        .await;

    mock_update_widget(widget_id, dashboard_id)
        .mount(&mock_server)
        .await;

    mock_update_dashboard(dashboard_id, "My Dashboard")
        .mount(&mock_server)
        .await;

    // Second GET: re-fetch after deploy
    mock_get_dashboard_with_slug(dashboard_id, "My Dashboard", slug, false)
        .mount(&mock_server)
        .await;

    let client = RedashClient::new(mock_server.uri(), "test-key").unwrap();

    std::fs::create_dir_all("dashboards").unwrap();

    let yaml_content = format!(
        "id: {dashboard_id}
name: My Dashboard
slug: {slug}
user_id: 530
is_draft: false
is_archived: false
dashboard_filters_enabled: false
tags: []
widgets:
  - id: {widget_id}
    query_id: {query_id}
    visualization_name: Updated Chart
    options:
      position:
        col: 3
        row: 5
        sizeX: 6
        sizeY: 4
"
    );
    std::fs::write(
        format!("dashboards/{dashboard_id}-{slug}.yaml"),
        yaml_content,
    )
    .unwrap();

    let result =
        stmo_cli::commands::dashboards::deploy(&client, vec![slug.to_string()], false).await;

    assert!(result.is_ok(), "Deploy failed: {:?}", result.err());

    let received = mock_server.received_requests().await.unwrap();

    let widget_update_req = received.iter().find(|r| {
        r.method.as_str() == "POST" && r.url.path() == format!("/api/widgets/{widget_id}")
    });

    assert!(
        widget_update_req.is_some(),
        "Expected POST /api/widgets/{widget_id} but got: {:?}",
        received
            .iter()
            .map(|r| format!("{} {}", r.method, r.url.path()))
            .collect::<Vec<_>>()
    );

    let body: serde_json::Value = serde_json::from_slice(&widget_update_req.unwrap().body).unwrap();
    assert_eq!(
        body["visualization_id"], 55557,
        "visualization_id should resolve to Updated Chart"
    );
    assert_eq!(body["options"]["position"]["col"], 3);
    assert_eq!(body["options"]["position"]["row"], 5);
}

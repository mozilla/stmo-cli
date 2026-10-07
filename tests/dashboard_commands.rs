#![allow(clippy::missing_errors_doc)]
#![allow(clippy::missing_panics_doc)]

mod common;

use common::*;
use std::env;
use std::sync::{Arc, Mutex as StdMutex, OnceLock};
use stmo_cli::api::RedashClient;
use tempfile::TempDir;
use tokio::sync::Mutex;
use wiremock::{Mock, Request, Respond, ResponseTemplate};

const SOURCE_QUERY_ID: u64 = 1_200_000_101;
const TARGET_QUERY_ID: u64 = 1_200_000_102;
const SOURCE_VISUALIZATION_ID: u64 = 1_300_000_101;
const TARGET_VISUALIZATION_ID: u64 = 1_300_000_102;
const TABLE_VISUALIZATION_ID: u64 = 1_300_000_103;

struct DashboardStateResponder {
    dashboard_id: u64,
    slug: String,
    widgets: Arc<StdMutex<Vec<serde_json::Value>>>,
}

impl Respond for DashboardStateResponder {
    fn respond(&self, _request: &Request) -> ResponseTemplate {
        ResponseTemplate::new(200).set_body_json(serde_json::json!({
            "id": self.dashboard_id,
            "name": "My Dashboard",
            "slug": self.slug,
            "user_id": SAMPLE_USER_ID,
            "is_archived": false,
            "is_draft": false,
            "dashboard_filters_enabled": false,
            "tags": [],
            "widgets": self.widgets.lock().unwrap().clone()
        }))
    }
}

struct CreateWidgetStateResponder {
    dashboard_id: u64,
    widgets: Arc<StdMutex<Vec<serde_json::Value>>>,
    fail_creation: bool,
}

impl Respond for CreateWidgetStateResponder {
    fn respond(&self, request: &Request) -> ResponseTemplate {
        if self.fail_creation {
            return ResponseTemplate::new(500);
        }

        let body: serde_json::Value = request.body_json().unwrap();
        let visualization_id = body["visualization_id"].as_u64().unwrap();
        let (query_id, name) = if visualization_id == TARGET_VISUALIZATION_ID {
            (TARGET_QUERY_ID, "New Chart")
        } else {
            (SOURCE_QUERY_ID, "Old Chart")
        };
        let widget = serde_json::json!({
            "id": SAMPLE_CREATED_WIDGET_ID,
            "dashboard_id": self.dashboard_id,
            "width": body["width"],
            "visualization_id": visualization_id,
            "visualization": {
                "id": visualization_id,
                "name": name,
                "query": {"id": query_id, "name": "My Query"}
            },
            "text": body["text"],
            "options": body["options"]
        });
        self.widgets.lock().unwrap().push(widget.clone());
        ResponseTemplate::new(200).set_body_json(widget)
    }
}

struct UpdateWidgetStateResponder {
    widgets: Arc<StdMutex<Vec<serde_json::Value>>>,
}

impl Respond for UpdateWidgetStateResponder {
    fn respond(&self, request: &Request) -> ResponseTemplate {
        let body: serde_json::Value = request.body_json().unwrap();
        let widget_id: u64 = request
            .url
            .path()
            .rsplit('/')
            .next()
            .unwrap()
            .parse()
            .unwrap();
        let mut widgets = self.widgets.lock().unwrap();
        let widget = widgets
            .iter_mut()
            .find(|widget| widget["id"] == widget_id)
            .unwrap();
        widget["text"] = body["text"].clone();
        widget["options"] = body["options"].clone();
        ResponseTemplate::new(200).set_body_json(widget.clone())
    }
}

struct DeleteWidgetStateResponder {
    widgets: Arc<StdMutex<Vec<serde_json::Value>>>,
    fail_widget_id: Option<u64>,
}

impl Respond for DeleteWidgetStateResponder {
    fn respond(&self, request: &Request) -> ResponseTemplate {
        let widget_id: u64 = request
            .url
            .path()
            .rsplit('/')
            .next()
            .unwrap()
            .parse()
            .unwrap();
        if self.fail_widget_id == Some(widget_id) {
            return ResponseTemplate::new(500);
        }
        self.widgets
            .lock()
            .unwrap()
            .retain(|widget| widget["id"] != widget_id);
        ResponseTemplate::new(204)
    }
}

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

struct QueryReassignmentFixture {
    _temp_dir: TempWorkDir,
    widgets: Arc<StdMutex<Vec<serde_json::Value>>>,
    client: RedashClient,
    yaml_path: String,
    slug: String,
}

async fn query_reassignment_fixture(
    fail_creation: bool,
    fail_delete_widget_id: Option<u64>,
) -> QueryReassignmentFixture {
    let temp_dir = TempWorkDir::new();
    let mock_server = wiremock::MockServer::start().await;
    let dashboard_id = SAMPLE_DASHBOARD_ID;
    let old_widget_id = SAMPLE_WIDGET_ID;
    let source_query_id = SOURCE_QUERY_ID;
    let target_query_id = TARGET_QUERY_ID;
    let slug = "query-reassignment-dashboard";
    let widgets = Arc::new(StdMutex::new(vec![serde_json::json!({
        "id": old_widget_id,
        "dashboard_id": dashboard_id,
        "width": 1,
        "visualization_id": SOURCE_VISUALIZATION_ID,
        "visualization": {
            "id": SOURCE_VISUALIZATION_ID,
            "name": "Old Chart",
            "query": {"id": source_query_id, "name": "Query A"}
        },
        "text": "",
        "options": {"position": {"col": 0, "row": 0, "sizeX": 3, "sizeY": 2}}
    })]));

    Mock::given(wiremock::matchers::method("GET"))
        .and(wiremock::matchers::path(format!("/api/dashboards/{slug}")))
        .respond_with(DashboardStateResponder {
            dashboard_id,
            slug: slug.to_string(),
            widgets: Arc::clone(&widgets),
        })
        .mount(&mock_server)
        .await;

    let query_b_visualizations = serde_json::json!([
        {"id": TARGET_VISUALIZATION_ID, "name": "New Chart", "type": "CHART", "options": {}, "description": null}
    ]);
    mock_get_query_with_vizs(target_query_id, "Query B", &query_b_visualizations)
        .mount(&mock_server)
        .await;

    Mock::given(wiremock::matchers::method("POST"))
        .and(wiremock::matchers::path("/api/widgets"))
        .respond_with(CreateWidgetStateResponder {
            dashboard_id,
            widgets: Arc::clone(&widgets),
            fail_creation,
        })
        .mount(&mock_server)
        .await;

    Mock::given(wiremock::matchers::method("POST"))
        .and(wiremock::matchers::path(format!(
            "/api/widgets/{old_widget_id}"
        )))
        .respond_with(UpdateWidgetStateResponder {
            widgets: Arc::clone(&widgets),
        })
        .mount(&mock_server)
        .await;

    Mock::given(wiremock::matchers::method("DELETE"))
        .and(wiremock::matchers::path_regex(r"/api/widgets/\d+"))
        .respond_with(DeleteWidgetStateResponder {
            widgets: Arc::clone(&widgets),
            fail_widget_id: fail_delete_widget_id,
        })
        .mount(&mock_server)
        .await;

    mock_update_dashboard(dashboard_id, "My Dashboard")
        .mount(&mock_server)
        .await;

    std::fs::create_dir_all("dashboards").unwrap();
    let yaml_content = format!(
        "id: {dashboard_id}
name: My Dashboard
slug: {slug}
user_id: {SAMPLE_USER_ID}
is_draft: false
is_archived: false
dashboard_filters_enabled: false
tags: []
widgets:
  - id: {old_widget_id}
    visualization_id: {SOURCE_VISUALIZATION_ID}
    query_id: {target_query_id}
    visualization_name: New Chart
    options:
      position:
        col: 3
        row: 5
        sizeX: 6
        sizeY: 4
"
    );
    let yaml_path = format!("dashboards/{dashboard_id}-{slug}.yaml");
    std::fs::write(&yaml_path, yaml_content).unwrap();
    let client = RedashClient::new(mock_server.uri(), "test-key").unwrap();

    QueryReassignmentFixture {
        _temp_dir: temp_dir,
        widgets,
        client,
        yaml_path,
        slug: slug.to_string(),
    }
}

#[tokio::test]
async fn test_fetch_with_all_failures_returns_error() {
    let _guard = get_test_lock().lock().await;
    let _temp_dir = TempWorkDir::new();
    let mock_server = wiremock::MockServer::start().await;

    mock_get_dashboard_not_found("example-dashboard")
        .mount(&mock_server)
        .await;

    mock_get_dashboard_not_found("test-dashboard")
        .mount(&mock_server)
        .await;

    let client = RedashClient::new(mock_server.uri(), "test-key").unwrap();

    let result = stmo_cli::commands::dashboards::fetch(
        &client,
        vec![
            "example-dashboard".to_string(),
            "test-dashboard".to_string(),
        ],
    )
    .await;

    assert!(result.is_err());
    let error = result.unwrap_err();
    assert!(error.to_string().contains("2 dashboard(s) failed to fetch"));
    assert!(error.to_string().contains("example-dashboard"));
    assert!(error.to_string().contains("test-dashboard"));
}

#[tokio::test]
async fn test_fetch_with_partial_failures_returns_error() {
    let _guard = get_test_lock().lock().await;
    let _temp_dir = TempWorkDir::new();
    let mock_server = wiremock::MockServer::start().await;

    mock_get_dashboard(SAMPLE_DASHBOARD_ID, "Example Dashboard", false)
        .mount(&mock_server)
        .await;

    mock_get_dashboard_not_found("test-dashboard")
        .mount(&mock_server)
        .await;

    let client = RedashClient::new(mock_server.uri(), "test-key").unwrap();

    let result = stmo_cli::commands::dashboards::fetch(
        &client,
        vec![
            "example-dashboard".to_string(),
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

    mock_get_dashboard(SAMPLE_DASHBOARD_ID, "Example Dashboard", false)
        .mount(&mock_server)
        .await;

    mock_get_dashboard(SAMPLE_SECOND_DASHBOARD_ID, "Test Dashboard", false)
        .mount(&mock_server)
        .await;

    let client = RedashClient::new(mock_server.uri(), "test-key").unwrap();

    let result = stmo_cli::commands::dashboards::fetch(
        &client,
        vec![
            "example-dashboard".to_string(),
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

    mock_get_dashboard_not_found("example-dashboard")
        .mount(&mock_server)
        .await;

    mock_get_dashboard_not_found("test-dashboard")
        .mount(&mock_server)
        .await;

    let client = RedashClient::new(mock_server.uri(), "test-key").unwrap();

    let result = stmo_cli::commands::dashboards::archive(
        &client,
        vec![
            "example-dashboard".to_string(),
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

    mock_get_dashboard(SAMPLE_DASHBOARD_ID, "Example Dashboard", true)
        .mount(&mock_server)
        .await;

    mock_unarchive_dashboard_forbidden(SAMPLE_DASHBOARD_ID)
        .mount(&mock_server)
        .await;

    mock_get_dashboard(SAMPLE_SECOND_DASHBOARD_ID, "Test Dashboard", true)
        .mount(&mock_server)
        .await;

    mock_unarchive_dashboard(SAMPLE_SECOND_DASHBOARD_ID, "Test Dashboard")
        .mount(&mock_server)
        .await;

    let client = RedashClient::new(mock_server.uri(), "test-key").unwrap();

    let result = stmo_cli::commands::dashboards::unarchive(
        &client,
        vec![
            "example-dashboard".to_string(),
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
    assert!(error.to_string().contains("example-dashboard"));
}

#[tokio::test]
async fn test_fetch_with_triple_dash_slug() {
    let _guard = get_test_lock().lock().await;
    let _temp_dir = TempWorkDir::new();
    let mock_server = wiremock::MockServer::start().await;

    mock_get_dashboard_with_slug(
        SAMPLE_DASHBOARD_ID,
        "Example - Dashboard",
        "example---dashboard",
        false,
    )
    .mount(&mock_server)
    .await;

    let client = RedashClient::new(mock_server.uri(), "test-key").unwrap();

    let result =
        stmo_cli::commands::dashboards::fetch(&client, vec!["example---dashboard".to_string()])
            .await;

    assert!(result.is_ok());

    let dashboards_dir = std::path::Path::new("dashboards");
    assert!(dashboards_dir.exists());

    let expected_file =
        dashboards_dir.join(format!("{SAMPLE_DASHBOARD_ID}-example---dashboard.yaml"));
    assert!(
        expected_file.exists(),
        "Expected file {expected_file:?} to exist"
    );

    let yaml_content = std::fs::read_to_string(&expected_file).unwrap();
    assert!(yaml_content.contains("slug: example---dashboard"));
    assert!(yaml_content.contains("Example - Dashboard"));
}

#[tokio::test]
async fn test_deploy_with_triple_dash_slug() {
    let _guard = get_test_lock().lock().await;
    let _temp_dir = TempWorkDir::new();
    let mock_server = wiremock::MockServer::start().await;

    mock_get_dashboard_with_slug(
        SAMPLE_DASHBOARD_ID,
        "Example - Dashboard",
        "example---dashboard",
        false,
    )
    .mount(&mock_server)
    .await;

    mock_update_dashboard(SAMPLE_DASHBOARD_ID, "Example - Dashboard")
        .mount(&mock_server)
        .await;

    // Re-fetch uses the original slug
    mock_get_dashboard_with_slug(
        SAMPLE_DASHBOARD_ID,
        "Example - Dashboard",
        "example---dashboard",
        false,
    )
    .mount(&mock_server)
    .await;

    let client = RedashClient::new(mock_server.uri(), "test-key").unwrap();

    std::fs::create_dir_all("dashboards").unwrap();

    let yaml_content = format!(
        "id: {SAMPLE_DASHBOARD_ID}
name: Example - Dashboard
slug: example---dashboard
user_id: {SAMPLE_USER_ID}
is_draft: false
is_archived: false
dashboard_filters_enabled: false
tags: []
widgets: []
"
    );
    std::fs::write(
        format!("dashboards/{SAMPLE_DASHBOARD_ID}-example---dashboard.yaml"),
        yaml_content,
    )
    .unwrap();

    let result = stmo_cli::commands::dashboards::deploy(
        &client,
        vec!["example---dashboard".to_string()],
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
        SAMPLE_DASHBOARD_ID,
        "Example - Dashboard",
        "example---dashboard",
        false,
    )
    .mount(&mock_server)
    .await;

    mock_archive_dashboard(SAMPLE_DASHBOARD_ID)
        .mount(&mock_server)
        .await;

    let client = RedashClient::new(mock_server.uri(), "test-key").unwrap();

    std::fs::create_dir_all("dashboards").unwrap();
    let yaml_file = format!("dashboards/{SAMPLE_DASHBOARD_ID}-example---dashboard.yaml");
    std::fs::write(&yaml_file, "test content").unwrap();

    assert!(std::path::Path::new(&yaml_file).exists());

    let result =
        stmo_cli::commands::dashboards::archive(&client, vec!["example---dashboard".to_string()])
            .await;

    assert!(result.is_ok());
    assert!(
        !std::path::Path::new(&yaml_file).exists(),
        "File should be deleted after archiving"
    );
}

#[tokio::test]
async fn test_deploy_new_dashboard_with_id_zero() {
    let _guard = get_test_lock().lock().await;
    let _temp_dir = TempWorkDir::new();
    let mock_server = wiremock::MockServer::start().await;

    mock_create_dashboard(
        SAMPLE_CREATED_DASHBOARD_ID,
        "My New Dashboard",
        "my-new-dashboard",
    )
    .mount(&mock_server)
    .await;

    mock_favorite_dashboard("my-new-dashboard")
        .mount(&mock_server)
        .await;

    mock_update_dashboard(SAMPLE_CREATED_DASHBOARD_ID, "My New Dashboard")
        .mount(&mock_server)
        .await;

    // Re-fetch uses the slug returned by the create response
    mock_get_dashboard_with_slug(
        SAMPLE_CREATED_DASHBOARD_ID,
        "My New Dashboard",
        "my-new-dashboard",
        false,
    )
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
        std::path::Path::new(&format!(
            "dashboards/{SAMPLE_CREATED_DASHBOARD_ID}-my-new-dashboard.yaml"
        ))
        .exists(),
        "New file with server ID should be created"
    );
}

#[tokio::test]
async fn test_deploy_auto_populates_parameter_mappings() {
    let _guard = get_test_lock().lock().await;
    let _temp_dir = TempWorkDir::new();
    let mock_server = wiremock::MockServer::start().await;

    let dashboard_id = SAMPLE_DASHBOARD_ID;
    let query_id = SAMPLE_QUERY_ID;
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

    mock_create_widget(dashboard_id, SAMPLE_CREATED_WIDGET_ID)
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
user_id: {SAMPLE_USER_ID}
is_draft: false
is_archived: false
dashboard_filters_enabled: false
tags: []
widgets:
  - id: 0
    visualization_id: {SAMPLE_VISUALIZATION_ID}
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

    let dashboard_id = SAMPLE_DASHBOARD_ID;
    let query_id = SAMPLE_QUERY_ID;
    let slug = "my-dashboard";
    let vizs = serde_json::json!([
        {"id": SAMPLE_VISUALIZATION_ID, "name": "My Chart", "type": "CHART", "options": {}, "description": null},
        {"id": SOURCE_VISUALIZATION_ID, "name": "Table", "type": "TABLE", "options": {}, "description": null}
    ]);

    mock_get_dashboard_with_slug(dashboard_id, "My Dashboard", slug, false)
        .mount(&mock_server)
        .await;

    mock_get_query_with_vizs(query_id, "My Query", &vizs)
        .mount(&mock_server)
        .await;

    mock_create_widget(dashboard_id, SAMPLE_CREATED_WIDGET_ID)
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
user_id: {SAMPLE_USER_ID}
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
        body["visualization_id"], SAMPLE_VISUALIZATION_ID,
        "visualization_id should be resolved from query_id + visualization_name, got: {}",
        body["visualization_id"]
    );
}

#[tokio::test]
async fn test_deploy_fails_when_visualization_name_not_found() {
    let _guard = get_test_lock().lock().await;
    let _temp_dir = TempWorkDir::new();
    let mock_server = wiremock::MockServer::start().await;

    let dashboard_id = SAMPLE_DASHBOARD_ID;
    let query_id = SAMPLE_QUERY_ID;
    let slug = "my-dashboard";
    let vizs = serde_json::json!([
        {"id": TABLE_VISUALIZATION_ID, "name": "Table", "type": "TABLE", "options": {}, "description": null}
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
user_id: {SAMPLE_USER_ID}
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

    let dashboard_id = SAMPLE_DASHBOARD_ID;
    let query_id = SAMPLE_QUERY_ID;
    let widget_id = SAMPLE_WIDGET_ID;
    let slug = "my-dashboard";
    let vizs = serde_json::json!([
        {"id": TARGET_VISUALIZATION_ID, "name": "Updated Chart", "type": "CHART", "options": {}, "description": null},
        {"id": SOURCE_VISUALIZATION_ID, "name": "Table", "type": "TABLE", "options": {}, "description": null}
    ]);

    // First GET: server dashboard already has the existing widget
    wiremock::Mock::given(wiremock::matchers::method("GET"))
        .and(wiremock::matchers::path(format!("/api/dashboards/{slug}")))
        .respond_with(wiremock::ResponseTemplate::new(200).set_body_json(
            serde_json::json!({
                "id": dashboard_id,
                "name": "My Dashboard",
                "slug": slug,
                "user_id": SAMPLE_USER_ID,
                "is_archived": false,
                "is_draft": false,
                "dashboard_filters_enabled": false,
                "tags": [],
                "widgets": [{
                    "id": widget_id,
                    "dashboard_id": dashboard_id,
                    "width": 1,
                    "visualization_id": TARGET_VISUALIZATION_ID,
                    "visualization": {"id": TARGET_VISUALIZATION_ID, "name": "Updated Chart", "query": {"id": query_id, "name": "My Query"}},
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
user_id: {SAMPLE_USER_ID}
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
        body["visualization_id"], TARGET_VISUALIZATION_ID,
        "visualization_id should resolve to Updated Chart"
    );
    assert_eq!(body["options"]["position"]["col"], 3);
    assert_eq!(body["options"]["position"]["row"], 5);
}

#[tokio::test]
async fn test_deploy_recreates_widget_when_query_changes() {
    let _guard = get_test_lock().lock().await;
    let fixture = query_reassignment_fixture(false, None).await;

    let result =
        stmo_cli::commands::dashboards::deploy(&fixture.client, vec![fixture.slug.clone()], false)
            .await;
    assert!(result.is_ok(), "Deploy failed: {:?}", result.err());

    let current_widgets = fixture.widgets.lock().unwrap();
    assert_eq!(current_widgets.len(), 1, "old widget should be replaced");
    assert_eq!(current_widgets[0]["id"], SAMPLE_CREATED_WIDGET_ID);
    assert_eq!(
        current_widgets[0]["visualization"]["query"]["id"],
        TARGET_QUERY_ID
    );
    assert_eq!(current_widgets[0]["visualization"]["name"], "New Chart");
    drop(current_widgets);

    let saved_yaml = std::fs::read_to_string(fixture.yaml_path).unwrap();
    let saved: serde_yaml::Value = serde_yaml::from_str(&saved_yaml).unwrap();
    let saved_widget = &saved["widgets"][0];
    assert_eq!(saved_widget["id"].as_u64(), Some(SAMPLE_CREATED_WIDGET_ID));
    assert_eq!(saved_widget["query_id"].as_u64(), Some(TARGET_QUERY_ID));
    assert_eq!(
        saved_widget["visualization_name"].as_str(),
        Some("New Chart")
    );
}

#[tokio::test]
async fn test_deploy_keeps_existing_widget_when_replacement_creation_fails() {
    let _guard = get_test_lock().lock().await;
    let fixture = query_reassignment_fixture(true, None).await;

    let result =
        stmo_cli::commands::dashboards::deploy(&fixture.client, vec![fixture.slug.clone()], false)
            .await;

    assert!(
        result.is_err(),
        "replacement creation failure should fail deploy"
    );
    let widgets = fixture.widgets.lock().unwrap();
    assert_eq!(widgets.len(), 1);
    assert_eq!(widgets[0]["id"], SAMPLE_WIDGET_ID);
    assert_eq!(widgets[0]["visualization"]["query"]["id"], SOURCE_QUERY_ID);
}

#[tokio::test]
async fn test_deploy_cleans_up_replacement_when_old_widget_deletion_fails() {
    let _guard = get_test_lock().lock().await;
    let fixture = query_reassignment_fixture(false, Some(SAMPLE_WIDGET_ID)).await;

    let result =
        stmo_cli::commands::dashboards::deploy(&fixture.client, vec![fixture.slug.clone()], false)
            .await;

    assert!(
        result.is_err(),
        "old widget deletion failure should fail deploy"
    );
    let widgets = fixture.widgets.lock().unwrap();
    assert_eq!(widgets.len(), 1, "replacement should be cleaned up");
    assert_eq!(widgets[0]["id"], SAMPLE_WIDGET_ID);
    assert_eq!(widgets[0]["visualization"]["query"]["id"], SOURCE_QUERY_ID);
}

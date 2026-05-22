#![allow(clippy::missing_errors_doc)]
#![allow(clippy::missing_panics_doc)]
#![allow(dead_code)]

use std::fs;
use std::path::PathBuf;
use tempfile::TempDir;
use wiremock::matchers::{method, path};
use wiremock::{Mock, MockServer, ResponseTemplate};

pub struct TestContext {
    pub mock_server: MockServer,
    pub temp_dir: TempDir,
    pub queries_dir: PathBuf,
}

impl TestContext {
    pub async fn new() -> Self {
        let mock_server = MockServer::start().await;
        let temp_dir = TempDir::new().unwrap();
        let queries_dir = temp_dir.path().join("queries");
        fs::create_dir(&queries_dir).unwrap();

        Self {
            mock_server,
            temp_dir,
            queries_dir,
        }
    }

    pub fn base_url(&self) -> String {
        self.mock_server.uri()
    }

    pub fn create_query_files(&self, id: u64, slug: &str, sql: &str, yaml_content: &str) {
        let sql_path = self.queries_dir.join(format!("{id}-{slug}.sql"));
        let yaml_path = self.queries_dir.join(format!("{id}-{slug}.yaml"));

        fs::write(sql_path, sql).unwrap();
        fs::write(yaml_path, yaml_content).unwrap();
    }
}

pub fn mock_refresh_query(query_id: u64, job_id: &str) -> Mock {
    Mock::given(method("POST"))
        .and(path(format!("/api/queries/{query_id}/results")))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
            "job": {
                "id": job_id,
                "status": 1,
                "query_result_id": null,
                "error": null
            }
        })))
}

pub fn mock_poll_job_pending(job_id: &str) -> Mock {
    Mock::given(method("GET"))
        .and(path(format!("/api/jobs/{job_id}")))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
            "job": {
                "id": job_id,
                "status": 1,
                "query_result_id": null,
                "error": null
            }
        })))
}

pub fn mock_poll_job_success(job_id: &str, result_id: u64) -> Mock {
    Mock::given(method("GET"))
        .and(path(format!("/api/jobs/{job_id}")))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
            "job": {
                "id": job_id,
                "status": 3,
                "query_result_id": result_id,
                "error": null
            }
        })))
}

pub fn mock_poll_job_failure(job_id: &str, error_msg: &str) -> Mock {
    Mock::given(method("GET"))
        .and(path(format!("/api/jobs/{job_id}")))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
            "job": {
                "id": job_id,
                "status": 4,
                "query_result_id": null,
                "error": error_msg
            }
        })))
}

pub fn mock_get_query_result(query_id: u64, result_id: u64) -> Mock {
    Mock::given(method("GET"))
        .and(path(format!(
            "/api/queries/{query_id}/results/{result_id}.json"
        )))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
            "query_result": {
                "id": result_id,
                "data": {
                    "columns": [
                        {"name": "col1", "type": "string"},
                        {"name": "col2", "type": "integer"}
                    ],
                    "rows": [
                        {"col1": "value1", "col2": 123},
                        {"col1": "value2", "col2": 456}
                    ]
                },
                "runtime": 1.5,
                "retrieved_at": "2026-01-21T10:00:00"
            }
        })))
}

pub fn mock_list_my_queries(page: u32, page_size: u32, total_count: u64) -> Mock {
    Mock::given(method("GET"))
        .and(path("/api/queries/my"))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
            "results": [],
            "count": total_count,
            "page": page,
            "page_size": page_size
        })))
}

pub fn mock_list_data_sources() -> Mock {
    Mock::given(method("GET"))
        .and(path("/api/data_sources"))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!([
            {
                "id": 63,
                "name": "Telemetry (BigQuery)",
                "type": "bigquery",
                "description": null,
                "syntax": "sql",
                "paused": 0,
                "pause_reason": null,
                "view_only": false,
                "queue_name": "bq_queries",
                "scheduled_queue_name": "bq_scheduled_queries",
                "groups": {"2": false},
                "options": {}
            },
            {
                "id": 10,
                "name": "Redash metadata",
                "type": "pg",
                "description": null,
                "syntax": "sql",
                "paused": 0,
                "pause_reason": null,
                "view_only": false,
                "queue_name": "queries",
                "scheduled_queue_name": "scheduled_queries",
                "groups": {"2": false},
                "options": {}
            }
        ])))
}

pub fn mock_list_data_sources_empty() -> Mock {
    Mock::given(method("GET"))
        .and(path("/api/data_sources"))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!([])))
}

pub fn mock_get_data_source(data_source_id: u64) -> Mock {
    Mock::given(method("GET"))
        .and(path(format!("/api/data_sources/{data_source_id}")))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
            "id": data_source_id,
            "name": "Test Data Source",
            "type": "bigquery",
            "description": "Test description",
            "syntax": "sql",
            "paused": 0,
            "pause_reason": null,
            "view_only": false,
            "queue_name": "queries",
            "scheduled_queue_name": "scheduled_queries",
            "groups": {},
            "options": {}
        })))
}

pub fn mock_get_data_source_not_found(data_source_id: u64) -> Mock {
    Mock::given(method("GET"))
        .and(path(format!("/api/data_sources/{data_source_id}")))
        .respond_with(ResponseTemplate::new(404))
}

pub fn mock_get_data_source_schema(data_source_id: u64) -> Mock {
    Mock::given(method("GET"))
        .and(path(format!("/api/data_sources/{data_source_id}/schema")))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
            "schema": [
                {
                    "name": "table1",
                    "columns": [
                        {"name": "col1", "type": "STRING"},
                        {"name": "col2", "type": "INTEGER"},
                        {"name": "col3", "type": "BOOLEAN"}
                    ]
                },
                {
                    "name": "table2",
                    "columns": [
                        {"name": "id", "type": "INTEGER"},
                        {"name": "name", "type": "STRING"}
                    ]
                }
            ]
        })))
}

pub fn mock_get_data_source_schema_unauthorized(data_source_id: u64) -> Mock {
    Mock::given(method("GET"))
        .and(path(format!("/api/data_sources/{data_source_id}/schema")))
        .respond_with(ResponseTemplate::new(403))
}

pub fn mock_get_query(query_id: u64, name: &str, is_archived: bool) -> Mock {
    Mock::given(method("GET"))
        .and(path(format!("/api/queries/{query_id}")))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
            "id": query_id,
            "name": name,
            "description": null,
            "query": "SELECT 1",
            "data_source_id": 63,
            "user": null,
            "schedule": null,
            "options": {"parameters": []},
            "visualizations": [],
            "tags": null,
            "is_archived": is_archived,
            "is_draft": false,
            "updated_at": "2026-01-21T10:00:00",
            "created_at": "2026-01-21T10:00:00"
        })))
}

pub fn mock_get_query_with_parameters(
    query_id: u64,
    name: &str,
    parameters: &[(&str, &str)],
) -> Mock {
    let params: Vec<serde_json::Value> = parameters
        .iter()
        .map(|(param_name, param_type)| {
            serde_json::json!({
                "name": param_name,
                "title": param_name,
                "type": param_type,
            })
        })
        .collect();

    Mock::given(method("GET"))
        .and(path(format!("/api/queries/{query_id}")))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
            "id": query_id,
            "name": name,
            "description": null,
            "query": "SELECT 1",
            "data_source_id": 63,
            "user": null,
            "schedule": null,
            "options": {"parameters": params},
            "visualizations": [],
            "tags": null,
            "is_archived": false,
            "is_draft": false,
            "updated_at": "2026-01-21T10:00:00",
            "created_at": "2026-01-21T10:00:00"
        })))
}

pub fn mock_get_query_with_table_viz(query_id: u64, name: &str) -> Mock {
    Mock::given(method("GET"))
        .and(path(format!("/api/queries/{query_id}")))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
            "id": query_id,
            "name": name,
            "description": null,
            "query": "SELECT 1",
            "data_source_id": 63,
            "user": null,
            "schedule": null,
            "options": {"parameters": []},
            "visualizations": [
                {
                    "id": 99999,
                    "name": "Table",
                    "type": "TABLE",
                    "options": {},
                    "description": null
                }
            ],
            "tags": null,
            "is_archived": false,
            "is_draft": false,
            "updated_at": "2026-01-21T10:00:00",
            "created_at": "2026-01-21T10:00:00"
        })))
}

pub fn mock_update_visualization(viz_id: u64) -> Mock {
    Mock::given(method("POST"))
        .and(path(format!("/api/visualizations/{viz_id}")))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
            "id": viz_id,
            "name": "Table",
            "type": "TABLE",
            "options": {},
            "description": null
        })))
}

pub fn mock_archive_query(query_id: u64, name: &str) -> Mock {
    Mock::given(method("POST"))
        .and(path(format!("/api/queries/{query_id}")))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
            "id": query_id,
            "name": name,
            "description": null,
            "query": "SELECT 1",
            "data_source_id": 63,
            "user": null,
            "schedule": null,
            "options": {"parameters": []},
            "visualizations": [],
            "tags": null,
            "is_archived": true,
            "is_draft": false,
            "updated_at": "2026-01-21T10:00:00",
            "created_at": "2026-01-21T10:00:00"
        })))
}

pub fn mock_unarchive_query(query_id: u64, name: &str) -> Mock {
    Mock::given(method("POST"))
        .and(path(format!("/api/queries/{query_id}")))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
            "id": query_id,
            "name": name,
            "description": null,
            "query": "SELECT 1",
            "data_source_id": 63,
            "user": null,
            "schedule": null,
            "options": {"parameters": []},
            "visualizations": [],
            "tags": null,
            "is_archived": false,
            "is_draft": false,
            "updated_at": "2026-01-21T10:00:00",
            "created_at": "2026-01-21T10:00:00"
        })))
}

pub fn mock_archive_query_not_found(query_id: u64) -> Mock {
    Mock::given(method("POST"))
        .and(path(format!("/api/queries/{query_id}")))
        .respond_with(ResponseTemplate::new(404))
}

pub fn mock_unarchive_query_forbidden(query_id: u64) -> Mock {
    Mock::given(method("POST"))
        .and(path(format!("/api/queries/{query_id}")))
        .respond_with(ResponseTemplate::new(403))
}

pub fn mock_create_query(id: u64, name: &str) -> Mock {
    Mock::given(method("POST"))
        .and(path("/api/queries"))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
            "id": id,
            "name": name,
            "description": null,
            "query": "SELECT 1",
            "data_source_id": 63,
            "user": null,
            "schedule": null,
            "options": {"parameters": []},
            "visualizations": [],
            "tags": null,
            "is_archived": false,
            "is_draft": false,
            "updated_at": "2026-01-21T10:00:00",
            "created_at": "2026-01-21T10:00:00"
        })))
}

pub fn mock_create_dashboard(id: u64, name: &str, slug: &str) -> Mock {
    Mock::given(method("POST"))
        .and(path("/api/dashboards"))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
            "id": id,
            "name": name,
            "slug": slug,
            "user_id": 530,
            "is_archived": false,
            "is_draft": true,
            "dashboard_filters_enabled": false,
            "tags": [],
            "widgets": null
        })))
}

pub fn mock_list_favorite_dashboards(count: u64) -> Mock {
    Mock::given(method("GET"))
        .and(path("/api/dashboards/favorites"))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
            "results": [
                {
                    "id": 2570,
                    "name": "Firefox Desktop on SteamOS",
                    "slug": "firefox-desktop-on-steamos",
                    "is_draft": false,
                    "is_archived": false
                },
                {
                    "id": 2558,
                    "name": "Test Dashboard",
                    "slug": "test-dashboard",
                    "is_draft": false,
                    "is_archived": false
                }
            ],
            "count": count
        })))
}

pub fn mock_list_favorite_dashboards_empty() -> Mock {
    Mock::given(method("GET"))
        .and(path("/api/dashboards/favorites"))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
            "results": [],
            "count": 0
        })))
}

pub fn mock_get_dashboard(id: u64, name: &str, is_archived: bool) -> Mock {
    let slug = name.to_lowercase().replace(' ', "-");
    mock_get_dashboard_with_slug(id, name, &slug, is_archived)
}

pub fn mock_get_dashboard_with_slug(id: u64, name: &str, slug: &str, is_archived: bool) -> Mock {
    Mock::given(method("GET"))
        .and(path(format!("/api/dashboards/{slug}")))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
            "id": id,
            "name": name,
            "slug": slug,
            "user_id": 530,
            "is_archived": is_archived,
            "is_draft": false,
            "dashboard_filters_enabled": false,
            "tags": [],
            "widgets": []
        })))
}

pub fn mock_get_dashboard_by_id(id: u64, name: &str, slug: &str, is_archived: bool) -> Mock {
    Mock::given(method("GET"))
        .and(path(format!("/api/dashboards/{id}")))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
            "id": id,
            "name": name,
            "slug": slug,
            "user_id": 530,
            "is_archived": is_archived,
            "is_draft": false,
            "dashboard_filters_enabled": false,
            "tags": [],
            "widgets": []
        })))
}

pub fn mock_get_dashboard_not_found(slug: &str) -> Mock {
    Mock::given(method("GET"))
        .and(path(format!("/api/dashboards/{slug}")))
        .respond_with(ResponseTemplate::new(404))
}

pub fn mock_update_dashboard(id: u64, name: &str) -> Mock {
    Mock::given(method("POST"))
        .and(path(format!("/api/dashboards/{id}")))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
            "id": id,
            "name": name,
            "slug": name.to_lowercase().replace(' ', "-"),
            "user_id": 530,
            "is_archived": false,
            "is_draft": false,
            "dashboard_filters_enabled": false,
            "tags": [],
            "widgets": []
        })))
}

pub fn mock_archive_dashboard(id: u64) -> Mock {
    Mock::given(method("POST"))
        .and(path(format!("/api/dashboards/{id}")))
        .respond_with(ResponseTemplate::new(200))
}

pub fn mock_archive_dashboard_not_found(id: u64) -> Mock {
    Mock::given(method("POST"))
        .and(path(format!("/api/dashboards/{id}")))
        .respond_with(ResponseTemplate::new(404))
}

pub fn mock_unarchive_dashboard(id: u64, name: &str) -> Mock {
    Mock::given(method("POST"))
        .and(path(format!("/api/dashboards/{id}")))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
            "id": id,
            "name": name,
            "slug": name.to_lowercase().replace(' ', "-"),
            "user_id": 530,
            "is_archived": false,
            "is_draft": false,
            "dashboard_filters_enabled": false,
            "tags": [],
            "widgets": []
        })))
}

pub fn mock_unarchive_dashboard_forbidden(id: u64) -> Mock {
    Mock::given(method("POST"))
        .and(path(format!("/api/dashboards/{id}")))
        .respond_with(ResponseTemplate::new(403))
}

pub fn mock_create_widget(dashboard_id: u64, widget_id: u64) -> Mock {
    Mock::given(method("POST"))
        .and(path("/api/widgets"))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
            "id": widget_id,
            "dashboard_id": dashboard_id,
            "width": 1,
            "visualization_id": null,
            "visualization": null,
            "text": "Test Widget",
            "options": {
                "position": {
                    "col": 0,
                    "row": 0,
                    "sizeX": 3,
                    "sizeY": 2
                }
            }
        })))
}

pub fn mock_update_widget(widget_id: u64, dashboard_id: u64) -> Mock {
    Mock::given(method("POST"))
        .and(path(format!("/api/widgets/{widget_id}")))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
            "id": widget_id,
            "dashboard_id": dashboard_id,
            "width": 1,
            "visualization_id": null,
            "visualization": null,
            "text": "",
            "options": {
                "position": { "col": 0, "row": 0, "sizeX": 3, "sizeY": 2 }
            }
        })))
}

pub fn mock_delete_widget(widget_id: u64) -> Mock {
    Mock::given(method("DELETE"))
        .and(path(format!("/api/widgets/{widget_id}")))
        .respond_with(ResponseTemplate::new(204))
}

pub fn mock_delete_widget_not_found(widget_id: u64) -> Mock {
    Mock::given(method("DELETE"))
        .and(path(format!("/api/widgets/{widget_id}")))
        .respond_with(ResponseTemplate::new(404))
}

pub fn mock_refresh_query_bad_request(query_id: u64, error_message: &str) -> Mock {
    Mock::given(method("POST"))
        .and(path(format!("/api/queries/{query_id}/results")))
        .respond_with(
            ResponseTemplate::new(400).set_body_json(serde_json::json!({"message": error_message})),
        )
}

pub fn mock_favorite_dashboard(slug: &str) -> Mock {
    Mock::given(method("POST"))
        .and(path(format!("/api/dashboards/{slug}/favorite")))
        .respond_with(ResponseTemplate::new(200))
}

pub fn mock_refresh_query_forbidden(query_id: u64) -> Mock {
    Mock::given(method("POST"))
        .and(path(format!("/api/queries/{query_id}/results")))
        .respond_with(
            ResponseTemplate::new(403)
                .set_body_json(serde_json::json!({"message": "Access denied"})),
        )
}

pub fn mock_create_visualization(viz_id: u64, name: &str) -> Mock {
    Mock::given(method("POST"))
        .and(path("/api/visualizations"))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
            "id": viz_id,
            "name": name,
            "type": "CHART",
            "options": {},
            "description": null
        })))
}

pub fn mock_update_query_with_vizs(query_id: u64, name: &str, vizs: &serde_json::Value) -> Mock {
    Mock::given(method("POST"))
        .and(path(format!("/api/queries/{query_id}")))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
            "id": query_id,
            "name": name,
            "description": null,
            "query": "SELECT 1",
            "data_source_id": 63,
            "user": null,
            "schedule": null,
            "options": {"parameters": []},
            "visualizations": vizs,
            "tags": null,
            "is_archived": false,
            "is_draft": false,
            "updated_at": "2026-01-21T10:00:00",
            "created_at": "2026-01-21T10:00:00"
        })))
}

pub fn mock_get_query_with_vizs(query_id: u64, name: &str, vizs: &serde_json::Value) -> Mock {
    Mock::given(method("GET"))
        .and(path(format!("/api/queries/{query_id}")))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
            "id": query_id,
            "name": name,
            "description": null,
            "query": "SELECT 1",
            "data_source_id": 63,
            "user": null,
            "schedule": null,
            "options": {"parameters": []},
            "visualizations": vizs,
            "tags": null,
            "is_archived": false,
            "is_draft": false,
            "updated_at": "2026-01-21T10:00:00",
            "created_at": "2026-01-21T10:00:00"
        })))
}

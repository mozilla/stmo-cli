#![allow(clippy::missing_errors_doc)]
#![allow(clippy::missing_panics_doc)]

mod common;

use common::*;
use stmo_cli::api::RedashClient;
use wiremock::matchers::{method, path, query_param};
use wiremock::{Mock, MockServer, ResponseTemplate};

#[tokio::test]
async fn test_refresh_query_success() {
    let mock_server = MockServer::start().await;

    mock_refresh_query(123, "test-job-id")
        .mount(&mock_server)
        .await;

    let client = RedashClient::new(mock_server.uri(), "test-key").unwrap();
    let job = client.refresh_query(123, None).await.unwrap();

    assert_eq!(job.id, "test-job-id");
    assert_eq!(job.status, 1);
    assert!(job.query_result_id.is_none());
}

#[tokio::test]
async fn test_refresh_query_with_parameters() {
    let mock_server = MockServer::start().await;

    Mock::given(method("POST"))
        .and(path("/api/queries/123/results"))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
            "job": {
                "id": "test-job-id",
                "status": 1,
                "query_result_id": null,
                "error": null
            }
        })))
        .mount(&mock_server)
        .await;

    let client = RedashClient::new(mock_server.uri(), "test-key").unwrap();
    let mut params = std::collections::HashMap::new();
    params.insert("start_date".to_string(), serde_json::json!("2025-01-01"));
    params.insert(
        "channels".to_string(),
        serde_json::json!(["release", "beta"]),
    );

    let job = client.refresh_query(123, Some(params)).await.unwrap();

    assert_eq!(job.id, "test-job-id");
    assert_eq!(job.status, 1);
}

#[tokio::test]
async fn test_poll_job_pending() {
    let mock_server = MockServer::start().await;

    mock_poll_job_pending("test-job-id")
        .mount(&mock_server)
        .await;

    let client = RedashClient::new(mock_server.uri(), "test-key").unwrap();
    let job = client.poll_job("test-job-id").await.unwrap();

    assert_eq!(job.id, "test-job-id");
    assert_eq!(job.status, 1);
    assert!(job.query_result_id.is_none());
}

#[tokio::test]
async fn test_poll_job_success() {
    let mock_server = MockServer::start().await;

    mock_poll_job_success("test-job-id", 456)
        .mount(&mock_server)
        .await;

    let client = RedashClient::new(mock_server.uri(), "test-key").unwrap();
    let job = client.poll_job("test-job-id").await.unwrap();

    assert_eq!(job.id, "test-job-id");
    assert_eq!(job.status, 3);
    assert_eq!(job.query_result_id, Some(456));
}

#[tokio::test]
async fn test_poll_job_failure() {
    let mock_server = MockServer::start().await;

    mock_poll_job_failure("test-job-id", "Query execution failed")
        .mount(&mock_server)
        .await;

    let client = RedashClient::new(mock_server.uri(), "test-key").unwrap();
    let job = client.poll_job("test-job-id").await.unwrap();

    assert_eq!(job.id, "test-job-id");
    assert_eq!(job.status, 4);
    assert_eq!(job.error, Some("Query execution failed".to_string()));
}

#[tokio::test]
async fn test_get_query_result_success() {
    let mock_server = MockServer::start().await;

    mock_get_query_result(123, 456).mount(&mock_server).await;

    let client = RedashClient::new(mock_server.uri(), "test-key").unwrap();
    let result = client.get_query_result(123, 456).await.unwrap();

    assert_eq!(result.id, 456);
    assert_eq!(result.data.columns.len(), 2);
    assert_eq!(result.data.columns[0].name, "col1");
    assert_eq!(result.data.rows.len(), 2);
    assert!((result.runtime - 1.5).abs() < f64::EPSILON);
}

#[tokio::test]
async fn test_execute_query_with_polling_success() {
    let mock_server = MockServer::start().await;

    mock_refresh_query(123, "test-job-id")
        .expect(1)
        .mount(&mock_server)
        .await;

    mock_poll_job_success("test-job-id", 456)
        .expect(1)
        .mount(&mock_server)
        .await;

    mock_get_query_result(123, 456)
        .expect(1)
        .mount(&mock_server)
        .await;

    let client = RedashClient::new(mock_server.uri(), "test-key").unwrap();
    let result = client
        .execute_query_with_polling(123, None, 10, 100)
        .await
        .unwrap();

    assert_eq!(result.id, 456);
    assert_eq!(result.data.columns.len(), 2);
    assert_eq!(result.data.rows.len(), 2);
}

#[tokio::test]
async fn test_execute_query_with_polling_failure() {
    let mock_server = MockServer::start().await;

    mock_refresh_query(123, "test-job-id")
        .mount(&mock_server)
        .await;

    mock_poll_job_failure("test-job-id", "Syntax error in SQL")
        .mount(&mock_server)
        .await;

    let client = RedashClient::new(mock_server.uri(), "test-key").unwrap();
    let result = client.execute_query_with_polling(123, None, 10, 100).await;

    assert!(result.is_err());
    let err = result.unwrap_err();
    assert!(err.to_string().contains("Query execution failed"));
    assert!(err.to_string().contains("Syntax error in SQL"));
}

#[tokio::test]
async fn test_execute_query_with_polling_timeout() {
    let mock_server = MockServer::start().await;

    mock_refresh_query(123, "test-job-id")
        .mount(&mock_server)
        .await;

    mock_poll_job_pending("test-job-id")
        .mount(&mock_server)
        .await;

    let client = RedashClient::new(mock_server.uri(), "test-key").unwrap();
    let result = client.execute_query_with_polling(123, None, 1, 100).await;

    assert!(result.is_err());
    let err = result.unwrap_err();
    assert!(err.to_string().contains("timed out"));
}

#[tokio::test]
async fn test_execute_query_with_polling_timeout_cancels_abandoned_job() {
    let mock_server = MockServer::start().await;

    mock_refresh_query(123, "test-job-id")
        .mount(&mock_server)
        .await;

    mock_poll_job_pending("test-job-id")
        .mount(&mock_server)
        .await;

    mock_cancel_job("test-job-id")
        .expect(1)
        .mount(&mock_server)
        .await;

    let client = RedashClient::new(mock_server.uri(), "test-key").unwrap();
    let result = client.execute_query_with_polling(123, None, 1, 100).await;

    assert!(result.is_err());
    assert!(result.unwrap_err().to_string().contains("timed out"));
}

#[tokio::test]
async fn test_execute_query_with_polling_poll_error_cancels_abandoned_job() {
    let mock_server = MockServer::start().await;

    mock_refresh_query(123, "test-job-id")
        .mount(&mock_server)
        .await;

    mock_poll_job_server_error("test-job-id")
        .mount(&mock_server)
        .await;

    mock_cancel_job("test-job-id")
        .expect(1)
        .mount(&mock_server)
        .await;

    let client = RedashClient::new(mock_server.uri(), "test-key").unwrap();
    let result = client.execute_query_with_polling(123, None, 10, 100).await;

    assert!(result.is_err());
}

#[tokio::test]
async fn test_execute_query_with_polling_success_does_not_cancel_job() {
    let mock_server = MockServer::start().await;

    mock_refresh_query(123, "test-job-id")
        .mount(&mock_server)
        .await;

    mock_poll_job_success("test-job-id", 456)
        .mount(&mock_server)
        .await;

    mock_get_query_result(123, 456).mount(&mock_server).await;

    mock_cancel_job("test-job-id")
        .expect(0)
        .mount(&mock_server)
        .await;

    let client = RedashClient::new(mock_server.uri(), "test-key").unwrap();
    let result = client.execute_query_with_polling(123, None, 10, 100).await;

    assert!(result.is_ok());
}

#[tokio::test]
async fn test_cancel_job_success() {
    let mock_server = MockServer::start().await;

    mock_cancel_job("test-job-id")
        .expect(1)
        .mount(&mock_server)
        .await;

    let client = RedashClient::new(mock_server.uri(), "test-key").unwrap();
    let result = client.cancel_job("test-job-id").await;

    assert!(result.is_ok());
}

#[tokio::test]
async fn test_cancel_job_error() {
    let mock_server = MockServer::start().await;

    Mock::given(method("DELETE"))
        .and(path("/api/jobs/test-job-id"))
        .respond_with(ResponseTemplate::new(404))
        .mount(&mock_server)
        .await;

    let client = RedashClient::new(mock_server.uri(), "test-key").unwrap();
    let result = client.cancel_job("test-job-id").await;

    assert!(result.is_err());
}

#[tokio::test]
async fn test_refresh_adhoc_query_success() {
    let mock_server = MockServer::start().await;

    mock_adhoc_refresh("adhoc-job-id")
        .expect(1)
        .mount(&mock_server)
        .await;

    let client = RedashClient::new(mock_server.uri(), "test-key").unwrap();
    let job = client
        .refresh_adhoc_query("SELECT 1 AS one", 63, None)
        .await
        .unwrap();

    assert_eq!(job.id, "adhoc-job-id");
    assert_eq!(job.status, 1);
}

#[tokio::test]
async fn test_get_adhoc_query_result_success() {
    let mock_server = MockServer::start().await;

    mock_get_adhoc_query_result(789).mount(&mock_server).await;

    let client = RedashClient::new(mock_server.uri(), "test-key").unwrap();
    let result = client.get_adhoc_query_result(789).await.unwrap();

    assert_eq!(result.id, 789);
    assert_eq!(result.data.columns.len(), 1);
    assert_eq!(result.data.columns[0].name, "one");
    assert_eq!(result.data.rows.len(), 1);
}

#[tokio::test]
async fn test_execute_adhoc_with_polling_success() {
    let mock_server = MockServer::start().await;

    mock_adhoc_refresh("adhoc-job-id")
        .expect(1)
        .mount(&mock_server)
        .await;

    mock_poll_job_success("adhoc-job-id", 789)
        .expect(1)
        .mount(&mock_server)
        .await;

    mock_get_adhoc_query_result(789)
        .expect(1)
        .mount(&mock_server)
        .await;

    let client = RedashClient::new(mock_server.uri(), "test-key").unwrap();
    let result = client
        .execute_adhoc_with_polling("SELECT 1 AS one", 63, None, 10, 100)
        .await
        .unwrap();

    assert_eq!(result.id, 789);
    assert_eq!(result.data.columns[0].name, "one");
}

#[tokio::test]
async fn test_execute_adhoc_with_polling_failure() {
    let mock_server = MockServer::start().await;

    mock_adhoc_refresh("adhoc-job-id").mount(&mock_server).await;

    mock_poll_job_failure("adhoc-job-id", "Syntax error in SQL")
        .mount(&mock_server)
        .await;

    let client = RedashClient::new(mock_server.uri(), "test-key").unwrap();
    let result = client
        .execute_adhoc_with_polling("SELECT bad syntax", 63, None, 10, 100)
        .await;

    assert!(result.is_err());
    let err = result.unwrap_err();
    assert!(err.to_string().contains("Query execution failed"));
    assert!(err.to_string().contains("Syntax error in SQL"));
}

#[tokio::test]
async fn test_list_my_queries_pagination() {
    let mock_server = MockServer::start().await;

    Mock::given(method("GET"))
        .and(path("/api/queries/my"))
        .and(query_param("page", "1"))
        .and(query_param("page_size", "100"))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
            "results": [],
            "count": 0,
            "page": 1,
            "page_size": 100
        })))
        .mount(&mock_server)
        .await;

    let client = RedashClient::new(mock_server.uri(), "test-key").unwrap();
    let response = client.list_my_queries(1, 100).await.unwrap();

    assert_eq!(response.count, 0);
    assert_eq!(response.page, 1);
    assert_eq!(response.page_size, 100);
}

#[tokio::test]
async fn test_list_data_sources_success() {
    let mock_server = MockServer::start().await;

    mock_list_data_sources().mount(&mock_server).await;

    let client = RedashClient::new(mock_server.uri(), "test-key").unwrap();
    let data_sources = client.list_data_sources().await.unwrap();

    assert_eq!(data_sources.len(), 2);
    assert_eq!(data_sources[0].id, 63);
    assert_eq!(data_sources[0].name, "Telemetry (BigQuery)");
    assert_eq!(data_sources[0].ds_type, "bigquery");
    assert_eq!(data_sources[1].id, 10);
    assert_eq!(data_sources[1].name, "Redash metadata");
    assert_eq!(data_sources[1].ds_type, "pg");
}

#[tokio::test]
async fn test_list_data_sources_empty() {
    let mock_server = MockServer::start().await;

    mock_list_data_sources_empty().mount(&mock_server).await;

    let client = RedashClient::new(mock_server.uri(), "test-key").unwrap();
    let data_sources = client.list_data_sources().await.unwrap();

    assert_eq!(data_sources.len(), 0);
}

#[tokio::test]
async fn test_get_data_source_success() {
    let mock_server = MockServer::start().await;

    mock_get_data_source(63).mount(&mock_server).await;

    let client = RedashClient::new(mock_server.uri(), "test-key").unwrap();
    let data_source = client.get_data_source(63).await.unwrap();

    assert_eq!(data_source.id, 63);
    assert_eq!(data_source.name, "Test Data Source");
    assert_eq!(data_source.ds_type, "bigquery");
    assert_eq!(
        data_source.description,
        Some("Test description".to_string())
    );
}

#[tokio::test]
async fn test_get_data_source_not_found() {
    let mock_server = MockServer::start().await;

    mock_get_data_source_not_found(999)
        .mount(&mock_server)
        .await;

    let client = RedashClient::new(mock_server.uri(), "test-key").unwrap();
    let result = client.get_data_source(999).await;

    assert!(result.is_err());
}

#[tokio::test]
async fn test_get_data_source_schema_success() {
    let mock_server = MockServer::start().await;

    mock_get_data_source_schema(63).mount(&mock_server).await;

    let client = RedashClient::new(mock_server.uri(), "test-key").unwrap();
    let schema = client.get_data_source_schema(63, false).await.unwrap();

    assert_eq!(schema.schema.len(), 2);
    assert_eq!(schema.schema[0].name, "table1");
    assert_eq!(schema.schema[0].columns.len(), 3);
    assert_eq!(schema.schema[0].columns[0].name, "col1");
    assert_eq!(schema.schema[0].columns[0].column_type, "STRING");
    assert_eq!(schema.schema[1].name, "table2");
    assert_eq!(schema.schema[1].columns.len(), 2);
    assert_eq!(schema.schema[1].columns[0].name, "id");
    assert_eq!(schema.schema[1].columns[0].column_type, "INTEGER");
}

#[tokio::test]
async fn test_get_data_source_schema_unauthorized() {
    let mock_server = MockServer::start().await;

    mock_get_data_source_schema_unauthorized(63)
        .mount(&mock_server)
        .await;

    let client = RedashClient::new(mock_server.uri(), "test-key").unwrap();
    let result = client.get_data_source_schema(63, false).await;

    assert!(result.is_err());
}

#[tokio::test]
async fn test_archive_query_success() {
    let mock_server = MockServer::start().await;

    mock_archive_query(123, "Test Query")
        .mount(&mock_server)
        .await;

    let client = RedashClient::new(mock_server.uri(), "test-key").unwrap();
    let query = client.archive_query(123).await.unwrap();

    assert_eq!(query.id, 123);
    assert_eq!(query.name, "Test Query");
    assert!(query.is_archived);
}

#[tokio::test]
async fn test_archive_query_not_found() {
    let mock_server = MockServer::start().await;

    mock_archive_query_not_found(999).mount(&mock_server).await;

    let client = RedashClient::new(mock_server.uri(), "test-key").unwrap();
    let result = client.archive_query(999).await;

    assert!(result.is_err());
}

#[tokio::test]
async fn test_unarchive_query_success() {
    let mock_server = MockServer::start().await;

    mock_unarchive_query(123, "Test Query")
        .mount(&mock_server)
        .await;

    let client = RedashClient::new(mock_server.uri(), "test-key").unwrap();
    let query = client.unarchive_query(123).await.unwrap();

    assert_eq!(query.id, 123);
    assert_eq!(query.name, "Test Query");
    assert!(!query.is_archived);
}

#[tokio::test]
async fn test_unarchive_query_forbidden() {
    let mock_server = MockServer::start().await;

    mock_unarchive_query_forbidden(123)
        .mount(&mock_server)
        .await;

    let client = RedashClient::new(mock_server.uri(), "test-key").unwrap();
    let result = client.unarchive_query(123).await;

    assert!(result.is_err());
}

#[tokio::test]
async fn test_get_query_archived() {
    let mock_server = MockServer::start().await;

    mock_get_query(123, "Archived Query", true)
        .mount(&mock_server)
        .await;

    let client = RedashClient::new(mock_server.uri(), "test-key").unwrap();
    let query = client.get_query(123).await.unwrap();

    assert_eq!(query.id, 123);
    assert_eq!(query.name, "Archived Query");
    assert!(query.is_archived);
}

#[tokio::test]
async fn test_get_query_not_archived() {
    let mock_server = MockServer::start().await;

    mock_get_query(123, "Active Query", false)
        .mount(&mock_server)
        .await;

    let client = RedashClient::new(mock_server.uri(), "test-key").unwrap();
    let query = client.get_query(123).await.unwrap();

    assert_eq!(query.id, 123);
    assert_eq!(query.name, "Active Query");
    assert!(!query.is_archived);
}

#[tokio::test]
async fn test_list_favorite_dashboards_success() {
    let mock_server = MockServer::start().await;

    mock_list_favorite_dashboards(2).mount(&mock_server).await;

    let client = RedashClient::new(mock_server.uri(), "test-key").unwrap();
    let response = client.list_favorite_dashboards(1, 100).await.unwrap();

    assert_eq!(response.count, 2);
    assert_eq!(response.results.len(), 2);
    assert_eq!(response.results[0].id, 2570);
    assert_eq!(response.results[0].name, "Firefox Desktop on SteamOS");
    assert!(!response.results[0].is_archived);
}

#[tokio::test]
async fn test_list_favorite_dashboards_empty() {
    let mock_server = MockServer::start().await;

    mock_list_favorite_dashboards_empty()
        .mount(&mock_server)
        .await;

    let client = RedashClient::new(mock_server.uri(), "test-key").unwrap();
    let response = client.list_favorite_dashboards(1, 100).await.unwrap();

    assert_eq!(response.count, 0);
    assert_eq!(response.results.len(), 0);
}

#[tokio::test]
async fn test_get_dashboard_success() {
    let mock_server = MockServer::start().await;

    mock_get_dashboard(2570, "Test Dashboard", false)
        .mount(&mock_server)
        .await;

    let client = RedashClient::new(mock_server.uri(), "test-key").unwrap();
    let dashboard = client.get_dashboard("test-dashboard").await.unwrap();

    assert_eq!(dashboard.id, 2570);
    assert_eq!(dashboard.name, "Test Dashboard");
    assert_eq!(dashboard.user_id, 530);
    assert!(!dashboard.is_archived);
    assert!(!dashboard.is_draft);
}

#[tokio::test]
async fn test_get_dashboard_not_found() {
    let mock_server = MockServer::start().await;

    mock_get_dashboard_not_found("nonexistent-dashboard")
        .mount(&mock_server)
        .await;

    let client = RedashClient::new(mock_server.uri(), "test-key").unwrap();
    let result = client.get_dashboard("nonexistent-dashboard").await;

    assert!(result.is_err());
}

#[tokio::test]
async fn test_get_dashboard_archived() {
    let mock_server = MockServer::start().await;

    mock_get_dashboard(2570, "Archived Dashboard", true)
        .mount(&mock_server)
        .await;

    let client = RedashClient::new(mock_server.uri(), "test-key").unwrap();
    let dashboard = client.get_dashboard("archived-dashboard").await.unwrap();

    assert_eq!(dashboard.id, 2570);
    assert_eq!(dashboard.name, "Archived Dashboard");
    assert!(dashboard.is_archived);
}

#[tokio::test]
async fn test_update_dashboard_success() {
    let mock_server = MockServer::start().await;

    mock_update_dashboard(2570, "Updated Dashboard")
        .mount(&mock_server)
        .await;

    let client = RedashClient::new(mock_server.uri(), "test-key").unwrap();

    let dashboard = stmo_cli::models::Dashboard {
        id: 2570,
        name: "Updated Dashboard".to_string(),
        slug: "updated-dashboard".to_string(),
        user_id: 530,
        is_archived: false,
        is_draft: false,
        filters_enabled: false,
        tags: vec![],
        widgets: vec![],
    };

    let result = client.update_dashboard(&dashboard).await.unwrap();

    assert_eq!(result.id, 2570);
    assert_eq!(result.name, "Updated Dashboard");
}

#[tokio::test]
async fn test_archive_dashboard_success() {
    let mock_server = MockServer::start().await;

    mock_archive_dashboard(2570).mount(&mock_server).await;

    let client = RedashClient::new(mock_server.uri(), "test-key").unwrap();
    let result = client.archive_dashboard(2570).await;

    assert!(result.is_ok());
}

#[tokio::test]
async fn test_archive_dashboard_not_found() {
    let mock_server = MockServer::start().await;

    mock_archive_dashboard_not_found(9999)
        .mount(&mock_server)
        .await;

    let client = RedashClient::new(mock_server.uri(), "test-key").unwrap();
    let result = client.archive_dashboard(9999).await;

    assert!(result.is_err());
}

#[tokio::test]
async fn test_unarchive_dashboard_success() {
    let mock_server = MockServer::start().await;

    mock_unarchive_dashboard(2570, "Test Dashboard")
        .mount(&mock_server)
        .await;

    let client = RedashClient::new(mock_server.uri(), "test-key").unwrap();
    let dashboard = client.unarchive_dashboard(2570).await.unwrap();

    assert_eq!(dashboard.id, 2570);
    assert_eq!(dashboard.name, "Test Dashboard");
    assert!(!dashboard.is_archived);
}

#[tokio::test]
async fn test_unarchive_dashboard_forbidden() {
    let mock_server = MockServer::start().await;

    mock_unarchive_dashboard_forbidden(2570)
        .mount(&mock_server)
        .await;

    let client = RedashClient::new(mock_server.uri(), "test-key").unwrap();
    let result = client.unarchive_dashboard(2570).await;

    assert!(result.is_err());
}

#[tokio::test]
async fn test_create_widget_success() {
    let mock_server = MockServer::start().await;

    mock_create_widget(2570, 75035).mount(&mock_server).await;

    let client = RedashClient::new(mock_server.uri(), "test-key").unwrap();

    let widget = stmo_cli::models::CreateWidget {
        dashboard_id: 2570,
        visualization_id: None,
        text: "Test Widget".to_string(),
        width: 1,
        options: stmo_cli::models::WidgetOptions {
            position: stmo_cli::models::WidgetPosition {
                col: 0,
                row: 0,
                size_x: 3,
                size_y: 2,
            },
            parameter_mappings: None,
        },
    };

    let result = client.create_widget(&widget).await.unwrap();

    assert_eq!(result.id, 75035);
    assert_eq!(result.dashboard_id, 2570);
}

#[tokio::test]
async fn test_delete_widget_success() {
    let mock_server = MockServer::start().await;

    mock_delete_widget(75035).mount(&mock_server).await;

    let client = RedashClient::new(mock_server.uri(), "test-key").unwrap();
    let result = client.delete_widget(75035).await;

    assert!(result.is_ok());
}

#[tokio::test]
async fn test_delete_widget_not_found() {
    let mock_server = MockServer::start().await;

    mock_delete_widget_not_found(999).mount(&mock_server).await;

    let client = RedashClient::new(mock_server.uri(), "test-key").unwrap();
    let result = client.delete_widget(999).await;

    assert!(result.is_err());
}

#[tokio::test]
async fn test_refresh_query_bad_request_includes_error_body() {
    let mock_server = MockServer::start().await;

    mock_refresh_query_bad_request(
        123,
        "The following parameter values are incompatible with their definitions: worker_pool",
    )
    .mount(&mock_server)
    .await;

    let client = RedashClient::new(mock_server.uri(), "test-key").unwrap();
    let result = client.refresh_query(123, None).await;

    assert!(result.is_err());
    let err = result.unwrap_err();
    assert!(err.to_string().contains("400"));
    assert!(
        err.to_string()
            .contains("parameter values are incompatible")
    );
}

#[tokio::test]
async fn test_refresh_query_forbidden_includes_error_body() {
    let mock_server = MockServer::start().await;

    mock_refresh_query_forbidden(123).mount(&mock_server).await;

    let client = RedashClient::new(mock_server.uri(), "test-key").unwrap();
    let result = client.refresh_query(123, None).await;

    assert!(result.is_err());
    let err = result.unwrap_err();
    assert!(err.to_string().contains("403"));
    assert!(err.to_string().contains("Access denied"));
}

fn query_json(id: u64, name: &str) -> serde_json::Value {
    serde_json::json!({
        "id": id,
        "name": name,
        "query": "SELECT 1",
        "data_source_id": 1,
        "is_archived": false,
        "is_draft": false,
        "schedule": null,
        "options": {"parameters": []},
        "visualizations": [],
        "tags": [],
        "updated_at": "2026-01-01T00:00:00",
        "created_at": "2026-01-01T00:00:00",
    })
}

fn dashboard_json(id: u64, slug: &str, name: &str) -> serde_json::Value {
    serde_json::json!({
        "id": id,
        "slug": slug,
        "name": name,
        "is_draft": false,
        "is_archived": false,
    })
}

#[tokio::test]
async fn test_search_queries_passes_q_and_respects_limit() {
    let mock_server = MockServer::start().await;

    Mock::given(method("GET"))
        .and(path("/api/queries"))
        .and(query_param("q", "firefox"))
        .and(query_param("page", "1"))
        .and(query_param("page_size", "2"))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
            "count": 100,
            "page": 1,
            "page_size": 2,
            "results": [
                query_json(1, "Firefox DAU"),
                query_json(2, "Firefox MAU"),
                query_json(3, "Firefox Crash Rate"),
            ]
        })))
        .mount(&mock_server)
        .await;

    let client = RedashClient::new(mock_server.uri(), "test-key").unwrap();
    let results = client.search_queries("firefox", 2).await.unwrap();

    assert_eq!(results.len(), 2);
    assert_eq!(results[0].name, "Firefox DAU");
    assert_eq!(results[1].name, "Firefox MAU");
}

#[tokio::test]
async fn test_search_dashboards_passes_q_and_respects_limit() {
    let mock_server = MockServer::start().await;

    Mock::given(method("GET"))
        .and(path("/api/dashboards"))
        .and(query_param("q", "firefox"))
        .and(query_param("page", "1"))
        .and(query_param("page_size", "2"))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
            "count": 50,
            "results": [
                dashboard_json(1, "firefox-dau", "Firefox DAU"),
                dashboard_json(2, "firefox-crash", "Firefox Crash"),
                dashboard_json(3, "firefox-beta", "Firefox Beta"),
            ]
        })))
        .mount(&mock_server)
        .await;

    let client = RedashClient::new(mock_server.uri(), "test-key").unwrap();
    let results = client.search_dashboards("firefox", 2).await.unwrap();

    assert_eq!(results.len(), 2);
    assert_eq!(results[0].name, "Firefox DAU");
    assert_eq!(results[1].name, "Firefox Crash");
}

#[tokio::test]
async fn test_search_queries_retries_on_429() {
    let mock_server = MockServer::start().await;

    Mock::given(method("GET"))
        .and(path("/api/queries"))
        .and(query_param("q", "firefox"))
        .respond_with(ResponseTemplate::new(429))
        .up_to_n_times(1)
        .with_priority(1)
        .mount(&mock_server)
        .await;

    Mock::given(method("GET"))
        .and(path("/api/queries"))
        .and(query_param("q", "firefox"))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
            "count": 1,
            "page": 1,
            "page_size": 1,
            "results": [query_json(1, "Firefox DAU")]
        })))
        .with_priority(2)
        .mount(&mock_server)
        .await;

    let client = RedashClient::new(mock_server.uri(), "test-key").unwrap();
    let results = client.search_queries("firefox", 10).await.unwrap();

    assert_eq!(results.len(), 1);
    assert_eq!(results[0].name, "Firefox DAU");
}

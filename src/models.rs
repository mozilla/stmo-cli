#![allow(clippy::missing_errors_doc)]

use serde::{Deserialize, Deserializer, Serialize};

fn deserialize_null_as_empty_vec<'de, D, T>(deserializer: D) -> Result<Vec<T>, D::Error>
where
    T: Deserialize<'de>,
    D: Deserializer<'de>,
{
    Ok(Option::deserialize(deserializer)?.unwrap_or_default())
}

fn deserialize_viz_id<'de, D>(deserializer: D) -> Result<Option<u64>, D::Error>
where
    D: Deserializer<'de>,
{
    let value: Option<u64> = Option::deserialize(deserializer)?;
    Ok(value.filter(|&id| id != 0))
}

fn default_width() -> u32 {
    1
}

fn deserialize_interval<'de, D>(deserializer: D) -> Result<Option<u64>, D::Error>
where
    D: Deserializer<'de>,
{
    use serde::de::Error;
    match Option::<serde_json::Value>::deserialize(deserializer)? {
        None | Some(serde_json::Value::Null) => Ok(None),
        Some(serde_json::Value::Number(n)) => n
            .as_u64()
            .ok_or_else(|| D::Error::custom("interval is not a valid u64"))
            .map(Some),
        Some(serde_json::Value::String(s)) => s.parse::<u64>().map_err(D::Error::custom).map(Some),
        Some(other) => Err(D::Error::custom(format!(
            "unexpected interval value: {other}"
        ))),
    }
}

fn deserialize_null_as_empty_string<'de, D>(deserializer: D) -> Result<String, D::Error>
where
    D: Deserializer<'de>,
{
    Ok(Option::deserialize(deserializer)?.unwrap_or_default())
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct Query {
    pub id: u64,
    pub name: String,
    pub description: Option<String>,
    #[serde(rename = "query")]
    pub sql: String,
    pub data_source_id: u64,
    #[serde(default)]
    pub user: Option<QueryUser>,
    pub schedule: Option<Schedule>,
    pub options: QueryOptions,
    #[serde(default)]
    pub visualizations: Vec<Visualization>,
    pub tags: Option<Vec<String>>,
    pub is_archived: bool,
    pub is_draft: bool,
    pub updated_at: String,
    pub created_at: String,
}

#[derive(Debug, Serialize, Clone)]
pub struct CreateQuery {
    pub name: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    #[serde(rename = "query")]
    pub sql: String,
    pub data_source_id: u64,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub schedule: Option<Schedule>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub options: Option<QueryOptions>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tags: Option<Vec<String>>,
    pub is_archived: bool,
    pub is_draft: bool,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct QueryUser {
    pub id: u64,
    pub name: String,
    pub email: String,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct QueryOptions {
    #[serde(default)]
    pub parameters: Vec<Parameter>,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct Parameter {
    pub name: String,
    pub title: String,
    #[serde(rename = "type")]
    pub param_type: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub value: Option<serde_json::Value>,
    #[serde(rename = "enumOptions", skip_serializing_if = "Option::is_none")]
    pub enum_options: Option<String>,
    #[serde(rename = "queryId", skip_serializing_if = "Option::is_none")]
    pub query_id: Option<u64>,
    #[serde(rename = "multiValuesOptions", skip_serializing_if = "Option::is_none")]
    pub multi_values_options: Option<MultiValuesOptions>,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct MultiValuesOptions {
    #[serde(rename = "prefix", skip_serializing_if = "Option::is_none")]
    pub prefix: Option<String>,
    #[serde(rename = "suffix", skip_serializing_if = "Option::is_none")]
    pub suffix: Option<String>,
    #[serde(rename = "separator", skip_serializing_if = "Option::is_none")]
    pub separator: Option<String>,
    #[serde(rename = "quoteCharacter", skip_serializing_if = "Option::is_none")]
    pub quote_character: Option<String>,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct Schedule {
    #[serde(default, deserialize_with = "deserialize_interval")]
    pub interval: Option<u64>,
    pub time: Option<String>,
    pub day_of_week: Option<String>,
    pub until: Option<String>,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct Visualization {
    pub id: u64,
    pub name: String,
    #[serde(rename = "type")]
    pub viz_type: String,
    pub options: serde_json::Value,
    pub description: Option<String>,
}

#[derive(Debug, Serialize, Clone)]
pub struct CreateVisualization {
    pub query_id: u64,
    pub name: String,
    #[serde(rename = "type")]
    pub viz_type: String,
    pub options: serde_json::Value,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct QueriesResponse {
    pub results: Vec<Query>,
    pub count: u64,
    pub page: u64,
    pub page_size: u64,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct VisualizationMetadata {
    #[serde(
        default,
        deserialize_with = "deserialize_viz_id",
        skip_serializing_if = "Option::is_none"
    )]
    pub id: Option<u64>,
    pub name: String,
    #[serde(rename = "type")]
    pub viz_type: String,
    pub options: serde_json::Value,
    pub description: Option<String>,
}

impl From<&Visualization> for VisualizationMetadata {
    fn from(v: &Visualization) -> Self {
        Self {
            id: Some(v.id),
            name: v.name.clone(),
            viz_type: v.viz_type.clone(),
            options: v.options.clone(),
            description: v.description.clone(),
        }
    }
}

#[derive(Debug, Serialize, Deserialize)]
pub struct QueryMetadata {
    pub id: u64,
    pub name: String,
    pub description: Option<String>,
    pub data_source_id: u64,
    #[serde(default)]
    pub user_id: Option<u64>,
    pub schedule: Option<Schedule>,
    pub options: QueryOptions,
    pub visualizations: Vec<VisualizationMetadata>,
    pub tags: Option<Vec<String>>,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
#[allow(dead_code)]
pub struct User {
    pub id: u64,
    pub name: String,
    pub email: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub profile_image_url: Option<String>,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct DataSource {
    pub id: u64,
    pub name: String,
    #[serde(rename = "type")]
    pub ds_type: String,
    pub syntax: Option<String>,
    pub description: Option<String>,
    pub paused: u8,
    pub pause_reason: Option<String>,
    pub view_only: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub queue_name: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub scheduled_queue_name: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub groups: Option<serde_json::Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub options: Option<serde_json::Value>,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct DataSourceSchema {
    pub schema: Vec<SchemaTable>,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct SchemaTable {
    pub name: String,
    pub columns: Vec<SchemaColumn>,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct SchemaColumn {
    pub name: String,
    #[serde(rename = "type")]
    pub column_type: String,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct RefreshRequest {
    pub max_age: u64,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub parameters: Option<std::collections::HashMap<String, serde_json::Value>>,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct AdhocRefreshRequest {
    #[serde(rename = "query")]
    pub sql: String,
    pub data_source_id: u64,
    pub max_age: u64,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub parameters: Option<std::collections::HashMap<String, serde_json::Value>>,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct JobResponse {
    pub job: Job,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct Job {
    pub id: String,
    pub status: u8,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub query_result_id: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct QueryResultResponse {
    pub query_result: QueryResult,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct QueryResult {
    pub id: u64,
    pub data: QueryResultData,
    pub runtime: f64,
    pub retrieved_at: String,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct QueryResultData {
    pub columns: Vec<Column>,
    pub rows: Vec<serde_json::Value>,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct Column {
    pub name: String,
    #[serde(rename = "type")]
    pub type_name: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub friendly_name: Option<String>,
}

#[derive(Debug, Clone, Copy)]
pub enum JobStatus {
    Pending = 1,
    Started = 2,
    Success = 3,
    Failure = 4,
    Cancelled = 5,
}

impl JobStatus {
    pub fn from_u8(status: u8) -> anyhow::Result<Self> {
        match status {
            1 => Ok(Self::Pending),
            2 => Ok(Self::Started),
            3 => Ok(Self::Success),
            4 => Ok(Self::Failure),
            5 => Ok(Self::Cancelled),
            _ => Err(anyhow::anyhow!("Invalid job status: {status}")),
        }
    }
}

#[derive(Debug, Serialize, Deserialize)]
pub struct Dashboard {
    pub id: u64,
    pub name: String,
    pub slug: String,
    pub user_id: u64,
    pub is_archived: bool,
    pub is_draft: bool,
    #[serde(rename = "dashboard_filters_enabled")]
    pub filters_enabled: bool,
    pub tags: Vec<String>,
    #[serde(default, deserialize_with = "deserialize_null_as_empty_vec")]
    pub widgets: Vec<Widget>,
}

#[derive(Debug, Serialize)]
pub struct CreateDashboard {
    pub name: String,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct Widget {
    pub id: u64,
    pub dashboard_id: u64,
    pub width: u32,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub visualization_id: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub visualization: Option<WidgetVisualization>,
    #[serde(default, deserialize_with = "deserialize_null_as_empty_string")]
    pub text: String,
    pub options: WidgetOptions,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct WidgetVisualization {
    pub id: u64,
    pub name: String,
    pub query: VisualizationQuery,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct VisualizationQuery {
    pub id: u64,
    pub name: String,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct WidgetOptions {
    pub position: WidgetPosition,
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        rename = "parameterMappings"
    )]
    pub parameter_mappings: Option<serde_json::Value>,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct WidgetPosition {
    pub col: u32,
    pub row: u32,
    #[serde(rename = "sizeX")]
    pub size_x: u32,
    #[serde(rename = "sizeY")]
    pub size_y: u32,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct DashboardMetadata {
    pub id: u64,
    pub name: String,
    pub slug: String,
    pub user_id: u64,
    pub is_draft: bool,
    pub is_archived: bool,
    #[serde(rename = "dashboard_filters_enabled")]
    pub filters_enabled: bool,
    pub tags: Vec<String>,
    pub widgets: Vec<WidgetMetadata>,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct WidgetMetadata {
    pub id: u64,
    #[serde(default = "default_width")]
    pub width: u32,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub visualization_id: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub query_id: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub visualization_name: Option<String>,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub text: String,
    pub options: WidgetOptions,
}

#[derive(Debug, Deserialize)]
pub struct DashboardsResponse {
    pub results: Vec<DashboardSummary>,
    pub count: u64,
}

#[derive(Debug, Deserialize)]
pub struct DashboardSummary {
    #[allow(dead_code)]
    pub id: u64,
    pub name: String,
    #[allow(dead_code)]
    pub slug: String,
    pub is_draft: bool,
    pub is_archived: bool,
}

#[derive(Debug, Serialize)]
pub struct CreateWidget {
    pub dashboard_id: u64,
    pub visualization_id: Option<u64>,
    pub text: String,
    pub width: u32,
    pub options: WidgetOptions,
}

#[must_use]
pub fn build_dashboard_level_parameter_mappings(parameters: &[Parameter]) -> serde_json::Value {
    let mut mappings = serde_json::Map::new();
    for param in parameters {
        mappings.insert(
            param.name.clone(),
            serde_json::json!({
                "mapTo": param.name,
                "name": param.name,
                "title": "",
                "type": "dashboard-level",
                "value": null,
            }),
        );
    }
    serde_json::Value::Object(mappings)
}

#[cfg(test)]
#[allow(clippy::missing_errors_doc)]
#[allow(clippy::unnecessary_literal_unwrap)]
mod tests {
    use super::*;

    #[test]
    fn test_job_status_from_u8_valid() {
        assert!(matches!(JobStatus::from_u8(1).unwrap(), JobStatus::Pending));
        assert!(matches!(JobStatus::from_u8(2).unwrap(), JobStatus::Started));
        assert!(matches!(JobStatus::from_u8(3).unwrap(), JobStatus::Success));
        assert!(matches!(JobStatus::from_u8(4).unwrap(), JobStatus::Failure));
        assert!(matches!(
            JobStatus::from_u8(5).unwrap(),
            JobStatus::Cancelled
        ));
    }

    #[test]
    fn test_job_status_from_u8_invalid() {
        assert!(JobStatus::from_u8(0).is_err());
        assert!(JobStatus::from_u8(6).is_err());
        assert!(JobStatus::from_u8(255).is_err());

        let err = JobStatus::from_u8(10).unwrap_err();
        assert!(err.to_string().contains("Invalid job status"));
    }

    #[test]
    fn test_query_serialization() {
        let query = Query {
            id: 1,
            name: "Test Query".to_string(),
            description: None,
            sql: "SELECT * FROM table".to_string(),
            data_source_id: 63,
            user: None,
            schedule: None,
            options: QueryOptions { parameters: vec![] },
            visualizations: vec![],
            tags: None,
            is_archived: false,
            is_draft: false,
            updated_at: "2026-01-21".to_string(),
            created_at: "2026-01-21".to_string(),
        };

        let json = serde_json::to_string(&query).unwrap();
        assert!(json.contains("\"query\":"));
        assert!(json.contains("SELECT * FROM table"));
    }

    #[test]
    fn test_query_metadata_deserialization() {
        let yaml = r"
id: 100064
name: Test Query
description: null
data_source_id: 63
user_id: 530
schedule: null
options:
  parameters:
    - name: project
      title: project
      type: enum
      value:
        - try
      enumOptions: |
        try
        autoland
visualizations: []
tags:
  - bug 1840828
";

        let metadata: QueryMetadata = serde_yaml::from_str(yaml).unwrap();
        assert_eq!(metadata.id, 100_064);
        assert_eq!(metadata.name, "Test Query");
        assert_eq!(metadata.data_source_id, 63);
        assert_eq!(metadata.options.parameters.len(), 1);
        assert_eq!(metadata.options.parameters[0].name, "project");
    }

    #[test]
    fn test_datasource_deserialization() {
        let json = r#"{
            "id": 63,
            "name": "Test DB",
            "type": "bigquery",
            "description": null,
            "syntax": "sql",
            "paused": 0,
            "pause_reason": null,
            "view_only": false,
            "queue_name": "queries",
            "scheduled_queue_name": "scheduled_queries",
            "groups": {},
            "options": {}
        }"#;

        let ds: DataSource = serde_json::from_str(json).unwrap();
        assert_eq!(ds.id, 63);
        assert_eq!(ds.name, "Test DB");
        assert_eq!(ds.ds_type, "bigquery");
        assert_eq!(ds.syntax, Some("sql".to_string()));
        assert_eq!(ds.description, None);
        assert_eq!(ds.paused, 0);
        assert!(!ds.view_only);
        assert_eq!(ds.queue_name, Some("queries".to_string()));
    }

    #[test]
    fn test_datasource_with_nulls() {
        let json = r#"{
            "id": 10,
            "name": "Minimal DB",
            "type": "pg",
            "description": "Test description",
            "syntax": null,
            "paused": 1,
            "pause_reason": "Maintenance",
            "view_only": true,
            "queue_name": null,
            "scheduled_queue_name": null,
            "groups": null,
            "options": null
        }"#;

        let ds: DataSource = serde_json::from_str(json).unwrap();
        assert_eq!(ds.id, 10);
        assert_eq!(ds.name, "Minimal DB");
        assert_eq!(ds.ds_type, "pg");
        assert_eq!(ds.description, Some("Test description".to_string()));
        assert_eq!(ds.syntax, None);
        assert_eq!(ds.paused, 1);
        assert_eq!(ds.pause_reason, Some("Maintenance".to_string()));
        assert!(ds.view_only);
        assert_eq!(ds.queue_name, None);
    }

    #[test]
    fn test_datasource_schema_deserialization() {
        let json = r#"{
            "schema": [
                {
                    "name": "table1",
                    "columns": [
                        {"name": "col1", "type": "STRING"},
                        {"name": "col2", "type": "INTEGER"}
                    ]
                },
                {
                    "name": "table2",
                    "columns": [{"name": "id", "type": "INTEGER"}]
                }
            ]
        }"#;

        let schema: DataSourceSchema = serde_json::from_str(json).unwrap();
        assert_eq!(schema.schema.len(), 2);
        assert_eq!(schema.schema[0].name, "table1");
        assert_eq!(schema.schema[0].columns.len(), 2);
        assert_eq!(schema.schema[0].columns[0].name, "col1");
        assert_eq!(schema.schema[0].columns[0].column_type, "STRING");
        assert_eq!(schema.schema[1].name, "table2");
        assert_eq!(schema.schema[1].columns.len(), 1);
    }

    #[test]
    fn test_schema_table_structure() {
        let json = r#"{
            "name": "users",
            "columns": [
                {"name": "id", "type": "INTEGER"},
                {"name": "name", "type": "STRING"},
                {"name": "email", "type": "STRING"}
            ]
        }"#;

        let table: SchemaTable = serde_json::from_str(json).unwrap();
        assert_eq!(table.name, "users");
        assert_eq!(table.columns.len(), 3);
        assert_eq!(table.columns[0].name, "id");
        assert_eq!(table.columns[0].column_type, "INTEGER");
        assert_eq!(table.columns[1].name, "name");
        assert_eq!(table.columns[1].column_type, "STRING");
        assert_eq!(table.columns[2].name, "email");
        assert_eq!(table.columns[2].column_type, "STRING");
    }

    #[test]
    fn test_datasource_serialization() {
        let ds = DataSource {
            id: 123,
            name: "My DB".to_string(),
            ds_type: "mysql".to_string(),
            syntax: Some("sql".to_string()),
            description: Some("Test".to_string()),
            paused: 0,
            pause_reason: None,
            view_only: false,
            queue_name: Some("queries".to_string()),
            scheduled_queue_name: None,
            groups: None,
            options: None,
        };

        let json = serde_json::to_string(&ds).unwrap();
        assert!(json.contains("\"id\":123"));
        assert!(json.contains("\"name\":\"My DB\""));
        assert!(json.contains("\"type\":\"mysql\""));
        assert!(json.contains("\"syntax\":\"sql\""));
    }

    #[test]
    fn test_dashboard_deserialization() {
        let json = r#"{
            "id": 2570,
            "name": "Test Dashboard",
            "slug": "test-dashboard",
            "user_id": 530,
            "is_archived": false,
            "is_draft": false,
            "dashboard_filters_enabled": true,
            "tags": ["tag1", "tag2"],
            "widgets": []
        }"#;

        let dashboard: Dashboard = serde_json::from_str(json).unwrap();
        assert_eq!(dashboard.id, 2570);
        assert_eq!(dashboard.name, "Test Dashboard");
        assert_eq!(dashboard.slug, "test-dashboard");
        assert_eq!(dashboard.user_id, 530);
        assert!(!dashboard.is_archived);
        assert!(!dashboard.is_draft);
        assert!(dashboard.filters_enabled);
        assert_eq!(dashboard.tags, vec!["tag1", "tag2"]);
        assert_eq!(dashboard.widgets.len(), 0);
    }

    #[test]
    fn test_dashboard_with_widgets() {
        let json = r##"{
            "id": 2570,
            "name": "Test Dashboard",
            "slug": "test-dashboard",
            "user_id": 530,
            "is_archived": false,
            "is_draft": false,
            "dashboard_filters_enabled": false,
            "tags": [],
            "widgets": [
                {
                    "id": 75035,
                    "dashboard_id": 2570,
                    "width": 1,
                    "text": "# Test Widget",
                    "options": {
                        "position": {
                            "col": 0,
                            "row": 0,
                            "sizeX": 6,
                            "sizeY": 2
                        }
                    }
                },
                {
                    "id": 75029,
                    "dashboard_id": 2570,
                    "width": 1,
                    "visualization_id": 279588,
                    "visualization": {
                        "id": 279588,
                        "name": "Total MAU",
                        "query": {
                            "id": 114049,
                            "name": "MAU Query"
                        }
                    },
                    "text": "",
                    "options": {
                        "position": {
                            "col": 3,
                            "row": 2,
                            "sizeX": 3,
                            "sizeY": 8
                        },
                        "parameterMappings": {
                            "channel": {
                                "name": "channel",
                                "type": "dashboard-level"
                            }
                        }
                    }
                }
            ]
        }"##;

        let dashboard: Dashboard = serde_json::from_str(json).unwrap();
        assert_eq!(dashboard.widgets.len(), 2);
        assert_eq!(dashboard.widgets[0].id, 75035);
        assert_eq!(dashboard.widgets[0].text, "# Test Widget");
        assert!(dashboard.widgets[0].visualization_id.is_none());
        assert_eq!(dashboard.widgets[1].id, 75029);
        assert_eq!(dashboard.widgets[1].visualization_id, Some(279_588));
        let viz = dashboard.widgets[1].visualization.as_ref().unwrap();
        assert_eq!(viz.id, 279_588);
        assert_eq!(viz.query.id, 114_049);
    }

    #[test]
    fn test_widget_position_serde() {
        let json = r#"{
            "col": 3,
            "row": 5,
            "sizeX": 6,
            "sizeY": 4
        }"#;

        let position: WidgetPosition = serde_json::from_str(json).unwrap();
        assert_eq!(position.col, 3);
        assert_eq!(position.row, 5);
        assert_eq!(position.size_x, 6);
        assert_eq!(position.size_y, 4);

        let serialized = serde_json::to_string(&position).unwrap();
        assert!(serialized.contains("\"sizeX\":6"));
        assert!(serialized.contains("\"sizeY\":4"));
    }

    #[test]
    fn test_dashboard_metadata_yaml() {
        let yaml = r"
id: 2570
name: Test Dashboard
slug: test-dashboard
user_id: 530
is_draft: false
is_archived: false
dashboard_filters_enabled: true
tags:
  - tag1
  - tag2
widgets:
  - id: 75035
    visualization_id: null
    query_id: null
    visualization_name: null
    text: '# Test Widget'
    options:
      position:
        col: 0
        row: 0
        sizeX: 6
        sizeY: 2
      parameter_mappings: null
";

        let metadata: DashboardMetadata = serde_yaml::from_str(yaml).unwrap();
        assert_eq!(metadata.id, 2570);
        assert_eq!(metadata.name, "Test Dashboard");
        assert_eq!(metadata.slug, "test-dashboard");
        assert_eq!(metadata.user_id, 530);
        assert!(!metadata.is_draft);
        assert!(!metadata.is_archived);
        assert!(metadata.filters_enabled);
        assert_eq!(metadata.tags, vec!["tag1", "tag2"]);
        assert_eq!(metadata.widgets.len(), 1);
        assert_eq!(metadata.widgets[0].id, 75035);
        assert_eq!(metadata.widgets[0].text, "# Test Widget");
    }

    #[test]
    fn test_widget_metadata_text_widget() {
        let yaml = r"
id: 75035
visualization_id: null
query_id: null
visualization_name: null
text: '## Section Header'
options:
  position:
    col: 0
    row: 0
    sizeX: 6
    sizeY: 2
  parameter_mappings: null
";

        let widget: WidgetMetadata = serde_yaml::from_str(yaml).unwrap();
        assert_eq!(widget.id, 75035);
        assert!(widget.visualization_id.is_none());
        assert!(widget.query_id.is_none());
        assert!(widget.visualization_name.is_none());
        assert_eq!(widget.text, "## Section Header");
        assert_eq!(widget.options.position.col, 0);
        assert_eq!(widget.options.position.size_x, 6);
    }

    #[test]
    fn test_widget_metadata_viz_widget() {
        let yaml = r"
id: 75029
visualization_id: 279588
query_id: 114049
visualization_name: Total MAU
text: ''
options:
  position:
    col: 3
    row: 2
    sizeX: 3
    sizeY: 8
  parameterMappings:
    channel:
      name: channel
      type: dashboard-level
";

        let widget: WidgetMetadata = serde_yaml::from_str(yaml).unwrap();
        assert_eq!(widget.id, 75029);
        assert_eq!(widget.visualization_id, Some(279_588));
        assert_eq!(widget.query_id, Some(114_049));
        assert_eq!(widget.visualization_name, Some("Total MAU".to_string()));
        assert_eq!(widget.text, "");
        assert!(widget.options.parameter_mappings.is_some());
    }

    #[test]
    fn test_create_widget_serialization() {
        let widget = CreateWidget {
            dashboard_id: 2570,
            visualization_id: Some(279_588),
            text: String::new(),
            width: 1,
            options: WidgetOptions {
                position: WidgetPosition {
                    col: 0,
                    row: 0,
                    size_x: 3,
                    size_y: 2,
                },
                parameter_mappings: None,
            },
        };

        let json = serde_json::to_string(&widget).unwrap();
        assert!(json.contains("\"dashboard_id\":2570"));
        assert!(json.contains("\"visualization_id\":279588"));
        assert!(json.contains("\"sizeX\":3"));
        assert!(json.contains("\"sizeY\":2"));
    }

    #[test]
    fn test_create_text_widget_serialization() {
        let widget = CreateWidget {
            dashboard_id: 2570,
            visualization_id: None,
            text: "Some text".to_string(),
            width: 1,
            options: WidgetOptions {
                position: WidgetPosition {
                    col: 0,
                    row: 0,
                    size_x: 3,
                    size_y: 2,
                },
                parameter_mappings: None,
            },
        };

        let json = serde_json::to_string(&widget).unwrap();
        assert!(json.contains("\"visualization_id\":null"));
    }

    #[test]
    fn test_dashboards_response() {
        let json = r#"{
            "results": [
                {
                    "id": 2570,
                    "name": "Dashboard 1",
                    "slug": "dashboard-1",
                    "is_draft": false,
                    "is_archived": false
                },
                {
                    "id": 2558,
                    "name": "Dashboard 2",
                    "slug": "dashboard-2",
                    "is_draft": true,
                    "is_archived": false
                }
            ],
            "count": 2
        }"#;

        let response: DashboardsResponse = serde_json::from_str(json).unwrap();
        assert_eq!(response.results.len(), 2);
        assert_eq!(response.count, 2);
        assert_eq!(response.results[0].id, 2570);
        assert_eq!(response.results[0].name, "Dashboard 1");
        assert_eq!(response.results[0].slug, "dashboard-1");
        assert!(!response.results[0].is_draft);
        assert!(!response.results[0].is_archived);
        assert_eq!(response.results[1].id, 2558);
        assert_eq!(response.results[1].slug, "dashboard-2");
        assert!(response.results[1].is_draft);
    }

    #[test]
    fn test_build_dashboard_level_parameter_mappings_empty() {
        let result = build_dashboard_level_parameter_mappings(&[]);
        assert_eq!(result, serde_json::json!({}));
    }

    #[test]
    fn test_build_dashboard_level_parameter_mappings_with_params() {
        let params = vec![
            Parameter {
                name: "channel".to_string(),
                title: "Channel".to_string(),
                param_type: "enum".to_string(),
                value: None,
                enum_options: None,
                query_id: None,
                multi_values_options: None,
            },
            Parameter {
                name: "date".to_string(),
                title: "Date".to_string(),
                param_type: "date".to_string(),
                value: None,
                enum_options: None,
                query_id: None,
                multi_values_options: None,
            },
        ];

        let result = build_dashboard_level_parameter_mappings(&params);

        let expected = serde_json::json!({
            "channel": {
                "mapTo": "channel",
                "name": "channel",
                "title": "",
                "type": "dashboard-level",
                "value": null,
            },
            "date": {
                "mapTo": "date",
                "name": "date",
                "title": "",
                "type": "dashboard-level",
                "value": null,
            },
        });

        assert_eq!(result, expected);
    }

    #[test]
    fn test_schedule_interval_as_integer() {
        let s: Schedule = serde_json::from_str(r#"{"interval": 3600}"#).unwrap();
        assert_eq!(s.interval, Some(3600));
    }

    #[test]
    fn test_schedule_interval_as_string() {
        let s: Schedule = serde_json::from_str(r#"{"interval": "3600"}"#).unwrap();
        assert_eq!(s.interval, Some(3600));
    }

    #[test]
    fn test_schedule_interval_null() {
        let s: Schedule = serde_json::from_str(r#"{"interval": null}"#).unwrap();
        assert_eq!(s.interval, None);
    }

    #[test]
    fn test_schedule_interval_absent() {
        let s: Schedule = serde_json::from_str(r"{}").unwrap();
        assert_eq!(s.interval, None);
    }
}

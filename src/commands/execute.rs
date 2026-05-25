#![allow(clippy::missing_errors_doc)]
#![allow(clippy::missing_panics_doc)]

use super::OutputFormat;
use crate::api::RedashClient;
use crate::models::{Parameter, QueryMetadata};
use anyhow::{Context, Result, bail};
use std::collections::HashMap;
use std::fs;
use std::io::IsTerminal;
use std::path::Path;

fn parse_parameter_arg(arg: &str) -> Result<(String, serde_json::Value)> {
    let parts: Vec<&str> = arg.splitn(2, '=').collect();
    if parts.len() != 2 {
        bail!("Invalid parameter format. Use: --param name=value");
    }

    let name = parts[0].to_string();
    let value_str = parts[1];

    let value = if let Ok(json_value) = serde_json::from_str(value_str) {
        json_value
    } else {
        serde_json::Value::String(value_str.to_string())
    };

    Ok((name, value))
}

fn load_query_metadata_by_id(query_id: u64) -> Result<(QueryMetadata, String, String)> {
    let queries_dir = Path::new("queries");

    for entry in fs::read_dir(queries_dir).context("Failed to read queries directory")? {
        let entry = entry.context("Failed to read directory entry")?;
        let path = entry.path();

        if path.extension().is_some_and(|ext| ext == "yaml")
            && let Some(filename) = path.file_name().and_then(|f| f.to_str())
            && let Some(id_str) = filename.split('-').next()
            && let Ok(id) = id_str.parse::<u64>()
            && id == query_id
        {
            let yaml_content =
                fs::read_to_string(&path).context(format!("Failed to read {}", path.display()))?;

            let metadata: QueryMetadata = serde_yaml::from_str(&yaml_content)
                .context(format!("Failed to parse {}", path.display()))?;

            let yaml_path = path.display().to_string();
            let sql_path = yaml_path.replace(".yaml", ".sql");

            if !Path::new(&sql_path).exists() {
                bail!("SQL file not found: {sql_path}");
            }

            let sql =
                fs::read_to_string(&sql_path).context(format!("Failed to read {sql_path}"))?;

            return Ok((metadata, sql, yaml_path));
        }
    }

    bail!(
        "Query {query_id} not found in queries/ directory. Run 'stmo-cli fetch {query_id}' first."
    );
}

fn prompt_for_parameter(param: &Parameter) -> Result<serde_json::Value> {
    use dialoguer::{Input, Select};

    let title = &param.title;

    match param.param_type.as_str() {
        "date" => {
            let input: String = Input::new()
                .with_prompt(format!("{title} (YYYY-MM-DD)"))
                .interact_text()?;
            Ok(serde_json::Value::String(input))
        }
        "enum" => {
            if let Some(enum_options) = &param.enum_options {
                let options: Vec<&str> = enum_options.lines().collect();

                if param.multi_values_options.is_some() {
                    use dialoguer::MultiSelect;
                    let selections = MultiSelect::new()
                        .with_prompt(title)
                        .items(&options)
                        .interact()?;

                    let selected: Vec<String> =
                        selections.iter().map(|&i| options[i].to_string()).collect();

                    Ok(serde_json::Value::Array(
                        selected
                            .into_iter()
                            .map(serde_json::Value::String)
                            .collect(),
                    ))
                } else {
                    let selection = Select::new()
                        .with_prompt(title)
                        .items(&options)
                        .default(0)
                        .interact()?;

                    Ok(serde_json::Value::String(options[selection].to_string()))
                }
            } else {
                let input: String = Input::new().with_prompt(title).interact_text()?;
                Ok(serde_json::Value::String(input))
            }
        }
        "number" => {
            let input: f64 = Input::new().with_prompt(title).interact_text()?;
            Ok(serde_json::json!(input))
        }
        _ => {
            let input: String = Input::new().with_prompt(title).interact_text()?;
            Ok(serde_json::Value::String(input))
        }
    }
}

fn coerce_for_type(value: &serde_json::Value, param_type: &str) -> serde_json::Value {
    match param_type {
        "text" | "date" => match value {
            serde_json::Value::String(_) => value.clone(),
            other => serde_json::Value::String(other.to_string()),
        },
        _ => value.clone(),
    }
}

fn build_parameter_map(
    metadata: &QueryMetadata,
    cli_params: &[(String, serde_json::Value)],
    interactive: bool,
) -> Result<Option<HashMap<String, serde_json::Value>>> {
    if metadata.options.parameters.is_empty() {
        return Ok(None);
    }

    let mut param_map = HashMap::new();

    for (name, value) in cli_params {
        let coerced = metadata
            .options
            .parameters
            .iter()
            .find(|p| p.name == *name)
            .map_or_else(|| value.clone(), |p| coerce_for_type(value, &p.param_type));
        param_map.insert(name.clone(), coerced);
    }

    let has_tty = std::io::stdin().is_terminal();

    for param in &metadata.options.parameters {
        if !param_map.contains_key(&param.name) {
            if interactive && has_tty {
                eprintln!("\nParameter '{}' required:", param.title);
                let value = prompt_for_parameter(param)?;
                param_map.insert(param.name.clone(), value);
            } else if interactive && !has_tty {
                if let Some(default_value) = &param.value {
                    param_map.insert(param.name.clone(), default_value.clone());
                } else {
                    bail!(
                        "No TTY available for interactive prompt. \
                         Supply parameter explicitly: --param {}=<value>",
                        param.name
                    );
                }
            } else if let Some(default_value) = &param.value {
                param_map.insert(param.name.clone(), default_value.clone());
            } else {
                bail!(
                    "Missing required parameter: '{}' ({}). Use --param {}=value or --interactive",
                    param.name,
                    param.title,
                    param.name
                );
            }
        }
    }

    Ok(if param_map.is_empty() {
        None
    } else {
        Some(param_map)
    })
}

fn format_results_json(
    result: &crate::models::QueryResult,
    limit: Option<usize>,
) -> Result<String> {
    let rows = if let Some(limit) = limit {
        result
            .data
            .rows
            .iter()
            .take(limit)
            .cloned()
            .collect::<Vec<_>>()
    } else {
        result.data.rows.clone()
    };

    serde_json::to_string_pretty(&rows).context("Failed to format results as JSON")
}

fn format_results_table(result: &crate::models::QueryResult, limit: Option<usize>) -> String {
    use std::fmt::Write;

    let mut output = String::new();

    let _ = writeln!(output);
    for col in &result.data.columns {
        let _ = write!(output, "{:20} ", col.name);
    }
    let _ = writeln!(output);
    let _ = writeln!(output, "{}", "-".repeat(result.data.columns.len() * 21));

    let rows_to_show = limit
        .unwrap_or(result.data.rows.len())
        .min(result.data.rows.len());

    for row in &result.data.rows[..rows_to_show] {
        if let serde_json::Value::Object(obj) = row {
            for col in &result.data.columns {
                let value = obj
                    .get(&col.name)
                    .map(|v| match v {
                        serde_json::Value::Null => "NULL".to_string(),
                        serde_json::Value::String(s) => s.clone(),
                        _ => v.to_string(),
                    })
                    .unwrap_or_default();

                let truncated = if value.len() > 18 {
                    format!("{}...", &value[..15])
                } else {
                    value
                };

                let _ = write!(output, "{truncated:20} ");
            }
            let _ = writeln!(output);
        }
    }

    if rows_to_show < result.data.rows.len() {
        let _ = write!(
            output,
            "\n... {} more rows (showing {} of {})\n",
            result.data.rows.len() - rows_to_show,
            rows_to_show,
            result.data.rows.len()
        );
    }

    let _ = write!(
        output,
        "\n✓ {} rows returned in {:.2}s\n",
        result.data.rows.len(),
        result.runtime
    );

    output
}

pub async fn execute(
    client: &RedashClient,
    query_id: u64,
    param_args: Vec<String>,
    format: OutputFormat,
    interactive: bool,
    timeout_secs: u64,
    limit_rows: Option<usize>,
) -> Result<()> {
    let (metadata, _sql, yaml_path) = load_query_metadata_by_id(query_id)?;

    eprintln!("Executing query: {} - {}", metadata.id, metadata.name);
    eprintln!("Source: {yaml_path}\n");

    let cli_params: Vec<(String, serde_json::Value)> = param_args
        .iter()
        .map(|arg| parse_parameter_arg(arg))
        .collect::<Result<Vec<_>>>()?;

    let parameters = build_parameter_map(&metadata, &cli_params, interactive)?;

    if let Some(ref params) = parameters {
        eprintln!("Parameters:");
        for (name, value) in params {
            eprintln!("  {name} = {value}");
        }
        eprintln!();
    }

    let result = client
        .execute_query_with_polling(query_id, parameters, timeout_secs, 500)
        .await?;

    match format {
        OutputFormat::Json => {
            let json = format_results_json(&result, limit_rows)?;
            println!("{json}");
        }
        OutputFormat::Table => {
            let table = format_results_table(&result, limit_rows);
            println!("{table}");
        }
    }

    Ok(())
}

#[cfg(test)]
#[allow(clippy::missing_errors_doc)]
mod tests {
    use super::*;
    use crate::models::{Column, QueryResult, QueryResultData};

    #[test]
    fn test_parse_parameter_arg_string() {
        let result = parse_parameter_arg("name=value").unwrap();
        assert_eq!(result.0, "name");
        assert_eq!(result.1, serde_json::Value::String("value".to_string()));
    }

    #[test]
    fn test_parse_parameter_arg_json_array() {
        let result = parse_parameter_arg("channels=[\"release\",\"beta\"]").unwrap();
        assert_eq!(result.0, "channels");
        assert_eq!(result.1, serde_json::json!(["release", "beta"]));
    }

    #[test]
    fn test_parse_parameter_arg_number() {
        let result = parse_parameter_arg("count=42").unwrap();
        assert_eq!(result.0, "count");
        assert_eq!(result.1, serde_json::json!(42));
    }

    #[test]
    fn test_parse_parameter_arg_invalid() {
        let result = parse_parameter_arg("invalid");
        assert!(result.is_err());
        assert!(
            result
                .unwrap_err()
                .to_string()
                .contains("Invalid parameter format")
        );
    }

    #[test]
    fn test_format_results_json() {
        let result = QueryResult {
            id: 1,
            data: QueryResultData {
                columns: vec![
                    Column {
                        name: "col1".to_string(),
                        type_name: "string".to_string(),
                        friendly_name: None,
                    },
                    Column {
                        name: "col2".to_string(),
                        type_name: "integer".to_string(),
                        friendly_name: None,
                    },
                ],
                rows: vec![
                    serde_json::json!({"col1": "value1", "col2": 123}),
                    serde_json::json!({"col1": "value2", "col2": 456}),
                ],
            },
            runtime: 1.5,
            retrieved_at: "2026-01-21T10:00:00".to_string(),
        };

        let json = format_results_json(&result, None).unwrap();
        let parsed: serde_json::Value = serde_json::from_str(&json).unwrap();

        let rows = parsed.as_array().unwrap();
        assert_eq!(rows.len(), 2);
        assert_eq!(rows[0]["col1"], "value1");
        assert_eq!(rows[0]["col2"], 123);
    }

    #[test]
    fn test_format_results_json_with_limit() {
        let result = QueryResult {
            id: 1,
            data: QueryResultData {
                columns: vec![Column {
                    name: "col1".to_string(),
                    type_name: "string".to_string(),
                    friendly_name: None,
                }],
                rows: vec![
                    serde_json::json!({"col1": "row1"}),
                    serde_json::json!({"col1": "row2"}),
                    serde_json::json!({"col1": "row3"}),
                ],
            },
            runtime: 1.0,
            retrieved_at: "2026-01-21T10:00:00".to_string(),
        };

        let json = format_results_json(&result, Some(2)).unwrap();
        let parsed: serde_json::Value = serde_json::from_str(&json).unwrap();

        assert_eq!(parsed.as_array().unwrap().len(), 2);
    }

    #[test]
    fn test_format_results_table() {
        let result = QueryResult {
            id: 1,
            data: QueryResultData {
                columns: vec![
                    Column {
                        name: "col1".to_string(),
                        type_name: "string".to_string(),
                        friendly_name: None,
                    },
                    Column {
                        name: "col2".to_string(),
                        type_name: "integer".to_string(),
                        friendly_name: None,
                    },
                ],
                rows: vec![
                    serde_json::json!({"col1": "value1", "col2": 123}),
                    serde_json::json!({"col1": "value2", "col2": 456}),
                ],
            },
            runtime: 1.5,
            retrieved_at: "2026-01-21T10:00:00".to_string(),
        };

        let table = format_results_table(&result, None);

        assert!(table.contains("col1"));
        assert!(table.contains("col2"));
        assert!(table.contains("value1"));
        assert!(table.contains("value2"));
        assert!(table.contains("2 rows returned"));
    }

    #[test]
    fn test_format_results_table_with_limit() {
        let result = QueryResult {
            id: 1,
            data: QueryResultData {
                columns: vec![Column {
                    name: "col1".to_string(),
                    type_name: "string".to_string(),
                    friendly_name: None,
                }],
                rows: vec![
                    serde_json::json!({"col1": "row1"}),
                    serde_json::json!({"col1": "row2"}),
                    serde_json::json!({"col1": "row3"}),
                ],
            },
            runtime: 1.0,
            retrieved_at: "2026-01-21T10:00:00".to_string(),
        };

        let table = format_results_table(&result, Some(2));

        assert!(table.contains("row1"));
        assert!(table.contains("row2"));
        assert!(table.contains("... 1 more rows"));
        assert!(table.contains("3 rows returned"));
    }

    #[test]
    fn test_format_results_table_truncation() {
        let result = QueryResult {
            id: 1,
            data: QueryResultData {
                columns: vec![Column {
                    name: "col1".to_string(),
                    type_name: "string".to_string(),
                    friendly_name: None,
                }],
                rows: vec![
                    serde_json::json!({"col1": "this_is_a_very_long_value_that_should_be_truncated"}),
                ],
            },
            runtime: 1.0,
            retrieved_at: "2026-01-21T10:00:00".to_string(),
        };

        let table = format_results_table(&result, None);

        assert!(table.contains("..."));
    }

    #[test]
    fn test_output_format_from_str() {
        assert!(matches!(
            "json".parse::<OutputFormat>().unwrap(),
            OutputFormat::Json
        ));
        assert!(matches!(
            "JSON".parse::<OutputFormat>().unwrap(),
            OutputFormat::Json
        ));
        assert!(matches!(
            "table".parse::<OutputFormat>().unwrap(),
            OutputFormat::Table
        ));
        assert!(matches!(
            "TABLE".parse::<OutputFormat>().unwrap(),
            OutputFormat::Table
        ));
    }

    #[test]
    fn test_output_format_from_str_invalid() {
        let result = "csv".parse::<OutputFormat>();
        assert!(result.is_err());
        assert!(result.unwrap_err().to_string().contains("Invalid format"));
    }

    #[test]
    fn test_coerce_for_type_text_coerces_number_to_string() {
        let result = coerce_for_type(&serde_json::json!(90), "text");
        assert_eq!(result, serde_json::Value::String("90".to_string()));
    }

    #[test]
    fn test_coerce_for_type_text_leaves_string_unchanged() {
        let result = coerce_for_type(&serde_json::json!("90"), "text");
        assert_eq!(result, serde_json::Value::String("90".to_string()));
    }

    #[test]
    fn test_coerce_for_type_date_coerces_to_string() {
        let result = coerce_for_type(&serde_json::json!(20_260_507), "date");
        assert_eq!(result, serde_json::Value::String("20260507".to_string()));
    }

    #[test]
    fn test_coerce_for_type_number_leaves_number_unchanged() {
        let result = coerce_for_type(&serde_json::json!(42), "number");
        assert_eq!(result, serde_json::json!(42));
    }

    fn make_metadata_with_param(name: &str, default: Option<serde_json::Value>) -> QueryMetadata {
        use crate::models::{Parameter, QueryOptions};
        QueryMetadata {
            id: 1,
            name: "test".to_string(),
            description: None,
            data_source_id: 1,
            user_id: None,
            schedule: None,
            options: QueryOptions {
                parameters: vec![Parameter {
                    name: name.to_string(),
                    title: name.to_string(),
                    param_type: "text".to_string(),
                    value: default,
                    enum_options: None,
                    query_id: None,
                    multi_values_options: None,
                }],
            },
            visualizations: vec![],
            tags: None,
        }
    }

    #[test]
    fn test_build_parameter_map_coerces_text_param() {
        let metadata = make_metadata_with_param("days", None);
        let cli_params = vec![("days".to_string(), serde_json::json!(90))];
        let result = build_parameter_map(&metadata, &cli_params, false)
            .unwrap()
            .unwrap();
        assert_eq!(result["days"], serde_json::Value::String("90".to_string()));
    }

    #[test]
    fn test_build_parameter_map_interactive_no_tty_uses_default() {
        let metadata = make_metadata_with_param("p", Some(serde_json::json!("default_val")));
        let result = build_parameter_map(&metadata, &[], true).unwrap();
        let map = result.unwrap();
        assert_eq!(map["p"], serde_json::json!("default_val"));
    }

    #[test]
    fn test_build_parameter_map_interactive_no_tty_no_default_bails() {
        let metadata = make_metadata_with_param("p", None);
        let err = build_parameter_map(&metadata, &[], true).unwrap_err();
        assert!(err.to_string().contains("--param p="));
    }

    #[test]
    fn test_build_parameter_map_interactive_no_tty_cli_param_overrides() {
        let metadata = make_metadata_with_param("p", None);
        let cli_params = vec![("p".to_string(), serde_json::json!("provided"))];
        let result = build_parameter_map(&metadata, &cli_params, true).unwrap();
        let map = result.unwrap();
        assert_eq!(map["p"], serde_json::json!("provided"));
    }
}

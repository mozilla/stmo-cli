#![allow(clippy::missing_errors_doc)]
#![allow(clippy::missing_panics_doc)]

//! Query execution and ad-hoc SQL.
//!
//! By default `execute ID` runs the local `queries/<id>.sql` through Redash's
//! schema-less ad-hoc endpoint (`POST /api/query_results`); `--remote` runs the
//! server-stored SQL via `POST /api/queries/{id}/results`; `--file`/stdin runs
//! arbitrary SQL with no tracked query.
//!
//! Parameter-rendering parity: the ad-hoc endpoint has no parameter schema, so
//! any rendering Redash's frontend does from the schema must be replicated here
//! before sending. Verified empirically (raw ad-hoc vs. a stored query, same SQL
//! + params) and against the Redash source — only two steps need replication:
//! - multi-value lists, joined client-side (`render_multi_value_parameters`)
//! - dynamic `d_*` dates, resolved client-side (see [`super::dynamic_dates`])
//!
//! Everything else already matches the stored path and needs no client work:
//! scalar coercion (`number`/`text`/`date`/single `enum`), `date-range` objects
//! (`{start,end}`, the canonical form on both endpoints), and string escaping
//! (neither endpoint escapes). The ad-hoc endpoint also does no parameter-name
//! validation, so unknown `--param` names are caught by `warn_unknown_parameters`.
//!
//! The schema-driven rendering above (multi-value joins, `d_*` dates) only applies
//! to the tracked paths, which have a parameter schema. Pure ad-hoc mode
//! (`--file`/stdin) has no schema, so its `--param` values are sent verbatim — `d_*`
//! tokens are not expanded and multi-value lists are not joined; inline such values
//! directly in the SQL instead.

use super::OutputFormat;
use crate::api::RedashClient;
use crate::models::{MultiValuesOptions, Parameter, QueryMetadata};
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

            let sql_path = path.with_extension("sql").display().to_string();

            if !Path::new(&sql_path).exists() {
                bail!("SQL file not found: {sql_path}");
            }

            let sql =
                fs::read_to_string(&sql_path).context(format!("Failed to read {sql_path}"))?;

            return Ok((metadata, sql, sql_path));
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
    warn_unknown_parameters(&metadata.options.parameters, cli_params);

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

fn unknown_parameter_names<'a>(
    parameters: &[Parameter],
    cli_params: &'a [(String, serde_json::Value)],
) -> Vec<&'a str> {
    cli_params
        .iter()
        .filter(|(name, _)| !parameters.iter().any(|p| p.name == *name))
        .map(|(name, _)| name.as_str())
        .collect()
}

fn warn_unknown_parameters(parameters: &[Parameter], cli_params: &[(String, serde_json::Value)]) {
    for name in unknown_parameter_names(parameters, cli_params) {
        eprintln!(
            "Warning: --param '{name}' does not match any parameter defined in this query; \
             it will have no effect."
        );
    }
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

fn plain_value(value: &serde_json::Value) -> String {
    match value {
        serde_json::Value::String(s) => s.clone(),
        other => other.to_string(),
    }
}

fn join_multi_value(values: &[serde_json::Value], options: &MultiValuesOptions) -> String {
    // Redash renders multi-value lists from prefix/suffix/separator only; it stores
    // quoteCharacter but ignores it when joining. Mirror that so the local (ad-hoc) path
    // matches the stored-query endpoint. Source of truth: upstream EnumParameter.js
    // getExecutionValue (prefix/suffix default "", separator ",", quoteCharacter unused).
    let prefix = options.prefix.as_deref().unwrap_or("");
    let suffix = options.suffix.as_deref().unwrap_or("");
    let separator = options.separator.as_deref().unwrap_or(",");

    values
        .iter()
        .map(|value| format!("{prefix}{}{suffix}", plain_value(value)))
        .collect::<Vec<_>>()
        .join(separator)
}

fn render_multi_value_parameters(
    metadata: &QueryMetadata,
    parameters: Option<HashMap<String, serde_json::Value>>,
) -> Option<HashMap<String, serde_json::Value>> {
    let mut parameters = parameters?;

    for param in &metadata.options.parameters {
        let Some(options) = &param.multi_values_options else {
            continue;
        };
        let Some(serde_json::Value::Array(values)) = parameters.get(&param.name) else {
            continue;
        };

        let joined = join_multi_value(values, options);
        parameters.insert(param.name.clone(), serde_json::Value::String(joined));
    }

    Some(parameters)
}

fn params_from_cli(
    cli_params: &[(String, serde_json::Value)],
) -> Option<HashMap<String, serde_json::Value>> {
    if cli_params.is_empty() {
        None
    } else {
        Some(cli_params.iter().cloned().collect())
    }
}

fn print_parameters(parameters: Option<&HashMap<String, serde_json::Value>>) {
    if let Some(params) = parameters {
        eprintln!("Parameters:");
        for (name, value) in params {
            eprintln!("  {name} = {value}");
        }
        eprintln!();
    }
}

#[derive(Debug, PartialEq, Eq)]
enum AdhocSource {
    File(String),
    Stdin,
}

fn adhoc_source(file: Option<&str>) -> AdhocSource {
    match file {
        Some("-") | None => AdhocSource::Stdin,
        Some(path) => AdhocSource::File(path.to_string()),
    }
}

fn read_sql<R: std::io::Read>(mut reader: R) -> Result<String> {
    let mut sql = String::new();
    reader
        .read_to_string(&mut sql)
        .context("Failed to read SQL from stdin")?;
    if sql.trim().is_empty() {
        bail!("No SQL provided on stdin.");
    }
    Ok(sql)
}

fn load_adhoc_sql(source: &AdhocSource) -> Result<(String, String)> {
    match source {
        AdhocSource::File(path) => {
            let sql = fs::read_to_string(path).context(format!("Failed to read {path}"))?;
            Ok((sql, path.clone()))
        }
        AdhocSource::Stdin => {
            if std::io::stdin().is_terminal() {
                bail!("No SQL on stdin. Pipe SQL in or pass --file <path>.");
            }
            let sql = read_sql(std::io::stdin().lock())?;
            Ok((sql, "<stdin>".to_string()))
        }
    }
}

async fn execute_adhoc(
    client: &RedashClient,
    file: Option<&str>,
    data_source_id: u64,
    cli_params: &[(String, serde_json::Value)],
    timeout_secs: u64,
) -> Result<crate::models::QueryResult> {
    let (sql, source_label) = load_adhoc_sql(&adhoc_source(file))?;

    eprintln!("Source: {source_label} (ad-hoc, data source {data_source_id})\n");

    let parameters = params_from_cli(cli_params);
    print_parameters(parameters.as_ref());

    client
        .execute_adhoc_with_polling(&sql, data_source_id, parameters, timeout_secs, 500)
        .await
}

async fn execute_tracked_query(
    client: &RedashClient,
    query_id: u64,
    cli_params: &[(String, serde_json::Value)],
    interactive: bool,
    timeout_secs: u64,
    remote: bool,
) -> Result<crate::models::QueryResult> {
    let (metadata, sql, sql_path) = load_query_metadata_by_id(query_id)?;

    eprintln!("Executing query: {} - {}", metadata.id, metadata.name);

    let parameters = build_parameter_map(&metadata, cli_params, interactive)?;

    if remote {
        eprintln!("Source: server query {} (remote)\n", metadata.id);
        print_parameters(parameters.as_ref());
        client
            .execute_query_with_polling(query_id, parameters, timeout_secs, 500)
            .await
    } else {
        eprintln!("Source: {sql_path} (local)\n");
        // Render multi-value lists before printing so the displayed parameters match
        // exactly what is sent to the ad-hoc endpoint (e.g. release,beta — not the array).
        let parameters = render_multi_value_parameters(&metadata, parameters);
        print_parameters(parameters.as_ref());
        client
            .execute_adhoc_with_polling(
                &sql,
                metadata.data_source_id,
                parameters,
                timeout_secs,
                500,
            )
            .await
    }
}

pub struct ExecuteArgs {
    pub query_id: Option<u64>,
    pub file: Option<String>,
    pub data_source: Option<u64>,
    pub param_args: Vec<String>,
    pub format: OutputFormat,
    pub interactive: bool,
    pub timeout_secs: u64,
    pub limit_rows: Option<usize>,
    pub remote: bool,
}

// The validated execution mode. Resolving the flag combinations into this enum once keeps
// the validation matrix in a single place and makes invalid combinations unrepresentable
// downstream (e.g. ad-hoc always carries a concrete data source).
enum ExecuteMode {
    Adhoc {
        file: Option<String>,
        data_source_id: u64,
    },
    Tracked {
        query_id: u64,
        remote: bool,
    },
}

fn resolve_mode(args: &ExecuteArgs) -> Result<ExecuteMode> {
    if args.file.is_some() && args.query_id.is_some() {
        bail!("Cannot combine a query ID with --file; choose one.");
    }
    if args.file.is_some() && args.remote {
        bail!("--remote cannot be combined with --file; --file always runs ad-hoc SQL.");
    }
    if args.query_id.is_some() && args.data_source.is_some() {
        bail!(
            "--data-source cannot be combined with a query ID; it only applies to --file ad-hoc SQL."
        );
    }
    if args.query_id.is_none() && args.remote {
        bail!(
            "--remote runs a tracked query's server-stored SQL; pass a query ID (e.g. stmo-cli execute 123 --remote)."
        );
    }

    if let Some(query_id) = args.query_id {
        return Ok(ExecuteMode::Tracked {
            query_id,
            remote: args.remote,
        });
    }

    if args.file.is_some() || args.data_source.is_some() {
        let data_source_id = args
            .data_source
            .context("ad-hoc execution requires --data-source <id> to run SQL")?;
        return Ok(ExecuteMode::Adhoc {
            file: args.file.clone(),
            data_source_id,
        });
    }

    bail!(
        "No query specified. Provide a query ID (stmo-cli execute 123) \
         or run ad-hoc SQL with --file <path> --data-source <id> (or pipe SQL via stdin)."
    );
}

pub async fn execute(client: &RedashClient, args: ExecuteArgs) -> Result<()> {
    let mode = resolve_mode(&args)?;

    let cli_params: Vec<(String, serde_json::Value)> = args
        .param_args
        .iter()
        .map(|arg| parse_parameter_arg(arg))
        .collect::<Result<Vec<_>>>()?;

    let result = match mode {
        ExecuteMode::Adhoc {
            file,
            data_source_id,
        } => {
            execute_adhoc(
                client,
                file.as_deref(),
                data_source_id,
                &cli_params,
                args.timeout_secs,
            )
            .await?
        }
        ExecuteMode::Tracked { query_id, remote } => {
            execute_tracked_query(
                client,
                query_id,
                &cli_params,
                args.interactive,
                args.timeout_secs,
                remote,
            )
            .await?
        }
    };

    match args.format {
        OutputFormat::Json => {
            let json = format_results_json(&result, args.limit_rows)?;
            println!("{json}");
        }
        OutputFormat::Table => {
            let table = format_results_table(&result, args.limit_rows);
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

    #[test]
    fn test_unknown_parameter_names_flags_only_unmatched() {
        let metadata = make_metadata_with_param("days", None);
        let cli_params = vec![
            ("days".to_string(), serde_json::json!(7)),
            ("dayz".to_string(), serde_json::json!(7)),
            ("channel".to_string(), serde_json::json!("beta")),
        ];
        let unknown = unknown_parameter_names(&metadata.options.parameters, &cli_params);
        assert_eq!(unknown, vec!["dayz", "channel"]);
    }

    #[test]
    fn test_unknown_parameter_names_empty_when_all_match() {
        let metadata = make_metadata_with_param("days", None);
        let cli_params = vec![("days".to_string(), serde_json::json!(7))];
        let unknown = unknown_parameter_names(&metadata.options.parameters, &cli_params);
        assert!(unknown.is_empty());
    }

    fn make_metadata_with_multi_value_param(name: &str) -> QueryMetadata {
        use crate::models::{MultiValuesOptions, Parameter, QueryOptions};
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
                    param_type: "enum".to_string(),
                    value: None,
                    enum_options: Some("nightly\nbeta\nrelease".to_string()),
                    query_id: None,
                    multi_values_options: Some(MultiValuesOptions {
                        prefix: Some("'".to_string()),
                        suffix: Some("'".to_string()),
                        separator: Some(",".to_string()),
                        quote_character: None,
                    }),
                }],
            },
            visualizations: vec![],
            tags: None,
        }
    }

    #[test]
    fn test_render_multi_value_parameters_quotes_and_joins_list() {
        let metadata = make_metadata_with_multi_value_param("channels");
        let mut params = HashMap::new();
        params.insert(
            "channels".to_string(),
            serde_json::json!(["nightly", "beta"]),
        );

        let rendered = render_multi_value_parameters(&metadata, Some(params)).unwrap();
        assert_eq!(
            rendered["channels"],
            serde_json::Value::String("'nightly','beta'".to_string())
        );
    }

    #[test]
    fn test_render_multi_value_parameters_leaves_scalar_untouched() {
        let metadata = make_metadata_with_multi_value_param("channels");
        let mut params = HashMap::new();
        params.insert("channels".to_string(), serde_json::json!("nightly"));

        let rendered = render_multi_value_parameters(&metadata, Some(params)).unwrap();
        assert_eq!(rendered["channels"], serde_json::json!("nightly"));
    }

    #[test]
    fn test_render_multi_value_parameters_ignores_params_without_options() {
        let metadata = make_metadata_with_param("days", None);
        let mut params = HashMap::new();
        params.insert("days".to_string(), serde_json::json!(["1", "2"]));

        let rendered = render_multi_value_parameters(&metadata, Some(params)).unwrap();
        assert_eq!(rendered["days"], serde_json::json!(["1", "2"]));
    }

    #[test]
    fn test_join_multi_value_ignores_quote_character() {
        // Redash ignores quoteCharacter when rendering, defaulting prefix/suffix to empty.
        let options = MultiValuesOptions {
            prefix: None,
            suffix: None,
            separator: None,
            quote_character: Some("\"".to_string()),
        };
        let values = vec![serde_json::json!("a"), serde_json::json!("b")];
        assert_eq!(join_multi_value(&values, &options), "a,b");
    }

    #[test]
    fn test_adhoc_source_stdin_for_dash_and_none() {
        assert_eq!(adhoc_source(Some("-")), AdhocSource::Stdin);
        assert_eq!(adhoc_source(None), AdhocSource::Stdin);
    }

    #[test]
    fn test_adhoc_source_file_for_path() {
        assert_eq!(
            adhoc_source(Some("scratch.sql")),
            AdhocSource::File("scratch.sql".to_string())
        );
    }

    #[test]
    fn test_read_sql_returns_content() {
        let sql = read_sql(std::io::Cursor::new("SELECT 1")).unwrap();
        assert_eq!(sql, "SELECT 1");
    }

    #[test]
    fn test_read_sql_bails_on_empty() {
        let err = read_sql(std::io::Cursor::new("   \n\t")).unwrap_err();
        assert!(err.to_string().contains("No SQL provided on stdin"));
    }
}

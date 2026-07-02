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
    has_tty: bool,
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

    resolve_dynamic_dates(&metadata.options.parameters, &mut param_map);

    Ok(if param_map.is_empty() {
        None
    } else {
        Some(param_map)
    })
}

// Resolve `d_*` tokens (from `--param` or a stored parameter default) before executing a
// tracked query. The stored-query API rejects raw `d_*` values — only Redash's frontend
// expands them — so this must run client-side first. See `dynamic_dates`. Ad-hoc execution
// has no parameter schema and sends `--param` values verbatim, so this does not apply there.
fn resolve_dynamic_dates(
    parameters: &[Parameter],
    param_map: &mut HashMap<String, serde_json::Value>,
) {
    for param in parameters {
        if let Some(value) = param_map.get(&param.name)
            && let Some(resolved) = super::dynamic_dates::resolve(value, &param.param_type)
        {
            param_map.insert(param.name.clone(), resolved);
        }
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

// Compare local vs. server on everything `deploy` would push, so `execute` never silently
// runs a stale server copy. SQL is compared byte-for-byte, matching how `fetch`/`deploy`
// round-trip it without modification.
fn tracked_query_differs(
    local_sql: &str,
    local_metadata: &QueryMetadata,
    server: &crate::models::Query,
) -> bool {
    local_sql != server.sql
        || local_metadata.name != server.name
        || local_metadata.data_source_id != server.data_source_id
        || serde_json::to_value(&local_metadata.options).ok()
            != serde_json::to_value(&server.options).ok()
}

async fn sync_if_changed(
    client: &RedashClient,
    query_id: u64,
    local_sql: &str,
    local_metadata: &QueryMetadata,
) -> Result<()> {
    let server = client.get_query(query_id).await?;

    if !tracked_query_differs(local_sql, local_metadata, &server) {
        return Ok(());
    }

    eprintln!("Local changes detected, deploying query {query_id}...");
    super::deploy::deploy_one(client, query_id, &local_metadata.name).await?;
    eprintln!("Deployed.");

    Ok(())
}

fn tracked_source_line(query_id: u64) -> String {
    format!("server-stored query {query_id} (kept in sync with your local copy)")
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
) -> Result<crate::models::QueryResult> {
    let (metadata, sql, _yaml_path) = load_query_metadata_by_id(query_id)?;

    sync_if_changed(client, query_id, &sql, &metadata).await?;

    eprintln!("Executing query: {} - {}", metadata.id, metadata.name);
    eprintln!("Source: {}\n", tracked_source_line(metadata.id));

    let has_tty = std::io::stdin().is_terminal();
    let parameters = build_parameter_map(&metadata, cli_params, interactive, has_tty)?;
    print_parameters(parameters.as_ref());

    client
        .execute_query_with_polling(query_id, parameters, timeout_secs, 500)
        .await
}

pub struct ExecuteArgs {
    pub query_id: Option<u64>,
    pub data_source: Option<u64>,
    pub file: Option<String>,
    pub param_args: Vec<String>,
    pub format: OutputFormat,
    pub interactive: bool,
    pub timeout_secs: u64,
    pub limit_rows: Option<usize>,
}

// The validated execution mode. Resolving the flag combinations into this enum once keeps
// the validation matrix in a single place and makes invalid combinations unrepresentable
// downstream (e.g. ad-hoc always carries a concrete data source).
#[derive(Debug)]
enum ExecuteMode {
    Adhoc {
        file: Option<String>,
        data_source_id: u64,
    },
    Tracked {
        query_id: u64,
    },
}

fn resolve_mode(args: &ExecuteArgs) -> Result<ExecuteMode> {
    if args.file.is_some() && args.query_id.is_some() {
        bail!("Cannot combine a query ID with --file; choose one.");
    }
    if args.query_id.is_some() && args.data_source.is_some() {
        bail!("--data-source cannot be combined with a query ID; it only applies to ad-hoc SQL.");
    }

    if let Some(query_id) = args.query_id {
        return Ok(ExecuteMode::Tracked { query_id });
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
        ExecuteMode::Tracked { query_id } => {
            execute_tracked_query(
                client,
                query_id,
                &cli_params,
                args.interactive,
                args.timeout_secs,
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
    use crate::models::{Column, QueryOptions, QueryResult, QueryResultData};

    fn make_query_metadata(name: &str, data_source_id: u64) -> QueryMetadata {
        QueryMetadata {
            id: 1,
            name: name.to_string(),
            description: None,
            data_source_id,
            user_id: None,
            schedule: None,
            options: QueryOptions { parameters: vec![] },
            visualizations: vec![],
            tags: None,
        }
    }

    fn make_server_query(sql: &str, name: &str, data_source_id: u64) -> crate::models::Query {
        crate::models::Query {
            id: 1,
            name: name.to_string(),
            description: None,
            sql: sql.to_string(),
            data_source_id,
            user: None,
            schedule: None,
            options: QueryOptions { parameters: vec![] },
            visualizations: vec![],
            tags: None,
            is_archived: false,
            is_draft: false,
            updated_at: String::new(),
            created_at: String::new(),
        }
    }

    #[test]
    fn test_tracked_query_differs_false_when_identical() {
        let metadata = make_query_metadata("Q", 1);
        let server = make_server_query("SELECT 1", "Q", 1);
        assert!(!tracked_query_differs("SELECT 1", &metadata, &server));
    }

    #[test]
    fn test_tracked_query_differs_true_when_sql_differs() {
        let metadata = make_query_metadata("Q", 1);
        let server = make_server_query("SELECT 1", "Q", 1);
        assert!(tracked_query_differs("SELECT 2", &metadata, &server));
    }

    #[test]
    fn test_tracked_query_differs_true_when_name_differs() {
        let metadata = make_query_metadata("Local Name", 1);
        let server = make_server_query("SELECT 1", "Server Name", 1);
        assert!(tracked_query_differs("SELECT 1", &metadata, &server));
    }

    #[test]
    fn test_tracked_query_differs_true_when_data_source_id_differs() {
        let metadata = make_query_metadata("Q", 1);
        let server = make_server_query("SELECT 1", "Q", 2);
        assert!(tracked_query_differs("SELECT 1", &metadata, &server));
    }

    #[test]
    fn test_tracked_query_differs_true_when_parameters_differ() {
        let mut metadata = make_query_metadata("Q", 1);
        metadata.options.parameters.push(Parameter {
            name: "p".to_string(),
            title: "P".to_string(),
            param_type: "text".to_string(),
            value: None,
            enum_options: None,
            query_id: None,
            multi_values_options: None,
        });
        let server = make_server_query("SELECT 1", "Q", 1);
        assert!(tracked_query_differs("SELECT 1", &metadata, &server));
    }

    #[test]
    fn test_tracked_source_line_identifies_server_stored_query() {
        let line = tracked_source_line(121_870);
        assert!(line.contains("121870"));
        assert!(line.contains("server-stored"));
    }

    fn make_execute_args(
        query_id: Option<u64>,
        data_source: Option<u64>,
        file: Option<&str>,
    ) -> ExecuteArgs {
        ExecuteArgs {
            query_id,
            data_source,
            file: file.map(str::to_string),
            param_args: vec![],
            format: OutputFormat::Json,
            interactive: false,
            timeout_secs: 300,
            limit_rows: None,
        }
    }

    #[test]
    fn test_resolve_mode_query_id_only_is_tracked() {
        let args = make_execute_args(Some(123), None, None);
        let mode = resolve_mode(&args).unwrap();
        assert!(matches!(mode, ExecuteMode::Tracked { query_id: 123 }));
    }

    #[test]
    fn test_resolve_mode_data_source_only_is_adhoc() {
        let args = make_execute_args(None, Some(63), None);
        let mode = resolve_mode(&args).unwrap();
        assert!(matches!(
            mode,
            ExecuteMode::Adhoc {
                data_source_id: 63,
                file: None
            }
        ));
    }

    #[test]
    fn test_resolve_mode_data_source_with_file_is_adhoc() {
        let args = make_execute_args(None, Some(63), Some("scratch.sql"));
        let mode = resolve_mode(&args).unwrap();
        match mode {
            ExecuteMode::Adhoc {
                data_source_id,
                file,
            } => {
                assert_eq!(data_source_id, 63);
                assert_eq!(file.as_deref(), Some("scratch.sql"));
            }
            ExecuteMode::Tracked { .. } => panic!("expected Adhoc"),
        }
    }

    #[test]
    fn test_resolve_mode_query_id_and_data_source_errors() {
        let args = make_execute_args(Some(123), Some(63), None);
        let err = resolve_mode(&args).unwrap_err();
        assert!(err.to_string().contains("--data-source"));
    }

    #[test]
    fn test_resolve_mode_query_id_and_file_errors() {
        let args = make_execute_args(Some(123), None, Some("scratch.sql"));
        let err = resolve_mode(&args).unwrap_err();
        assert!(err.to_string().contains("--file"));
    }

    #[test]
    fn test_resolve_mode_file_without_data_source_errors() {
        let args = make_execute_args(None, None, Some("scratch.sql"));
        let err = resolve_mode(&args).unwrap_err();
        assert!(err.to_string().contains("--data-source"));
    }

    #[test]
    fn test_resolve_mode_no_input_errors() {
        let args = make_execute_args(None, None, None);
        let err = resolve_mode(&args).unwrap_err();
        assert!(err.to_string().contains("No query specified"));
    }

    #[test]
    fn test_adhoc_source_none_is_stdin() {
        assert_eq!(adhoc_source(None), AdhocSource::Stdin);
    }

    #[test]
    fn test_adhoc_source_dash_is_stdin() {
        assert_eq!(adhoc_source(Some("-")), AdhocSource::Stdin);
    }

    #[test]
    fn test_adhoc_source_path_is_file() {
        assert_eq!(
            adhoc_source(Some("scratch.sql")),
            AdhocSource::File("scratch.sql".to_string())
        );
    }

    #[test]
    fn test_read_sql_empty_input_errors() {
        let err = read_sql(std::io::Cursor::new(b"" as &[u8])).unwrap_err();
        assert!(err.to_string().contains("No SQL provided"));
    }

    #[test]
    fn test_read_sql_whitespace_only_errors() {
        let err = read_sql(std::io::Cursor::new(b"   \n" as &[u8])).unwrap_err();
        assert!(err.to_string().contains("No SQL provided"));
    }

    #[test]
    fn test_read_sql_returns_content() {
        let sql = read_sql(std::io::Cursor::new(b"SELECT 1" as &[u8])).unwrap();
        assert_eq!(sql, "SELECT 1");
    }

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
        make_metadata_with_typed_param(name, "text", default)
    }

    fn make_metadata_with_typed_param(
        name: &str,
        param_type: &str,
        default: Option<serde_json::Value>,
    ) -> QueryMetadata {
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
                    param_type: param_type.to_string(),
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
        let result = build_parameter_map(&metadata, &cli_params, false, false)
            .unwrap()
            .unwrap();
        assert_eq!(result["days"], serde_json::Value::String("90".to_string()));
    }

    #[test]
    fn test_build_parameter_map_interactive_no_tty_uses_default() {
        let metadata = make_metadata_with_param("p", Some(serde_json::json!("default_val")));
        let result = build_parameter_map(&metadata, &[], true, false).unwrap();
        let map = result.unwrap();
        assert_eq!(map["p"], serde_json::json!("default_val"));
    }

    #[test]
    fn test_build_parameter_map_interactive_no_tty_no_default_bails() {
        let metadata = make_metadata_with_param("p", None);
        let err = build_parameter_map(&metadata, &[], true, false).unwrap_err();
        assert!(err.to_string().contains("--param p="));
    }

    #[test]
    fn test_build_parameter_map_interactive_no_tty_cli_param_overrides() {
        let metadata = make_metadata_with_param("p", None);
        let cli_params = vec![("p".to_string(), serde_json::json!("provided"))];
        let result = build_parameter_map(&metadata, &cli_params, true, false).unwrap();
        let map = result.unwrap();
        assert_eq!(map["p"], serde_json::json!("provided"));
    }

    #[test]
    fn test_build_parameter_map_interactive_tty_cli_param_skips_prompt() {
        let metadata = make_metadata_with_param("p", None);
        let cli_params = vec![("p".to_string(), serde_json::json!("provided"))];
        // interactive + has_tty would prompt for a missing param (blocking on
        // stdin); supplying it via CLI must satisfy it without prompting.
        let result = build_parameter_map(&metadata, &cli_params, true, true).unwrap();
        let map = result.unwrap();
        assert_eq!(map["p"], serde_json::json!("provided"));
    }

    #[test]
    fn test_build_parameter_map_resolves_range_default_token() {
        use chrono::{Duration, Local};
        let metadata = make_metadata_with_typed_param(
            "range",
            "date-range",
            Some(serde_json::json!("d_last_7_days")),
        );
        let map = build_parameter_map(&metadata, &[], false, false)
            .unwrap()
            .unwrap();
        let range = &map["range"];

        let today = Local::now().naive_local().date();
        let expected_start = (today - Duration::days(7)).format("%Y-%m-%d").to_string();
        let expected_end = today.format("%Y-%m-%d").to_string();
        assert_eq!(range["start"], serde_json::json!(expected_start));
        assert_eq!(range["end"], serde_json::json!(expected_end));
    }

    #[test]
    fn test_build_parameter_map_resolves_cli_date_token() {
        use chrono::Local;
        let metadata = make_metadata_with_typed_param("d", "date", None);
        let cli_params = vec![("d".to_string(), serde_json::json!("d_now"))];
        let map = build_parameter_map(&metadata, &cli_params, false, false)
            .unwrap()
            .unwrap();
        let expected = Local::now().naive_local().format("%Y-%m-%d").to_string();
        assert_eq!(map["d"], serde_json::json!(expected));
    }

    #[test]
    fn test_build_parameter_map_leaves_text_token_literal() {
        let metadata = make_metadata_with_typed_param("t", "text", None);
        let cli_params = vec![("t".to_string(), serde_json::json!("d_now"))];
        let map = build_parameter_map(&metadata, &cli_params, false, false)
            .unwrap()
            .unwrap();
        assert_eq!(map["t"], serde_json::json!("d_now"));
    }

    #[test]
    fn test_build_parameter_map_resolves_dynamic_date_range_default() {
        use crate::models::{Parameter, QueryOptions};
        let metadata = QueryMetadata {
            id: 1,
            name: "test".to_string(),
            description: None,
            data_source_id: 1,
            user_id: None,
            schedule: None,
            options: QueryOptions {
                parameters: vec![Parameter {
                    name: "period".to_string(),
                    title: "period".to_string(),
                    param_type: "date-range".to_string(),
                    value: Some(serde_json::json!("d_last_7_days")),
                    enum_options: None,
                    query_id: None,
                    multi_values_options: None,
                }],
            },
            visualizations: vec![],
            tags: None,
        };

        let map = build_parameter_map(&metadata, &[], false, false)
            .unwrap()
            .unwrap();
        let period = map.get("period").unwrap();
        assert!(
            period.get("start").is_some(),
            "expected resolved start: {period}"
        );
        assert!(
            period.get("end").is_some(),
            "expected resolved end: {period}"
        );
    }

    #[test]
    fn test_build_parameter_map_interactive_tty_cli_param_coerces() {
        let metadata = make_metadata_with_param("days", None);
        let cli_params = vec![("days".to_string(), serde_json::json!(90))];
        let result = build_parameter_map(&metadata, &cli_params, true, true)
            .unwrap()
            .unwrap();
        assert_eq!(result["days"], serde_json::Value::String("90".to_string()));
    }

    #[test]
    fn test_build_parameter_map_non_interactive_tty_uses_default() {
        let metadata = make_metadata_with_param("p", Some(serde_json::json!("default_val")));
        // has_tty alone must not trigger prompting when not interactive.
        let result = build_parameter_map(&metadata, &[], false, true).unwrap();
        let map = result.unwrap();
        assert_eq!(map["p"], serde_json::json!("default_val"));
    }

    #[test]
    fn test_build_parameter_map_non_interactive_tty_no_default_bails() {
        let metadata = make_metadata_with_param("p", None);
        let err = build_parameter_map(&metadata, &[], false, true).unwrap_err();
        assert!(err.to_string().contains("Missing required parameter"));
    }
}

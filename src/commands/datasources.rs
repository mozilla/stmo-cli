#![allow(clippy::missing_errors_doc)]

use super::OutputFormat;
use crate::api::RedashClient;
use crate::models::DataSource;
use anyhow::Result;

fn build_status_string(ds: &DataSource) -> String {
    let mut status_parts = Vec::new();
    if ds.paused != 0 {
        status_parts.push("paused");
    }
    if ds.view_only {
        status_parts.push("view-only");
    }
    if let Some(desc) = &ds.description
        && desc.to_lowercase().contains("deprecated")
    {
        status_parts.push("deprecated");
    }

    if status_parts.is_empty() {
        String::new()
    } else {
        format!("[{}]", status_parts.join(", "))
    }
}

pub async fn list_data_sources(client: &RedashClient, format: OutputFormat) -> Result<()> {
    let data_sources = client.list_data_sources().await?;

    match format {
        OutputFormat::Json => {
            let json = serde_json::to_string_pretty(&data_sources)?;
            println!("{json}");
        }
        OutputFormat::Table => {
            println!("Fetching data sources from Redash...\n");
            println!("=== DATA SOURCES ({}) ===\n", data_sources.len());
            println!("{:<6} {:<40} {:<15} Status", "ID", "Name", "Type");
            println!("{}", "-".repeat(80));

            for ds in &data_sources {
                let status = build_status_string(ds);
                println!("{:<6} {:<40} {:<15} {}", ds.id, ds.name, ds.ds_type, status);
            }

            println!("\nUse 'stmo-cli data-sources <id>' to view details.");
            println!("Use 'stmo-cli data-sources <id> --schema' to view table schema.");
        }
    }

    Ok(())
}

pub async fn show_data_source(
    client: &RedashClient,
    data_source_id: u64,
    show_schema: bool,
    refresh_schema: bool,
    format: OutputFormat,
) -> Result<()> {
    let ds = client.get_data_source(data_source_id).await?;

    let schema = if show_schema {
        match client
            .get_data_source_schema(data_source_id, refresh_schema)
            .await
        {
            Ok(s) => Some(s),
            Err(e) => {
                eprintln!("Error fetching schema: {e:#}");
                None
            }
        }
    } else {
        None
    };

    match format {
        OutputFormat::Json => {
            let output = serde_json::json!({
                "data_source": ds,
                "schema": schema,
            });
            let json = serde_json::to_string_pretty(&output)?;
            println!("{json}");
        }
        OutputFormat::Table => {
            println!("=== DATA SOURCE: {} ===\n", ds.name);
            println!("ID:          {}", ds.id);
            println!("Type:        {}", ds.ds_type);

            if let Some(syntax) = &ds.syntax {
                println!("Syntax:      {syntax}");
            }

            if let Some(description) = &ds.description {
                println!("Description: {description}");
            }

            if ds.paused != 0 {
                println!("Status:      PAUSED");
                if let Some(reason) = &ds.pause_reason {
                    println!("Reason:      {reason}");
                }
            } else {
                println!("Status:      Active");
            }

            if ds.view_only {
                println!("Access:      View-only");
            }

            if let Some(queue) = &ds.queue_name {
                println!("Queue:       {queue}");
            }

            if show_schema {
                if let Some(schema) = schema {
                    println!("\n=== SCHEMA ({} tables) ===\n", schema.schema.len());

                    for table in &schema.schema {
                        println!("\nTable: {} ({} columns)", table.name, table.columns.len());
                        println!("  {:<40} Type", "Column");
                        println!("  {}", "-".repeat(60));

                        for column in &table.columns {
                            println!("  {:<40} {}", column.name, column.column_type);
                        }
                    }
                } else {
                    eprintln!("\nNote: Schema could not be fetched. See error above for details.");
                }
            } else {
                println!("\nUse --schema flag to view table schema.");
            }
        }
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn create_test_datasource(
        id: u64,
        name: &str,
        ds_type: &str,
        paused: u8,
        view_only: bool,
        description: Option<String>,
    ) -> DataSource {
        DataSource {
            id,
            name: name.to_string(),
            ds_type: ds_type.to_string(),
            syntax: Some("sql".to_string()),
            description,
            paused,
            pause_reason: None,
            view_only,
            queue_name: None,
            scheduled_queue_name: None,
            groups: None,
            options: None,
        }
    }

    #[test]
    fn test_build_status_string_no_status() {
        let ds = create_test_datasource(1, "Test DB", "bigquery", 0, false, None);
        assert_eq!(build_status_string(&ds), "");
    }

    #[test]
    fn test_build_status_string_paused() {
        let ds = create_test_datasource(1, "Test DB", "bigquery", 1, false, None);
        assert_eq!(build_status_string(&ds), "[paused]");
    }

    #[test]
    fn test_build_status_string_view_only() {
        let ds = create_test_datasource(1, "Test DB", "bigquery", 0, true, None);
        assert_eq!(build_status_string(&ds), "[view-only]");
    }

    #[test]
    fn test_build_status_string_deprecated() {
        let ds = create_test_datasource(
            1,
            "Test DB",
            "bigquery",
            0,
            false,
            Some("This is deprecated".to_string()),
        );
        assert_eq!(build_status_string(&ds), "[deprecated]");
    }

    #[test]
    fn test_build_status_string_multiple() {
        let ds = create_test_datasource(
            1,
            "Test DB",
            "bigquery",
            1,
            true,
            Some("This is deprecated".to_string()),
        );
        assert_eq!(build_status_string(&ds), "[paused, view-only, deprecated]");
    }

    #[test]
    fn test_build_status_string_deprecated_case_insensitive() {
        let ds1 = create_test_datasource(
            1,
            "Test DB",
            "bigquery",
            0,
            false,
            Some("DEPRECATED: do not use".to_string()),
        );
        assert_eq!(build_status_string(&ds1), "[deprecated]");

        let ds2 = create_test_datasource(
            2,
            "Test DB",
            "bigquery",
            0,
            false,
            Some("Deprecated API".to_string()),
        );
        assert_eq!(build_status_string(&ds2), "[deprecated]");
    }

    #[test]
    fn test_build_status_string_no_description() {
        let ds = create_test_datasource(1, "Test DB", "bigquery", 0, true, None);
        assert_eq!(build_status_string(&ds), "[view-only]");
    }

    #[test]
    fn test_build_status_string_description_no_deprecated() {
        let ds = create_test_datasource(
            1,
            "Test DB",
            "bigquery",
            0,
            false,
            Some("Some other description".to_string()),
        );
        assert_eq!(build_status_string(&ds), "");
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
        assert!("invalid".parse::<OutputFormat>().is_err());
        assert!("csv".parse::<OutputFormat>().is_err());
    }
}

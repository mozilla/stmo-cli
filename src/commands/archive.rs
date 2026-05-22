#![allow(clippy::missing_errors_doc)]

use anyhow::{Context, Result};
use std::fs;
use std::path::Path;

use crate::api::RedashClient;

fn find_query_files(query_id: u64) -> Result<Option<(String, String)>> {
    let queries_dir = Path::new("queries");

    if !queries_dir.exists() {
        return Ok(None);
    }

    let mut sql_path = None;
    let mut yaml_path = None;

    for entry in fs::read_dir(queries_dir).context("Failed to read queries directory")? {
        let entry = entry.context("Failed to read directory entry")?;
        let path = entry.path();

        if let Some(filename) = path.file_name().and_then(|f| f.to_str())
            && let Some(id_str) = filename.split('-').next()
            && let Ok(id) = id_str.parse::<u64>()
            && id == query_id
        {
            if path.extension().is_some_and(|ext| ext == "sql") {
                sql_path = Some(path.to_string_lossy().to_string());
            } else if path.extension().is_some_and(|ext| ext == "yaml") {
                yaml_path = Some(path.to_string_lossy().to_string());
            }
        }
    }

    match (sql_path, yaml_path) {
        (Some(sql), Some(yaml)) => Ok(Some((sql, yaml))),
        _ => Ok(None),
    }
}

fn delete_query_files(sql_path: &str, yaml_path: &str) -> Result<()> {
    fs::remove_file(sql_path).context(format!("Failed to delete {sql_path}"))?;
    fs::remove_file(yaml_path).context(format!("Failed to delete {yaml_path}"))?;
    Ok(())
}

pub async fn archive(client: &RedashClient, query_ids: Vec<u64>) -> Result<()> {
    let mut errors = Vec::new();
    let mut archived_count = 0;

    println!("Archiving {} queries...\n", query_ids.len());

    for query_id in &query_ids {
        match client.archive_query(*query_id).await {
            Ok(query) => {
                println!("  ✓ Archived query {query_id} - {}", query.name);

                if let Ok(Some((sql_path, yaml_path))) = find_query_files(*query_id) {
                    if let Err(e) = delete_query_files(&sql_path, &yaml_path) {
                        eprintln!("  ⚠ Failed to delete local files for query {query_id}: {e}");
                    } else {
                        println!("    Deleted local files");
                    }
                } else {
                    println!("    No local files found");
                }

                archived_count += 1;
            }
            Err(e) => {
                eprintln!("  ✗ Failed to archive query {query_id}: {e}");
                errors.push((*query_id, e));
            }
        }
    }

    println!("\n✓ Archived {archived_count}/{} queries", query_ids.len());

    if !errors.is_empty() {
        anyhow::bail!("Failed to archive {} queries", errors.len());
    }

    Ok(())
}

pub async fn cleanup(client: &RedashClient) -> Result<()> {
    let queries_dir = Path::new("queries");

    if !queries_dir.exists() {
        println!("No queries directory found");
        return Ok(());
    }

    let mut query_ids = Vec::new();

    for entry in fs::read_dir(queries_dir).context("Failed to read queries directory")? {
        let entry = entry.context("Failed to read directory entry")?;
        let path = entry.path();

        if path.extension().is_some_and(|ext| ext == "yaml")
            && let Some(filename) = path.file_name().and_then(|f| f.to_str())
            && let Some(id_str) = filename.split('-').next()
            && let Ok(id) = id_str.parse::<u64>()
        {
            query_ids.push(id);
        }
    }

    query_ids.sort_unstable();
    query_ids.dedup();

    if query_ids.is_empty() {
        println!("No queries found in queries/ directory");
        return Ok(());
    }

    println!(
        "Checking {} queries for archive status...\n",
        query_ids.len()
    );

    let mut cleaned_count = 0;
    let mut errors = Vec::new();

    for query_id in &query_ids {
        match client.get_query(*query_id).await {
            Ok(query) => {
                if query.is_archived {
                    println!("  Found archived query {query_id} - {}", query.name);

                    if let Ok(Some((sql_path, yaml_path))) = find_query_files(*query_id) {
                        if let Err(e) = delete_query_files(&sql_path, &yaml_path) {
                            eprintln!("    ✗ Failed to delete files: {e}");
                            errors.push((*query_id, e));
                        } else {
                            println!("    ✓ Deleted local files");
                            cleaned_count += 1;
                        }
                    }
                }
            }
            Err(e) => {
                eprintln!("  ⚠ Failed to check query {query_id}: {e}");
            }
        }
    }

    if cleaned_count > 0 {
        println!("\n✓ Cleaned up {cleaned_count} archived queries");
    } else {
        println!("\n✓ No archived queries with local files found");
    }

    if !errors.is_empty() {
        anyhow::bail!("Failed to clean up {} queries", errors.len());
    }

    Ok(())
}

pub async fn unarchive(client: &RedashClient, query_ids: Vec<u64>) -> Result<()> {
    let mut errors = Vec::new();
    let mut unarchived_count = 0;

    println!("Unarchiving {} queries...\n", query_ids.len());

    for query_id in &query_ids {
        match client.unarchive_query(*query_id).await {
            Ok(query) => {
                println!("  ✓ Unarchived query {query_id} - {}", query.name);
                unarchived_count += 1;
            }
            Err(e) => {
                let error_msg = e.to_string();
                if error_msg.contains("403") || error_msg.contains("Permission") {
                    eprintln!("  ✗ Permission denied to unarchive query {query_id}");
                } else {
                    eprintln!("  ✗ Failed to unarchive query {query_id}: {e}");
                }
                errors.push((*query_id, e));
            }
        }
    }

    println!(
        "\n✓ Unarchived {unarchived_count}/{} queries",
        query_ids.len()
    );

    if !errors.is_empty() {
        anyhow::bail!("Failed to unarchive {} queries", errors.len());
    }

    Ok(())
}

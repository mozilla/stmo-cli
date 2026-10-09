#![allow(clippy::missing_errors_doc)]

use anyhow::{Context, Result};
use std::fs;
use std::path::Path;

use crate::api::RedashClient;

fn find_query_files(query_id: u64) -> Result<Option<(String, String)>> {
    find_query_files_in(Path::new("queries"), query_id)
}

fn find_query_files_in(queries_dir: &Path, query_id: u64) -> Result<Option<(String, String)>> {
    let Some(file_set) = crate::commands::unique_query_file_set_by_id(queries_dir, query_id)?
    else {
        return Ok(None);
    };
    let (Some(sql_path), Some(yaml_path)) = (file_set.sql, file_set.yaml) else {
        return Ok(None);
    };

    Ok(Some((
        sql_path.to_string_lossy().to_string(),
        yaml_path.to_string_lossy().to_string(),
    )))
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
        let local_files = find_query_files(*query_id)?;
        match client.archive_query(*query_id).await {
            Ok(query) => {
                println!("  ✓ Archived query {query_id} - {}", query.name);

                if let Some((sql_path, yaml_path)) = local_files {
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
            && let Some(id) = crate::commands::query_id_from_path(&path)
            && id != 0
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

                    match find_query_files(*query_id) {
                        Ok(Some((sql_path, yaml_path))) => {
                            if let Err(e) = delete_query_files(&sql_path, &yaml_path) {
                                eprintln!("    ✗ Failed to delete files: {e}");
                                errors.push((*query_id, e));
                            } else {
                                println!("    ✓ Deleted local files");
                                cleaned_count += 1;
                            }
                        }
                        Ok(None) => {}
                        Err(e) => {
                            eprintln!("    ✗ Failed to locate local files: {e}");
                            errors.push((*query_id, e));
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

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    #[test]
    fn find_query_files_reads_id_only_files() {
        let temp_dir = TempDir::new().unwrap();
        let queries_dir = temp_dir.path();
        let sql_path = queries_dir.join("1200000001.sql");
        let yaml_path = queries_dir.join("1200000001.yaml");
        fs::write(&sql_path, "SELECT 1").unwrap();
        fs::write(&yaml_path, "id: 1200000001").unwrap();

        let found = find_query_files_in(queries_dir, 1_200_000_001).unwrap();
        assert_eq!(
            found,
            Some((
                sql_path.to_string_lossy().to_string(),
                yaml_path.to_string_lossy().to_string()
            ))
        );
    }

    #[test]
    fn find_query_files_returns_none_for_an_incomplete_pair() {
        let temp_dir = TempDir::new().unwrap();
        let queries_dir = temp_dir.path();
        fs::write(queries_dir.join("1200000001.yaml"), "id: 1200000001").unwrap();

        assert_eq!(
            find_query_files_in(queries_dir, 1_200_000_001).unwrap(),
            None
        );
    }
}

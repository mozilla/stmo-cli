#![allow(clippy::missing_errors_doc)]

use anyhow::{Context, Result};
use std::fs;
use std::path::Path;

use crate::api::RedashClient;

fn slugify(s: &str) -> String {
    s.to_lowercase()
        .chars()
        .map(|c| if c.is_alphanumeric() { c } else { '-' })
        .collect::<String>()
        .split('-')
        .filter(|s| !s.is_empty())
        .collect::<Vec<_>>()
        .join("-")
}

fn extract_query_ids_from_directory() -> Result<Vec<u64>> {
    let queries_dir = Path::new("queries");

    if !queries_dir.exists() {
        return Ok(Vec::new());
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

    Ok(query_ids)
}

pub async fn fetch(client: &RedashClient, query_ids: Vec<u64>, all: bool) -> Result<()> {
    fs::create_dir_all("queries").context("Failed to create queries directory")?;

    let existing_query_ids = extract_query_ids_from_directory()?;

    let queries_to_fetch = if all {
        if existing_query_ids.is_empty() {
            anyhow::bail!(
                "No queries found in queries/ directory. Use specific query IDs or run 'discover' to see available queries."
            );
        }
        println!(
            "Fetching {} queries from local directory...\n",
            existing_query_ids.len()
        );
        let mut queries = Vec::new();
        for id in &existing_query_ids {
            match client.get_query(*id).await {
                Ok(query) => queries.push(query),
                Err(e) => eprintln!("  ⚠ Query {id} failed to fetch: {e}"),
            }
        }
        queries
    } else if !query_ids.is_empty() {
        println!("Fetching {} specific queries...\n", query_ids.len());
        let mut queries = Vec::new();
        for id in &query_ids {
            match client.get_query(*id).await {
                Ok(query) => queries.push(query),
                Err(e) => eprintln!("  ⚠ Query {id} failed to fetch: {e}"),
            }
        }
        queries
    } else {
        anyhow::bail!(
            "No query IDs specified. Use --all to fetch tracked queries, or provide specific query IDs.\n\nExamples:\n  stmo-cli fetch --all\n  stmo-cli fetch 123 456 789\n  stmo-cli discover  (to see available queries)"
        );
    };

    println!("Fetching {} queries...", queries_to_fetch.len());

    let mut archived_queries = Vec::new();

    for query in &queries_to_fetch {
        let slug = slugify(&query.name);
        let filename_base = format!("{}-{}", query.id, slug);

        let sql_path = format!("queries/{filename_base}.sql");
        fs::write(&sql_path, &query.sql).context(format!("Failed to write {sql_path}"))?;

        let mut visualizations: Vec<crate::models::VisualizationMetadata> = query
            .visualizations
            .iter()
            .map(crate::models::VisualizationMetadata::from)
            .collect();
        visualizations.sort_by_key(|v| v.id);
        let metadata = crate::models::QueryMetadata {
            id: query.id,
            name: query.name.clone(),
            description: query.description.clone(),
            data_source_id: query.data_source_id,
            user_id: query.user.as_ref().map(|u| u.id),
            schedule: query.schedule.clone(),
            options: query.options.clone(),
            visualizations,
            tags: query.tags.clone(),
        };

        let yaml_path = format!("queries/{filename_base}.yaml");
        let yaml_content =
            serde_yaml::to_string(&metadata).context("Failed to serialize query metadata")?;
        fs::write(&yaml_path, yaml_content).context(format!("Failed to write {yaml_path}"))?;

        if query.is_archived {
            archived_queries.push((query.id, query.name.clone()));
            println!("  ✓ {} - {} [ARCHIVED]", query.id, query.name);
        } else {
            println!("  ✓ {} - {}", query.id, query.name);
        }
    }

    println!("\n✓ All resources fetched successfully");

    if !archived_queries.is_empty() {
        println!(
            "\n⚠ Warning: {} archived queries have local files:",
            archived_queries.len()
        );
        for (id, name) in &archived_queries {
            println!("  - {id}: {name}");
        }
        let binary_name = std::env::args()
            .next()
            .and_then(|path| {
                std::path::Path::new(&path)
                    .file_name()
                    .map(|s| s.to_string_lossy().to_string())
            })
            .unwrap_or_else(|| "stmo-cli".to_string());
        println!("\nConsider cleaning up with: {binary_name} archive --cleanup");
    }

    Ok(())
}

#[cfg(test)]
#[allow(clippy::missing_errors_doc)]
mod tests {
    use super::*;

    #[test]
    fn test_slugify_simple() {
        assert_eq!(slugify("Hello World"), "hello-world");
    }

    #[test]
    fn test_slugify_special_chars() {
        assert_eq!(slugify("Foo & Bar!"), "foo-bar");
        assert_eq!(slugify("Test@#$%Query"), "test-query");
    }

    #[test]
    fn test_slugify_unicode() {
        assert_eq!(slugify("Café Münch"), "café-münch");
        assert_eq!(slugify("日本語"), "日本語");
    }

    #[test]
    fn test_slugify_multiple_spaces() {
        assert_eq!(slugify("a  b   c"), "a-b-c");
        assert_eq!(slugify("  leading and trailing  "), "leading-and-trailing");
    }

    #[test]
    fn test_slugify_already_slugified() {
        assert_eq!(slugify("already-slug"), "already-slug");
        assert_eq!(slugify("some-kebab-case"), "some-kebab-case");
    }

    #[test]
    fn test_slugify_numbers() {
        assert_eq!(slugify("Query 123"), "query-123");
        assert_eq!(slugify("123-456"), "123-456");
    }

    #[test]
    fn test_slugify_mixed() {
        assert_eq!(slugify("Mozilla's .deb Package!"), "mozilla-s-deb-package");
        assert_eq!(
            slugify("Copy of 100234 - Gecko decision task"),
            "copy-of-100234-gecko-decision-task"
        );
    }
}

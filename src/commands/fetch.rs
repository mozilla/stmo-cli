#![allow(clippy::missing_errors_doc)]

use anyhow::{Context, Result};
use std::fs;
use std::path::Path;

use crate::api::RedashClient;
use crate::commands::QueryFileSet;
use crate::models::{Query, QueryMetadata, VisualizationMetadata};

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
            && let Some(id) = crate::commands::query_id_from_path(&path)
            && id != 0
        {
            query_ids.push(id);
        }
    }

    query_ids.sort_unstable();
    query_ids.dedup();

    Ok(query_ids)
}

fn write_fetched_query(queries_dir: &Path, query: &Query) -> Result<bool> {
    let file_sets = crate::commands::query_file_sets_by_id(queries_dir, query.id)?;

    for file_set in &file_sets {
        let (Some(sql_path), Some(yaml_path)) = (&file_set.sql, &file_set.yaml) else {
            anyhow::bail!(
                "Incomplete local query files at {}; leaving them unchanged",
                file_set.base.display()
            );
        };

        let sql = fs::read_to_string(sql_path)
            .context(format!("Failed to read {}", sql_path.display()))?;
        let yaml_content = fs::read_to_string(yaml_path)
            .context(format!("Failed to read {}", yaml_path.display()))?;
        let metadata: QueryMetadata = serde_yaml::from_str(&yaml_content)
            .context(format!("Failed to parse {}", yaml_path.display()))?;

        if metadata.id != query.id {
            anyhow::bail!(
                "{} declares query ID {}, expected {}; leaving local files unchanged",
                yaml_path.display(),
                metadata.id,
                query.id
            );
        }

        crate::commands::ensure_query_filename_matches_identity(
            yaml_path,
            metadata.id,
            &metadata.name,
        )?;

        if crate::commands::deploy::tracked_query_content_differs(&sql, &metadata, query) {
            anyhow::bail!(
                "Local SQL or metadata differs from Redash for query {}; leaving {}.* unchanged. Resolve or deploy the local changes before fetching",
                query.id,
                file_set.base.display()
            );
        }
    }

    let mut visualizations: Vec<VisualizationMetadata> = query
        .visualizations
        .iter()
        .map(VisualizationMetadata::from)
        .collect();
    visualizations.sort_by_key(|visualization| visualization.id);
    let metadata = QueryMetadata {
        id: query.id,
        name: query.name.clone(),
        description: query.description.clone(),
        data_source_id: query.data_source_id,
        user_id: query.user.as_ref().map(|user| user.id),
        schedule: query.schedule.clone(),
        options: query.options.clone(),
        visualizations,
        tags: query.tags.clone(),
    };
    let yaml_content =
        serde_yaml::to_string(&metadata).context("Failed to serialize query metadata")?;

    let canonical_base = queries_dir.join(query.id.to_string());
    let sql_path = canonical_base.with_extension("sql");
    let yaml_path = canonical_base.with_extension("yaml");
    fs::write(&sql_path, &query.sql).context(format!("Failed to write {}", sql_path.display()))?;
    fs::write(&yaml_path, yaml_content)
        .context(format!("Failed to write {}", yaml_path.display()))?;

    remove_legacy_file_sets(&file_sets, &canonical_base)
}

fn remove_legacy_file_sets(file_sets: &[QueryFileSet], canonical_base: &Path) -> Result<bool> {
    let mut migrated = false;
    let mut leftovers = Vec::new();

    for file_set in file_sets
        .iter()
        .filter(|file_set| file_set.base != canonical_base)
    {
        for path in file_set.sql.iter().chain(&file_set.yaml) {
            if let Err(error) = fs::remove_file(path) {
                leftovers.push(format!("{} ({error})", path.display()));
            }
        }
        migrated = true;
    }

    if leftovers.is_empty() {
        return Ok(migrated);
    }
    anyhow::bail!(
        "Wrote {}.* but could not delete legacy query files: {}",
        canonical_base.display(),
        leftovers.join(", ")
    );
}

pub async fn fetch(client: &RedashClient, query_ids: Vec<u64>, all: bool) -> Result<()> {
    fs::create_dir_all("queries").context("Failed to create queries directory")?;

    let existing_query_ids = extract_query_ids_from_directory()?;

    let mut failures = Vec::new();
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
                Err(e) => {
                    eprintln!("  ⚠ Query {id} failed to fetch: {e}");
                    failures.push(format!("{id}: {e}"));
                }
            }
        }
        queries
    } else if !query_ids.is_empty() {
        println!("Fetching {} specific queries...\n", query_ids.len());
        let mut queries = Vec::new();
        for id in &query_ids {
            match client.get_query(*id).await {
                Ok(query) => queries.push(query),
                Err(e) => {
                    eprintln!("  ⚠ Query {id} failed to fetch: {e}");
                    failures.push(format!("{id}: {e}"));
                }
            }
        }
        queries
    } else {
        anyhow::bail!(
            "No query IDs specified. Use --all to fetch tracked queries, or provide specific query IDs.\n\nExamples:\n  stmo-cli fetch --all\n  stmo-cli fetch <query-id> [<query-id>...]\n  stmo-cli discover  (to see available queries)"
        );
    };

    println!("Fetching {} queries...", queries_to_fetch.len());

    let mut archived_queries = Vec::new();
    let mut fetched_count = 0;

    for query in &queries_to_fetch {
        let migrated = match write_fetched_query(Path::new("queries"), query) {
            Ok(migrated) => migrated,
            Err(error) => {
                eprintln!("  ✗ Query {} - {}: {error:#}", query.id, query.name);
                failures.push(format!("{}: {error:#}", query.id));
                continue;
            }
        };
        fetched_count += 1;

        if query.is_archived {
            archived_queries.push((query.id, query.name.clone()));
            println!("  ✓ {} - {} [ARCHIVED]", query.id, query.name);
        } else {
            println!("  ✓ {} - {}", query.id, query.name);
        }
        if migrated {
            println!("    Migrated local files to queries/{}.*", query.id);
        }
    }

    println!(
        "\n✓ Fetched {fetched_count}/{} queries",
        queries_to_fetch.len()
    );

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

    if !failures.is_empty() {
        anyhow::bail!(
            "Failed to fetch {} query operation(s): {}",
            failures.len(),
            failures.join(", ")
        );
    }

    Ok(())
}

#[cfg(test)]
#[allow(clippy::missing_errors_doc)]
mod tests {
    use super::*;
    use crate::models::QueryOptions;
    use std::os::unix::fs::PermissionsExt;
    use tempfile::TempDir;

    fn make_query(sql: &str) -> Query {
        Query {
            id: 1_200_000_001,
            name: "Example Query".to_string(),
            description: None,
            sql: sql.to_string(),
            data_source_id: 1_500_000_001,
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

    #[cfg(unix)]
    #[test]
    fn write_fetched_query_names_every_legacy_file_it_cannot_remove() {
        let temp_dir = TempDir::new().unwrap();
        let queries_dir = temp_dir.path();
        let query = make_query("SELECT 1");
        write_fetched_query(queries_dir, &query).unwrap();
        let legacy_sql = queries_dir.join("1200000001-old-slug.sql");
        let legacy_yaml = queries_dir.join("1200000001-old-slug.yaml");
        fs::copy(queries_dir.join("1200000001.sql"), &legacy_sql).unwrap();
        fs::copy(queries_dir.join("1200000001.yaml"), &legacy_yaml).unwrap();
        fs::set_permissions(queries_dir, fs::Permissions::from_mode(0o555)).unwrap();

        let result = write_fetched_query(queries_dir, &query);

        fs::set_permissions(queries_dir, fs::Permissions::from_mode(0o755)).unwrap();
        let message = format!("{:#}", result.unwrap_err());
        assert!(
            message.contains(&legacy_sql.display().to_string()),
            "{message}"
        );
        assert!(
            message.contains(&legacy_yaml.display().to_string()),
            "{message}"
        );
    }

    #[test]
    fn test_slugify_simple() {
        assert_eq!(crate::commands::query_slugify("Hello World"), "hello-world");
    }

    #[test]
    fn test_slugify_special_chars() {
        assert_eq!(crate::commands::query_slugify("Foo & Bar!"), "foo-bar");
        assert_eq!(
            crate::commands::query_slugify("Test@#$%Query"),
            "test-query"
        );
    }

    #[test]
    fn test_slugify_unicode() {
        assert_eq!(crate::commands::query_slugify("Café Münch"), "café-münch");
        assert_eq!(crate::commands::query_slugify("日本語"), "日本語");
    }

    #[test]
    fn test_slugify_multiple_spaces() {
        assert_eq!(crate::commands::query_slugify("a  b   c"), "a-b-c");
        assert_eq!(
            crate::commands::query_slugify("  leading and trailing  "),
            "leading-and-trailing"
        );
    }

    #[test]
    fn test_slugify_already_slugified() {
        assert_eq!(
            crate::commands::query_slugify("already-slug"),
            "already-slug"
        );
        assert_eq!(
            crate::commands::query_slugify("some-kebab-case"),
            "some-kebab-case"
        );
    }

    #[test]
    fn test_slugify_numbers() {
        assert_eq!(crate::commands::query_slugify("Query 123"), "query-123");
        assert_eq!(crate::commands::query_slugify("123-456"), "123-456");
    }

    #[test]
    fn test_slugify_mixed() {
        assert_eq!(
            crate::commands::query_slugify("Mozilla's .deb Package!"),
            "mozilla-s-deb-package"
        );
        assert_eq!(
            crate::commands::query_slugify("Copy of an example query"),
            "copy-of-an-example-query"
        );
    }
}

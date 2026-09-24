#![allow(clippy::missing_errors_doc)]

use crate::api::RedashClient;
use crate::models::{Query, QueryMetadata, Visualization, VisualizationMetadata};
use anyhow::{Context, Result, bail};
use std::collections::{BTreeMap, HashSet};
use std::fs;
use std::path::Path;
use std::sync::Arc;
use tokio::sync::Semaphore;
use tokio::task::JoinSet;

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

fn validate_enum_options(metadata: &crate::models::QueryMetadata, yaml_path: &str) -> Result<()> {
    for param in &metadata.options.parameters {
        if let Some(enum_opts) = &param.enum_options
            && enum_opts.contains("\\n")
        {
            bail!(
                "In {yaml_path}: parameter '{}' has enumOptions with escaped newlines. \
                Use YAML multiline format instead:\n\n\
                enumOptions: |-\n  option1\n  option2",
                param.name
            );
        }
    }
    Ok(())
}

// A local visualization with no `id` is an unsaved one the user is asking to
// create — it always counts as "different" regardless of what's on the
// server.
pub(crate) fn visualizations_differ(
    local: &[VisualizationMetadata],
    server: &[Visualization],
) -> bool {
    if local.len() != server.len() {
        return true;
    }

    for viz in local {
        let Some(id) = viz.id else {
            return true;
        };
        let Some(server_viz) = server.iter().find(|sv| sv.id == id) else {
            return true;
        };
        if viz.name != server_viz.name
            || viz.viz_type != server_viz.viz_type
            || viz.options != server_viz.options
            || viz.description != server_viz.description
        {
            return true;
        }
    }

    false
}

// Shared by `deploy` (deciding what to push) and `execute` (deciding whether
// to sync before running) — compares everything `deploy_one` actually pushes
// to Redash against what's already there, so "changed" means "differs from
// the server", not "differs from git's working tree".
pub(crate) fn tracked_query_differs(
    local_sql: &str,
    local_metadata: &QueryMetadata,
    server: &Query,
) -> bool {
    local_sql != server.sql
        || local_metadata.name != server.name
        || local_metadata.description != server.description
        || local_metadata.data_source_id != server.data_source_id
        || local_metadata.schedule != server.schedule
        || local_metadata.tags != server.tags
        || serde_json::to_value(&local_metadata.options).ok()
            != serde_json::to_value(&server.options).ok()
        || visualizations_differ(&local_metadata.visualizations, &server.visualizations)
}

fn read_local_query(id: u64, name: &str) -> Result<(String, QueryMetadata)> {
    let slug = slugify(name);
    let sql_path = format!("queries/{id}-{slug}.sql");
    let yaml_path = format!("queries/{id}-{slug}.yaml");

    let sql = fs::read_to_string(&sql_path).context(format!("Failed to read {sql_path}"))?;
    let metadata_content =
        fs::read_to_string(&yaml_path).context(format!("Failed to read {yaml_path}"))?;
    let metadata: QueryMetadata =
        serde_yaml::from_str(&metadata_content).context(format!("Failed to parse {yaml_path}"))?;

    Ok((sql, metadata))
}

async fn query_changed(client: &RedashClient, id: u64, name: &str) -> Result<bool> {
    let (sql, metadata) = read_local_query(id, name)?;
    let server = client
        .get_query(id)
        .await
        .context(format!("Failed to fetch query {id} from Redash"))?;
    Ok(tracked_query_differs(&sql, &metadata, &server))
}

const MAX_CONCURRENT_COMPARISONS: usize = 8;

// Compares every tracked query (except `id == 0`, which has nothing on the
// server yet and is always deployed) against its server copy, up to
// `MAX_CONCURRENT_COMPARISONS` GETs in flight at once. A query that fails to
// compare (deleted or archived server-side, unreadable local files, ...) is
// skipped with a warning rather than aborting the whole run.
async fn find_changed_queries(
    client: &RedashClient,
    all_queries: &[(u64, String)],
) -> HashSet<u64> {
    let total = all_queries.len();
    let semaphore = Arc::new(Semaphore::new(MAX_CONCURRENT_COMPARISONS));
    let mut join_set = JoinSet::new();
    let mut changed_ids = HashSet::new();

    for (id, name) in all_queries {
        let id = *id;
        if id == 0 {
            changed_ids.insert(id);
            continue;
        }

        let name = name.clone();
        let client = client.clone();
        let semaphore = Arc::clone(&semaphore);
        join_set.spawn(async move {
            let _permit = semaphore
                .acquire()
                .await
                .expect("semaphore is never closed");
            (id, query_changed(&client, id, &name).await)
        });
    }

    eprintln!("Comparing {total} tracked queries against Redash...");
    let mut compared = 0;

    while let Some(result) = join_set.join_next().await {
        compared += 1;
        match result {
            Ok((id, Ok(true))) => {
                changed_ids.insert(id);
            }
            Ok((_id, Ok(false))) => {}
            Ok((id, Err(e))) => {
                eprintln!("  ⚠ Skipping query {id}: {e}");
            }
            Err(join_err) => {
                eprintln!("  ⚠ A deploy comparison task failed unexpectedly: {join_err}");
            }
        }
        eprintln!("Compared {compared} / {total} queries...");
    }

    changed_ids
}

fn get_all_query_metadata_from_path(queries_dir: &Path) -> Result<Vec<(u64, String)>> {
    if !queries_dir.exists() {
        bail!("queries directory not found. Run 'stmo-cli fetch' first.");
    }

    let mut queries = Vec::new();
    let mut paths_by_target: BTreeMap<String, Vec<String>> = BTreeMap::new();

    for entry in fs::read_dir(queries_dir).context("Failed to read queries directory")? {
        let entry = entry.context("Failed to read directory entry")?;
        let path = entry.path();

        if path.extension().is_some_and(|ext| ext == "yaml") {
            let metadata_content =
                fs::read_to_string(&path).context(format!("Failed to read {}", path.display()))?;

            let metadata: crate::models::QueryMetadata = serde_yaml::from_str(&metadata_content)
                .context(format!("Failed to parse {}", path.display()))?;

            crate::commands::ensure_filename_matches_identity(
                &path,
                &format!("{}-{}", metadata.id, slugify(&metadata.name)),
                "name",
            )?;

            // A tracked id can legitimately be claimed by two differently
            // named files; `id: 0` cannot, because the filename check above
            // already pins each new resource to a unique file base.
            if metadata.id != 0 {
                paths_by_target
                    .entry(format!("id {}", metadata.id))
                    .or_default()
                    .push(path.display().to_string());
            }
            queries.push((metadata.id, metadata.name));
        }
    }

    crate::commands::bail_on_duplicate_targets(&paths_by_target)?;

    queries.sort_by_key(|(id, _)| *id);

    Ok(queries)
}

fn get_all_query_metadata() -> Result<Vec<(u64, String)>> {
    get_all_query_metadata_from_path(Path::new("queries"))
}

fn write_query_yaml(path: &str, query: &Query) -> Result<()> {
    let mut visualizations: Vec<VisualizationMetadata> = query
        .visualizations
        .iter()
        .map(VisualizationMetadata::from)
        .collect();
    visualizations.sort_by_key(|v| v.id);

    let metadata = QueryMetadata {
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

    let yaml_content =
        serde_yaml::to_string(&metadata).context("Failed to serialize query metadata")?;
    fs::write(path, yaml_content).context(format!("Failed to write {path}"))
}

async fn deploy_visualizations(
    client: &RedashClient,
    query_id: u64,
    visualizations: &[crate::models::VisualizationMetadata],
    server_visualizations: &[crate::models::Visualization],
) -> Result<()> {
    let mut matched_server_ids: HashSet<u64> = HashSet::new();
    for viz in visualizations {
        if let Some(id) = viz.id {
            matched_server_ids.insert(id);
            let viz_to_update = crate::models::Visualization {
                id,
                name: viz.name.clone(),
                viz_type: viz.viz_type.clone(),
                options: viz.options.clone(),
                description: viz.description.clone(),
            };
            client.update_visualization(&viz_to_update).await?;
            println!("    ✓ Updated visualization: {} (ID: {id})", viz.name);
        } else {
            let server_match = server_visualizations
                .iter()
                .find(|sv| sv.viz_type == viz.viz_type && !matched_server_ids.contains(&sv.id));
            if let Some(server_viz) = server_match {
                matched_server_ids.insert(server_viz.id);
                let viz_to_update = crate::models::Visualization {
                    id: server_viz.id,
                    name: viz.name.clone(),
                    viz_type: viz.viz_type.clone(),
                    options: viz.options.clone(),
                    description: viz.description.clone(),
                };
                client.update_visualization(&viz_to_update).await?;
                println!(
                    "    ✓ Updated visualization: {} (ID: {})",
                    viz_to_update.name, server_viz.id
                );
            } else {
                let viz_to_create = crate::models::CreateVisualization {
                    query_id,
                    name: viz.name.clone(),
                    viz_type: viz.viz_type.clone(),
                    options: viz.options.clone(),
                    description: viz.description.clone(),
                };
                let created = client
                    .create_visualization(query_id, &viz_to_create)
                    .await?;
                println!(
                    "    ✓ Created visualization: {} (ID: {})",
                    created.name, created.id
                );
            }
        }
    }
    Ok(())
}

#[allow(clippy::too_many_lines)]
pub async fn deploy_one(client: &RedashClient, id: u64, name: &str) -> Result<Query> {
    let slug = slugify(name);
    let sql_path = format!("queries/{id}-{slug}.sql");
    let yaml_path = format!("queries/{id}-{slug}.yaml");

    if !Path::new(&sql_path).exists() {
        bail!("Query SQL file not found: {sql_path}");
    }
    if !Path::new(&yaml_path).exists() {
        bail!("Query metadata file not found: {yaml_path}");
    }

    let sql = fs::read_to_string(&sql_path).context(format!("Failed to read {sql_path}"))?;

    let metadata_content =
        fs::read_to_string(&yaml_path).context(format!("Failed to read {yaml_path}"))?;

    let metadata: crate::models::QueryMetadata =
        serde_yaml::from_str(&metadata_content).context(format!("Failed to parse {yaml_path}"))?;

    validate_enum_options(&metadata, &yaml_path)?;

    let (result_query, final_yaml_path) = if id == 0 {
        let create_query = crate::models::CreateQuery {
            name: metadata.name.clone(),
            description: metadata.description.clone(),
            sql,
            data_source_id: metadata.data_source_id,
            schedule: metadata.schedule.clone(),
            options: Some(metadata.options.clone()),
            tags: metadata.tags.clone(),
            is_archived: false,
            is_draft: false,
        };
        let created = client.create_query(&create_query).await?;
        let fetched = client.get_query(created.id).await?;
        let new_slug = slugify(&fetched.name);
        let new_base = format!("queries/{}-{new_slug}", fetched.id);
        let new_yaml_path = format!("{new_base}.yaml");
        fs::write(format!("{new_base}.sql"), &fetched.sql)
            .context(format!("Failed to write {new_base}.sql"))?;
        write_query_yaml(&new_yaml_path, &fetched)?;
        fs::remove_file(&sql_path).context(format!("Failed to delete {sql_path}"))?;
        fs::remove_file(&yaml_path).context(format!("Failed to delete {yaml_path}"))?;
        println!("  ✓ Created new query: {} - {name}", fetched.id);
        println!("    Renamed: 0-{slug}.* → {}-{new_slug}.*", fetched.id);
        (fetched, new_yaml_path)
    } else {
        let query = Query {
            id: metadata.id,
            name: metadata.name.clone(),
            description: metadata.description.clone(),
            sql,
            data_source_id: metadata.data_source_id,
            user: None,
            schedule: metadata.schedule.clone(),
            options: metadata.options.clone(),
            visualizations: vec![],
            tags: metadata.tags.clone(),
            is_archived: false,
            is_draft: false,
            updated_at: String::new(),
            created_at: String::new(),
        };
        let result = client.create_or_update_query(&query).await?;
        println!("  ✓ {id} - {name}");
        (result, yaml_path.clone())
    };

    deploy_visualizations(
        client,
        result_query.id,
        &metadata.visualizations,
        &result_query.visualizations,
    )
    .await?;

    let final_query = client.get_query(result_query.id).await?;
    write_query_yaml(&final_yaml_path, &final_query)?;

    Ok(final_query)
}

pub async fn deploy(client: &RedashClient, query_ids: Vec<u64>, all: bool) -> Result<()> {
    let all_queries = get_all_query_metadata()?;

    let queries_to_deploy = if !query_ids.is_empty() {
        let ids_set: HashSet<_> = query_ids.iter().copied().collect();
        let filtered: Vec<_> = all_queries
            .into_iter()
            .filter(|(id, _)| ids_set.contains(id))
            .collect();

        if filtered.is_empty() {
            bail!("None of the specified query IDs were found in queries/ directory");
        }

        println!("Deploying {} specific queries...", filtered.len());
        for (id, name) in &filtered {
            println!("  → {id} - {name}");
        }
        println!();

        filtered
    } else if all {
        println!("Deploying all {} queries...\n", all_queries.len());
        all_queries
    } else {
        let changed_ids = find_changed_queries(client, &all_queries).await;

        if changed_ids.is_empty() {
            println!("No changed queries detected.");
            println!("Tip: Use --all to deploy all queries regardless of differences.");
            return Ok(());
        }

        let filtered: Vec<_> = all_queries
            .into_iter()
            .filter(|(id, _)| changed_ids.contains(id))
            .collect();

        println!("Deploying {} changed queries...", filtered.len());
        for (id, name) in &filtered {
            println!("  → {id} - {name}");
        }
        println!();

        filtered
    };

    for (id, name) in &queries_to_deploy {
        deploy_one(client, *id, name).await?;
    }

    println!("\n✓ All resources deployed successfully");

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_validate_enum_options_rejects_escaped_newlines() {
        let metadata = crate::models::QueryMetadata {
            id: 1,
            name: "Test Query".to_string(),
            description: None,
            data_source_id: 1,
            user_id: None,
            schedule: None,
            options: crate::models::QueryOptions {
                parameters: vec![crate::models::Parameter {
                    name: "test_param".to_string(),
                    title: "Test Param".to_string(),
                    param_type: "enum".to_string(),
                    enum_options: Some("option1\\noption2\\noption3".to_string()),
                    query_id: Some(1),
                    value: None,
                    multi_values_options: None,
                }],
            },
            visualizations: vec![],
            tags: None,
        };

        let result = validate_enum_options(&metadata, "test.yaml");
        assert!(result.is_err());
        let err_msg = result.unwrap_err().to_string();
        assert!(err_msg.contains("escaped newlines"));
        assert!(err_msg.contains("test_param"));
        assert!(err_msg.contains("YAML multiline format"));
    }

    #[test]
    fn test_validate_enum_options_accepts_multiline() {
        let metadata = crate::models::QueryMetadata {
            id: 1,
            name: "Test Query".to_string(),
            description: None,
            data_source_id: 1,
            user_id: None,
            schedule: None,
            options: crate::models::QueryOptions {
                parameters: vec![crate::models::Parameter {
                    name: "test_param".to_string(),
                    title: "Test Param".to_string(),
                    param_type: "enum".to_string(),
                    enum_options: Some("option1\noption2\noption3".to_string()),
                    query_id: Some(1),
                    value: None,
                    multi_values_options: None,
                }],
            },
            visualizations: vec![],
            tags: None,
        };

        let result = validate_enum_options(&metadata, "test.yaml");
        assert!(result.is_ok());
    }

    #[test]
    fn test_validate_enum_options_accepts_no_enum() {
        let metadata = crate::models::QueryMetadata {
            id: 1,
            name: "Test Query".to_string(),
            description: None,
            data_source_id: 1,
            user_id: None,
            schedule: None,
            options: crate::models::QueryOptions {
                parameters: vec![crate::models::Parameter {
                    name: "test_param".to_string(),
                    title: "Test Param".to_string(),
                    param_type: "text".to_string(),
                    enum_options: None,
                    query_id: Some(1),
                    value: None,
                    multi_values_options: None,
                }],
            },
            visualizations: vec![],
            tags: None,
        };

        let result = validate_enum_options(&metadata, "test.yaml");
        assert!(result.is_ok());
    }

    fn make_query_metadata(name: &str, data_source_id: u64) -> QueryMetadata {
        QueryMetadata {
            id: 1,
            name: name.to_string(),
            description: None,
            data_source_id,
            user_id: None,
            schedule: None,
            options: crate::models::QueryOptions { parameters: vec![] },
            visualizations: vec![],
            tags: None,
        }
    }

    fn make_server_query(sql: &str, name: &str, data_source_id: u64) -> Query {
        Query {
            id: 1,
            name: name.to_string(),
            description: None,
            sql: sql.to_string(),
            data_source_id,
            user: None,
            schedule: None,
            options: crate::models::QueryOptions { parameters: vec![] },
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
        metadata.options.parameters.push(crate::models::Parameter {
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
    fn test_tracked_query_differs_true_when_description_differs() {
        let mut metadata = make_query_metadata("Q", 1);
        metadata.description = Some("local".to_string());
        let mut server = make_server_query("SELECT 1", "Q", 1);
        server.description = Some("server".to_string());
        assert!(tracked_query_differs("SELECT 1", &metadata, &server));
    }

    #[test]
    fn test_tracked_query_differs_true_when_schedule_differs() {
        let mut metadata = make_query_metadata("Q", 1);
        metadata.schedule = Some(crate::models::Schedule {
            interval: Some(3600),
            time: None,
            day_of_week: None,
            until: None,
        });
        let server = make_server_query("SELECT 1", "Q", 1);
        assert!(tracked_query_differs("SELECT 1", &metadata, &server));
    }

    #[test]
    fn test_tracked_query_differs_true_when_tags_differ() {
        let mut metadata = make_query_metadata("Q", 1);
        metadata.tags = Some(vec!["a".to_string()]);
        let server = make_server_query("SELECT 1", "Q", 1);
        assert!(tracked_query_differs("SELECT 1", &metadata, &server));
    }

    fn make_visualization(id: u64, name: &str) -> Visualization {
        Visualization {
            id,
            name: name.to_string(),
            viz_type: "CHART".to_string(),
            options: serde_json::json!({}),
            description: None,
        }
    }

    fn make_visualization_metadata(id: Option<u64>, name: &str) -> VisualizationMetadata {
        VisualizationMetadata {
            id,
            name: name.to_string(),
            viz_type: "CHART".to_string(),
            options: serde_json::json!({}),
            description: None,
        }
    }

    #[test]
    fn test_visualizations_differ_false_when_identical() {
        let local = vec![make_visualization_metadata(Some(1), "Chart")];
        let server = vec![make_visualization(1, "Chart")];
        assert!(!visualizations_differ(&local, &server));
    }

    #[test]
    fn test_visualizations_differ_true_when_name_differs() {
        let local = vec![make_visualization_metadata(Some(1), "New name")];
        let server = vec![make_visualization(1, "Chart")];
        assert!(visualizations_differ(&local, &server));
    }

    #[test]
    fn test_visualizations_differ_true_when_local_has_no_id() {
        // No id means "create new" — always counts as a difference, even
        // though a same-named viz happens to already exist server-side.
        let local = vec![make_visualization_metadata(None, "Chart")];
        let server = vec![make_visualization(1, "Chart")];
        assert!(visualizations_differ(&local, &server));
    }

    #[test]
    fn test_visualizations_differ_true_when_counts_differ() {
        let local = vec![make_visualization_metadata(Some(1), "Chart")];
        let server = vec![];
        assert!(visualizations_differ(&local, &server));
    }

    #[test]
    fn test_visualizations_differ_true_when_referenced_id_missing_server_side() {
        let local = vec![make_visualization_metadata(Some(99), "Chart")];
        let server = vec![make_visualization(1, "Chart")];
        assert!(visualizations_differ(&local, &server));
    }

    #[test]
    fn test_tracked_query_differs_true_when_visualization_only_change() {
        let mut metadata = make_query_metadata("Q", 1);
        metadata.visualizations = vec![make_visualization_metadata(Some(1), "New name")];
        let mut server = make_server_query("SELECT 1", "Q", 1);
        server.visualizations = vec![make_visualization(1, "Chart")];
        assert!(tracked_query_differs("SELECT 1", &metadata, &server));
    }

    const MINIMAL_QUERY_METADATA_YAML: &str =
        "data_source_id: 1\noptions:\n  parameters: []\nvisualizations: []\n";

    #[test]
    fn test_get_all_query_metadata_from_path_basic() {
        let temp_dir = tempfile::TempDir::new().unwrap();
        let dir = temp_dir.path();

        fs::write(
            dir.join("42-my-query.yaml"),
            format!("id: 42\nname: my-query\n{MINIMAL_QUERY_METADATA_YAML}"),
        )
        .unwrap();

        let metadata = get_all_query_metadata_from_path(dir).unwrap();
        assert_eq!(metadata, vec![(42, "my-query".to_string())]);
    }

    #[test]
    fn test_get_all_query_metadata_from_path_missing_directory_errors() {
        let temp_dir = tempfile::TempDir::new().unwrap();
        let missing = temp_dir.path().join("does-not-exist");

        let result = get_all_query_metadata_from_path(&missing);
        assert!(result.is_err());
        assert!(
            result
                .unwrap_err()
                .to_string()
                .contains("queries directory not found")
        );
    }

    #[test]
    fn test_get_all_query_metadata_from_path_allows_several_new_queries() {
        let temp_dir = tempfile::TempDir::new().unwrap();
        let dir = temp_dir.path();

        fs::write(
            dir.join("0-first-new-query.yaml"),
            format!("id: 0\nname: first-new-query\n{MINIMAL_QUERY_METADATA_YAML}"),
        )
        .unwrap();
        fs::write(
            dir.join("0-second-new-query.yaml"),
            format!("id: 0\nname: second-new-query\n{MINIMAL_QUERY_METADATA_YAML}"),
        )
        .unwrap();

        let mut metadata = get_all_query_metadata_from_path(dir).unwrap();
        metadata.sort();
        assert_eq!(
            metadata,
            vec![
                (0, "first-new-query".to_string()),
                (0, "second-new-query".to_string())
            ]
        );
    }

    #[test]
    fn test_get_all_query_metadata_from_path_rejects_filename_not_matching_name() {
        let temp_dir = tempfile::TempDir::new().unwrap();
        let dir = temp_dir.path();

        fs::write(
            dir.join("duplicate.yaml"),
            format!("id: 0\nname: My Query\n{MINIMAL_QUERY_METADATA_YAML}"),
        )
        .unwrap();

        let result = get_all_query_metadata_from_path(dir);
        assert!(result.is_err());
        let err_msg = result.unwrap_err().to_string();
        assert!(err_msg.contains("duplicate.yaml"));
        assert!(err_msg.contains("0-my-query.yaml"));
        assert!(err_msg.contains("rename this file and its .sql to 0-my-query.*"));
    }

    #[test]
    fn test_get_all_query_metadata_from_path_accepts_renamed_query_with_matching_filename() {
        let temp_dir = tempfile::TempDir::new().unwrap();
        let dir = temp_dir.path();

        fs::write(
            dir.join("120506-my-query.yaml"),
            format!("id: 120506\nname: My Query\n{MINIMAL_QUERY_METADATA_YAML}"),
        )
        .unwrap();

        let metadata = get_all_query_metadata_from_path(dir).unwrap();
        assert_eq!(metadata, vec![(120_506, "My Query".to_string())]);
    }

    #[test]
    fn test_get_all_query_metadata_from_path_rejects_duplicate_ids() {
        let temp_dir = tempfile::TempDir::new().unwrap();
        let dir = temp_dir.path();

        fs::write(
            dir.join("120506-claude-code-direct-reports-model-mix.yaml"),
            format!(
                "id: 120506\nname: claude-code-direct-reports-model-mix\n{MINIMAL_QUERY_METADATA_YAML}"
            ),
        )
        .unwrap();
        fs::write(
            dir.join("120506-claude-code-user-model-mix.yaml"),
            format!("id: 120506\nname: claude-code-user-model-mix\n{MINIMAL_QUERY_METADATA_YAML}"),
        )
        .unwrap();

        let result = get_all_query_metadata_from_path(dir);
        assert!(result.is_err());
        let err_msg = result.unwrap_err().to_string();
        assert!(err_msg.contains("Multiple local files resolve to the same target"));
        assert!(err_msg.contains("id 120506"));
        assert!(err_msg.contains("claude-code-direct-reports-model-mix.yaml"));
        assert!(err_msg.contains("claude-code-user-model-mix.yaml"));
    }
}

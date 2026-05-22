#![allow(clippy::missing_errors_doc)]

use crate::api::RedashClient;
use crate::models::Query;
use anyhow::{Context, Result, bail};
use std::collections::HashSet;
use std::fs;
use std::path::Path;
use std::process::Command;

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

fn get_changed_query_ids() -> Option<HashSet<u64>> {
    let output = Command::new("git")
        .args(["status", "--porcelain"])
        .output()
        .ok()?;

    if !output.status.success() {
        return None;
    }

    let stdout = String::from_utf8(output.stdout).ok()?;

    let mut changed_ids = HashSet::new();

    for line in stdout.lines() {
        if line.len() < 3 {
            continue;
        }

        let file_path = &line[3..];
        let path = Path::new(file_path);

        if file_path.starts_with("queries/")
            && path.extension().is_some_and(|ext| {
                ext.eq_ignore_ascii_case("sql") || ext.eq_ignore_ascii_case("yaml")
            })
            && let Some(filename) = file_path.strip_prefix("queries/")
            && let Some(id_str) = filename.split('-').next()
            && let Ok(id) = id_str.parse::<u64>()
        {
            changed_ids.insert(id);
        }
    }

    Some(changed_ids)
}

fn get_all_query_metadata() -> Result<Vec<(u64, String)>> {
    let queries_dir = Path::new("queries");

    if !queries_dir.exists() {
        bail!("queries directory not found. Run 'stmo-cli fetch' first.");
    }

    let mut queries = Vec::new();

    for entry in fs::read_dir(queries_dir).context("Failed to read queries directory")? {
        let entry = entry.context("Failed to read directory entry")?;
        let path = entry.path();

        if path.extension().is_some_and(|ext| ext == "yaml") {
            let metadata_content =
                fs::read_to_string(&path).context(format!("Failed to read {}", path.display()))?;

            let metadata: crate::models::QueryMetadata = serde_yaml::from_str(&metadata_content)
                .context(format!("Failed to parse {}", path.display()))?;

            queries.push((metadata.id, metadata.name));
        }
    }

    queries.sort_by_key(|(id, _)| *id);

    Ok(queries)
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
        let Some(changed_ids) = get_changed_query_ids() else {
            println!("No git repository detected.");
            println!("Tip: Use --all to deploy all queries, or specify query IDs.");
            return Ok(());
        };

        if changed_ids.is_empty() {
            println!("No changed queries detected.");
            println!("Tip: Use --all to deploy all queries regardless of git status.");
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

        let metadata: crate::models::QueryMetadata = serde_yaml::from_str(&metadata_content)
            .context(format!("Failed to parse {yaml_path}"))?;

        validate_enum_options(&metadata, &yaml_path)?;

        let result_query = if *id == 0 {
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
            fs::write(format!("{new_base}.sql"), &fetched.sql)
                .context(format!("Failed to write {new_base}.sql"))?;
            let mut new_visualizations: Vec<crate::models::VisualizationMetadata> = fetched
                .visualizations
                .iter()
                .map(crate::models::VisualizationMetadata::from)
                .collect();
            new_visualizations.sort_by_key(|v| v.id);
            let new_metadata = crate::models::QueryMetadata {
                id: fetched.id,
                name: fetched.name.clone(),
                description: fetched.description.clone(),
                data_source_id: fetched.data_source_id,
                user_id: fetched.user.as_ref().map(|u| u.id),
                schedule: fetched.schedule.clone(),
                options: fetched.options.clone(),
                visualizations: new_visualizations,
                tags: fetched.tags.clone(),
            };
            let yaml_content = serde_yaml::to_string(&new_metadata)
                .context("Failed to serialize query metadata")?;
            fs::write(format!("{new_base}.yaml"), yaml_content)
                .context(format!("Failed to write {new_base}.yaml"))?;
            fs::remove_file(&sql_path).context(format!("Failed to delete {sql_path}"))?;
            fs::remove_file(&yaml_path).context(format!("Failed to delete {yaml_path}"))?;
            println!("  ✓ Created new query: {} - {name}", fetched.id);
            println!("    Renamed: 0-{slug}.* → {}-{new_slug}.*", fetched.id);
            fetched
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
            let fetched = client.get_query(*id).await?;
            let mut updated_visualizations: Vec<crate::models::VisualizationMetadata> = fetched
                .visualizations
                .iter()
                .map(crate::models::VisualizationMetadata::from)
                .collect();
            updated_visualizations.sort_by_key(|v| v.id);
            let updated_metadata = crate::models::QueryMetadata {
                id: fetched.id,
                name: fetched.name.clone(),
                description: fetched.description.clone(),
                data_source_id: fetched.data_source_id,
                user_id: fetched.user.as_ref().map(|u| u.id),
                schedule: fetched.schedule.clone(),
                options: fetched.options.clone(),
                visualizations: updated_visualizations,
                tags: fetched.tags.clone(),
            };
            let yaml_content = serde_yaml::to_string(&updated_metadata)
                .context("Failed to serialize query metadata")?;
            fs::write(&yaml_path, yaml_content).context(format!("Failed to write {yaml_path}"))?;
            println!("  ✓ {id} - {name}");
            result
        };

        deploy_visualizations(
            client,
            result_query.id,
            &metadata.visualizations,
            &result_query.visualizations,
        )
        .await?;
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
}

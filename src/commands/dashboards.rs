#![allow(clippy::missing_errors_doc)]

use anyhow::{Context, Result};
use std::collections::HashMap;
use std::fs;
use std::path::{Path, PathBuf};

use crate::api::RedashClient;
use crate::models::{
    CreateDashboard, CreateWidget, Dashboard, DashboardMetadata, Query, WidgetMetadata,
    build_dashboard_level_parameter_mappings,
};

struct ServerWidgetVisualization {
    query_id: Option<u64>,
    name: Option<String>,
    id: Option<u64>,
}

struct DashboardDeploymentTarget {
    id: u64,
    slug: String,
    old_yaml_path: Option<PathBuf>,
    server_widget_visualizations: HashMap<u64, ServerWidgetVisualization>,
}

fn dashboard_metadata(dashboard: &Dashboard) -> DashboardMetadata {
    DashboardMetadata {
        id: dashboard.id,
        name: dashboard.name.clone(),
        slug: dashboard.slug.clone(),
        user_id: dashboard.user_id,
        is_draft: dashboard.is_draft,
        is_archived: dashboard.is_archived,
        filters_enabled: dashboard.filters_enabled,
        tags: dashboard.tags.clone(),
        widgets: dashboard
            .widgets
            .iter()
            .map(|widget| WidgetMetadata {
                id: widget.id,
                width: widget.width,
                visualization_id: widget.visualization_id,
                query_id: widget.visualization.as_ref().map(|viz| viz.query.id),
                visualization_name: widget.visualization.as_ref().map(|viz| viz.name.clone()),
                text: widget.text.clone(),
                options: widget.options.clone(),
            })
            .collect(),
    }
}

fn write_dashboard_metadata(path: &Path, metadata: &DashboardMetadata) -> Result<()> {
    let yaml_content =
        serde_yaml::to_string(metadata).context("Failed to serialize dashboard metadata")?;
    fs::write(path, yaml_content).context(format!("Failed to write {}", path.display()))?;
    Ok(())
}

fn dashboard_yaml_paths_for_slug(
    dashboards_dir: &Path,
    dashboard_slug: &str,
) -> Result<Vec<PathBuf>> {
    Ok(fs::read_dir(dashboards_dir)
        .context("Failed to read dashboards directory")?
        .filter_map(std::result::Result::ok)
        .filter(|entry| {
            entry.path().extension().is_some_and(|ext| ext == "yaml")
                && entry
                    .file_name()
                    .to_str()
                    .and_then(|name| name.strip_suffix(".yaml"))
                    .and_then(|name| name.split_once('-'))
                    .is_some_and(|(_, slug)| slug == dashboard_slug)
        })
        .map(|entry| entry.path())
        .collect())
}

fn extract_dashboard_slugs_from_path(dashboards_dir: &Path) -> Result<Vec<String>> {
    if !dashboards_dir.exists() {
        return Ok(Vec::new());
    }

    let mut dashboard_slugs = Vec::new();

    for entry in fs::read_dir(dashboards_dir).context("Failed to read dashboards directory")? {
        let entry = entry.context("Failed to read directory entry")?;
        let path = entry.path();

        if path.extension().is_some_and(|ext| ext == "yaml")
            && let Some(filename) = path.file_name().and_then(|f| f.to_str())
            && let Some(slug) = filename
                .strip_suffix(".yaml")
                .and_then(|s| s.split_once('-'))
                .map(|(_, slug)| slug)
        {
            dashboard_slugs.push(slug.to_string());
        }
    }

    dashboard_slugs.sort_unstable();
    dashboard_slugs.dedup();

    Ok(dashboard_slugs)
}

pub async fn discover(client: &RedashClient) -> Result<()> {
    println!("Fetching your favorite dashboards from Redash...\n");
    let dashboards = client.fetch_favorite_dashboards().await?;

    if dashboards.is_empty() {
        println!("No dashboards found.");
        return Ok(());
    }

    println!("Found {} dashboards:\n", dashboards.len());

    for dashboard in &dashboards {
        let status_flags = match (dashboard.is_draft, dashboard.is_archived) {
            (true, true) => " [DRAFT, ARCHIVED]",
            (true, false) => " [DRAFT]",
            (false, true) => " [ARCHIVED]",
            (false, false) => "",
        };
        println!("  {} - {}{}", dashboard.slug, dashboard.name, status_flags);
    }

    println!("\nUsage:");
    println!("  stmo-cli dashboards fetch <slug> [<slug>...]");
    println!(
        "  stmo-cli dashboards fetch firefox-desktop-on-steamos bug-2006698---ccov-build-regression"
    );

    Ok(())
}

async fn fetch_dashboard_to_file(
    client: &RedashClient,
    slug: &str,
    dashboards_dir: &Path,
) -> Result<Dashboard> {
    let dashboard = client.get_dashboard(slug).await?;
    let path = dashboards_dir.join(format!("{}-{}.yaml", dashboard.id, dashboard.slug));
    write_dashboard_metadata(&path, &dashboard_metadata(&dashboard))?;
    Ok(dashboard)
}

pub async fn fetch(client: &RedashClient, dashboard_slugs: Vec<String>) -> Result<()> {
    if dashboard_slugs.is_empty() {
        anyhow::bail!(
            "No dashboard slugs specified. Use 'dashboards discover' to see available dashboards.\n\nExample:\n  stmo-cli dashboards fetch firefox-desktop-on-steamos bug-2006698---ccov-build-regression"
        );
    }

    fs::create_dir_all("dashboards").context("Failed to create dashboards directory")?;

    println!("Fetching {} dashboards...\n", dashboard_slugs.len());

    let mut success_count = 0;
    let mut failed_slugs = Vec::new();

    for slug in &dashboard_slugs {
        match fetch_dashboard_to_file(client, slug, Path::new("dashboards")).await {
            Ok(dashboard) => {
                let status = if dashboard.is_archived {
                    " [ARCHIVED]"
                } else {
                    ""
                };
                println!("  ✓ {} - {}{}", dashboard.id, dashboard.name, status);
                success_count += 1;
            }
            Err(e) => {
                eprintln!("  ⚠ Dashboard '{slug}' failed to fetch: {e}");
                failed_slugs.push(slug.clone());
            }
        }
    }

    if failed_slugs.is_empty() {
        println!("\n✓ All dashboards fetched successfully");
        println!(
            "\nTip: Favorite these dashboards in the Redash web UI so they appear in 'dashboards discover'."
        );
        Ok(())
    } else {
        println!("\n✓ {success_count} dashboard(s) fetched successfully");
        anyhow::bail!(
            "{} dashboard(s) failed to fetch: {}",
            failed_slugs.len(),
            failed_slugs.join(", ")
        );
    }
}

pub async fn deploy(client: &RedashClient, dashboard_slugs: Vec<String>, all: bool) -> Result<()> {
    let dashboards_dir = Path::new("dashboards");
    let slugs_to_deploy = dashboard_slugs_to_deploy(dashboard_slugs, all, dashboards_dir)?;
    let (success_count, failed_slugs) =
        deploy_dashboards(client, &slugs_to_deploy, dashboards_dir).await;

    report_deployment_results(success_count, &failed_slugs)
}

fn dashboard_slugs_to_deploy(
    dashboard_slugs: Vec<String>,
    all: bool,
    dashboards_dir: &Path,
) -> Result<Vec<String>> {
    if all {
        let existing_dashboard_slugs = extract_dashboard_slugs_from_path(dashboards_dir)?;
        if existing_dashboard_slugs.is_empty() {
            anyhow::bail!("No dashboards found in dashboards/ directory. Use 'fetch' first.");
        }
        println!(
            "Deploying {} dashboards from local directory...\n",
            existing_dashboard_slugs.len()
        );
        Ok(existing_dashboard_slugs)
    } else if !dashboard_slugs.is_empty() {
        println!(
            "Deploying {} specific dashboards...\n",
            dashboard_slugs.len()
        );
        Ok(dashboard_slugs)
    } else {
        anyhow::bail!(
            "No dashboard slugs specified. Use --all to deploy all tracked dashboards, or provide specific slugs.\n\nExamples:\n  stmo-cli dashboards deploy --all\n  stmo-cli dashboards deploy firefox-desktop-on-steamos bug-2006698---ccov-build-regression"
        );
    }
}

async fn deploy_dashboards(
    client: &RedashClient,
    dashboard_slugs: &[String],
    dashboards_dir: &Path,
) -> (usize, Vec<String>) {
    let mut success_count = 0;
    let mut failed_slugs = Vec::new();

    for slug in dashboard_slugs {
        match deploy_single_dashboard(client, slug, dashboards_dir).await {
            Ok(name) => {
                println!("  ✓ {name}");
                success_count += 1;
            }
            Err(e) => {
                eprintln!("  ⚠ Dashboard '{slug}' failed to deploy: {e}");
                failed_slugs.push(slug.clone());
            }
        }
    }

    (success_count, failed_slugs)
}

fn report_deployment_results(success_count: usize, failed_slugs: &[String]) -> Result<()> {
    if failed_slugs.is_empty() {
        println!("\n✓ All dashboards deployed successfully");
        Ok(())
    } else {
        println!("\n✓ {success_count} dashboard(s) deployed successfully");
        anyhow::bail!(
            "{} dashboard(s) failed to deploy: {}",
            failed_slugs.len(),
            failed_slugs.join(", ")
        );
    }
}

fn save_dashboard_yaml(
    dashboard: &crate::models::Dashboard,
    old_yaml_path: Option<std::path::PathBuf>,
) -> Result<()> {
    let filename = format!("dashboards/{}-{}.yaml", dashboard.id, dashboard.slug);
    write_dashboard_metadata(Path::new(&filename), &dashboard_metadata(dashboard))?;

    if let Some(old_path) = old_yaml_path
        && old_path != std::path::Path::new(&filename)
    {
        fs::remove_file(&old_path).context(format!("Failed to delete {}", old_path.display()))?;
    }

    Ok(())
}

async fn resolve_visualization_id(
    client: &RedashClient,
    widget: &WidgetMetadata,
    query_cache: &mut HashMap<u64, Query>,
    prefer_query_selection: bool,
) -> Result<Option<u64>> {
    if !prefer_query_selection && let Some(viz_id) = widget.visualization_id {
        return Ok(Some(viz_id));
    }

    let (Some(query_id), Some(viz_name)) = (widget.query_id, widget.visualization_name.as_deref())
    else {
        return Ok(widget.visualization_id);
    };

    if let std::collections::hash_map::Entry::Vacant(e) = query_cache.entry(query_id) {
        e.insert(client.get_query(query_id).await?);
    }

    let query = query_cache.get(&query_id).expect("just inserted");
    if let Some(viz) = query.visualizations.iter().find(|v| v.name == viz_name) {
        Ok(Some(viz.id))
    } else {
        let available: Vec<&str> = query
            .visualizations
            .iter()
            .map(|v| v.name.as_str())
            .collect();
        anyhow::bail!(
            "No visualization named '{viz_name}' found on query {query_id}. Available: {available:?}"
        );
    }
}

async fn auto_populate_parameter_mappings(
    client: &RedashClient,
    query_id: u64,
    existing_mappings: Option<&serde_json::Value>,
    query_cache: &mut HashMap<u64, Query>,
) -> Result<Option<serde_json::Value>> {
    let should_build = match existing_mappings {
        None => true,
        Some(serde_json::Value::Object(m)) => m.is_empty(),
        Some(_) => false,
    };
    if !should_build {
        return Ok(None);
    }
    if let std::collections::hash_map::Entry::Vacant(e) = query_cache.entry(query_id) {
        e.insert(client.get_query(query_id).await?);
    }
    Ok(query_cache
        .get(&query_id)
        .filter(|q| !q.options.parameters.is_empty())
        .map(|q| build_dashboard_level_parameter_mappings(&q.options.parameters)))
}

fn find_dashboard_yaml(dashboard_slug: &str, dashboards_dir: &Path) -> Result<PathBuf> {
    let yaml_files = dashboard_yaml_paths_for_slug(dashboards_dir, dashboard_slug)?;

    if yaml_files.is_empty() {
        anyhow::bail!("No YAML file found for dashboard '{dashboard_slug}'");
    }
    if yaml_files.len() > 1 {
        anyhow::bail!("Multiple YAML files found for dashboard '{dashboard_slug}'");
    }
    Ok(yaml_files[0].clone())
}

async fn resolve_widget_options(
    client: &RedashClient,
    widget: &WidgetMetadata,
    query_cache: &mut HashMap<u64, Query>,
) -> Result<(crate::models::WidgetOptions, bool)> {
    let mut options = widget.options.clone();
    let has_params = if let Some(query_id) = widget.query_id
        && let Some(mappings) = auto_populate_parameter_mappings(
            client,
            query_id,
            options.parameter_mappings.as_ref(),
            query_cache,
        )
        .await?
    {
        options.parameter_mappings = Some(mappings);
        true
    } else {
        false
    };
    Ok((options, has_params))
}

async fn get_existing_widget_visualizations(
    client: &RedashClient,
    dashboard_slug: &str,
    local_widgets: &[WidgetMetadata],
) -> Result<(u64, HashMap<u64, ServerWidgetVisualization>)> {
    let server_dashboard = client.get_dashboard(dashboard_slug).await?;
    let server_widget_visualizations: HashMap<u64, ServerWidgetVisualization> = server_dashboard
        .widgets
        .iter()
        .map(|w| {
            let visualization = w.visualization.as_ref();
            (
                w.id,
                ServerWidgetVisualization {
                    query_id: visualization.map(|v| v.query.id),
                    name: visualization.map(|v| v.name.clone()),
                    id: visualization.map(|v| v.id).or(w.visualization_id),
                },
            )
        })
        .collect();

    let local_widget_ids: std::collections::HashSet<u64> = local_widgets
        .iter()
        .filter(|w| w.id != 0)
        .map(|w| w.id)
        .collect();

    for widget_id in server_widget_visualizations.keys() {
        if !local_widget_ids.contains(widget_id) {
            client.delete_widget(*widget_id).await?;
        }
    }

    Ok((server_dashboard.id, server_widget_visualizations))
}

fn server_widget_selection_changed(
    widget: &WidgetMetadata,
    server_visualization: &ServerWidgetVisualization,
) -> bool {
    if widget.query_id.is_none() && widget.visualization_name.is_none() {
        server_visualization.id != widget.visualization_id
    } else {
        widget
            .query_id
            .is_some_and(|query_id| server_visualization.query_id != Some(query_id))
            || widget
                .visualization_name
                .as_deref()
                .is_some_and(|name| server_visualization.name.as_deref() != Some(name))
    }
}

async fn deploy_dashboard_widgets(
    client: &RedashClient,
    dashboard_id: u64,
    widgets: &[WidgetMetadata],
    server_widget_visualizations: &HashMap<u64, ServerWidgetVisualization>,
) -> Result<bool> {
    let mut query_cache: HashMap<u64, Query> = HashMap::new();
    let mut any_widget_has_params = false;

    for widget in widgets {
        any_widget_has_params |= deploy_dashboard_widget(
            client,
            dashboard_id,
            widget,
            server_widget_visualizations,
            &mut query_cache,
        )
        .await?;
    }

    Ok(any_widget_has_params)
}

async fn deploy_dashboard_widget(
    client: &RedashClient,
    dashboard_id: u64,
    widget: &WidgetMetadata,
    server_widget_visualizations: &HashMap<u64, ServerWidgetVisualization>,
    query_cache: &mut HashMap<u64, Query>,
) -> Result<bool> {
    let server_visualization = server_widget_visualizations.get(&widget.id);
    let server_selection_changed = server_visualization
        .is_some_and(|server_viz| server_widget_selection_changed(widget, server_viz));
    let (options, has_params) = resolve_widget_options(client, widget, query_cache).await?;
    let visualization_id =
        resolve_visualization_id(client, widget, query_cache, server_selection_changed).await?;
    let payload = CreateWidget {
        dashboard_id,
        visualization_id,
        text: widget.text.clone(),
        options,
        width: if widget.id == 0 { 1 } else { widget.width },
    };

    if widget.id == 0 {
        client.create_widget(&payload).await?;
    } else if server_selection_changed
        || server_visualization.is_some_and(|server_viz| server_viz.id != visualization_id)
    {
        replace_dashboard_widget(client, widget.id, &payload).await?;
    } else {
        client.update_widget(widget.id, &payload).await?;
    }

    Ok(has_params)
}

async fn replace_dashboard_widget(
    client: &RedashClient,
    old_widget_id: u64,
    payload: &CreateWidget,
) -> Result<()> {
    // Redash's widget update endpoint only changes text and options.
    let replacement = client.create_widget(payload).await?;
    if let Err(error) = client.delete_widget(old_widget_id).await {
        if let Err(cleanup_error) = client.delete_widget(replacement.id).await {
            anyhow::bail!(
                "Failed to delete old widget {old_widget_id} after creating replacement {}: {error}; also failed to remove replacement: {cleanup_error}",
                replacement.id
            );
        }
        return Err(error).context(format!(
            "Failed to delete old widget {old_widget_id} after creating its replacement"
        ));
    }
    Ok(())
}

async fn prepare_dashboard_deployment(
    client: &RedashClient,
    dashboard_slug: &str,
    yaml_path: &Path,
    metadata: &DashboardMetadata,
) -> Result<DashboardDeploymentTarget> {
    if metadata.id == 0 {
        let created = client
            .create_dashboard(&CreateDashboard {
                name: metadata.name.clone(),
            })
            .await?;
        println!(
            "  ✓ Created new dashboard: {} - {}",
            created.id, created.name
        );
        client.favorite_dashboard(&created.slug).await?;
        Ok(DashboardDeploymentTarget {
            id: created.id,
            slug: created.slug,
            old_yaml_path: Some(yaml_path.to_path_buf()),
            server_widget_visualizations: HashMap::new(),
        })
    } else {
        let (dashboard_id, server_widget_visualizations) =
            get_existing_widget_visualizations(client, dashboard_slug, &metadata.widgets).await?;
        Ok(DashboardDeploymentTarget {
            id: dashboard_id,
            slug: dashboard_slug.to_string(),
            old_yaml_path: None,
            server_widget_visualizations,
        })
    }
}

async fn update_dashboard_settings(
    client: &RedashClient,
    dashboard_id: u64,
    metadata: &DashboardMetadata,
    any_widget_has_params: bool,
) -> Result<()> {
    let dashboard = Dashboard {
        id: dashboard_id,
        name: metadata.name.clone(),
        slug: metadata.slug.clone(),
        user_id: metadata.user_id,
        is_archived: metadata.is_archived,
        is_draft: metadata.is_draft,
        filters_enabled: any_widget_has_params || metadata.filters_enabled,
        tags: metadata.tags.clone(),
        widgets: vec![],
    };
    client.update_dashboard(&dashboard).await?;
    Ok(())
}

async fn deploy_single_dashboard(
    client: &RedashClient,
    dashboard_slug: &str,
    dashboards_dir: &Path,
) -> Result<String> {
    let yaml_path = find_dashboard_yaml(dashboard_slug, dashboards_dir)?;
    let yaml_content = fs::read_to_string(&yaml_path)
        .context(format!("Failed to read {}", yaml_path.display()))?;

    let local_metadata: DashboardMetadata =
        serde_yaml::from_str(&yaml_content).context("Failed to parse dashboard YAML")?;

    let target =
        prepare_dashboard_deployment(client, dashboard_slug, &yaml_path, &local_metadata).await?;

    let any_widget_has_params = deploy_dashboard_widgets(
        client,
        target.id,
        &local_metadata.widgets,
        &target.server_widget_visualizations,
    )
    .await?;

    update_dashboard_settings(client, target.id, &local_metadata, any_widget_has_params).await?;

    let refreshed = client.get_dashboard(&target.slug).await?;
    save_dashboard_yaml(&refreshed, target.old_yaml_path)?;

    Ok(refreshed.name)
}

pub async fn archive(client: &RedashClient, dashboard_slugs: Vec<String>) -> Result<()> {
    if dashboard_slugs.is_empty() {
        anyhow::bail!(
            "No dashboard slugs specified.\n\nExample:\n  stmo-cli dashboards archive firefox-desktop-on-steamos bug-2006698---ccov-build-regression"
        );
    }

    println!("Archiving {} dashboards...\n", dashboard_slugs.len());

    let mut success_count = 0;
    let mut failed_slugs = Vec::new();

    for slug in &dashboard_slugs {
        match client.get_dashboard(slug).await {
            Ok(dashboard) => match client.archive_dashboard(dashboard.id).await {
                Ok(()) => {
                    remove_local_dashboard_files(slug, Path::new("dashboards"))?;
                    println!("  ✓ {} archived and local file deleted", dashboard.name);
                    success_count += 1;
                }
                Err(e) => {
                    eprintln!("  ⚠ Dashboard '{slug}' failed to archive: {e}");
                    failed_slugs.push(slug.clone());
                }
            },
            Err(e) => {
                eprintln!("  ⚠ Dashboard '{slug}' failed to fetch for archival: {e}");
                failed_slugs.push(slug.clone());
            }
        }
    }

    if failed_slugs.is_empty() {
        println!("\n✓ All dashboards archived successfully");
        Ok(())
    } else {
        println!("\n✓ {success_count} dashboard(s) archived successfully");
        anyhow::bail!(
            "{} dashboard(s) failed to archive: {}",
            failed_slugs.len(),
            failed_slugs.join(", ")
        );
    }
}

fn remove_local_dashboard_files(slug: &str, dashboards_dir: &Path) -> Result<()> {
    for path in dashboard_yaml_paths_for_slug(dashboards_dir, slug)? {
        fs::remove_file(&path).context(format!("Failed to delete {}", path.display()))?;
    }
    Ok(())
}

pub async fn unarchive(client: &RedashClient, dashboard_slugs: Vec<String>) -> Result<()> {
    if dashboard_slugs.is_empty() {
        anyhow::bail!(
            "No dashboard slugs specified.\n\nExample:\n  stmo-cli dashboards unarchive firefox-desktop-on-steamos bug-2006698---ccov-build-regression"
        );
    }

    println!("Unarchiving {} dashboards...\n", dashboard_slugs.len());

    let mut success_count = 0;
    let mut failed_slugs = Vec::new();

    for slug in &dashboard_slugs {
        match client.get_dashboard(slug).await {
            Ok(dashboard) => match client.unarchive_dashboard(dashboard.id).await {
                Ok(unarchived) => {
                    println!("  ✓ {} unarchived", unarchived.name);
                    success_count += 1;
                }
                Err(e) => {
                    eprintln!("  ⚠ Dashboard '{slug}' failed to unarchive: {e}");
                    failed_slugs.push(slug.clone());
                }
            },
            Err(e) => {
                eprintln!("  ⚠ Dashboard '{slug}' failed to fetch for unarchival: {e}");
                failed_slugs.push(slug.clone());
            }
        }
    }

    if failed_slugs.is_empty() {
        println!("\n✓ All dashboards unarchived successfully");
        println!("\nUse 'dashboards fetch' to download the YAML files:");
        println!("  stmo-cli dashboards fetch {}", dashboard_slugs.join(" "));
        Ok(())
    } else {
        println!("\n✓ {success_count} dashboard(s) unarchived successfully");
        anyhow::bail!(
            "{} dashboard(s) failed to unarchive: {}",
            failed_slugs.len(),
            failed_slugs.join(", ")
        );
    }
}

#[cfg(test)]
#[allow(clippy::missing_errors_doc)]
mod tests {
    use super::*;
    use crate::models::{CreateWidget, WidgetOptions, WidgetPosition};
    use tempfile::TempDir;
    use wiremock::matchers::{method, path};
    use wiremock::{Mock, MockServer, ResponseTemplate};

    const SAMPLE_DASHBOARD_ID: u64 = 9_000_000_001;
    const SECOND_SAMPLE_DASHBOARD_ID: u64 = 9_000_000_002;
    const THIRD_SAMPLE_DASHBOARD_ID: u64 = 9_000_000_003;
    const FOURTH_SAMPLE_DASHBOARD_ID: u64 = 9_000_000_004;
    const SAMPLE_USER_ID: u64 = 9_600_000_001;
    const SAMPLE_WIDGET_ID: u64 = 9_100_000_001;
    const CREATED_WIDGET_ID: u64 = 9_100_000_002;
    const SAMPLE_VISUALIZATION_ID: u64 = 9_300_000_001;
    const SECOND_SAMPLE_VISUALIZATION_ID: u64 = 9_300_000_002;
    const SAMPLE_QUERY_ID: u64 = 9_200_000_001;

    fn test_dashboard_metadata(id: u64, slug: &str) -> DashboardMetadata {
        DashboardMetadata {
            id,
            name: "Test Dashboard".to_string(),
            slug: slug.to_string(),
            user_id: SAMPLE_USER_ID,
            is_draft: false,
            is_archived: false,
            filters_enabled: false,
            tags: vec!["test".to_string()],
            widgets: vec![],
        }
    }

    fn test_widget_metadata(id: u64, visualization_id: Option<u64>) -> WidgetMetadata {
        WidgetMetadata {
            id,
            width: 1,
            visualization_id,
            query_id: None,
            visualization_name: None,
            text: String::new(),
            options: WidgetOptions {
                position: WidgetPosition {
                    col: 0,
                    row: 0,
                    size_x: 3,
                    size_y: 2,
                },
                parameter_mappings: None,
            },
        }
    }

    fn test_dashboard_json(
        id: u64,
        name: &str,
        slug: &str,
        widgets: &serde_json::Value,
    ) -> serde_json::Value {
        serde_json::json!({
            "id": id,
            "name": name,
            "slug": slug,
            "user_id": SAMPLE_USER_ID,
            "is_archived": false,
            "is_draft": false,
            "dashboard_filters_enabled": false,
            "tags": ["test"],
            "widgets": widgets
        })
    }

    fn test_widget_json(id: u64, dashboard_id: u64) -> serde_json::Value {
        serde_json::json!({
            "id": id,
            "dashboard_id": dashboard_id,
            "width": 1,
            "visualization_id": SAMPLE_VISUALIZATION_ID,
            "visualization": null,
            "text": "",
            "options": {
                "position": {"col": 0, "row": 0, "sizeX": 3, "sizeY": 2}
            }
        })
    }

    fn test_create_widget(dashboard_id: u64) -> CreateWidget {
        CreateWidget {
            dashboard_id,
            visualization_id: Some(SAMPLE_VISUALIZATION_ID),
            text: String::new(),
            width: 1,
            options: test_widget_metadata(0, None).options,
        }
    }

    fn test_client(mock_server: &MockServer) -> RedashClient {
        RedashClient::new(mock_server.uri(), "test-key").unwrap()
    }

    #[test]
    fn dashboard_metadata_copies_server_widget_metadata() {
        let dashboard = serde_json::from_value::<Dashboard>(test_dashboard_json(
            SAMPLE_DASHBOARD_ID,
            "Test Dashboard",
            "test-dashboard",
            &serde_json::json!([{
                "id": SAMPLE_WIDGET_ID,
                "dashboard_id": SAMPLE_DASHBOARD_ID,
                "width": 2,
                "visualization_id": SAMPLE_VISUALIZATION_ID,
                "visualization": {
                    "id": SAMPLE_VISUALIZATION_ID,
                    "name": "Chart",
                    "query": {"id": SAMPLE_QUERY_ID, "name": "Query"}
                },
                "text": "caption",
                "options": {
                    "position": {"col": 1, "row": 2, "sizeX": 3, "sizeY": 4}
                }
            }]),
        ))
        .unwrap();

        let metadata = dashboard_metadata(&dashboard);
        assert_eq!(metadata.id, SAMPLE_DASHBOARD_ID);
        assert_eq!(metadata.widgets.len(), 1);
        assert_eq!(metadata.widgets[0].query_id, Some(SAMPLE_QUERY_ID));
        assert_eq!(
            metadata.widgets[0].visualization_name.as_deref(),
            Some("Chart")
        );
        assert_eq!(metadata.widgets[0].options.position.col, 1);
    }

    #[test]
    fn write_dashboard_metadata_creates_readable_yaml() {
        let temp_dir = TempDir::new().unwrap();
        let path = temp_dir.path().join("dashboard.yaml");
        let metadata = test_dashboard_metadata(SAMPLE_DASHBOARD_ID, "test-dashboard");

        write_dashboard_metadata(&path, &metadata).unwrap();

        let saved: DashboardMetadata =
            serde_yaml::from_str(&fs::read_to_string(path).unwrap()).unwrap();
        assert_eq!(saved.id, SAMPLE_DASHBOARD_ID);
        assert_eq!(saved.slug, "test-dashboard");
        assert_eq!(saved.tags, ["test"]);
    }

    #[test]
    fn dashboard_yaml_paths_for_slug_filters_by_complete_slug() {
        let temp_dir = TempDir::new().unwrap();
        let slug = "sample-dashboard";
        let files = [
            format!("{SAMPLE_DASHBOARD_ID}-{slug}.yaml"),
            format!("{SECOND_SAMPLE_DASHBOARD_ID}-{slug}.yaml"),
            format!("{THIRD_SAMPLE_DASHBOARD_ID}-other-dashboard.yaml"),
            format!("{FOURTH_SAMPLE_DASHBOARD_ID}-{slug}.txt"),
        ];
        for file in &files {
            fs::write(temp_dir.path().join(file), "test").unwrap();
        }

        let paths = dashboard_yaml_paths_for_slug(temp_dir.path(), slug).unwrap();
        let mut filenames: Vec<_> = paths
            .iter()
            .map(|path| path.file_name().unwrap().to_str().unwrap())
            .collect();
        filenames.sort_unstable();
        let expected_filenames = [
            format!("{SAMPLE_DASHBOARD_ID}-{slug}.yaml"),
            format!("{SECOND_SAMPLE_DASHBOARD_ID}-{slug}.yaml"),
        ];
        assert_eq!(
            filenames,
            expected_filenames
                .iter()
                .map(String::as_str)
                .collect::<Vec<_>>()
        );
    }

    #[tokio::test]
    async fn fetch_dashboard_to_file_writes_the_downloaded_dashboard() {
        let mock_server = MockServer::start().await;
        let dashboards_dir = TempDir::new().unwrap();
        Mock::given(method("GET"))
            .and(path("/api/dashboards/test-dashboard"))
            .respond_with(
                ResponseTemplate::new(200).set_body_json(test_dashboard_json(
                    SAMPLE_DASHBOARD_ID,
                    "Test Dashboard",
                    "test-dashboard",
                    &serde_json::json!([]),
                )),
            )
            .mount(&mock_server)
            .await;

        let dashboard = fetch_dashboard_to_file(
            &test_client(&mock_server),
            "test-dashboard",
            dashboards_dir.path(),
        )
        .await
        .unwrap();

        assert_eq!(dashboard.id, SAMPLE_DASHBOARD_ID);
        let saved: DashboardMetadata = serde_yaml::from_str(
            &fs::read_to_string(
                dashboards_dir
                    .path()
                    .join(format!("{SAMPLE_DASHBOARD_ID}-test-dashboard.yaml")),
            )
            .unwrap(),
        )
        .unwrap();
        assert_eq!(saved.name, "Test Dashboard");
    }

    #[test]
    fn dashboard_slugs_to_deploy_selects_explicit_or_tracked_slugs() {
        let temp_dir = TempDir::new().unwrap();
        fs::write(
            temp_dir
                .path()
                .join(format!("{SECOND_SAMPLE_DASHBOARD_ID}-zebra.yaml")),
            "test",
        )
        .unwrap();
        fs::write(
            temp_dir
                .path()
                .join(format!("{SAMPLE_DASHBOARD_ID}-alpha.yaml")),
            "test",
        )
        .unwrap();

        assert_eq!(
            dashboard_slugs_to_deploy(vec!["chosen-dashboard".to_string()], false, temp_dir.path())
                .unwrap(),
            ["chosen-dashboard"]
        );
        assert_eq!(
            dashboard_slugs_to_deploy(vec![], true, temp_dir.path()).unwrap(),
            ["alpha", "zebra"]
        );
        assert!(dashboard_slugs_to_deploy(vec![], false, temp_dir.path()).is_err());

        let empty_dir = TempDir::new().unwrap();
        assert!(dashboard_slugs_to_deploy(vec![], true, empty_dir.path()).is_err());
    }

    #[tokio::test]
    async fn deploy_dashboards_collects_per_dashboard_failures() {
        let mock_server = MockServer::start().await;
        let dashboards_dir = TempDir::new().unwrap();
        fs::write(
            dashboards_dir
                .path()
                .join(format!("{THIRD_SAMPLE_DASHBOARD_ID}-first.yaml")),
            "widgets: [",
        )
        .unwrap();
        fs::write(
            dashboards_dir
                .path()
                .join(format!("{FOURTH_SAMPLE_DASHBOARD_ID}-second.yaml")),
            "widgets: [",
        )
        .unwrap();

        let (success_count, failed_slugs) = deploy_dashboards(
            &test_client(&mock_server),
            &["first".to_string(), "second".to_string()],
            dashboards_dir.path(),
        )
        .await;

        assert_eq!(success_count, 0);
        assert_eq!(failed_slugs, ["first", "second"]);
    }

    #[test]
    fn report_deployment_results_returns_success_or_failure() {
        assert!(report_deployment_results(2, &[]).is_ok());
        let error = report_deployment_results(1, &["failed-dashboard".to_string()])
            .unwrap_err()
            .to_string();
        assert!(error.contains("1 dashboard(s) failed to deploy"));
        assert!(error.contains("failed-dashboard"));
    }

    #[tokio::test]
    async fn deploy_dashboard_widget_creates_new_widget() {
        let mock_server = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path("/api/widgets"))
            .respond_with(
                ResponseTemplate::new(200)
                    .set_body_json(test_widget_json(CREATED_WIDGET_ID, SAMPLE_DASHBOARD_ID)),
            )
            .mount(&mock_server)
            .await;
        let widget = test_widget_metadata(0, Some(SAMPLE_VISUALIZATION_ID));
        let server_visualizations = HashMap::new();
        let mut query_cache = HashMap::new();

        let has_params = deploy_dashboard_widget(
            &test_client(&mock_server),
            SAMPLE_DASHBOARD_ID,
            &widget,
            &server_visualizations,
            &mut query_cache,
        )
        .await
        .unwrap();

        assert!(!has_params);
        let requests = mock_server.received_requests().await.unwrap();
        let body: serde_json::Value = serde_json::from_slice(&requests[0].body).unwrap();
        assert_eq!(body["dashboard_id"], SAMPLE_DASHBOARD_ID);
        assert_eq!(body["visualization_id"], SAMPLE_VISUALIZATION_ID);
    }

    #[tokio::test]
    async fn deploy_dashboard_widget_updates_unchanged_selection() {
        let mock_server = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path(format!("/api/widgets/{SAMPLE_WIDGET_ID}")))
            .respond_with(
                ResponseTemplate::new(200)
                    .set_body_json(test_widget_json(SAMPLE_WIDGET_ID, SAMPLE_DASHBOARD_ID)),
            )
            .mount(&mock_server)
            .await;
        let widget = test_widget_metadata(SAMPLE_WIDGET_ID, Some(SAMPLE_VISUALIZATION_ID));
        let server_visualizations = HashMap::from([(
            SAMPLE_WIDGET_ID,
            ServerWidgetVisualization {
                query_id: None,
                name: None,
                id: Some(SAMPLE_VISUALIZATION_ID),
            },
        )]);

        deploy_dashboard_widget(
            &test_client(&mock_server),
            SAMPLE_DASHBOARD_ID,
            &widget,
            &server_visualizations,
            &mut HashMap::new(),
        )
        .await
        .unwrap();

        let requests = mock_server.received_requests().await.unwrap();
        assert_eq!(requests.len(), 1);
        assert_eq!(
            requests[0].url.path(),
            format!("/api/widgets/{SAMPLE_WIDGET_ID}")
        );
    }

    #[tokio::test]
    async fn deploy_dashboard_widget_replaces_changed_selection() {
        let mock_server = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path("/api/widgets"))
            .respond_with(
                ResponseTemplate::new(200)
                    .set_body_json(test_widget_json(CREATED_WIDGET_ID, SAMPLE_DASHBOARD_ID)),
            )
            .mount(&mock_server)
            .await;
        Mock::given(method("DELETE"))
            .and(path(format!("/api/widgets/{SAMPLE_WIDGET_ID}")))
            .respond_with(ResponseTemplate::new(204))
            .mount(&mock_server)
            .await;
        let widget = test_widget_metadata(SAMPLE_WIDGET_ID, Some(SAMPLE_VISUALIZATION_ID));
        let server_visualizations = HashMap::from([(
            SAMPLE_WIDGET_ID,
            ServerWidgetVisualization {
                query_id: None,
                name: None,
                id: Some(SECOND_SAMPLE_VISUALIZATION_ID),
            },
        )]);

        deploy_dashboard_widget(
            &test_client(&mock_server),
            SAMPLE_DASHBOARD_ID,
            &widget,
            &server_visualizations,
            &mut HashMap::new(),
        )
        .await
        .unwrap();

        let requests = mock_server.received_requests().await.unwrap();
        assert_eq!(requests.len(), 2);
        assert_eq!(requests[0].url.path(), "/api/widgets");
        assert_eq!(
            requests[1].url.path(),
            format!("/api/widgets/{SAMPLE_WIDGET_ID}")
        );
    }

    #[tokio::test]
    async fn replace_dashboard_widget_creates_then_deletes_old_widget() {
        let mock_server = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path("/api/widgets"))
            .respond_with(
                ResponseTemplate::new(200)
                    .set_body_json(test_widget_json(CREATED_WIDGET_ID, SAMPLE_DASHBOARD_ID)),
            )
            .mount(&mock_server)
            .await;
        Mock::given(method("DELETE"))
            .and(path(format!("/api/widgets/{SAMPLE_WIDGET_ID}")))
            .respond_with(ResponseTemplate::new(204))
            .mount(&mock_server)
            .await;

        replace_dashboard_widget(
            &test_client(&mock_server),
            SAMPLE_WIDGET_ID,
            &test_create_widget(SAMPLE_DASHBOARD_ID),
        )
        .await
        .unwrap();

        let requests = mock_server.received_requests().await.unwrap();
        assert_eq!(requests.len(), 2);
        assert_eq!(requests[0].url.path(), "/api/widgets");
        assert_eq!(
            requests[1].url.path(),
            format!("/api/widgets/{SAMPLE_WIDGET_ID}")
        );
    }

    #[tokio::test]
    async fn replace_dashboard_widget_cleans_up_replacement_after_delete_failure() {
        let mock_server = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path("/api/widgets"))
            .respond_with(
                ResponseTemplate::new(200)
                    .set_body_json(test_widget_json(CREATED_WIDGET_ID, SAMPLE_DASHBOARD_ID)),
            )
            .mount(&mock_server)
            .await;
        Mock::given(method("DELETE"))
            .and(path(format!("/api/widgets/{SAMPLE_WIDGET_ID}")))
            .respond_with(ResponseTemplate::new(500))
            .mount(&mock_server)
            .await;
        Mock::given(method("DELETE"))
            .and(path(format!("/api/widgets/{CREATED_WIDGET_ID}")))
            .respond_with(ResponseTemplate::new(204))
            .mount(&mock_server)
            .await;

        let error = replace_dashboard_widget(
            &test_client(&mock_server),
            SAMPLE_WIDGET_ID,
            &test_create_widget(SAMPLE_DASHBOARD_ID),
        )
        .await
        .unwrap_err();

        assert!(
            error
                .to_string()
                .contains(&format!("Failed to delete old widget {SAMPLE_WIDGET_ID}"))
        );
        let requests = mock_server.received_requests().await.unwrap();
        assert!(
            requests
                .iter()
                .any(|request| request.url.path() == format!("/api/widgets/{CREATED_WIDGET_ID}"))
        );
    }

    #[tokio::test]
    async fn prepare_dashboard_deployment_creates_and_favorites_new_dashboard() {
        let mock_server = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path("/api/dashboards"))
            .respond_with(
                ResponseTemplate::new(200).set_body_json(test_dashboard_json(
                    SAMPLE_DASHBOARD_ID,
                    "New Dashboard",
                    "new-dashboard",
                    &serde_json::json!([]),
                )),
            )
            .mount(&mock_server)
            .await;
        Mock::given(method("POST"))
            .and(path("/api/dashboards/new-dashboard/favorite"))
            .respond_with(ResponseTemplate::new(200))
            .mount(&mock_server)
            .await;
        let temp_dir = TempDir::new().unwrap();
        let yaml_path = temp_dir.path().join("0-new-dashboard.yaml");

        let target = prepare_dashboard_deployment(
            &test_client(&mock_server),
            "new-dashboard",
            &yaml_path,
            &test_dashboard_metadata(0, "new-dashboard"),
        )
        .await
        .unwrap();

        assert_eq!(target.id, SAMPLE_DASHBOARD_ID);
        assert_eq!(target.slug, "new-dashboard");
        assert_eq!(target.old_yaml_path, Some(yaml_path));
        assert!(target.server_widget_visualizations.is_empty());
        let requests = mock_server.received_requests().await.unwrap();
        assert!(
            requests
                .iter()
                .any(|request| { request.url.path() == "/api/dashboards/new-dashboard/favorite" })
        );
    }

    #[tokio::test]
    async fn prepare_dashboard_deployment_loads_existing_dashboard_state() {
        let mock_server = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path("/api/dashboards/existing-dashboard"))
            .respond_with(
                ResponseTemplate::new(200).set_body_json(test_dashboard_json(
                    SAMPLE_DASHBOARD_ID,
                    "Existing Dashboard",
                    "existing-dashboard",
                    &serde_json::json!([]),
                )),
            )
            .mount(&mock_server)
            .await;
        let temp_dir = TempDir::new().unwrap();
        let yaml_path = temp_dir
            .path()
            .join(format!("{SAMPLE_DASHBOARD_ID}-existing-dashboard.yaml"));

        let target = prepare_dashboard_deployment(
            &test_client(&mock_server),
            "existing-dashboard",
            &yaml_path,
            &test_dashboard_metadata(SAMPLE_DASHBOARD_ID, "existing-dashboard"),
        )
        .await
        .unwrap();

        assert_eq!(target.id, SAMPLE_DASHBOARD_ID);
        assert_eq!(target.slug, "existing-dashboard");
        assert!(target.old_yaml_path.is_none());
        assert!(target.server_widget_visualizations.is_empty());
    }

    #[tokio::test]
    async fn update_dashboard_settings_sends_local_metadata_and_parameter_state() {
        let mock_server = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path(format!("/api/dashboards/{SAMPLE_DASHBOARD_ID}")))
            .respond_with(
                ResponseTemplate::new(200).set_body_json(test_dashboard_json(
                    SAMPLE_DASHBOARD_ID,
                    "Test Dashboard",
                    "test-dashboard",
                    &serde_json::json!([]),
                )),
            )
            .mount(&mock_server)
            .await;

        update_dashboard_settings(
            &test_client(&mock_server),
            SAMPLE_DASHBOARD_ID,
            &test_dashboard_metadata(SAMPLE_DASHBOARD_ID, "test-dashboard"),
            true,
        )
        .await
        .unwrap();

        let requests = mock_server.received_requests().await.unwrap();
        let body: serde_json::Value = serde_json::from_slice(&requests[0].body).unwrap();
        assert_eq!(body["id"], SAMPLE_DASHBOARD_ID);
        assert_eq!(body["dashboard_filters_enabled"], true);
        assert_eq!(body["tags"], serde_json::json!(["test"]));
    }

    #[test]
    fn remove_local_dashboard_files_only_removes_matching_slug() {
        let temp_dir = TempDir::new().unwrap();
        let matching_files = [
            format!("{SAMPLE_DASHBOARD_ID}-target-dashboard.yaml"),
            format!("{SECOND_SAMPLE_DASHBOARD_ID}-target-dashboard.yaml"),
        ];
        let other_file = format!("{THIRD_SAMPLE_DASHBOARD_ID}-other.yaml");
        for file in matching_files.iter().chain(std::iter::once(&other_file)) {
            fs::write(temp_dir.path().join(file), "test").unwrap();
        }

        remove_local_dashboard_files("target-dashboard", temp_dir.path()).unwrap();

        for file in matching_files {
            assert!(!temp_dir.path().join(file).exists());
        }
        assert!(temp_dir.path().join(other_file).exists());
    }

    #[test]
    fn test_extract_dashboard_slugs_from_directory_empty() {
        let temp_dir = TempDir::new().unwrap();
        let result = extract_dashboard_slugs_from_path(temp_dir.path());
        assert!(result.is_ok());
        let slugs = result.unwrap();
        assert_eq!(slugs, [] as [String; 0]);
    }

    #[test]
    fn test_extract_dashboard_slugs_with_triple_dash() {
        let temp_dir = TempDir::new().unwrap();
        let temp_path = temp_dir.path();

        fs::write(
            temp_path.join(format!(
                "{SAMPLE_DASHBOARD_ID}-sample-dashboard---build-regression.yaml"
            )),
            "test",
        )
        .unwrap();
        fs::write(
            temp_path.join(format!(
                "{SECOND_SAMPLE_DASHBOARD_ID}-browser-dashboard.yaml"
            )),
            "test",
        )
        .unwrap();

        let result = extract_dashboard_slugs_from_path(temp_path);
        assert!(result.is_ok());

        let slugs = result.unwrap();

        assert!(slugs.contains(&"sample-dashboard---build-regression".to_string()));
        assert!(slugs.contains(&"browser-dashboard".to_string()));
    }

    #[test]
    fn test_extract_dashboard_slugs_deduplication() {
        let temp_dir = TempDir::new().unwrap();
        let temp_path = temp_dir.path();

        fs::write(
            temp_path.join(format!(
                "{SAMPLE_DASHBOARD_ID}-sample-dashboard---build-regression.yaml"
            )),
            "test",
        )
        .unwrap();
        fs::write(
            temp_path.join(format!(
                "{SECOND_SAMPLE_DASHBOARD_ID}-sample-dashboard---build-regression.yaml"
            )),
            "test",
        )
        .unwrap();

        let result = extract_dashboard_slugs_from_path(temp_path);
        assert!(result.is_ok());

        let slugs = result.unwrap();

        assert_eq!(slugs.len(), 1);
        assert_eq!(slugs[0], "sample-dashboard---build-regression");
    }

    #[test]
    fn test_extract_dashboard_slugs_ignores_non_yaml() {
        let temp_dir = TempDir::new().unwrap();
        let temp_path = temp_dir.path();

        fs::write(
            temp_path.join(format!(
                "{SAMPLE_DASHBOARD_ID}-sample-dashboard---build-regression.yaml"
            )),
            "test",
        )
        .unwrap();
        fs::write(
            temp_path.join(format!(
                "{SECOND_SAMPLE_DASHBOARD_ID}-browser-dashboard.txt"
            )),
            "test",
        )
        .unwrap();
        fs::write(temp_path.join("README.md"), "test").unwrap();

        let result = extract_dashboard_slugs_from_path(temp_path);
        assert!(result.is_ok());

        let slugs = result.unwrap();

        assert_eq!(slugs.len(), 1);
        assert_eq!(slugs[0], "sample-dashboard---build-regression");
    }

    #[test]
    fn test_extract_dashboard_slugs_sorted() {
        let temp_dir = TempDir::new().unwrap();
        let temp_path = temp_dir.path();

        fs::write(
            temp_path.join(format!("{THIRD_SAMPLE_DASHBOARD_ID}-zebra-dashboard.yaml")),
            "test",
        )
        .unwrap();
        fs::write(
            temp_path.join(format!(
                "{SAMPLE_DASHBOARD_ID}-sample-dashboard---build-regression.yaml"
            )),
            "test",
        )
        .unwrap();
        fs::write(
            temp_path.join(format!("{SECOND_SAMPLE_DASHBOARD_ID}-alpha-dashboard.yaml")),
            "test",
        )
        .unwrap();

        let result = extract_dashboard_slugs_from_path(temp_path);
        assert!(result.is_ok());

        let slugs = result.unwrap();

        assert_eq!(slugs.len(), 3);
        assert_eq!(slugs[0], "alpha-dashboard");
        assert_eq!(slugs[1], "sample-dashboard---build-regression");
        assert_eq!(slugs[2], "zebra-dashboard");
    }
}

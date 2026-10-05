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
    let slugs_to_deploy = dashboard_slugs_to_deploy(dashboard_slugs, all, Path::new("dashboards"))?;

    let mut success_count = 0;
    let mut failed_slugs = Vec::new();

    for slug in &slugs_to_deploy {
        match deploy_single_dashboard(client, slug, Path::new("dashboards")).await {
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

fn save_dashboard_yaml(
    dashboard: &crate::models::Dashboard,
    old_yaml_path: Option<std::path::PathBuf>,
) -> Result<()> {
    let filename = format!("dashboards/{}-{}.yaml", dashboard.id, dashboard.slug);
    let metadata = dashboard_metadata(dashboard);
    write_dashboard_metadata(Path::new(&filename), &metadata)?;

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
) -> Result<Option<u64>> {
    if let Some(viz_id) = widget.visualization_id {
        return Ok(Some(viz_id));
    }

    let (Some(query_id), Some(viz_name)) = (widget.query_id, widget.visualization_name.as_deref())
    else {
        return Ok(None);
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

    let (server_dashboard_id, slug_for_refetch, old_yaml_path) = if local_metadata.id == 0 {
        let created = client
            .create_dashboard(&CreateDashboard {
                name: local_metadata.name.clone(),
            })
            .await?;
        println!(
            "  ✓ Created new dashboard: {} - {}",
            created.id, created.name
        );
        client.favorite_dashboard(&created.slug).await?;
        (created.id, created.slug.clone(), Some(yaml_path.clone()))
    } else {
        let server_dashboard = client.get_dashboard(dashboard_slug).await?;

        let server_widget_ids: std::collections::HashSet<u64> =
            server_dashboard.widgets.iter().map(|w| w.id).collect();

        let local_widget_ids: std::collections::HashSet<u64> = local_metadata
            .widgets
            .iter()
            .filter(|w| w.id != 0)
            .map(|w| w.id)
            .collect();

        for widget_id in &server_widget_ids {
            if !local_widget_ids.contains(widget_id) {
                client.delete_widget(*widget_id).await?;
            }
        }

        (server_dashboard.id, dashboard_slug.to_string(), None)
    };

    let mut query_cache: HashMap<u64, Query> = HashMap::new();
    let mut any_widget_has_params = false;

    for widget in &local_metadata.widgets {
        let (options, has_params) =
            resolve_widget_options(client, widget, &mut query_cache).await?;
        if has_params {
            any_widget_has_params = true;
        }
        let payload = CreateWidget {
            dashboard_id: server_dashboard_id,
            visualization_id: resolve_visualization_id(client, widget, &mut query_cache).await?,
            text: widget.text.clone(),
            options,
            width: if widget.id == 0 { 1 } else { widget.width },
        };
        if widget.id == 0 {
            client.create_widget(&payload).await?;
        } else {
            client.update_widget(widget.id, &payload).await?;
        }
    }

    let updated_dashboard = Dashboard {
        id: server_dashboard_id,
        name: local_metadata.name.clone(),
        slug: local_metadata.slug.clone(),
        user_id: local_metadata.user_id,
        is_archived: local_metadata.is_archived,
        is_draft: local_metadata.is_draft,
        filters_enabled: any_widget_has_params || local_metadata.filters_enabled,
        tags: local_metadata.tags.clone(),
        widgets: vec![],
    };

    client.update_dashboard(&updated_dashboard).await?;

    let refreshed = client.get_dashboard(&slug_for_refetch).await?;

    save_dashboard_yaml(&refreshed, old_yaml_path)?;

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
                    let yaml_files: Vec<_> = fs::read_dir("dashboards")
                        .context("Failed to read dashboards directory")?
                        .filter_map(std::result::Result::ok)
                        .filter(|entry| {
                            entry.path().extension().is_some_and(|ext| ext == "yaml")
                                && entry
                                    .file_name()
                                    .to_str()
                                    .and_then(|name| name.strip_suffix(".yaml"))
                                    .and_then(|name| name.split_once('-'))
                                    .map(|(_, file_slug)| file_slug)
                                    .is_some_and(|file_slug| file_slug == slug)
                        })
                        .collect();

                    for file in yaml_files {
                        fs::remove_file(file.path())
                            .context(format!("Failed to delete {}", file.path().display()))?;
                    }

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
    use tempfile::TempDir;
    use wiremock::matchers::{method, path};
    use wiremock::{Mock, MockServer, ResponseTemplate};

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
            "user_id": 530,
            "is_archived": false,
            "is_draft": false,
            "dashboard_filters_enabled": false,
            "tags": ["test"],
            "widgets": widgets
        })
    }

    fn test_client(mock_server: &MockServer) -> RedashClient {
        RedashClient::new(mock_server.uri(), "test-key").unwrap()
    }

    const SAMPLE_DASHBOARD_ID: u64 = 9_000_000_001;
    const SECOND_SAMPLE_DASHBOARD_ID: u64 = 9_000_000_002;
    const THIRD_SAMPLE_DASHBOARD_ID: u64 = 9_000_000_003;
    const FOURTH_SAMPLE_DASHBOARD_ID: u64 = 9_000_000_004;
    const SAMPLE_USER_ID: u64 = 9_600_000_001;

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

    #[test]
    fn dashboard_metadata_copies_server_widget_metadata() {
        let dashboard = serde_json::from_value::<Dashboard>(serde_json::json!({
            "id": SAMPLE_DASHBOARD_ID,
            "name": "Test Dashboard",
            "slug": "test-dashboard",
            "user_id": SAMPLE_USER_ID,
            "is_archived": false,
            "is_draft": false,
            "dashboard_filters_enabled": false,
            "tags": ["test"],
            "widgets": [{
                "id": 7,
                "dashboard_id": SAMPLE_DASHBOARD_ID,
                "width": 2,
                "visualization_id": 8,
                "visualization": {
                    "id": 8,
                    "name": "Chart",
                    "query": {"id": 9, "name": "Query"}
                },
                "text": "caption",
                "options": {
                    "position": {"col": 1, "row": 2, "sizeX": 3, "sizeY": 4}
                }
            }]
        }))
        .unwrap();

        let metadata = dashboard_metadata(&dashboard);
        assert_eq!(metadata.id, SAMPLE_DASHBOARD_ID);
        assert_eq!(metadata.widgets.len(), 1);
        assert_eq!(metadata.widgets[0].query_id, Some(9));
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
                    42,
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

        assert_eq!(dashboard.id, 42);
        let saved: DashboardMetadata = serde_yaml::from_str(
            &fs::read_to_string(dashboards_dir.path().join("42-test-dashboard.yaml")).unwrap(),
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

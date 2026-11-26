#![allow(clippy::missing_errors_doc)]

use crate::api::RedashClient;
use anyhow::Result;
use std::path::Path;

fn format_resource_line(id: u64, name: &str, url: &str, is_draft: bool) -> String {
    let draft = if is_draft { " [DRAFT]" } else { "" };
    format!("  {id} - {name}{draft}\n  {url}")
}

// discover never touches the filesystem to decide what to print — the caller
// passes in what it already knows, so this stays trivially unit-testable.
fn next_steps(queries_dir_exists: bool, first_query_id: Option<u64>) -> String {
    if let Some(id) = first_query_id {
        if queries_dir_exists {
            format!("\nNext: stmo-cli fetch {id}")
        } else {
            format!(
                "\nNo queries/ directory here.\nRun 'stmo-cli init <dir>' to set one up, \
                 then 'stmo-cli fetch {id}'."
            )
        }
    } else {
        "\nNo queries of your own on Redash yet.\nTry 'stmo-cli discover --search <term>' \
         to find other people's."
            .to_string()
    }
}

pub async fn discover(client: &RedashClient, search: Option<&str>, limit: usize) -> Result<()> {
    if let Some(term) = search {
        discover_by_search(client, term, limit).await
    } else {
        discover_own_queries(client).await
    }
}

async fn discover_own_queries(client: &RedashClient) -> Result<()> {
    println!("Fetching your queries from Redash...\n");

    let queries = client.fetch_all_queries().await?;

    println!("=== QUERIES ({}) ===\n", queries.len());
    for query in &queries {
        let archived = if query.is_archived { " [ARCHIVED]" } else { "" };
        let draft = if query.is_draft { " [DRAFT]" } else { "" };
        println!("  {} - {}{}{}", query.id, query.name, archived, draft);
    }

    println!(
        "{}",
        next_steps(Path::new("queries").exists(), queries.first().map(|q| q.id))
    );

    Ok(())
}

async fn discover_by_search(client: &RedashClient, term: &str, limit: usize) -> Result<()> {
    println!("Searching for '{term}'...\n");

    let (queries, dashboards) = tokio::join!(
        client.search_queries(term, limit),
        client.search_dashboards(term, limit),
    );
    let queries = queries?;
    let dashboards = dashboards?;

    println!("=== QUERIES ({}) ===\n", queries.len());
    if queries.is_empty() {
        println!("  No queries found for '{term}'.");
    } else {
        for query in &queries {
            let url = client.query_url(query.id);
            println!(
                "{}",
                format_resource_line(query.id, &query.name, &url, query.is_draft)
            );
        }
        if queries.len() == limit {
            println!("\n  (showing first {limit}; raise --limit for more)");
        }
    }

    println!("\n=== DASHBOARDS ({}) ===\n", dashboards.len());
    if dashboards.is_empty() {
        println!("  No dashboards found for '{term}'.");
    } else {
        for dashboard in &dashboards {
            let url = client.dashboard_url(&dashboard.slug);
            println!(
                "{}",
                format_resource_line(dashboard.id, &dashboard.name, &url, dashboard.is_draft)
            );
        }
        if dashboards.len() == limit {
            println!("\n  (showing first {limit}; raise --limit for more)");
        }
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn format_resource_line_plain() {
        let line = format_resource_line(
            42,
            "My Dashboard",
            "https://example.com/dashboard/my",
            false,
        );
        assert!(line.contains("42"));
        assert!(line.contains("My Dashboard"));
        assert!(line.contains("https://example.com/dashboard/my"));
        assert!(!line.contains("[DRAFT]"));
    }

    #[test]
    fn format_resource_line_draft() {
        let line = format_resource_line(7, "WIP", "https://example.com/dashboard/wip", true);
        assert!(line.contains("[DRAFT]"));
        assert!(line.contains("WIP"));
    }

    #[test]
    fn next_steps_dir_exists_suggests_fetch() {
        let message = next_steps(true, Some(123));
        assert!(message.contains("fetch 123"));
        assert!(!message.contains("init"));
    }

    #[test]
    fn next_steps_dir_missing_suggests_init() {
        let message = next_steps(false, Some(123));
        assert!(message.contains("queries/"));
        assert!(message.contains("stmo-cli init"));
    }

    #[test]
    fn next_steps_no_queries_suggests_search() {
        let message = next_steps(false, None);
        assert!(message.contains("--search"));
        assert!(!message.contains("stmo-cli init"));
    }
}

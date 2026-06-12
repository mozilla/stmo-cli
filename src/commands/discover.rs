#![allow(clippy::missing_errors_doc)]

use crate::api::RedashClient;
use anyhow::Result;

fn format_resource_line(id: u64, name: &str, url: &str, is_draft: bool) -> String {
    let draft = if is_draft { " [DRAFT]" } else { "" };
    format!("  {id} - {name}{draft}\n  {url}")
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

    println!("\nUse 'stmo-cli init' to create the queries directory.");

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

    let base = client.base_url();

    println!("=== QUERIES ({}) ===\n", queries.len());
    if queries.is_empty() {
        println!("  No queries found for '{term}'.");
    } else {
        for query in &queries {
            let url = format!("{base}/queries/{}", query.id);
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
            let url = format!("{base}/dashboard/{}", dashboard.slug);
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
}

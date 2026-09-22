pub mod archive;
pub mod dashboards;
pub mod datasources;
pub mod deploy;
pub mod discover;
pub mod dynamic_dates;
pub mod execute;
pub mod fetch;
pub mod init;
pub mod schedule;
pub mod snippets;
pub mod update;

use anyhow::{Result, bail};
use std::collections::BTreeMap;

#[derive(Debug, Clone, Copy)]
pub enum OutputFormat {
    Json,
    Table,
}

impl std::str::FromStr for OutputFormat {
    type Err = anyhow::Error;

    fn from_str(s: &str) -> Result<Self> {
        match s.to_lowercase().as_str() {
            "json" => Ok(Self::Json),
            "table" => Ok(Self::Table),
            _ => bail!("Invalid format. Use: json or table"),
        }
    }
}

// Two local files that resolve to the same Redash resource would silently
// overwrite each other on deploy. Callers label each target the way the user
// addresses it: an id for a tracked resource, the local file base for one that
// hasn't been created yet.
pub(crate) fn bail_on_duplicate_targets(
    paths_by_target: &BTreeMap<String, Vec<String>>,
) -> Result<()> {
    let details: Vec<String> = paths_by_target
        .iter()
        .filter(|(_, paths)| paths.len() > 1)
        .map(|(target, paths)| {
            let mut paths = paths.clone();
            paths.sort();
            format!("  {target}: {}", paths.join(", "))
        })
        .collect();

    if details.is_empty() {
        return Ok(());
    }

    bail!(
        "Multiple local files resolve to the same target — resolve the conflict before deploying:\n{}",
        details.join("\n")
    );
}

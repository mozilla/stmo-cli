pub mod archive;
pub mod dashboards;
pub mod datasources;
pub mod deploy;
pub mod discover;
pub mod dynamic_dates;
pub mod execute;
pub mod fetch;
pub mod guide;
pub mod init;
pub mod schedule;
pub mod snippets;
pub mod update;

use anyhow::{Context, Result, bail};
use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};

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

#[derive(Debug, Default)]
pub(crate) struct QueryFileSet {
    pub base: PathBuf,
    pub sql: Option<PathBuf>,
    pub yaml: Option<PathBuf>,
}

pub(crate) fn query_slugify(name: &str) -> String {
    name.to_lowercase()
        .chars()
        .map(|c| if c.is_alphanumeric() { c } else { '-' })
        .collect::<String>()
        .split('-')
        .filter(|part| !part.is_empty())
        .collect::<Vec<_>>()
        .join("-")
}

pub(crate) fn query_id_from_path(path: &Path) -> Option<u64> {
    let stem = path.file_stem()?.to_str()?;
    let id = stem.split_once('-').map_or(stem, |(id, _)| id);
    id.parse().ok()
}

pub(crate) fn query_file_sets_by_id(dir: &Path, id: u64) -> Result<Vec<QueryFileSet>> {
    if !dir.exists() {
        return Ok(Vec::new());
    }

    let mut file_sets: BTreeMap<PathBuf, QueryFileSet> = BTreeMap::new();

    for entry in fs::read_dir(dir).context("Failed to read queries directory")? {
        let entry = entry.context("Failed to read directory entry")?;
        let path = entry.path();
        let Some(extension) = path.extension().and_then(|ext| ext.to_str()) else {
            continue;
        };

        if !matches!(extension, "sql" | "yaml") || query_id_from_path(&path) != Some(id) {
            continue;
        }

        let base = path.with_extension("");
        let file_set = file_sets
            .entry(base.clone())
            .or_insert_with(|| QueryFileSet {
                base,
                ..QueryFileSet::default()
            });

        match extension {
            "sql" => file_set.sql = Some(path),
            "yaml" => file_set.yaml = Some(path),
            _ => unreachable!("extensions were filtered above"),
        }
    }

    Ok(file_sets.into_values().collect())
}

pub(crate) fn unique_query_file_set_by_id(dir: &Path, id: u64) -> Result<Option<QueryFileSet>> {
    let file_sets = query_file_sets_by_id(dir, id)?;
    if file_sets.len() > 1 {
        let paths = file_sets
            .iter()
            .map(|file_set| file_set.base.display().to_string())
            .collect::<Vec<_>>()
            .join(", ");
        bail!("Multiple local query files found for ID {id}: {paths}");
    }

    Ok(file_sets.into_iter().next())
}

pub(crate) fn query_file_base(dir: &Path, id: u64, name: &str) -> Result<PathBuf> {
    if id != 0
        && let Some(file_set) = unique_query_file_set_by_id(dir, id)?
    {
        return Ok(file_set.base);
    }

    Ok(dir.join(format!("{id}-{}", query_slugify(name))))
}

pub(crate) fn ensure_query_filename_matches_identity(
    path: &Path,
    id: u64,
    name: &str,
) -> Result<()> {
    let actual_stem = path
        .file_stem()
        .and_then(std::ffi::OsStr::to_str)
        .unwrap_or_default();
    let legacy_stem = format!("{id}-{}", query_slugify(name));

    if (id != 0 && query_id_from_path(path) == Some(id)) || actual_stem == legacy_stem {
        return Ok(());
    }

    let expected_stem = if id == 0 { legacy_stem } else { id.to_string() };
    let expected = path.with_file_name(format!("{expected_stem}.yaml"));
    bail!(
        "{} is named after neither its id nor its `name:`, so deploy would read {} instead — rename this file and its .sql to {expected_stem}.*",
        path.display(),
        expected.display(),
    );
}

// Used by non-query resources whose filenames still include a slug, such as
// dashboards and snippets.
pub(crate) fn ensure_filename_matches_identity(
    path: &Path,
    expected_stem: &str,
    name_field: &str,
) -> Result<()> {
    let actual_stem = path
        .file_stem()
        .and_then(std::ffi::OsStr::to_str)
        .unwrap_or_default();

    if actual_stem == expected_stem {
        return Ok(());
    }

    let expected = path.with_file_name(format!("{expected_stem}.yaml"));
    bail!(
        "{} is named after neither its id nor its `{name_field}:`, so deploy would read \
         {} instead — rename this file and its .sql to {expected_stem}.*, or change \
         `{name_field}:` to match the current filename",
        path.display(),
        expected.display(),
    );
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    #[test]
    fn identity_check_accepts_legacy_file_whose_slug_is_stale() {
        let path = Path::new("queries/1200000001-old-slug.yaml");

        ensure_query_filename_matches_identity(path, 1_200_000_001, "New Name").unwrap();
    }

    #[test]
    fn identity_check_rejects_file_named_after_neither_id_nor_name() {
        let path = Path::new("queries/1200000002-old-slug.yaml");

        assert!(ensure_query_filename_matches_identity(path, 1_200_000_001, "New Name").is_err());
    }

    #[test]
    fn query_file_base_reuses_stale_legacy_pair() {
        let temp_dir = TempDir::new().unwrap();
        fs::write(temp_dir.path().join("1200000001-old-slug.sql"), "SELECT 1").unwrap();
        fs::write(
            temp_dir.path().join("1200000001-old-slug.yaml"),
            "id: 1200000001",
        )
        .unwrap();

        assert_eq!(
            query_file_base(temp_dir.path(), 1_200_000_001, "New Name").unwrap(),
            temp_dir.path().join("1200000001-old-slug")
        );
    }
}

#![allow(clippy::missing_errors_doc)]

use anyhow::{Context, Result, bail};
use std::fs;
use std::path::{Path, PathBuf};

use crate::models::{QueryMetadata, Schedule};

fn find_yaml_path_in(dir: &Path, query_id: u64) -> Result<Option<PathBuf>> {
    let Some(file_set) = crate::commands::unique_query_file_set_by_id(dir, query_id)? else {
        return Ok(None);
    };
    let Some(yaml_path) = file_set.yaml else {
        return Ok(None);
    };

    let content = fs::read_to_string(&yaml_path)
        .context(format!("Failed to read {}", yaml_path.display()))?;
    let metadata: QueryMetadata = serde_yaml::from_str(&content)
        .context(format!("Failed to parse {}", yaml_path.display()))?;
    if metadata.id != query_id {
        bail!(
            "{} declares query ID {}, expected {query_id}",
            yaml_path.display(),
            metadata.id
        );
    }
    crate::commands::ensure_query_filename_matches_identity(
        &yaml_path,
        metadata.id,
        &metadata.name,
    )?;

    Ok(Some(yaml_path))
}

fn update_yaml_schedule(yaml_path: &Path, schedule: Option<Schedule>) -> Result<String> {
    let content =
        fs::read_to_string(yaml_path).context(format!("Failed to read {}", yaml_path.display()))?;
    let mut metadata: QueryMetadata = serde_yaml::from_str(&content)
        .context(format!("Failed to parse {}", yaml_path.display()))?;

    let name = metadata.name.clone();
    metadata.schedule = schedule;

    let yaml_content =
        serde_yaml::to_string(&metadata).context("Failed to serialize query metadata")?;
    fs::write(yaml_path, yaml_content)
        .context(format!("Failed to write {}", yaml_path.display()))?;

    Ok(name)
}

pub fn schedule(
    query_ids: &[u64],
    interval: Option<u64>,
    time: Option<&str>,
    day_of_week: Option<&str>,
    clear: bool,
) -> Result<()> {
    if !clear && interval.is_none() {
        bail!(
            "Either --interval or --clear must be specified.\n\nExamples:\n  stmo-cli schedule <query-id> [<query-id>...] --interval 86400 --time 07:15\n  stmo-cli schedule <query-id> --clear"
        );
    }

    let new_schedule = if clear {
        None
    } else {
        Some(Schedule {
            interval,
            time: time.map(str::to_owned),
            day_of_week: day_of_week.map(str::to_owned),
            until: None,
        })
    };

    let queries_dir = Path::new("queries");
    let mut errors: Vec<(u64, anyhow::Error)> = Vec::new();
    let mut updated_count = 0;

    for &query_id in query_ids {
        match find_yaml_path_in(queries_dir, query_id) {
            Err(e) => {
                eprintln!("  ✗ Failed to search for query {query_id}: {e}");
                errors.push((query_id, e));
            }
            Ok(None) => {
                let e = anyhow::anyhow!(
                    "No local file found for query {query_id}. Run 'stmo-cli fetch {query_id}' first."
                );
                eprintln!("  ✗ {e}");
                errors.push((query_id, e));
            }
            Ok(Some(yaml_path)) => match update_yaml_schedule(&yaml_path, new_schedule.clone()) {
                Ok(name) => {
                    let action = if clear {
                        "Cleared schedule from"
                    } else {
                        "Set schedule on"
                    };
                    println!("  ✓ {action} query {query_id} - {name}");
                    updated_count += 1;
                }
                Err(e) => {
                    eprintln!("  ✗ Failed to update query {query_id}: {e}");
                    errors.push((query_id, e));
                }
            },
        }
    }

    println!("\n✓ Updated {updated_count}/{} queries", query_ids.len());
    println!("Run 'stmo-cli deploy' to push the schedule changes to Redash.");

    if !errors.is_empty() {
        bail!("Failed to update {} queries", errors.len());
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use tempfile::TempDir;

    const SAMPLE_YAML: &str = "id: 1200000001\nname: Example Query\ndescription: null\ndata_source_id: 1500000001\nschedule: null\noptions:\n  parameters: []\nvisualizations: []\ntags: null\n";

    fn create_test_query(dir: &Path, id: u64, yaml_content: &str) {
        let yaml_path = dir.join(format!("{id}-example-query.yaml"));
        fs::write(yaml_path, yaml_content).unwrap();
        let sql_path = dir.join(format!("{id}-example-query.sql"));
        fs::write(sql_path, "SELECT 1").unwrap();
    }

    #[test]
    fn set_schedule_writes_yaml() {
        let temp_dir = TempDir::new().unwrap();
        let queries_dir = temp_dir.path();
        create_test_query(queries_dir, 1_200_000_001, SAMPLE_YAML);

        let yaml_path = queries_dir.join("1200000001-example-query.yaml");
        update_yaml_schedule(
            &yaml_path,
            Some(Schedule {
                interval: Some(86400),
                time: Some("07:15".to_owned()),
                day_of_week: None,
                until: None,
            }),
        )
        .unwrap();

        let content = fs::read_to_string(&yaml_path).unwrap();
        let metadata: QueryMetadata = serde_yaml::from_str(&content).unwrap();
        let s = metadata.schedule.unwrap();
        assert_eq!(s.interval, Some(86_400));
        assert_eq!(s.time, Some("07:15".to_owned()));
        assert_eq!(s.day_of_week, None);
        assert_eq!(s.until, None);
    }

    #[test]
    fn clear_schedule_writes_null() {
        let temp_dir = TempDir::new().unwrap();
        let queries_dir = temp_dir.path();
        let yaml_with_schedule = "id: 1200000001\nname: Example Query\ndescription: null\ndata_source_id: 1500000001\nschedule:\n  interval: 86400\n  time: '07:15'\n  day_of_week: null\n  until: null\noptions:\n  parameters: []\nvisualizations: []\ntags: null\n";
        create_test_query(queries_dir, 1_200_000_001, yaml_with_schedule);

        let yaml_path = queries_dir.join("1200000001-example-query.yaml");
        update_yaml_schedule(&yaml_path, None).unwrap();

        let content = fs::read_to_string(&yaml_path).unwrap();
        let metadata: QueryMetadata = serde_yaml::from_str(&content).unwrap();
        assert!(metadata.schedule.is_none());
    }

    #[test]
    fn find_yaml_finds_by_id_prefix() {
        let temp_dir = TempDir::new().unwrap();
        let queries_dir = temp_dir.path();
        create_test_query(queries_dir, 1_200_000_001, SAMPLE_YAML);

        let found = find_yaml_path_in(queries_dir, 1_200_000_001).unwrap();
        assert!(found.is_some());

        let not_found = find_yaml_path_in(queries_dir, 99_999).unwrap();
        assert!(not_found.is_none());
    }

    #[test]
    fn find_yaml_finds_id_only_files() {
        let temp_dir = TempDir::new().unwrap();
        let queries_dir = temp_dir.path();
        fs::write(queries_dir.join("1200000001.yaml"), SAMPLE_YAML).unwrap();
        fs::write(queries_dir.join("1200000001.sql"), "SELECT 1").unwrap();

        let found = find_yaml_path_in(queries_dir, 1_200_000_001).unwrap();
        assert_eq!(found, Some(queries_dir.join("1200000001.yaml")));
    }

    #[test]
    fn find_yaml_does_not_require_a_local_sql_file() {
        let temp_dir = TempDir::new().unwrap();
        let queries_dir = temp_dir.path();
        let yaml_path = queries_dir.join("1200000001-example-query.yaml");
        fs::write(&yaml_path, SAMPLE_YAML).unwrap();

        let found = find_yaml_path_in(queries_dir, 1_200_000_001).unwrap();
        assert_eq!(found, Some(yaml_path));
    }

    #[test]
    fn find_yaml_returns_none_for_missing_dir() {
        let path = find_yaml_path_in(Path::new("/nonexistent/path"), 123).unwrap();
        assert!(path.is_none());
    }
}

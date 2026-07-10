#![allow(clippy::missing_errors_doc)]

use anyhow::{Context, Result, bail};
use std::collections::HashSet;
use std::fs;
use std::path::Path;
use std::process::Command;

use crate::api::RedashClient;
use crate::models::{CreateQuerySnippet, QuerySnippet, SnippetMetadata};

fn find_snippet_files_in(snippets_dir: &Path, snippet_id: u64) -> Result<Option<(String, String)>> {
    if !snippets_dir.exists() {
        return Ok(None);
    }

    let mut sql_path = None;
    let mut yaml_path = None;

    for entry in fs::read_dir(snippets_dir).context("Failed to read snippets directory")? {
        let entry = entry.context("Failed to read directory entry")?;
        let path = entry.path();

        if let Some(filename) = path.file_name().and_then(|f| f.to_str())
            && let Some(id_str) = filename.split('-').next()
            && let Ok(id) = id_str.parse::<u64>()
            && id == snippet_id
        {
            if path.extension().is_some_and(|ext| ext == "sql") {
                sql_path = Some(path.to_string_lossy().to_string());
            } else if path.extension().is_some_and(|ext| ext == "yaml") {
                yaml_path = Some(path.to_string_lossy().to_string());
            }
        }
    }

    match (sql_path, yaml_path) {
        (Some(sql), Some(yaml)) => Ok(Some((sql, yaml))),
        _ => Ok(None),
    }
}

fn find_snippet_files(snippet_id: u64) -> Result<Option<(String, String)>> {
    find_snippet_files_in(Path::new("snippets"), snippet_id)
}

fn extract_snippet_ids_from_path(snippets_dir: &Path) -> Result<Vec<u64>> {
    if !snippets_dir.exists() {
        return Ok(Vec::new());
    }

    let mut snippet_ids = Vec::new();

    for entry in fs::read_dir(snippets_dir).context("Failed to read snippets directory")? {
        let entry = entry.context("Failed to read directory entry")?;
        let path = entry.path();

        if path.extension().is_some_and(|ext| ext == "yaml")
            && let Some(filename) = path.file_name().and_then(|f| f.to_str())
            && let Some(id_str) = filename.split('-').next()
            && let Ok(id) = id_str.parse::<u64>()
        {
            snippet_ids.push(id);
        }
    }

    snippet_ids.sort_unstable();
    snippet_ids.dedup();

    Ok(snippet_ids)
}

fn extract_snippet_ids_from_directory() -> Result<Vec<u64>> {
    extract_snippet_ids_from_path(Path::new("snippets"))
}

fn get_all_snippet_metadata_from_path(snippets_dir: &Path) -> Result<Vec<(u64, String)>> {
    if !snippets_dir.exists() {
        bail!("snippets directory not found. Run 'stmo-cli snippets fetch' first.");
    }

    let mut snippets = Vec::new();

    for entry in fs::read_dir(snippets_dir).context("Failed to read snippets directory")? {
        let entry = entry.context("Failed to read directory entry")?;
        let path = entry.path();

        if path.extension().is_some_and(|ext| ext == "yaml") {
            let metadata_content =
                fs::read_to_string(&path).context(format!("Failed to read {}", path.display()))?;

            let metadata: SnippetMetadata = serde_yaml::from_str(&metadata_content)
                .context(format!("Failed to parse {}", path.display()))?;

            snippets.push((metadata.id, metadata.trigger));
        }
    }

    snippets.sort_by_key(|(id, _)| *id);

    Ok(snippets)
}

fn get_all_snippet_metadata() -> Result<Vec<(u64, String)>> {
    get_all_snippet_metadata_from_path(Path::new("snippets"))
}

fn parse_changed_snippet_ids(porcelain: &str) -> HashSet<u64> {
    let mut changed_ids = HashSet::new();

    for line in porcelain.lines() {
        if line.len() < 3 {
            continue;
        }

        let raw_path = &line[3..];
        // Rename entries are formatted as "old/path -> new/path"; the new path is
        // what matters for deciding which id is currently changed.
        let file_path = raw_path
            .rsplit_once(" -> ")
            .map_or(raw_path, |(_old, new_path)| new_path);
        let path = Path::new(file_path);

        if file_path.starts_with("snippets/")
            && path.extension().is_some_and(|ext| {
                ext.eq_ignore_ascii_case("sql") || ext.eq_ignore_ascii_case("yaml")
            })
            && let Some(filename) = file_path.strip_prefix("snippets/")
            && let Some(id_str) = filename.split('-').next()
            && let Ok(id) = id_str.parse::<u64>()
        {
            changed_ids.insert(id);
        }
    }

    changed_ids
}

fn get_changed_snippet_ids() -> Option<HashSet<u64>> {
    let output = Command::new("git")
        .args(["status", "--porcelain"])
        .output()
        .ok()?;

    if !output.status.success() {
        return None;
    }

    let stdout = String::from_utf8(output.stdout).ok()?;

    Some(parse_changed_snippet_ids(&stdout))
}

fn write_snippet_files(snippet: &QuerySnippet) -> Result<()> {
    fs::create_dir_all("snippets").context("Failed to create snippets directory")?;

    let filename_base = format!("{}-{}", snippet.id, snippet.trigger);

    let sql_path = format!("snippets/{filename_base}.sql");
    fs::write(&sql_path, &snippet.snippet).context(format!("Failed to write {sql_path}"))?;

    let metadata = SnippetMetadata {
        id: snippet.id,
        trigger: snippet.trigger.clone(),
        description: snippet.description.clone(),
    };
    let yaml_path = format!("snippets/{filename_base}.yaml");
    let yaml_content =
        serde_yaml::to_string(&metadata).context("Failed to serialize snippet metadata")?;
    fs::write(&yaml_path, yaml_content).context(format!("Failed to write {yaml_path}"))?;

    Ok(())
}

fn delete_snippet_files(sql_path: &str, yaml_path: &str) -> Result<()> {
    fs::remove_file(sql_path).context(format!("Failed to delete {sql_path}"))?;
    fs::remove_file(yaml_path).context(format!("Failed to delete {yaml_path}"))?;
    Ok(())
}

pub async fn list(client: &RedashClient) -> Result<()> {
    let mut snippets = client.list_query_snippets().await?;
    snippets.sort_by_key(|s| s.id);

    println!("=== QUERY SNIPPETS ({}) ===\n", snippets.len());
    for snippet in &snippets {
        let desc = snippet.description.as_deref().unwrap_or("");
        println!("  {} - {}", snippet.id, snippet.trigger);
        if !desc.is_empty() {
            println!("    {desc}");
        }
    }

    Ok(())
}

pub async fn fetch(client: &RedashClient, snippet_ids: Vec<u64>, all: bool) -> Result<()> {
    fs::create_dir_all("snippets").context("Failed to create snippets directory")?;

    let snippets_to_fetch = if all {
        let existing_ids = extract_snippet_ids_from_directory()?;
        if existing_ids.is_empty() {
            bail!(
                "No snippets found in snippets/ directory. Use specific snippet IDs or run 'snippets list' to see available snippets."
            );
        }
        println!(
            "Fetching {} snippets from local directory...\n",
            existing_ids.len()
        );
        let mut fetched = Vec::new();
        for id in &existing_ids {
            match client.get_query_snippet(*id).await {
                Ok(snippet) => fetched.push(snippet),
                Err(e) => eprintln!("  ⚠ Snippet {id} failed to fetch: {e}"),
            }
        }
        fetched
    } else if !snippet_ids.is_empty() {
        println!("Fetching {} specific snippets...\n", snippet_ids.len());
        let mut fetched = Vec::new();
        for id in &snippet_ids {
            match client.get_query_snippet(*id).await {
                Ok(snippet) => fetched.push(snippet),
                Err(e) => eprintln!("  ⚠ Snippet {id} failed to fetch: {e}"),
            }
        }
        fetched
    } else {
        bail!(
            "No snippet IDs specified. Use --all to fetch tracked snippets, or provide specific snippet IDs.\n\nExamples:\n  stmo-cli snippets fetch --all\n  stmo-cli snippets fetch 31\n  stmo-cli snippets list  (to see available snippets)"
        );
    };

    println!("Fetching {} snippets...", snippets_to_fetch.len());

    for snippet in &snippets_to_fetch {
        write_snippet_files(snippet)?;
        println!("  ✓ {} - {}", snippet.id, snippet.trigger);
    }

    println!("\n✓ All snippets fetched successfully");

    Ok(())
}

pub async fn deploy_one(client: &RedashClient, id: u64, trigger: &str) -> Result<QuerySnippet> {
    let sql_path = format!("snippets/{id}-{trigger}.sql");
    let yaml_path = format!("snippets/{id}-{trigger}.yaml");

    if !Path::new(&sql_path).exists() {
        bail!("Snippet SQL file not found: {sql_path}");
    }
    if !Path::new(&yaml_path).exists() {
        bail!("Snippet metadata file not found: {yaml_path}");
    }

    let body = fs::read_to_string(&sql_path).context(format!("Failed to read {sql_path}"))?;

    let metadata_content =
        fs::read_to_string(&yaml_path).context(format!("Failed to read {yaml_path}"))?;

    let metadata: SnippetMetadata =
        serde_yaml::from_str(&metadata_content).context(format!("Failed to parse {yaml_path}"))?;

    let result = if id == 0 {
        let create = CreateQuerySnippet {
            trigger: metadata.trigger.clone(),
            description: metadata.description.clone(),
            snippet: body,
        };
        let created = client.create_query_snippet(&create).await?;
        write_snippet_files(&created)?;
        fs::remove_file(&sql_path).context(format!("Failed to delete {sql_path}"))?;
        fs::remove_file(&yaml_path).context(format!("Failed to delete {yaml_path}"))?;
        println!(
            "  ✓ Created new snippet: {} - {}",
            created.id, created.trigger
        );
        println!(
            "    Renamed: 0-{trigger}.* → {}-{}.*",
            created.id, created.trigger
        );
        created
    } else {
        let snippet = QuerySnippet {
            id,
            trigger: metadata.trigger.clone(),
            description: metadata.description.clone(),
            snippet: body,
            user: None,
            updated_at: String::new(),
            created_at: String::new(),
        };
        let updated = client.update_query_snippet(&snippet).await?;
        write_snippet_files(&updated)?;
        println!("  ✓ {id} - {}", updated.trigger);
        updated
    };

    Ok(result)
}

pub async fn deploy(client: &RedashClient, snippet_ids: Vec<u64>, all: bool) -> Result<()> {
    let all_snippets = get_all_snippet_metadata()?;

    let snippets_to_deploy = if !snippet_ids.is_empty() {
        let ids_set: HashSet<_> = snippet_ids.iter().copied().collect();
        let filtered: Vec<_> = all_snippets
            .into_iter()
            .filter(|(id, _)| ids_set.contains(id))
            .collect();

        if filtered.is_empty() {
            bail!("None of the specified snippet IDs were found in snippets/ directory");
        }

        println!("Deploying {} specific snippets...", filtered.len());
        for (id, trigger) in &filtered {
            println!("  → {id} - {trigger}");
        }
        println!();

        filtered
    } else if all {
        println!("Deploying all {} snippets...\n", all_snippets.len());
        all_snippets
    } else {
        let Some(changed_ids) = get_changed_snippet_ids() else {
            println!("No git repository detected.");
            println!("Tip: Use --all to deploy all snippets, or specify snippet IDs.");
            return Ok(());
        };

        if changed_ids.is_empty() {
            println!("No changed snippets detected.");
            println!("Tip: Use --all to deploy all snippets regardless of git status.");
            return Ok(());
        }

        let filtered: Vec<_> = all_snippets
            .into_iter()
            .filter(|(id, _)| changed_ids.contains(id))
            .collect();

        println!("Deploying {} changed snippets...", filtered.len());
        for (id, trigger) in &filtered {
            println!("  → {id} - {trigger}");
        }
        println!();

        filtered
    };

    for (id, trigger) in &snippets_to_deploy {
        deploy_one(client, *id, trigger).await?;
    }

    println!("\n✓ All snippets deployed successfully");

    Ok(())
}

pub async fn delete(client: &RedashClient, snippet_ids: Vec<u64>) -> Result<()> {
    let mut errors = Vec::new();
    let mut deleted_count = 0;

    println!("Deleting {} query snippets...\n", snippet_ids.len());

    for snippet_id in &snippet_ids {
        match client.delete_query_snippet(*snippet_id).await {
            Ok(()) => {
                println!("  ✓ Deleted snippet {snippet_id}");

                if let Ok(Some((sql_path, yaml_path))) = find_snippet_files(*snippet_id) {
                    if let Err(e) = delete_snippet_files(&sql_path, &yaml_path) {
                        eprintln!("  ⚠ Failed to delete local files for snippet {snippet_id}: {e}");
                    } else {
                        println!("    Deleted local files");
                    }
                } else {
                    println!("    No local files found");
                }

                deleted_count += 1;
            }
            Err(e) => {
                eprintln!("  ✗ Failed to delete snippet {snippet_id}: {e}");
                errors.push((*snippet_id, e));
            }
        }
    }

    println!(
        "\n✓ Deleted {deleted_count}/{} query snippets",
        snippet_ids.len()
    );

    if !errors.is_empty() {
        anyhow::bail!("Failed to delete {} query snippets", errors.len());
    }

    Ok(())
}

#[cfg(test)]
#[allow(clippy::missing_errors_doc)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    #[test]
    fn test_extract_snippet_ids_from_path_empty() {
        let temp_dir = TempDir::new().unwrap();
        let result = extract_snippet_ids_from_path(temp_dir.path());
        assert!(result.is_ok());
        assert!(result.unwrap().is_empty());
    }

    #[test]
    fn test_extract_snippet_ids_from_path_missing_directory() {
        let temp_dir = TempDir::new().unwrap();
        let missing = temp_dir.path().join("does-not-exist");
        let result = extract_snippet_ids_from_path(&missing);
        assert!(result.is_ok());
        assert!(result.unwrap().is_empty());
    }

    #[test]
    fn test_extract_snippet_ids_from_path_deduplication() {
        let temp_dir = TempDir::new().unwrap();
        let dir = temp_dir.path();

        fs::write(dir.join("31-old_trigger_name.yaml"), "test").unwrap();
        fs::write(dir.join("31-new_trigger_name.yaml"), "test").unwrap();

        let ids = extract_snippet_ids_from_path(dir).unwrap();
        assert_eq!(ids, vec![31]);
    }

    #[test]
    fn test_extract_snippet_ids_from_path_ignores_non_yaml() {
        let temp_dir = TempDir::new().unwrap();
        let dir = temp_dir.path();

        fs::write(dir.join("31-reviewbot_e2e_action_ctcs.yaml"), "test").unwrap();
        fs::write(dir.join("31-reviewbot_e2e_action_ctcs.sql"), "test").unwrap();
        fs::write(dir.join("README.md"), "test").unwrap();

        let ids = extract_snippet_ids_from_path(dir).unwrap();
        assert_eq!(ids, vec![31]);
    }

    #[test]
    fn test_extract_snippet_ids_from_path_ignores_id_without_separator() {
        let temp_dir = TempDir::new().unwrap();
        let dir = temp_dir.path();

        // "31.yaml" has no '-' separator, so the whole stem fails to parse as a u64
        // and must be silently skipped, not mistaken for id 31.
        fs::write(dir.join("31.yaml"), "test").unwrap();
        fs::write(dir.join("42-zebra.yaml"), "test").unwrap();

        let ids = extract_snippet_ids_from_path(dir).unwrap();
        assert_eq!(ids, vec![42]);
    }

    #[test]
    fn test_extract_snippet_ids_from_path_includes_id_zero() {
        let temp_dir = TempDir::new().unwrap();
        let dir = temp_dir.path();

        // id 0 is the sentinel for "not yet created" mid-deploy; it must still be
        // discovered like any other id, not treated as absent/falsy.
        fs::write(dir.join("0-stmo_cli_selftest.yaml"), "test").unwrap();

        let ids = extract_snippet_ids_from_path(dir).unwrap();
        assert_eq!(ids, vec![0]);
    }

    #[test]
    fn test_extract_snippet_ids_from_path_sorted() {
        let temp_dir = TempDir::new().unwrap();
        let dir = temp_dir.path();

        fs::write(dir.join("42-zebra.yaml"), "test").unwrap();
        fs::write(dir.join("9-hll_convert.yaml"), "test").unwrap();
        fs::write(dir.join("31-reviewbot_e2e_action_ctcs.yaml"), "test").unwrap();

        let ids = extract_snippet_ids_from_path(dir).unwrap();
        assert_eq!(ids, vec![9, 31, 42]);
    }

    #[test]
    fn test_find_snippet_files_in_found() {
        let temp_dir = TempDir::new().unwrap();
        let dir = temp_dir.path();

        fs::write(dir.join("31-reviewbot_e2e_action_ctcs.sql"), "SELECT 1").unwrap();
        fs::write(dir.join("31-reviewbot_e2e_action_ctcs.yaml"), "id: 31").unwrap();

        let result = find_snippet_files_in(dir, 31).unwrap();
        assert!(result.is_some());
        let (sql_path, yaml_path) = result.unwrap();
        assert_eq!(
            Path::new(&sql_path).extension(),
            Some(std::ffi::OsStr::new("sql"))
        );
        assert_eq!(
            Path::new(&yaml_path).extension(),
            Some(std::ffi::OsStr::new("yaml"))
        );
    }

    #[test]
    fn test_find_snippet_files_in_no_matching_id() {
        let temp_dir = TempDir::new().unwrap();
        let dir = temp_dir.path();

        fs::write(dir.join("31-reviewbot_e2e_action_ctcs.sql"), "SELECT 1").unwrap();
        fs::write(dir.join("31-reviewbot_e2e_action_ctcs.yaml"), "id: 31").unwrap();

        let result = find_snippet_files_in(dir, 99).unwrap();
        assert!(result.is_none());
    }

    #[test]
    fn test_find_snippet_files_in_missing_yaml() {
        let temp_dir = TempDir::new().unwrap();
        let dir = temp_dir.path();

        fs::write(dir.join("31-reviewbot_e2e_action_ctcs.sql"), "SELECT 1").unwrap();

        let result = find_snippet_files_in(dir, 31).unwrap();
        assert!(result.is_none());
    }

    #[test]
    fn test_find_snippet_files_in_missing_directory() {
        let temp_dir = TempDir::new().unwrap();
        let missing = temp_dir.path().join("does-not-exist");

        let result = find_snippet_files_in(&missing, 31).unwrap();
        assert!(result.is_none());
    }

    #[test]
    fn test_find_snippet_files_in_id_zero() {
        let temp_dir = TempDir::new().unwrap();
        let dir = temp_dir.path();

        // id 0 is the sentinel used for "not yet created" mid-deploy, before the
        // server assigns a real id and the files get renamed.
        fs::write(dir.join("0-stmo_cli_selftest.sql"), "SELECT 1").unwrap();
        fs::write(dir.join("0-stmo_cli_selftest.yaml"), "id: 0").unwrap();

        let result = find_snippet_files_in(dir, 0).unwrap();
        assert!(result.is_some());
    }

    #[test]
    fn test_find_snippet_files_in_missing_sql() {
        let temp_dir = TempDir::new().unwrap();
        let dir = temp_dir.path();

        fs::write(dir.join("31-reviewbot_e2e_action_ctcs.yaml"), "id: 31").unwrap();

        let result = find_snippet_files_in(dir, 31).unwrap();
        assert!(result.is_none());
    }

    #[test]
    fn test_find_snippet_files_in_exact_id_match_not_prefix() {
        let temp_dir = TempDir::new().unwrap();
        let dir = temp_dir.path();

        fs::write(dir.join("31-reviewbot_e2e_action_ctcs.sql"), "SELECT 1").unwrap();
        fs::write(dir.join("31-reviewbot_e2e_action_ctcs.yaml"), "id: 31").unwrap();

        // Searching for id 3 must not match a file whose id (31) merely starts with "3".
        let result = find_snippet_files_in(dir, 3).unwrap();
        assert!(result.is_none());
    }

    #[test]
    fn test_get_all_snippet_metadata_from_path_basic() {
        let temp_dir = TempDir::new().unwrap();
        let dir = temp_dir.path();

        fs::write(
            dir.join("31-reviewbot_e2e_action_ctcs.yaml"),
            "id: 31\ntrigger: reviewbot_e2e_action_ctcs\ndescription: null\n",
        )
        .unwrap();

        let metadata = get_all_snippet_metadata_from_path(dir).unwrap();
        assert_eq!(
            metadata,
            vec![(31, "reviewbot_e2e_action_ctcs".to_string())]
        );
    }

    #[test]
    fn test_get_all_snippet_metadata_from_path_sorted_by_id() {
        let temp_dir = TempDir::new().unwrap();
        let dir = temp_dir.path();

        fs::write(
            dir.join("42-zebra.yaml"),
            "id: 42\ntrigger: zebra\ndescription: null\n",
        )
        .unwrap();
        fs::write(
            dir.join("9-hll_convert.yaml"),
            "id: 9\ntrigger: hll_convert\ndescription: null\n",
        )
        .unwrap();

        let metadata = get_all_snippet_metadata_from_path(dir).unwrap();
        assert_eq!(
            metadata,
            vec![(9, "hll_convert".to_string()), (42, "zebra".to_string())]
        );
    }

    #[test]
    fn test_get_all_snippet_metadata_from_path_missing_directory_errors() {
        let temp_dir = TempDir::new().unwrap();
        let missing = temp_dir.path().join("does-not-exist");

        let result = get_all_snippet_metadata_from_path(&missing);
        assert!(result.is_err());
        assert!(
            result
                .unwrap_err()
                .to_string()
                .contains("snippets directory not found")
        );
    }

    #[test]
    fn test_get_all_snippet_metadata_from_path_empty_directory() {
        let temp_dir = TempDir::new().unwrap();

        let metadata = get_all_snippet_metadata_from_path(temp_dir.path()).unwrap();
        assert!(metadata.is_empty());
    }

    #[test]
    fn test_get_all_snippet_metadata_from_path_malformed_yaml_errors() {
        let temp_dir = TempDir::new().unwrap();
        let dir = temp_dir.path();

        fs::write(
            dir.join("31-broken.yaml"),
            "description: missing required fields\n",
        )
        .unwrap();

        let result = get_all_snippet_metadata_from_path(dir);
        assert!(result.is_err());
        assert!(result.unwrap_err().to_string().contains("Failed to parse"));
    }

    #[test]
    fn test_parse_changed_snippet_ids_basic() {
        let porcelain = " M snippets/31-reviewbot_e2e_action_ctcs.sql\n M snippets/31-reviewbot_e2e_action_ctcs.yaml\n M queries/999-unrelated.sql\n";
        let ids = parse_changed_snippet_ids(porcelain);
        assert_eq!(ids, HashSet::from([31]));
    }

    #[test]
    fn test_parse_changed_snippet_ids_untracked() {
        let porcelain =
            "?? snippets/42-stmo_cli_selftest.sql\n?? snippets/42-stmo_cli_selftest.yaml\n";
        let ids = parse_changed_snippet_ids(porcelain);
        assert_eq!(ids, HashSet::from([42]));
    }

    #[test]
    fn test_parse_changed_snippet_ids_ignores_non_snippet_extensions() {
        let porcelain = " M snippets/31-notes.md\n";
        let ids = parse_changed_snippet_ids(porcelain);
        assert!(ids.is_empty());
    }

    #[test]
    fn test_parse_changed_snippet_ids_empty_input() {
        assert!(parse_changed_snippet_ids("").is_empty());
    }

    #[test]
    fn test_parse_changed_snippet_ids_short_lines_do_not_panic() {
        // Lines shorter than the 2-char status + 1-space prefix must be skipped,
        // not sliced into (which would panic on a short/blank line).
        let porcelain = "\nM\n M\n M snippets/31-reviewbot_e2e_action_ctcs.sql\n";
        let ids = parse_changed_snippet_ids(porcelain);
        assert_eq!(ids, HashSet::from([31]));
    }

    #[test]
    fn test_parse_changed_snippet_ids_exact_boundary_length_no_panic() {
        // A line of exactly 3 chars (e.g. " M ") does NOT hit the `len < 3` guard,
        // so &line[3..] slices to an empty string -- must not panic, and the empty
        // path must not match "snippets/".
        let porcelain = " M \n";
        let ids = parse_changed_snippet_ids(porcelain);
        assert!(ids.is_empty());
    }

    #[test]
    fn test_parse_changed_snippet_ids_handles_rename() {
        // After `snippets deploy` renames 0-*.* -> {id}-*.*, `git status --porcelain`
        // reports a rename as "old -> new"; the *new* id must be the one detected.
        let porcelain = "R  snippets/0-stmo_cli_selftest.sql -> snippets/42-stmo_cli_selftest.sql\nR  snippets/0-stmo_cli_selftest.yaml -> snippets/42-stmo_cli_selftest.yaml\n";
        let ids = parse_changed_snippet_ids(porcelain);
        assert_eq!(ids, HashSet::from([42]));
    }
}

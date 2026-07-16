#![allow(clippy::missing_errors_doc)]

use anyhow::{Context, Result};

pub fn extract_changelog_section(changelog: &str, version: &str) -> Result<String> {
    let heading_prefix = format!("## [{version}]");
    let start = changelog
        .lines()
        .position(|line| line.starts_with(&heading_prefix))
        .with_context(|| format!("no `{heading_prefix}` section in CHANGELOG.md"))?;

    let lines: Vec<&str> = changelog.lines().collect();
    let body_start = start + 1;
    let body_end = lines[body_start..]
        .iter()
        .position(|line| line.starts_with("## ["))
        .map_or(lines.len(), |offset| body_start + offset);

    let body: Vec<&str> = lines[body_start..body_end]
        .iter()
        .copied()
        .skip_while(|line| line.is_empty())
        .collect();
    let trimmed_end = body
        .iter()
        .rposition(|line| !line.is_empty())
        .map_or(0, |i| i + 1);

    Ok(format!("{}\n", body[..trimmed_end].join("\n")))
}

#[cfg(test)]
mod tests {
    use super::*;

    const FIXTURE: &str = "\
# Changelog

## [0.9.0] - 2026-07-16

### Features
- feature one
- feature two

### Fixes
- fix one

## [0.8.0] - 2026-07-02

### Features
- old feature

## [0.1.0] - 2026-02-26

Initial release.
";

    #[test]
    fn extracts_middle_section_bounded_by_next_heading() {
        let section = extract_changelog_section(FIXTURE, "0.8.0").unwrap();
        assert_eq!(section, "### Features\n- old feature\n");
    }

    #[test]
    fn extracts_top_section() {
        let section = extract_changelog_section(FIXTURE, "0.9.0").unwrap();
        assert_eq!(
            section,
            "### Features\n- feature one\n- feature two\n\n### Fixes\n- fix one\n"
        );
    }

    #[test]
    fn extracts_last_section_at_eof() {
        let section = extract_changelog_section(FIXTURE, "0.1.0").unwrap();
        assert_eq!(section, "Initial release.\n");
    }

    #[test]
    fn missing_version_is_an_error() {
        assert!(extract_changelog_section(FIXTURE, "9.9.9").is_err());
    }

    #[test]
    fn does_not_match_version_as_a_substring() {
        assert!(extract_changelog_section(FIXTURE, "8.0").is_err());
        assert!(extract_changelog_section(FIXTURE, "0.8.0-rc1").is_err());
    }
}

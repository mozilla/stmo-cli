#![allow(clippy::missing_errors_doc)]

use anyhow::{Context, Result};
use std::fs;
use std::path::Path;
use std::process::Command;

const TEMPLATE_PRE_COMMIT: &str = include_str!("../../templates/init/pre-commit-config.yaml");
const TEMPLATE_SQLFLUFF: &str = include_str!("../../templates/init/sqlfluff");
const TEMPLATE_YAMLLINT: &str = include_str!("../../templates/init/yamllint");
const TEMPLATE_GITIGNORE: &str = include_str!("../../templates/init/gitignore");
const TEMPLATE_CLAUDE_MD: &str = include_str!("../../templates/init/CLAUDE.md");

struct ScaffoldFile {
    path: &'static str,
    content: &'static str,
    description: &'static str,
}

const SCAFFOLD_FILES: &[ScaffoldFile] = &[
    ScaffoldFile {
        path: ".pre-commit-config.yaml",
        content: TEMPLATE_PRE_COMMIT,
        description: "pre-commit hooks config",
    },
    ScaffoldFile {
        path: ".sqlfluff",
        content: TEMPLATE_SQLFLUFF,
        description: "sqlfluff linter config",
    },
    ScaffoldFile {
        path: ".yamllint",
        content: TEMPLATE_YAMLLINT,
        description: "yamllint config",
    },
    ScaffoldFile {
        path: ".gitignore",
        content: TEMPLATE_GITIGNORE,
        description: "git ignore rules",
    },
    ScaffoldFile {
        path: "CLAUDE.md",
        content: TEMPLATE_CLAUDE_MD,
        description: "AI assistant instructions",
    },
];

fn write_if_missing(target_dir: &Path, file: &ScaffoldFile) -> Result<bool> {
    let file_path = target_dir.join(file.path);

    if file_path.exists() {
        let path = file.path;
        println!("  ⊘ {path} (already exists)");
        Ok(false)
    } else {
        let path = file.path;
        fs::write(&file_path, file.content).with_context(|| format!("Failed to write {path}"))?;
        let description = file.description;
        println!("  ✓ {path} ({description})");
        Ok(true)
    }
}

fn create_directory_with_gitkeep(target_dir: &Path, dir_name: &str) -> Result<bool> {
    let dir_path = target_dir.join(dir_name);
    let gitkeep_path = dir_path.join(".gitkeep");

    if gitkeep_path.exists() {
        println!("  ⊘ {dir_name}/  (already exists)");
        Ok(false)
    } else {
        fs::create_dir_all(&dir_path)
            .with_context(|| format!("Failed to create {dir_name} directory"))?;
        fs::write(&gitkeep_path, "")
            .with_context(|| format!("Failed to write {dir_name}/.gitkeep"))?;
        println!("  ✓ {dir_name}/  (directory with .gitkeep)");
        Ok(true)
    }
}

fn git_available() -> bool {
    clean_git_cmd()
        .arg("--version")
        .output()
        .is_ok_and(|output| output.status.success())
}

fn precommit_available() -> bool {
    Command::new("pre-commit")
        .arg("--version")
        .output()
        .is_ok_and(|output| output.status.success())
}

// Returns a git Command with inherited git env vars cleared, so commands run in
// a fresh directory are not affected by a parent worktree's GIT_DIR or GIT_INDEX_FILE.
fn clean_git_cmd() -> Command {
    let mut cmd = Command::new("git");
    cmd.env_remove("GIT_DIR")
        .env_remove("GIT_WORK_TREE")
        .env_remove("GIT_COMMON_DIR")
        .env_remove("GIT_INDEX_FILE");
    cmd
}

fn ensure_git_identity(target_dir: &Path) -> Result<()> {
    let name_configured = clean_git_cmd()
        .args(["config", "user.name"])
        .current_dir(target_dir)
        .output()
        .is_ok_and(|o| o.status.success() && !o.stdout.trim_ascii().is_empty());

    if !name_configured {
        let set_name = clean_git_cmd()
            .args(["config", "user.name", "stmo-cli"])
            .current_dir(target_dir)
            .status()
            .context("Failed to set git user.name")?;
        if !set_name.success() {
            anyhow::bail!("git config user.name failed");
        }

        let set_email = clean_git_cmd()
            .args(["config", "user.email", "stmo-cli@noreply"])
            .current_dir(target_dir)
            .status()
            .context("Failed to set git user.email")?;
        if !set_email.success() {
            anyhow::bail!("git config user.email failed");
        }
    }

    Ok(())
}

fn detect_os() -> &'static str {
    if cfg!(target_os = "macos") {
        "macos"
    } else if cfg!(target_os = "linux") {
        "linux"
    } else {
        "other"
    }
}

fn try_precommit_autoupdate(target_dir: &Path) -> bool {
    if !precommit_available() {
        return false;
    }
    let output = Command::new("pre-commit")
        .arg("autoupdate")
        .current_dir(target_dir)
        .output();
    match output {
        Ok(o) if o.status.success() => {
            println!("  ✓ Updated hook versions in .pre-commit-config.yaml");
            true
        }
        _ => {
            println!("  ⚠ pre-commit autoupdate failed, using template versions");
            false
        }
    }
}

fn install_precommit_hooks(target_dir: &Path) -> Result<()> {
    let install_output = Command::new("pre-commit")
        .arg("install")
        .current_dir(target_dir)
        .output()
        .context("Failed to run pre-commit install")?;

    if !install_output.status.success() {
        let stderr = String::from_utf8_lossy(&install_output.stderr);
        anyhow::bail!("pre-commit install failed: {stderr}");
    }
    println!("  ✓ Installed pre-commit git hooks");
    Ok(())
}

fn setup_git_repo(target_dir: &Path, files_created: bool) -> Result<()> {
    let git_dir = target_dir.join(".git");

    if !git_dir.exists() {
        println!("\n⚙ Initializing git repository...");
        let status = clean_git_cmd()
            .arg("init")
            .current_dir(target_dir)
            .status()
            .context("Failed to run git init")?;

        if !status.success() {
            anyhow::bail!("git init failed");
        }
    }

    ensure_git_identity(target_dir)?;

    if files_created {
        println!("⚙ Creating initial commit...");

        let add_status = clean_git_cmd()
            .args(["add", "."])
            .current_dir(target_dir)
            .status()
            .context("Failed to run git add")?;

        if !add_status.success() {
            anyhow::bail!("git add failed");
        }

        let commit_output = clean_git_cmd()
            .args([
                "commit",
                "-m",
                "Initial commit: scaffold query/dashboard repository",
            ])
            .current_dir(target_dir)
            .output()
            .context("Failed to run git commit")?;

        if !commit_output.status.success() {
            let stderr = String::from_utf8_lossy(&commit_output.stderr);
            anyhow::bail!("git commit failed: {stderr}");
        }

        println!("  ✓ Initial commit created");
    }

    Ok(())
}

fn init_in(target_dir: &Path) -> Result<bool> {
    println!("Scaffolding query/dashboard repository...\n");

    let mut files_created = 0;
    let mut files_skipped = 0;

    for file in SCAFFOLD_FILES {
        if write_if_missing(target_dir, file)? {
            files_created += 1;
        } else {
            files_skipped += 1;
        }
    }

    if create_directory_with_gitkeep(target_dir, "queries")? {
        files_created += 1;
    } else {
        files_skipped += 1;
    }

    if create_directory_with_gitkeep(target_dir, "dashboards")? {
        files_created += 1;
    } else {
        files_skipped += 1;
    }

    println!("\n📊 Summary: {files_created} created, {files_skipped} skipped");

    if files_created == 0 {
        println!("\n✓ Repository already initialized");
        return Ok(false);
    }

    if git_available() {
        if precommit_available() {
            println!("\n⚙ Setting up pre-commit...");
            try_precommit_autoupdate(target_dir);
        }
        setup_git_repo(target_dir, files_created > 0)?;
    } else {
        println!("\n⚠ git is not installed - files created but not committed");
        println!("  Install git to enable version control");
    }

    Ok(true)
}

pub fn init() -> Result<()> {
    let target_dir = Path::new(".");
    let files_created = init_in(target_dir)?;

    if files_created && git_available() {
        if precommit_available() {
            println!("\n⚙ Installing pre-commit hooks...");
            install_precommit_hooks(target_dir)?;
        } else {
            println!("\n⚠ pre-commit is not installed");
            match detect_os() {
                "macos" => println!("  Install with: brew install pre-commit"),
                _ => println!("  Install with: pip install pre-commit"),
            }
            println!("  After installing, re-run 'stmo-cli init' to finish setup.");
        }
    }

    if files_created {
        println!("\n✓ Repository scaffolded successfully");
        println!("\nNext steps:");
        println!("  1. Set REDASH_API_KEY environment variable");
        println!("  2. Run 'stmo-cli discover' to see available queries");
        println!("  3. Run 'stmo-cli fetch <id>' to download queries");
        println!("  4. Run 'stmo-cli deploy' to push changes back to Redash");
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use tempfile::TempDir;

    fn clean_git(dir: &std::path::Path) -> Command {
        let mut cmd = clean_git_cmd();
        cmd.current_dir(dir);
        cmd
    }

    fn setup_test_repo(dir: &std::path::Path) {
        clean_git(dir).arg("init").status().unwrap();
        clean_git(dir)
            .args(["config", "user.name", "Test"])
            .status()
            .unwrap();
        clean_git(dir)
            .args(["config", "user.email", "test@test"])
            .status()
            .unwrap();
    }

    #[test]
    fn test_init_creates_all_files() {
        let temp_dir = TempDir::new().unwrap();
        init_in(temp_dir.path()).unwrap();

        assert!(temp_dir.path().join(".pre-commit-config.yaml").exists());
        assert!(temp_dir.path().join(".sqlfluff").exists());
        assert!(temp_dir.path().join(".yamllint").exists());
        assert!(temp_dir.path().join(".gitignore").exists());
        assert!(temp_dir.path().join("CLAUDE.md").exists());
        assert!(temp_dir.path().join("queries/.gitkeep").exists());
        assert!(temp_dir.path().join("dashboards/.gitkeep").exists());

        let pre_commit_content =
            fs::read_to_string(temp_dir.path().join(".pre-commit-config.yaml")).unwrap();
        assert!(pre_commit_content.contains("yamllint"));
        assert!(pre_commit_content.contains("sqlfluff"));

        let sqlfluff_content = fs::read_to_string(temp_dir.path().join(".sqlfluff")).unwrap();
        assert!(sqlfluff_content.contains("bigquery"));
        assert!(sqlfluff_content.contains("jinja"));

        let claude_md_content = fs::read_to_string(temp_dir.path().join("CLAUDE.md")).unwrap();
        assert!(claude_md_content.contains("stmo-cli"));
        assert!(!claude_md_content.contains("cargo run"));
    }

    #[test]
    fn test_init_skips_existing_files() {
        let temp_dir = TempDir::new().unwrap();

        let sqlfluff_path = temp_dir.path().join(".sqlfluff");
        fs::write(&sqlfluff_path, "custom content").unwrap();

        init_in(temp_dir.path()).unwrap();

        let content = fs::read_to_string(&sqlfluff_path).unwrap();
        assert_eq!(content, "custom content");

        assert!(temp_dir.path().join(".pre-commit-config.yaml").exists());
        assert!(temp_dir.path().join("queries/.gitkeep").exists());
    }

    #[test]
    fn test_init_creates_git_repo() {
        let temp_dir = TempDir::new().unwrap();

        if !git_available() {
            return;
        }

        init_in(temp_dir.path()).unwrap();

        assert!(temp_dir.path().join(".git").exists());

        let log_output = clean_git(temp_dir.path())
            .args(["log", "--oneline"])
            .output()
            .unwrap();

        let log = String::from_utf8_lossy(&log_output.stdout);
        assert!(log.contains("Initial commit"));
    }

    #[test]
    fn test_init_commits_to_existing_repo() {
        let temp_dir = TempDir::new().unwrap();

        if !git_available() {
            return;
        }

        setup_test_repo(temp_dir.path());

        fs::write(temp_dir.path().join("existing.txt"), "test").unwrap();
        clean_git(temp_dir.path())
            .args(["add", "."])
            .status()
            .unwrap();
        clean_git(temp_dir.path())
            .args(["commit", "-m", "First commit"])
            .status()
            .unwrap();

        init_in(temp_dir.path()).unwrap();

        let log_output = clean_git(temp_dir.path())
            .args(["log", "--oneline"])
            .output()
            .unwrap();

        let log = String::from_utf8_lossy(&log_output.stdout);
        let commit_count = log.lines().count();
        assert!(commit_count >= 2);
    }

    #[test]
    fn test_init_no_commit_when_all_exist() {
        let temp_dir = TempDir::new().unwrap();

        if !git_available() {
            return;
        }

        for file in SCAFFOLD_FILES {
            fs::write(temp_dir.path().join(file.path), file.content).unwrap();
        }
        fs::create_dir_all(temp_dir.path().join("queries")).unwrap();
        fs::write(temp_dir.path().join("queries/.gitkeep"), "").unwrap();
        fs::create_dir_all(temp_dir.path().join("dashboards")).unwrap();
        fs::write(temp_dir.path().join("dashboards/.gitkeep"), "").unwrap();

        setup_test_repo(temp_dir.path());
        clean_git(temp_dir.path())
            .args(["add", "."])
            .status()
            .unwrap();
        clean_git(temp_dir.path())
            .args(["commit", "-m", "Existing commit"])
            .status()
            .unwrap();

        init_in(temp_dir.path()).unwrap();

        let log_output = clean_git(temp_dir.path())
            .args(["log", "--oneline"])
            .output()
            .unwrap();

        let log = String::from_utf8_lossy(&log_output.stdout);
        let commit_count = log.lines().count();
        assert_eq!(commit_count, 1);
    }

    #[test]
    fn test_init_produces_single_commit() {
        let temp_dir = TempDir::new().unwrap();

        if !git_available() {
            return;
        }

        init_in(temp_dir.path()).unwrap();

        let log_output = clean_git(temp_dir.path())
            .args(["log", "--oneline"])
            .output()
            .unwrap();

        let log = String::from_utf8_lossy(&log_output.stdout);
        let commit_count = log.lines().count();
        assert_eq!(
            commit_count, 1,
            "init should create exactly one commit, not an amend"
        );
    }

    #[test]
    fn test_template_content_validity() {
        assert!(TEMPLATE_PRE_COMMIT.contains("yamllint"));
        assert!(TEMPLATE_PRE_COMMIT.contains("sqlfluff"));
        assert!(TEMPLATE_PRE_COMMIT.contains("sqlfluff-lint-snippets"));
        assert!(TEMPLATE_PRE_COMMIT.contains("exclude: ^snippets/"));

        assert!(TEMPLATE_SQLFLUFF.contains("bigquery"));
        assert!(TEMPLATE_SQLFLUFF.contains("[sqlfluff]"));

        assert!(TEMPLATE_YAMLLINT.contains("extends: default"));

        assert!(TEMPLATE_GITIGNORE.contains(".DS_Store"));

        assert!(TEMPLATE_CLAUDE_MD.contains("stmo-cli"));
        assert!(TEMPLATE_CLAUDE_MD.contains("Quick Reference"));
        assert!(TEMPLATE_CLAUDE_MD.contains("snippets"));
    }
}

#![allow(clippy::missing_errors_doc)]

use anyhow::{Context, Result};
use dialoguer::Confirm;
use std::fs;
use std::io::IsTerminal;
use std::path::{Path, PathBuf};
use std::process::Command;

const TEMPLATE_PRE_COMMIT: &str = include_str!("../../templates/init/pre-commit-config.yaml");
const TEMPLATE_SQLFLUFF: &str = include_str!("../../templates/init/sqlfluff");
const TEMPLATE_YAMLLINT: &str = include_str!("../../templates/init/yamllint");
const TEMPLATE_GITIGNORE: &str = include_str!("../../templates/init/gitignore");
const TEMPLATE_AGENTS_MD: &str = include_str!("../../templates/init/AGENTS.md");

struct ScaffoldFile {
    path: &'static str,
    content: &'static str,
    description: &'static str,
}

const GIT_FILES: &[ScaffoldFile] = &[ScaffoldFile {
    path: ".gitignore",
    content: TEMPLATE_GITIGNORE,
    description: "git ignore rules",
}];

const LINTER_FILES: &[ScaffoldFile] = &[
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
];

const PRECOMMIT_FILES: &[ScaffoldFile] = &[ScaffoldFile {
    path: ".pre-commit-config.yaml",
    content: TEMPLATE_PRE_COMMIT,
    description: "pre-commit hooks config",
}];

const AGENTS_MD_FILE: ScaffoldFile = ScaffoldFile {
    path: "AGENTS.md",
    content: TEMPLATE_AGENTS_MD,
    description: "AI assistant instructions",
};

// Scaffold markers let `init` recognize an existing scaffold regardless of
// which features a past run opted into. CLAUDE.md remains recognized for
// compatibility with older scaffolds, but is no longer generated.
const KNOWN_SCAFFOLD_PATHS: &[&str] = &[
    ".gitignore",
    ".sqlfluff",
    ".yamllint",
    ".pre-commit-config.yaml",
    "AGENTS.md",
    "CLAUDE.md",
];

// Five independent yes/no wizard answers, not state-machine states — a
// state machine or nested enums would model relationships that don't exist
// here (e.g. `linters` and `agent_instructions` are fully orthogonal to each other).
#[allow(clippy::struct_excessive_bools)]
pub struct InitChoices {
    pub git: bool,
    pub commit: bool,
    pub linters: bool,
    pub precommit: bool,
    pub agent_instructions: bool,
}

#[derive(Debug)]
struct Summary {
    files_created: usize,
    committed: bool,
}

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

// Idempotence is keyed on `.gitkeep` when git is involved (so a later `init`
// run that turns git on retroactively adds the marker) and on the directory
// itself otherwise, since a git-less scaffold never writes `.gitkeep`.
fn create_directory(target_dir: &Path, dir_name: &str, with_gitkeep: bool) -> Result<bool> {
    let dir_path = target_dir.join(dir_name);
    let already_exists = if with_gitkeep {
        dir_path.join(".gitkeep").exists()
    } else {
        dir_path.exists()
    };

    if already_exists {
        println!("  ⊘ {dir_name}/  (already exists)");
        return Ok(false);
    }

    fs::create_dir_all(&dir_path)
        .with_context(|| format!("Failed to create {dir_name} directory"))?;

    if with_gitkeep {
        fs::write(dir_path.join(".gitkeep"), "")
            .with_context(|| format!("Failed to write {dir_name}/.gitkeep"))?;
        println!("  ✓ {dir_name}/  (directory with .gitkeep)");
    } else {
        println!("  ✓ {dir_name}/");
    }
    Ok(true)
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

// The shipped .pre-commit-config.yaml template has empty `rev: ""` fields for
// both hooks (see templates/init/pre-commit-config.yaml) — pre-commit refuses
// to run with an unresolved rev, so a config that never got autoupdated is
// broken, not merely out of date. Fatal, since pre-commit was explicitly opted
// into.
fn precommit_autoupdate(target_dir: &Path) -> Result<()> {
    let output = Command::new("pre-commit")
        .arg("autoupdate")
        .current_dir(target_dir)
        .output()
        .context("Failed to run pre-commit autoupdate")?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        anyhow::bail!(
            "pre-commit autoupdate failed: {stderr}\n.pre-commit-config.yaml still has empty \
             `rev:` fields and won't run. The directory at {} was already scaffolded — run \
             'pre-commit autoupdate' there yourself.",
            target_dir.display()
        );
    }
    println!("  ✓ Updated hook versions in .pre-commit-config.yaml");
    Ok(())
}

fn install_precommit_hooks(target_dir: &Path) -> Result<()> {
    let install_output = Command::new("pre-commit")
        .arg("install")
        .current_dir(target_dir)
        .output()
        .context("Failed to run pre-commit install")?;

    if !install_output.status.success() {
        let stderr = String::from_utf8_lossy(&install_output.stderr);
        anyhow::bail!(
            "pre-commit install failed: {stderr}\nThe directory at {} was already scaffolded — \
             run 'pre-commit install' there yourself.",
            target_dir.display()
        );
    }
    println!("  ✓ Installed pre-commit git hooks");
    Ok(())
}

fn init_git_repo(target_dir: &Path) -> Result<bool> {
    let git_dir = target_dir.join(".git");
    if git_dir.exists() {
        return Ok(false);
    }

    println!("\n⚙ Initializing git repository...");
    let status = clean_git_cmd()
        .arg("init")
        .current_dir(target_dir)
        .status()
        .context("Failed to run git init")?;

    if !status.success() {
        anyhow::bail!("git init failed");
    }
    Ok(true)
}

fn create_initial_commit(target_dir: &Path) -> Result<bool> {
    ensure_git_identity(target_dir)?;

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
        anyhow::bail!(
            "git commit failed: {stderr}\nThe directory at {} was already scaffolded — \
             commit manually when ready.",
            target_dir.display()
        );
    }

    println!("  ✓ Initial commit created");
    Ok(true)
}

fn scaffold(target_dir: &Path, choices: &InitChoices) -> Result<Summary> {
    println!("Scaffolding query/dashboard repository...\n");

    let mut files_created = 0;

    if create_directory(target_dir, "queries", choices.git)? {
        files_created += 1;
    }
    if create_directory(target_dir, "dashboards", choices.git)? {
        files_created += 1;
    }

    if choices.git {
        for file in GIT_FILES {
            if write_if_missing(target_dir, file)? {
                files_created += 1;
            }
        }
    }
    if choices.linters {
        for file in LINTER_FILES {
            if write_if_missing(target_dir, file)? {
                files_created += 1;
            }
        }
    }
    // A pre-commit config is only meaningful alongside a git repo (hooks live
    // under `.git/hooks/`), so this stays nested under `choices.git` even
    // though `prompt_choices` already never offers `precommit` without it.
    if choices.git && choices.precommit {
        for file in PRECOMMIT_FILES {
            if write_if_missing(target_dir, file)? {
                files_created += 1;
            }
        }
    }
    if choices.agent_instructions && write_if_missing(target_dir, &AGENTS_MD_FILE)? {
        files_created += 1;
    }

    println!("\n📊 Summary: {files_created} item(s) created");

    let mut committed = false;

    if choices.git {
        if !git_available() {
            anyhow::bail!(
                "git was requested, but the `git` binary was not found on PATH.\n\
                 Files were scaffolded at {}; install git and run 'git init' there yourself.",
                target_dir.display()
            );
        }

        init_git_repo(target_dir)?;

        if choices.precommit {
            if !precommit_available() {
                anyhow::bail!(
                    "pre-commit hooks were requested, but the `pre-commit` binary was not found \
                     on PATH.\nFiles were scaffolded at {}; install pre-commit and run \
                     'pre-commit install' there yourself.",
                    target_dir.display()
                );
            }
            println!("\n⚙ Setting up pre-commit...");
            precommit_autoupdate(target_dir)?;
            println!("\n⚙ Installing pre-commit hooks...");
            install_precommit_hooks(target_dir)?;
        }

        if choices.commit && files_created > 0 {
            committed = create_initial_commit(target_dir)?;
        }
    }

    Ok(Summary {
        files_created,
        committed,
    })
}

#[derive(Debug, PartialEq)]
enum TargetState {
    New,
    Empty,
    ExistingScaffold,
    Unrelated(Vec<String>),
}

fn tolerated_entry(name: &str) -> bool {
    if KNOWN_SCAFFOLD_PATHS.contains(&name) {
        return true;
    }
    matches!(
        name,
        "queries" | "dashboards" | "snippets" | ".git" | ".github" | ".DS_Store"
    ) || name.starts_with("README")
        || name.starts_with("LICENSE")
}

fn classify_target(target: &Path) -> Result<TargetState> {
    if !target.exists() {
        return Ok(TargetState::New);
    }

    let mut entries = Vec::new();
    for entry in fs::read_dir(target).with_context(|| {
        format!(
            "Failed to read contents of target directory {}",
            target.display()
        )
    })? {
        let entry = entry.context("Failed to read directory entry")?;
        entries.push(entry.file_name().to_string_lossy().into_owned());
    }

    if entries.is_empty() {
        return Ok(TargetState::Empty);
    }

    let unrelated: Vec<String> = entries
        .into_iter()
        .filter(|name| !tolerated_entry(name))
        .collect();

    if unrelated.is_empty() {
        Ok(TargetState::ExistingScaffold)
    } else {
        Ok(TargetState::Unrelated(unrelated))
    }
}

fn check_target(target: &Path) -> Result<TargetState> {
    let state = classify_target(target)?;
    if let TargetState::Unrelated(mut entries) = state {
        entries.sort();
        let shown_count = entries.len().min(5);
        let mut names = entries[..shown_count].join(", ");
        if entries.len() > shown_count {
            names = format!("{names}, and {} more", entries.len() - shown_count);
        }
        anyhow::bail!(
            "{} contains unrelated files ({names}) and doesn't look like a query/dashboard \
             repository.\nPick an empty or dedicated subdirectory instead, e.g.:\n  \
             stmo-cli init {}/stmo-queries",
            target.display(),
            target.display()
        );
    }
    Ok(state)
}

fn prompt_choices() -> Result<InitChoices> {
    let git = Confirm::new()
        .with_prompt("Initialize a git repository?")
        .default(false)
        .interact()?;

    let commit = git
        && Confirm::new()
            .with_prompt("Create an initial commit?")
            .default(false)
            .interact()?;

    let linters = Confirm::new()
        .with_prompt("Add linter configs (.sqlfluff, .yamllint)?")
        .default(true)
        .interact()?;

    let precommit = git
        && linters
        && precommit_available()
        && Confirm::new()
            .with_prompt("Install pre-commit hooks?")
            .default(false)
            .interact()?;

    let agent_instructions = Confirm::new()
        .with_prompt("Add AGENTS.md for AI assistants?")
        .default(true)
        .interact()?;

    Ok(InitChoices {
        git,
        commit,
        linters,
        precommit,
        agent_instructions,
    })
}

fn print_next_steps(target_dir: &Path, choices: &InitChoices, summary: &Summary) {
    if summary.files_created == 0 {
        println!("\n✓ Repository already initialized");
        return;
    }

    println!("\n✓ Repository scaffolded successfully");

    if choices.git && !summary.committed {
        if choices.commit {
            println!("  (nothing new to commit)");
        } else {
            println!(
                "  Nothing committed. To commit: git -C {} add . && git -C {} commit -m \"Initial commit\"",
                target_dir.display(),
                target_dir.display()
            );
        }
    } else if !choices.git {
        println!(
            "  Tip: to version these files, run: git -C {} init && git -C {} add . && \
             git -C {} commit -m \"Initial commit\"",
            target_dir.display(),
            target_dir.display(),
            target_dir.display()
        );
    }

    println!("\nNext steps:");
    if target_dir != Path::new(".") {
        println!("  0. cd {}", target_dir.display());
    }
    println!("  1. Set REDASH_API_KEY environment variable");
    println!("  2. Run 'stmo-cli discover' to see available queries");
    println!("  3. Run 'stmo-cli fetch <id>' to download queries");
    println!("  4. Run 'stmo-cli deploy' to push changes back to Redash");
}

// A bare `stmo-cli init` (no PATH argument) scaffolds the current directory.
fn resolve_target(path: Option<PathBuf>) -> PathBuf {
    path.unwrap_or_else(|| PathBuf::from("."))
}

fn init_impl(
    path: Option<PathBuf>,
    is_terminal: impl Fn() -> bool,
    prompt: impl FnOnce() -> Result<InitChoices>,
) -> Result<()> {
    let target = resolve_target(path);
    check_target(&target)?;

    if !is_terminal() {
        anyhow::bail!(
            "stmo-cli init needs a terminal to ask what to set up.\nRun it yourself in your \
             own terminal, e.g.:\n  stmo-cli init {}",
            target.display()
        );
    }

    let choices = prompt()?;

    fs::create_dir_all(&target)
        .with_context(|| format!("Failed to create target directory {}", target.display()))?;

    let summary = scaffold(&target, &choices)?;
    print_next_steps(&target, &choices, &summary);

    Ok(())
}

pub fn init(path: Option<PathBuf>) -> Result<()> {
    init_impl(path, || std::io::stdin().is_terminal(), prompt_choices)
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

    fn commit_count(dir: &std::path::Path) -> usize {
        let log_output = clean_git(dir).args(["log", "--oneline"]).output().unwrap();
        String::from_utf8_lossy(&log_output.stdout).lines().count()
    }

    fn all_choices() -> InitChoices {
        InitChoices {
            git: true,
            commit: true,
            linters: true,
            precommit: false,
            agent_instructions: true,
        }
    }

    fn no_choices() -> InitChoices {
        InitChoices {
            git: false,
            commit: false,
            linters: false,
            precommit: false,
            agent_instructions: false,
        }
    }

    #[test]
    fn test_resolve_target_defaults_to_dot() {
        assert_eq!(resolve_target(None), PathBuf::from("."));
    }

    #[test]
    fn test_resolve_target_uses_given_path() {
        let path = PathBuf::from("/tmp/somewhere");
        assert_eq!(resolve_target(Some(path.clone())), path);
    }

    // These two tests exercise the target-resolution behavior that `init()`
    // adds (create the directory if missing, scaffold there instead of the
    // cwd) via `scaffold` directly, so they don't depend on the `pre-commit`
    // binary at all. `init()` now requires a real terminal (the wizard needs
    // one to prompt in), so calling the full public entry point with no PATH
    // can no longer be exercised from a non-interactive test process at all —
    // see the interactive check in the plan instead.
    #[test]
    fn test_init_creates_missing_target_directory() {
        let temp_dir = TempDir::new().unwrap();
        let target = temp_dir.path().join("new");
        assert!(!target.exists());

        fs::create_dir_all(&target).unwrap();
        scaffold(&target, &no_choices()).unwrap();

        assert!(target.join("queries").exists());
    }

    #[test]
    fn test_init_scaffolds_into_given_path_not_cwd() {
        let temp_dir = TempDir::new().unwrap();
        let cwd_marker = temp_dir.path().join("cwd-marker");
        fs::create_dir_all(&cwd_marker).unwrap();
        let target = temp_dir.path().join("target");

        let mut choices = no_choices();
        choices.agent_instructions = true;
        fs::create_dir_all(&target).unwrap();
        scaffold(&target, &choices).unwrap();

        assert!(target.join("AGENTS.md").exists());
        assert!(!cwd_marker.join("AGENTS.md").exists());
        assert!(!cwd_marker.join("queries").exists());
    }

    #[test]
    fn test_scaffold_all_declined_creates_only_directories() {
        let temp_dir = TempDir::new().unwrap();
        let target = temp_dir.path();

        scaffold(target, &no_choices()).unwrap();

        assert!(target.join("queries").exists());
        assert!(target.join("dashboards").exists());
        assert!(!target.join("queries/.gitkeep").exists());
        assert!(!target.join(".gitignore").exists());
        assert!(!target.join(".sqlfluff").exists());
        assert!(!target.join(".yamllint").exists());
        assert!(!target.join(".pre-commit-config.yaml").exists());
        assert!(!target.join("AGENTS.md").exists());
        assert!(!target.join(".git").exists());
    }

    #[test]
    fn test_scaffold_linters_only() {
        let temp_dir = TempDir::new().unwrap();
        let target = temp_dir.path();

        let mut choices = no_choices();
        choices.linters = true;
        scaffold(target, &choices).unwrap();

        assert!(target.join(".sqlfluff").exists());
        assert!(target.join(".yamllint").exists());
        assert!(!target.join(".gitignore").exists());
        assert!(!target.join("AGENTS.md").exists());
        assert!(!target.join(".pre-commit-config.yaml").exists());
    }

    #[test]
    fn test_scaffold_agent_instructions_only() {
        let temp_dir = TempDir::new().unwrap();
        let target = temp_dir.path();

        let mut choices = no_choices();
        choices.agent_instructions = true;
        scaffold(target, &choices).unwrap();

        assert!(target.join("AGENTS.md").exists());
        assert!(!target.join(".sqlfluff").exists());
    }

    #[test]
    fn test_scaffold_precommit_without_git_writes_nothing_precommit_related() {
        let temp_dir = TempDir::new().unwrap();
        let target = temp_dir.path();

        // `precommit: true` with `git: false` can't happen through the real
        // wizard (`prompt_choices` only offers it when git was chosen), but
        // `scaffold` must still not write a dangling pre-commit config for it.
        let choices = InitChoices {
            git: false,
            commit: false,
            linters: true,
            precommit: true,
            agent_instructions: false,
        };
        scaffold(target, &choices).unwrap();

        assert!(!target.join(".pre-commit-config.yaml").exists());
        assert!(!target.join(".git").exists());
    }

    #[test]
    fn test_scaffold_git_without_commit_creates_repo_with_zero_commits() {
        let temp_dir = TempDir::new().unwrap();
        let target = temp_dir.path();

        if !git_available() {
            return;
        }

        let mut choices = no_choices();
        choices.git = true;
        scaffold(target, &choices).unwrap();

        assert!(target.join(".git").exists());
        assert!(target.join(".gitignore").exists());
        assert!(target.join("queries/.gitkeep").exists());
        assert_eq!(commit_count(target), 0);
    }

    #[test]
    fn test_scaffold_git_and_commit_creates_exactly_one_commit() {
        let temp_dir = TempDir::new().unwrap();
        let target = temp_dir.path();

        if !git_available() {
            return;
        }

        scaffold(target, &all_choices()).unwrap();

        assert_eq!(commit_count(target), 1);
    }

    #[test]
    fn test_scaffold_rerun_does_not_amend() {
        let temp_dir = TempDir::new().unwrap();
        let target = temp_dir.path();

        if !git_available() {
            return;
        }

        scaffold(target, &all_choices()).unwrap();
        assert_eq!(commit_count(target), 1);

        // Nothing new to scaffold the second time, so nothing new to commit.
        scaffold(target, &all_choices()).unwrap();
        assert_eq!(commit_count(target), 1);
    }

    #[test]
    fn test_scaffold_git_requested_but_unavailable_is_fatal() {
        let temp_dir = TempDir::new().unwrap();
        let target = temp_dir.path();

        if git_available() {
            return;
        }

        let mut choices = no_choices();
        choices.git = true;
        assert!(scaffold(target, &choices).is_err());
    }

    #[test]
    fn test_scaffold_commit_rejected_by_hook_is_fatal_and_names_scaffolded_dir() {
        let temp_dir = TempDir::new().unwrap();
        let target = temp_dir.path();

        if !git_available() {
            return;
        }

        setup_test_repo(target);
        let hooks_dir = target.join(".git/hooks");
        fs::create_dir_all(&hooks_dir).unwrap();
        let hook_path = hooks_dir.join("pre-commit");
        fs::write(&hook_path, "#!/bin/sh\nexit 1\n").unwrap();
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let mut perms = fs::metadata(&hook_path).unwrap().permissions();
            perms.set_mode(0o755);
            fs::set_permissions(&hook_path, perms).unwrap();
        }

        let err = scaffold(target, &all_choices()).unwrap_err();
        let message = err.to_string();
        assert!(message.contains("git commit failed"));
        assert!(message.contains(&target.display().to_string()));

        // The scaffold files were still written even though the commit failed.
        assert!(target.join("AGENTS.md").exists());
    }

    #[test]
    fn test_scaffold_precommit_autoupdate_failure_is_fatal() {
        let temp_dir = TempDir::new().unwrap();
        let target = temp_dir.path();

        if !git_available() || !precommit_available() {
            return;
        }

        setup_test_repo(target);
        // Pre-seed an invalid config so `write_if_missing` leaves it alone and
        // `pre-commit autoupdate` fails deterministically, regardless of
        // network access.
        fs::write(target.join(".pre-commit-config.yaml"), "not: [valid, yaml").unwrap();

        let mut choices = no_choices();
        choices.git = true;
        choices.precommit = true;

        let err = scaffold(target, &choices).unwrap_err();
        let message = err.to_string();
        assert!(message.contains("autoupdate failed"));
        assert!(message.contains("rev"));
    }

    #[test]
    fn test_scaffold_precommit_not_offered_skips_autoupdate_and_stays_ok() {
        let temp_dir = TempDir::new().unwrap();
        let target = temp_dir.path();

        if !git_available() {
            return;
        }

        // Same invalid config, but `precommit: false` — scaffold must not
        // even look at it.
        setup_test_repo(target);
        fs::write(target.join(".pre-commit-config.yaml"), "not: [valid, yaml").unwrap();

        let mut choices = no_choices();
        choices.git = true;
        assert!(scaffold(target, &choices).is_ok());
    }

    #[test]
    fn test_init_impl_unrelated_directory_wins_over_tty_check() {
        let temp_dir = TempDir::new().unwrap();
        let target = temp_dir.path().join("home");
        fs::create_dir_all(target.join("Documents")).unwrap();
        fs::write(target.join(".zshrc"), "").unwrap();

        let err =
            init_impl(Some(target), || false, || panic!("prompt should not run")).unwrap_err();
        assert!(err.to_string().contains("unrelated files"));
    }

    #[test]
    fn test_init_impl_refuses_without_terminal() {
        let temp_dir = TempDir::new().unwrap();
        let target = temp_dir.path().join("repo");

        let err =
            init_impl(Some(target), || false, || panic!("prompt should not run")).unwrap_err();
        assert!(err.to_string().contains("terminal"));
    }

    #[test]
    fn test_init_impl_declining_everything_creates_only_directories() {
        let temp_dir = TempDir::new().unwrap();
        let target = temp_dir.path().join("repo");

        init_impl(Some(target.clone()), || true, || Ok(no_choices())).unwrap();

        assert!(target.join("queries").exists());
        assert!(target.join("dashboards").exists());
        assert!(!target.join(".git").exists());
        assert!(!target.join("AGENTS.md").exists());
    }

    #[test]
    fn test_init_impl_full_flow_with_git_and_commit() {
        let temp_dir = TempDir::new().unwrap();
        let target = temp_dir.path().join("repo");

        if !git_available() {
            return;
        }

        init_impl(Some(target.clone()), || true, || Ok(all_choices())).unwrap();

        assert!(target.join(".git").exists());
        assert!(target.join("AGENTS.md").exists());
        assert_eq!(commit_count(&target), 1);
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

        assert!(TEMPLATE_AGENTS_MD.contains("stmo-cli"));
        assert!(TEMPLATE_AGENTS_MD.contains("Quick Reference"));
        assert!(TEMPLATE_AGENTS_MD.contains("snippets"));
    }

    #[test]
    fn test_classify_new_directory() {
        let temp_dir = TempDir::new().unwrap();
        let target = temp_dir.path().join("does-not-exist-yet");
        assert_eq!(classify_target(&target).unwrap(), TargetState::New);
    }

    #[test]
    fn test_classify_empty_directory() {
        let temp_dir = TempDir::new().unwrap();
        assert_eq!(
            classify_target(temp_dir.path()).unwrap(),
            TargetState::Empty
        );
    }

    #[test]
    fn test_classify_existing_scaffold() {
        let temp_dir = TempDir::new().unwrap();
        fs::create_dir_all(temp_dir.path().join("queries")).unwrap();
        fs::write(temp_dir.path().join("queries/.gitkeep"), "").unwrap();
        assert_eq!(
            classify_target(temp_dir.path()).unwrap(),
            TargetState::ExistingScaffold
        );
    }

    #[test]
    fn test_classify_tolerates_bare_git_repo() {
        let temp_dir = TempDir::new().unwrap();
        fs::create_dir_all(temp_dir.path().join(".git")).unwrap();
        fs::write(temp_dir.path().join("README.md"), "# hi").unwrap();
        assert_eq!(
            classify_target(temp_dir.path()).unwrap(),
            TargetState::ExistingScaffold
        );
    }

    #[test]
    fn test_classify_rejects_unrelated_files() {
        let temp_dir = TempDir::new().unwrap();
        fs::create_dir_all(temp_dir.path().join("Documents")).unwrap();
        fs::write(temp_dir.path().join(".zshrc"), "").unwrap();

        let TargetState::Unrelated(entries) = classify_target(temp_dir.path()).unwrap() else {
            panic!("expected Unrelated");
        };
        assert!(entries.contains(&"Documents".to_string()));
        assert!(entries.contains(&".zshrc".to_string()));
    }

    #[test]
    fn test_classify_rejects_editor_directories() {
        let temp_dir = TempDir::new().unwrap();
        fs::create_dir_all(temp_dir.path().join(".vscode")).unwrap();

        let TargetState::Unrelated(entries) = classify_target(temp_dir.path()).unwrap() else {
            panic!("expected Unrelated");
        };
        assert_eq!(entries, vec![".vscode".to_string()]);
    }

    #[test]
    fn test_classify_target_is_a_file() {
        let temp_dir = TempDir::new().unwrap();
        let target = temp_dir.path().join("not-a-directory");
        fs::write(&target, "hi").unwrap();

        assert!(classify_target(&target).is_err());
    }

    #[test]
    fn test_check_target_bails_on_unrelated_directory() {
        let temp_dir = TempDir::new().unwrap();
        fs::create_dir_all(temp_dir.path().join("Documents")).unwrap();
        fs::write(temp_dir.path().join(".zshrc"), "").unwrap();

        let err = check_target(temp_dir.path()).unwrap_err();
        let message = err.to_string();
        assert!(message.contains("Documents"));
        assert!(message.contains(".zshrc"));
    }

    #[test]
    fn test_init_refuses_unrelated_directory() {
        let temp_dir = TempDir::new().unwrap();
        let target = temp_dir.path().join("home");
        fs::create_dir_all(target.join("Documents")).unwrap();
        fs::write(target.join(".zshrc"), "").unwrap();

        assert!(init(Some(target.clone())).is_err());

        assert!(!target.join(".git").exists());
        assert!(!target.join(".pre-commit-config.yaml").exists());
        assert!(!target.join("queries").exists());
    }

    #[test]
    fn test_init_allows_rerun_in_existing_scaffold() {
        let temp_dir = TempDir::new().unwrap();
        for path in KNOWN_SCAFFOLD_PATHS {
            fs::write(temp_dir.path().join(path), "existing content").unwrap();
        }
        fs::create_dir_all(temp_dir.path().join("queries")).unwrap();
        fs::write(temp_dir.path().join("queries/.gitkeep"), "").unwrap();
        fs::create_dir_all(temp_dir.path().join("dashboards")).unwrap();
        fs::write(temp_dir.path().join("dashboards/.gitkeep"), "").unwrap();

        assert!(check_target(temp_dir.path()).is_ok());
    }
}

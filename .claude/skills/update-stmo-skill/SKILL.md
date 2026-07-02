---
name: update-stmo-skill
description: >
  Sync the vendored stmo Claude skill (.claude/skills/stmo/SKILL.md) with a
  stmo-cli change and prepare delivery to the mozilla-firefox/firefox repo via
  moz-phab. Use after a stmo-cli change (new command, flag, or workflow
  behavior) that the firefox-facing skill should reflect, or as part of
  preparing a stmo-cli release.
allowed-tools:
  - Bash(cargo test:*)
  - Bash(scripts/sync-firefox-skill.sh:*)
  - Bash(git log:*)
  - Bash(git diff:*)
  - Read
  - Edit
  - Grep
  - Glob
---

# update-stmo-skill

Keeps `.claude/skills/stmo/SKILL.md` (the vendored copy, source of truth for
the firefox-facing skill) in sync with stmo-cli, then hands off delivery to a
firefox checkout.

## Background

The command/flag catalog is deliberately **not** restated in `SKILL.md` — it
defers to `stmo-cli --help` (see the "Command reference" section at the top of
the skill), which is guarded by `src/main.rs`'s `llm_help_guard` tests against
the real clap definitions. Most stmo-cli changes (new flag, new subcommand,
tweaked default) therefore need **no** SKILL.md edit at all: fixing
`LLM_HELP` in `src/main.rs` is enough, and the guard enforces that it stays
complete.

`SKILL.md` only needs a manual edit when the change affects **workflow
narrative** that `--help` output can't convey — e.g. a new multi-step
procedure, a changed recommendation (working directory, cleanup rules,
export/analyze guidance), or a new gotcha worth calling out explicitly (like
the `schedule` and dynamic-date-token sections added when those features
shipped).

## Steps

1. **Identify what changed.** Read the relevant diff or changelog entry
   (`git log -p -- src/main.rs src/commands/`, `git diff <last-release>..HEAD`,
   or `CHANGELOG.md`) to understand the stmo-cli change.

2. **Decide if `SKILL.md` needs an edit.**
   - Flag/command surface change only → run `cargo test llm_help_guard` (or
     the full `cargo test`) to confirm `LLM_HELP` in `src/main.rs` already
     covers it. If the guard fails, fix `LLM_HELP`, not `SKILL.md`.
   - Workflow/behavior change → edit `.claude/skills/stmo/SKILL.md` directly.
     Keep edits narrative (what to do and why), not a restatement of flags —
     let `--help` stay the source of truth for those.

3. **Run the full pre-commit gate** before committing any `SKILL.md` or
   `src/main.rs` edit: `cargo test`, `cargo clippy --all-targets --all-features
   -- -W clippy::pedantic -D warnings`, `cargo fmt -- --check`.

4. **Commit the stmo-cli-side change** (if any) following this repo's normal
   commit discipline — one commit per logical change.

5. **Deliver to firefox**: run
   ```bash
   scripts/sync-firefox-skill.sh <path-to-local-firefox-checkout> [bug-number]
   ```
   This copies the vendored `SKILL.md`, stages it, and creates a commit in the
   firefox checkout. It does **not** run `moz-phab` — it prints the exact
   command to run next.

6. **Stop and hand off to the human** for the Phabricator submission — do not
   run `moz-phab submit` automatically. Report the printed `moz-phab submit`
   command and the reminder to set the `testing-exception-unchanged` Testing
   Policy project tag (this is a docs-only change).

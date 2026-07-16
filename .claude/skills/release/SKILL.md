---
name: release
description: >
  Cut a new stmo-cli release: curate the changelog, bump the version, open the
  release PR, tag it, and let CI publish the GitHub Release and crates.io
  package. Use when the user asks to release, cut a release, ship a new
  version, or publish stmo-cli.
allowed-tools:
  - Bash(cargo xtask:*)
  - Bash(git checkout:*)
  - Bash(git fetch:*)
  - Bash(git reset:*)
  - Bash(git tag:*)
  - Bash(git push:*)
  - Bash(cargo binstall:*)
  - Bash(scripts/sync-firefox-skill.sh:*)
  - Read
  - Grep
  - Glob
---

# release

Releases stmo-cli. Most of the work is scripted via the `xtask` crate
(`cargo xtask --help` for the full command list); this skill is the order of
operations and the parts that stay manual by design.

## Changelog

Only include user-facing changes. Internal changes (test fixes, code
formatting, dependency updates, CI improvements) should be omitted unless they
affect user-visible behavior. Group entries under `### Features` and
`### Fixes`.

Curate the `## [Unreleased]` section's content — this is a human judgment
call — before running `prepare-release` below. It only dates and renames the
heading; it does not compose the entries.

## Remotes

`origin` is the fork (`JohanLorenzo/stmo-cli-fork`); `upstream` is the
canonical repo (`mozilla/stmo-cli`). A repo-safety hook blocks direct pushes to
`upstream` for branches, so the PR flow below goes through the fork — but it
does **not** block pushing a signed tag, which is why tagging stays a manual,
deliberate step.

## Steps

1. `cargo xtask prepare-release X.Y.Z` — bumps `Cargo.toml`, dates the
   CHANGELOG `Unreleased` heading, runs the full gate (`cargo
   test`/`clippy`/`fmt --check`), and commits as `Release X.Y.Z` on a new
   `release-X.Y.Z` branch. Requires a clean tree on `main` synced with
   `upstream/main`.
2. `cargo xtask cut-release X.Y.Z` — pushes `release-X.Y.Z` to `origin` and
   opens a **draft** PR against `mozilla/stmo-cli` `main`. Review and merge
   it.
3. After merge, sync `main` and cut the signed tag yourself:
   ```
   git checkout main && git fetch upstream && git reset --hard upstream/main
   git tag -s X.Y.Z -m "X.Y.Z"
   git push upstream X.Y.Z
   ```
4. CI (`release.yml`) takes over from the tag: it validates the tag looks
   like a version, creates the GitHub Release with notes extracted straight
   from `CHANGELOG.md` (`cargo xtask extract-changelog X.Y.Z` — no manual
   `gh release edit --notes`), builds and attaches all 6 target binaries,
   then publishes to crates.io via Trusted Publishing (OIDC, no stored
   token).
5. Verify: `cargo binstall --dry-run stmo-cli`.
6. **Only if this release changed a user-facing command, flag, or workflow:**
   sync the firefox skill — run `scripts/sync-firefox-skill.sh
   <firefox-checkout> [bug-number]` (or invoke the `update-stmo-skill` skill)
   to update both `mozilla-firefox/firefox/.claude/skills/stmo/SKILL.md` and
   `mozilla-firefox/firefox/.agents/skills/stmo/SKILL.md` (firefox mirrors
   the two and enforces they match via its `agent-skills-sync` linter) and
   prepare a moz-phab submission. See `.claude/skills/stmo/SKILL.md`
   (vendored canonical copy) and `.claude/skills/update-stmo-skill/SKILL.md`.

## One-time setup

The crates.io publish step requires a Trusted Publisher configured for the
`stmo-cli` crate (crates.io → package settings → Trusted Publishing → GitHub
repo `mozilla/stmo-cli`, workflow `release.yml`). If it's missing or
misconfigured, the `publish` job fails but the GitHub Release and binaries
from the earlier jobs still stand — fall back to a local `cargo publish` and
fix the Trusted Publisher config before the next release.

## Re-running a tag

crates.io versions are immutable, so re-pushing an already-published tag
makes `cargo publish` fail as expected; the GitHub Release and binaries still
regenerate cleanly.

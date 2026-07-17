---
name: release
description: >
  Cut a new stmo-cli release: curate the changelog, bump the version, open the
  release PR, tag it, let CI publish the GitHub Release with binaries, then
  publish to crates.io by hand. Use when the user asks to release, cut a
  release, ship a new version, or publish stmo-cli.
allowed-tools:
  - Bash(cargo xtask:*)
  - Bash(git checkout:*)
  - Bash(git fetch:*)
  - Bash(git reset:*)
  - Bash(git tag:*)
  - Bash(git push:*)
  - Bash(cargo publish:*)
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
canonical repo (`mozilla/stmo-cli`). A repo-safety hook blocks **any** push to
`upstream` — branch or tag — so the PR flow below goes through the fork, and
tagging (step 3) is a manual, human-only step for the same reason.

## Steps

1. `cargo xtask prepare-release X.Y.Z` — bumps `Cargo.toml`, dates the
   CHANGELOG `Unreleased` heading, runs the full gate (`cargo
   test`/`clippy`/`fmt --check`), and commits as `Release X.Y.Z` on a new
   `release-X.Y.Z` branch. Requires a clean tree on `main` synced with
   `upstream/main`.
2. `cargo xtask cut-release X.Y.Z` — pushes `release-X.Y.Z` to `origin` and
   opens a **draft** PR against `mozilla/stmo-cli` `main`. Review and merge
   it.
3. **You run this step, not Claude/the skill.** After the PR merges, sync
   `main` and cut the signed tag yourself:
   ```
   git checkout main && git fetch upstream && git reset --hard upstream/main
   git tag -s X.Y.Z -m "X.Y.Z"
   git push upstream X.Y.Z
   ```
   Two separate hard blockers, not a style choice:
   - **Signing** needs your GPG key and an interactive passphrase/pinentry
     prompt — Claude has no way to answer that prompt.
   - **Pushing to `upstream`** is refused outright by the local
     `check_push_target.py` safety hook, which blocks any push to a
     non-fork remote — tag or branch — regardless of who's asking.
4. CI (`release.yml`) takes over from the tag: it validates the tag looks
   like a version, creates the GitHub Release with notes extracted straight
   from `CHANGELOG.md` (`cargo xtask extract-changelog X.Y.Z` — no manual
   `gh release edit --notes`), and builds and attaches all 6 target binaries.
   It does **not** publish to crates.io — see the note below.
5. Publish to crates.io yourself: `cargo publish -p stmo-cli`.
6. Verify: `cargo binstall --dry-run stmo-cli`.
7. **Only if this release changed a user-facing command, flag, or workflow:**
   sync the firefox skill — run `scripts/sync-firefox-skill.sh
   <firefox-checkout> [bug-number]` (or invoke the `update-stmo-skill` skill)
   to update both `mozilla-firefox/firefox/.claude/skills/stmo/SKILL.md` and
   `mozilla-firefox/firefox/.agents/skills/stmo/SKILL.md` (firefox mirrors
   the two and enforces they match via its `agent-skills-sync` linter) and
   prepare a moz-phab submission. See `.claude/skills/stmo/SKILL.md`
   (vendored canonical copy) and `.claude/skills/update-stmo-skill/SKILL.md`.

## Why crates.io publish isn't in CI

It was, briefly (`softprops/action-gh-release` + `rust-lang/crates-io-auth-action`
via OIDC Trusted Publishing), but Mozilla's GitHub org enforces an actions
allowlist ([MoCo-GHE-Admin/Approved-GHE-add-ons][allowlist]) and neither
action is on it. A tag push with either in the workflow fails the whole run
at `startup_failure` before any job even starts — GitHub-first-party actions
(`actions/*`) and the pre-approved `dtolnay/rust-toolchain` /
`Swatinem/rust-cache` are fine, but third-party marketplace actions need a
[Mozilla security review][bug] first. `release.yml` now uses the `gh` CLI
directly (preinstalled on all runner images, authenticated via the built-in
`GITHUB_TOKEN`) for release creation and asset upload instead — that's a
CLI invocation, not a `uses:` action, so it isn't subject to the allowlist.
crates.io has no such CLI-only path for Trusted Publishing, so it stays a
manual `cargo publish` step.

[allowlist]: https://github.com/MoCo-GHE-Admin/Approved-GHE-add-ons/blob/main/GitHub_Actions.md
[bug]: https://bugzilla.mozilla.org/enter_bug.cgi?product=mozilla.org&component=Github%3A%20Administration

## Re-running a tag

crates.io versions are immutable, so re-running `cargo publish` for an
already-published version fails as expected; the GitHub Release and binaries
still regenerate cleanly from a re-tagged commit.

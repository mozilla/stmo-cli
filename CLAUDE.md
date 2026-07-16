# stmo-cli

Redash CLI that gives Claude Code direct access to sql.telemetry.mozilla.org — explore data sources, write and run queries, and deploy dashboards.

## Quick Reference

**Commands**: `discover [--search TEXT] [--limit N]` `init` `fetch` `deploy` `execute [ID] [--data-source ID [--file PATH|-]]` `data-sources` `archive` `unarchive` `dashboards` `schedule` `snippets`
**Env Vars**: `REDASH_API_KEY` (required), `REDASH_URL` (optional, defaults to sql.telemetry.mozilla.org)

## Key Constraints

- All clippy::pedantic warnings = errors (pre-commit enforced)
- `cargo fmt` must pass (enforced pre-commit and in CI)
- No docstrings (user preference)
- Borrowed strings preferred (`&str` vs `String`)
- Break complex logic into well-named functions
- Error handling via `anyhow`
- No redundant comments (clear naming instead)

## Project Structure

```
src/
├── main.rs              # CLI entry point with clap
├── lib.rs               # Library exports for testing
├── api.rs               # Redash API client
├── models.rs            # Data structures
└── commands/
    ├── mod.rs           # OutputFormat enum
    ├── discover.rs      # List own queries or full-text search queries + dashboards
    ├── init.rs          # Create directory
    ├── fetch.rs         # Download queries, slugify()
    ├── deploy.rs        # Upload changes
    ├── execute.rs       # Execute queries
    ├── dynamic_dates.rs # Resolve d_* date/range tokens client-side (port of Redash frontend)
    ├── datasources.rs   # List/explore data sources
    ├── archive.rs       # Archive/unarchive queries
    ├── schedule.rs      # Set/clear query refresh schedules (local YAML only; deploy to push)
    ├── dashboards.rs    # Dashboard management
    └── snippets.rs      # Query snippet management (no archive concept — delete is direct)
```

## Data Models

**Query**: Full Redash query (id, name, sql, data_source_id, options.parameters, visualizations, schedule, user)
**QueryMetadata**: YAML variant (excludes user, uses user_id; visualizations use VisualizationMetadata)
**VisualizationMetadata**: Like Visualization but id is `Option<u64>` — null/absent means create new
**CreateQuery**: For query id=0 workflow (new query creation)
**DataSource**: id, name, ds_type, syntax, paused, view_only
**JobStatus**: Pending=1, Started=2, Success=3, Failure=4, Cancelled=5
**QuerySnippet**: Full Redash snippet (id, trigger, description, snippet, user, timestamps)
**SnippetMetadata**: YAML variant (id, trigger, description only — no user/timestamps)
**CreateQuerySnippet**: For snippet id=0 workflow (new snippet creation)

## API Client (api.rs)

**Query**: list_my_queries, get_query, fetch_all_queries, create_query, create_or_update_query
**Search**: search_queries(q, limit), search_dashboards(q, limit); `base_url()` accessor
**Visualization**: create_visualization, update_visualization
**Execution**: refresh_query, poll_job, get_query_result, execute_query_with_polling, refresh_adhoc_query, get_adhoc_query_result, execute_adhoc_with_polling
**Data Source**: list_data_sources, get_data_source, get_data_source_schema
**Archive**: archive_query, unarchive_query
**Widget**: create_widget, update_widget, delete_widget
**Query Snippet**: list_query_snippets, get_query_snippet, create_query_snippet, update_query_snippet, delete_query_snippet
**HTTP**: get_json, post_json
**Errors**: ensure_success

## Testing

**Run**: `cargo test` (wiremock for HTTP mocking)
**Locations**: tests/api_integration.rs, src/models.rs, src/commands/*.rs

## Testing Guidelines

### Test Isolation
Tests must NEVER touch production directories (`queries/`, `dashboards/`). Use `tempfile::TempDir`:

```rust
use tempfile::TempDir;

#[test]
fn test_something() {
    let temp_dir = TempDir::new().unwrap();
    // Use temp_dir.path() for all file operations
}
```

For integration tests needing current directory context, use mutex + TempWorkDir pattern (see `tests/dashboard_commands.rs`).

### API Error Handling
Route every response through the `ensure_success` helper in `api.rs`. It returns the
response on success and bails with a uniform `API error {status}: {body}` message
(status `Display` already includes the canonical reason, e.g. `404 Not Found`) on
failure, which helps debugging:

```rust
let response = ensure_success(response).await?;
```

For endpoints that return JSON, achieve this by using `get_json<T>` / `post_json<T, B>`
helpers: they send the request, route it through `ensure_success`, and parse the body
into `T`. The last parameter `ctx` is used for request and parse failure messages
(`Failed to request {ctx}` / `Failed to parse {ctx} response`):

```rust
let response: crate::models::MyResponse =
  self.get_json(&url, "my query").await?;

let request: crate::models::MyRequest = ...;
let response: crate::models::MyResponse =
  self.post_json(&url, &request, "my query").await?;
```

For wrapper responses, parse into the wrapper type then return the inner field. Endpoints
returning `()` (no body) or needing retries use `ensure_success` / `get_with_retry` directly.

## Redash API Development

**IMPORTANT: Always verify before planning**

1. **Test endpoints exist** - Don't assume an endpoint exists because a similar one does. Test with curl:
   ```bash
   curl -s -w "%{http_code}" "https://sql.telemetry.mozilla.org/api/<endpoint>" \
     -H "Authorization: Key ${REDASH_API_KEY}"
   ```

2. **Check actual response fields** - The models may not match what the API returns. Inspect raw JSON:
   ```bash
   curl -s "https://sql.telemetry.mozilla.org/api/<endpoint>?page=1&page_size=1" \
     -H "Authorization: Key ${REDASH_API_KEY}" | jq '.results[0]'
   ```

3. **Verify filter/mutation support** - A field in responses doesn't mean you can filter by it or set it via API. Test POST/query params explicitly.

4. **STMO may differ from upstream** - Mozilla's instance may have endpoints disabled or behave differently than Redash documentation suggests.

## Releasing

### Changelog

Only include user-facing changes. Internal changes (test fixes, code formatting, dependency
updates, CI improvements) should be omitted unless they affect user-visible behavior.
Group entries under `### Features` and `### Fixes`.

Curate the `## [Unreleased]` section's content (this is a human judgment call) before
running `prepare-release` below — it only dates and renames the heading, it does not
compose the entries.

### Steps

Release tooling lives in the `xtask` crate (`cargo xtask --help` for the full list).
`origin` is the fork (`JohanLorenzo/stmo-cli-fork`); `upstream` is the canonical repo
(`mozilla/stmo-cli`). A repo-safety hook blocks direct pushes to `upstream` for
branches, so the PR flow below goes through the fork — but it does **not** block
pushing a signed tag, which is why tagging stays a manual, deliberate step.

1. `cargo xtask prepare-release X.Y.Z` — bumps `Cargo.toml`, dates the CHANGELOG
   `Unreleased` heading, runs the full gate (`cargo test`/`clippy`/`fmt --check`), and
   commits as `Release X.Y.Z` on a new `release-X.Y.Z` branch. Requires a clean tree on
   `main` synced with `upstream/main`.
2. `cargo xtask cut-release X.Y.Z` — pushes `release-X.Y.Z` to `origin` and opens a
   **draft** PR against `mozilla/stmo-cli` `main`. Review and merge it.
3. After merge, sync `main` and cut the signed tag yourself:
   ```
   git checkout main && git fetch upstream && git reset --hard upstream/main
   git tag -s X.Y.Z -m "X.Y.Z"
   git push upstream X.Y.Z
   ```
4. CI (`release.yml`) takes over from the tag: it validates the tag looks like a
   version, creates the GitHub Release with notes extracted straight from
   `CHANGELOG.md` (`cargo xtask extract-changelog X.Y.Z` — no more manual
   `gh release edit --notes`), builds and attaches all 6 target binaries, then
   publishes to crates.io via Trusted Publishing (OIDC, no stored token).
5. Verify: `cargo binstall --dry-run stmo-cli`.
6. **Only if this release changed a user-facing command, flag, or workflow:** sync the
   firefox skill — run `scripts/sync-firefox-skill.sh <firefox-checkout> [bug-number]`
   (or invoke the `update-stmo-skill` skill) to update both
   `mozilla-firefox/firefox/.claude/skills/stmo/SKILL.md` and
   `mozilla-firefox/firefox/.agents/skills/stmo/SKILL.md` (firefox mirrors the two and
   enforces they match via its `agent-skills-sync` linter) and prepare a moz-phab
   submission. See `.claude/skills/stmo/SKILL.md` (vendored canonical copy) and
   `.claude/skills/update-stmo-skill/SKILL.md`.

**One-time setup:** the crates.io publish step requires a Trusted Publisher configured
for the `stmo-cli` crate (crates.io → package settings → Trusted Publishing → GitHub
repo `mozilla/stmo-cli`, workflow `release.yml`). If it's missing or misconfigured, the
`publish` job fails but the GitHub Release and binaries from the earlier jobs still
stand — fall back to a local `cargo publish` and fix the Trusted Publisher config
before the next release.

**Re-running a tag:** crates.io versions are immutable, so re-pushing an
already-published tag makes `cargo publish` fail as expected; the GitHub Release and
binaries still regenerate cleanly.

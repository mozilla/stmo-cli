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

### Steps

1. Update `CHANGELOG.md` with a new version section
2. Bump `version` in `Cargo.toml`
3. Commit as `Release X.Y.Z`
4. Push to main: `git push origin main`
5. Create signed tag: `git tag -s X.Y.Z -m "X.Y.Z"` (tag message is the bare version)
6. Push tag: `git push origin X.Y.Z`
7. Wait for release workflow to create GitHub Release with binaries
8. Add changelog to release: `gh release edit X.Y.Z --notes "..."`
9. Publish to crates.io: `cargo publish`
10. Verify: `cargo binstall --dry-run stmo-cli`
11. Sync the firefox skill: run `scripts/sync-firefox-skill.sh <firefox-checkout> [bug-number]`
    (or invoke the `update-stmo-skill` skill) to update
    `mozilla-firefox/firefox/.claude/skills/stmo/SKILL.md` and prepare a moz-phab
    submission. See `.claude/skills/stmo/SKILL.md` (vendored canonical copy) and
    `.claude/skills/update-stmo-skill/SKILL.md`.

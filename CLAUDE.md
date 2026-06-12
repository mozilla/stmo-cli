# stmo-cli

Redash CLI that gives Claude Code direct access to sql.telemetry.mozilla.org — explore data sources, write and run queries, and deploy dashboards.

## Quick Reference

**Commands**: `discover [--search TEXT] [--limit N]` `init` `fetch` `deploy` `execute` `data-sources` `archive` `unarchive` `dashboards`
**Env Vars**: `REDASH_API_KEY` (required), `REDASH_URL` (optional, defaults to sql.telemetry.mozilla.org)

## Key Constraints

- All clippy::pedantic warnings = errors (pre-commit enforced)
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
    ├── datasources.rs   # List/explore data sources
    ├── archive.rs       # Archive/unarchive queries
    └── dashboards.rs    # Dashboard management
```

## Data Models

**Query**: Full Redash query (id, name, sql, data_source_id, options.parameters, visualizations, schedule, user)
**QueryMetadata**: YAML variant (excludes user, uses user_id; visualizations use VisualizationMetadata)
**VisualizationMetadata**: Like Visualization but id is `Option<u64>` — null/absent means create new
**CreateQuery**: For query id=0 workflow (new query creation)
**DataSource**: id, name, ds_type, syntax, paused, view_only
**JobStatus**: Pending=1, Started=2, Success=3, Failure=4, Cancelled=5

## API Client (api.rs)

**Query**: list_my_queries, get_query, fetch_all_queries, create_query, create_or_update_query
**Search**: search_queries(q, limit), search_dashboards(q, limit); `base_url()` accessor
**Visualization**: create_visualization, update_visualization
**Execution**: refresh_query, poll_job, get_query_result, execute_query_with_polling
**Data Source**: list_data_sources, get_data_source, get_data_source_schema
**Archive**: archive_query, unarchive_query
**Widget**: create_widget, update_widget, delete_widget

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
Don't use `.error_for_status()` - it discards the response body. Instead:

```rust
let status = response.status();
if !status.is_success() {
    let error_body = response.text().await.unwrap_or_default();
    anyhow::bail!("API error {status}: {error_body}");
}
```

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

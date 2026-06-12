# Redash Version Control Repository

This repository contains version-controlled Redash queries and dashboards managed by `stmo-cli`.

## Quick Reference

**Install**: `cargo install stmo-cli`
**Commands**: `discover [--search TEXT] [--limit N]` `fetch` `deploy` `execute` `data-sources` `archive` `unarchive` `dashboards`
**File Naming**: `queries/{id}-{slug}.sql` + `queries/{id}-{slug}.yaml`, `dashboards/{id}-{slug}.yaml`
**Env Vars**: `REDASH_API_KEY` (required), `REDASH_URL` (optional, defaults to sql.telemetry.mozilla.org)

## Data Exploration (AI Assistants)

**Setup** (first time only): Install the [mozdata-claude-plugin](https://github.com/akkomar/mozdata-claude-plugin?tab=readme-ov-file#installation) for Mozilla telemetry expertise and discovery.

**IMPORTANT**: Clean up after exploration. Archive any queries you fetch.

1. **Find data sources**: `stmo-cli data-sources`
2. **Explore schema**: `stmo-cli data-sources <id> --schema`
3. **Discover queries**: `stmo-cli discover --search "<keyword>"` (or bare `stmo-cli discover` to list your own queries)
4. **Fetch query**: `stmo-cli fetch <id>` → read `queries/<id>-*.sql`
5. **Execute**: `stmo-cli execute <id> --format table`
6. **Clean up**: `stmo-cli archive <id>` (MANDATORY)

To restore: `stmo-cli unarchive <id> && stmo-cli fetch <id>`

## Commands

### Queries
**discover**: List your own queries (IDs + names). With `--search TEXT` (short: `-q`), performs a full-text search across all queries and dashboards; `--limit N` caps results per section (default 50)
**fetch**: Download queries (`--all` for tracked, or `<ids>`)
**deploy**: Upload changes (no args = git-changed files only or all if not in a git repo, `--all` for everything, or `<ids>`)
**execute**: Run query (`--param key=val`, `--format table|json`, `--interactive`)
**data-sources**: List sources, `<id> --schema` for tables
**archive**: Archive queries + delete local (`<ids>` or `--cleanup`)
**unarchive**: Restore archived queries (`<ids>`)

### Dashboards
**dashboards discover**: List your favorite dashboards (IDs + names + slugs)
**dashboards fetch**: Download dashboards (`<slugs>`)
**dashboards deploy**: Upload changes (`--all` for everything, or `<slugs>`)
**dashboards archive**: Archive dashboards + delete local (`<slugs>`)
**dashboards unarchive**: Restore archived dashboards (`<slugs>`)

**Note**: Only dashboards you've favorited in the Redash web UI will appear in `dashboards discover`.

Examples:
- `stmo-cli dashboards fetch firefox-desktop-on-steamos`
- `stmo-cli dashboards deploy --all`
- `stmo-cli dashboards archive bug-2006698---ccov-build-regression`

## File Format

**SQL**: `queries/{id}-{slug}.sql` - query text
**YAML**: `queries/{id}-{slug}.yaml` - metadata (name, data_source_id, parameters, visualizations)

Example YAML with visualizations:
```yaml
id: 123
name: My Query
data_source_id: 63
options:
  parameters: []
visualizations:
  - id: 456          # existing visualization — deploy updates it by ID
    name: Chart
    type: CHART
    options: {}
    description: null
  - name: New Chart  # no id — deploy creates it as a new visualization
    type: CHART
    options: {}
    description: null
```

To add a visualization to an existing query: add an entry to the `visualizations` list **without** an `id` field (or `id: null`). Do not change the `id` of existing visualizations — they will be updated in place.

## SQL Style

sqlfluff (BigQuery) enforced via pre-commit. Match existing `queries/*.sql` formatting.

## Query Creation

1. Create `0-{slug}.sql` + `0-{slug}.yaml` with `id: 0`
2. `stmo-cli deploy` → creates query in Redash and auto-renames local files to `{new-id}-{slug}.*`
3. Commit the renamed `{new-id}-{slug}.*` files — **never commit `0-*.` files**

## Dashboard Creation

1. Create `dashboards/0-{slug}.yaml` with `id: 0`
2. `stmo-cli dashboards deploy {slug}` → creates dashboard in Redash
3. Local file is automatically renamed to `{new-id}-{server-slug}.yaml`

## Query/Dashboard Authoring

### Before Deploying SQL
Run `pre-commit run sqlfluff-lint --all-files` to catch formatting issues early (lowercase identifiers, proper indentation, etc).

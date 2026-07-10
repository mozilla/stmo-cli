# Redash Version Control Repository

This repository contains version-controlled Redash queries and dashboards managed by `stmo-cli`.

## Quick Reference

**Install**: `cargo install stmo-cli`
**Commands**: `discover [--search TEXT] [--limit N]` `fetch` `deploy` `execute [ID] [--data-source ID [--file PATH|-]]` `data-sources` `archive` `unarchive` `dashboards` `schedule` `snippets`
**File Naming**: `queries/{id}-{slug}.sql` + `queries/{id}-{slug}.yaml`, `dashboards/{id}-{slug}.yaml`, `snippets/{id}-{trigger}.sql` + `snippets/{id}-{trigger}.yaml`
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

**Scratch/exploratory SQL that doesn't need to be a tracked query**: skip fetch/execute/archive
entirely — `echo "SELECT ..." | stmo-cli execute --data-source <id>` runs arbitrary SQL
directly against a data source and creates no query, so there's nothing to clean up. Has no
parameter schema, so inline any values directly in the SQL.

## Commands

### Queries
**discover**: List your own queries (IDs + names). With `--search TEXT` (short: `-q`), performs a full-text search across all queries and dashboards; `--limit N` caps results per section (default 50)
**fetch**: Download queries (`--all` for tracked, or `<ids>`)
**deploy**: Upload changes (no args = git-changed files only or all if not in a git repo, `--all` for everything, or `<ids>`)
**execute**: Run a tracked query by ID (deploys local changes first if they differ from the server); `--param key=val`, `--format table|json`, `--interactive`
**execute --data-source ID**: Run ad-hoc SQL (stdin or `--file PATH`) against a data source, creating no tracked query — no parameter schema, so inline values directly in the SQL
**data-sources**: List sources, `<id> --schema` for tables
**archive**: Archive queries + delete local (`<ids>` or `--cleanup`)
**unarchive**: Restore archived queries (`<ids>`)
**schedule**: Set or clear a query's refresh schedule in the local YAML (`--interval SECS [--time HH:MM] [--day-of-week N]` or `--clear`); run `deploy` afterwards to push to Redash

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

### Snippets

Redash [query snippets](https://redash.io/help/user-guide/querying/query-snippets) are reusable
SQL fragments (e.g. a shared set of CTEs) that get pasted into a query's editor by typing their
`trigger`. Pasting is a one-time text expansion, not a live reference — editing a snippet later
does **not** update queries that already pasted it; re-paste (or hand-edit) each query after
changing a snippet it's used in.

**snippets list**: List query snippets from Redash (id, trigger, description)
**snippets fetch**: Download snippets (`--all` for tracked, or `<ids>`)
**snippets deploy**: Upload changes (no args = git-changed files only or all if not in a git repo, `--all` for everything, or `<ids>`)
**snippets delete**: Delete snippets in Redash **and** remove local files (`<ids>`) — snippets have no archive concept, so this is a direct, irreversible delete (unlike `archive`, which keeps the resource recoverable via `unarchive`)

Examples:
- `stmo-cli snippets fetch 31`
- `stmo-cli snippets deploy --all`
- `stmo-cli snippets delete 31 42`

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

**Snippets**: `snippets/{id}-{trigger}.sql` - the fragment body. This is a bare list of CTEs
(no leading `WITH`, no trailing `SELECT`) meant to be pasted into a larger query, so it is
**not standalone SQL** — it can't be run or linted on its own; see "Snippet SQL Style" below.
**Snippets YAML**: `snippets/{id}-{trigger}.yaml` - metadata (`id`, `trigger`, `description`)

## SQL Style

sqlfluff (BigQuery) enforced via pre-commit. Match existing `queries/*.sql` formatting.

### Snippet SQL Style

Snippet fragments can't be parsed standalone (see above), so the normal `sqlfluff-lint` hook
excludes `snippets/`. If this repo's `.pre-commit-config.yaml` includes the `sqlfluff-lint-snippets`
hook (from the `stmo-cli` snippet-support template), it lints each `snippets/*.sql` file by
temporarily wrapping it in `WITH ... SELECT 1` before running sqlfluff, then maps errors back to
the original file — real style/structure feedback without needing a full standalone query.

## Query Creation

1. Create `0-{slug}.sql` + `0-{slug}.yaml` with `id: 0`
2. `stmo-cli deploy` → creates query in Redash and auto-renames local files to `{new-id}-{slug}.*`
3. Commit the renamed `{new-id}-{slug}.*` files — **never commit `0-*.` files**

## Dashboard Creation

1. Create `dashboards/0-{slug}.yaml` with `id: 0`
2. `stmo-cli dashboards deploy {slug}` → creates dashboard in Redash
3. Local file is automatically renamed to `{new-id}-{server-slug}.yaml`

## Snippet Creation

1. Create `snippets/0-{trigger}.sql` + `snippets/0-{trigger}.yaml` with `id: 0`
2. `stmo-cli snippets deploy 0` → creates the snippet in Redash and auto-renames local files to `{new-id}-{trigger}.*`
3. Commit the renamed `{new-id}-{trigger}.*` files — **never commit `0-*.` files**

## Query/Dashboard/Snippet Authoring

### Before Deploying SQL
Run `pre-commit run sqlfluff-lint --all-files` to catch formatting issues early (lowercase identifiers, proper indentation, etc). This also runs `sqlfluff-lint-snippets` if configured (see "Snippet SQL Style" above).

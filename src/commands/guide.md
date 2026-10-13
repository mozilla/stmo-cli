# stmo-cli Redash Workflow

`stmo-cli` manages Redash queries and dashboards on sql.telemetry.mozilla.org. It works from any directory, not only a repository.
Optional for Claude Code users: install the [mozdata Claude plugin](https://github.com/akkomar/mozdata-claude-plugin?tab=readme-ov-file#installation) for Mozilla telemetry expertise and discovery.

## Data Exploration

**IMPORTANT**: Clean up after exploration. Archive any query you fetch.

1. Find data sources: `stmo-cli data-sources`
2. Explore schema: `stmo-cli data-sources <id> --schema`
3. Discover queries: `stmo-cli discover --search "<keyword>"` (or bare `stmo-cli discover` to list your own)
4. Fetch a query, then read `queries/<id>-*.sql`: `stmo-cli fetch <id>`
5. Execute: `stmo-cli execute <id> --format table`
6. Clean up (MANDATORY): `stmo-cli archive <id>`

To restore: `stmo-cli unarchive <id>`, then `stmo-cli fetch <id>`.

Scratch SQL that doesn't need to be a tracked query: skip fetch, execute, and archive. Pipe the SQL to `stmo-cli execute --data-source <id>`, which runs it directly and creates no query. Inline any values in the SQL, because ad-hoc runs take no parameters.

## File Format

- `queries/{id}-{slug}.sql` holds the query text. `queries/{id}-{slug}.yaml` holds the metadata: name, data_source_id, parameters, visualizations.
- `dashboards/{id}-{slug}.yaml` holds a dashboard.
- `snippets/{id}-{trigger}.sql` holds a snippet body, and `snippets/{id}-{trigger}.yaml` its metadata (`id`, `trigger`, `description`).

Example query YAML:

```yaml
id: 1200000001
name: My Query
data_source_id: 1500000001
options:
  parameters: []
visualizations:
  - id: 1300000001   # existing visualization, deploy updates it by ID
    name: Chart
    type: CHART
    options: {}
    description: null
  - name: New Chart  # no id, deploy creates it
    type: CHART
    options: {}
    description: null
```

- To add a visualization, add an entry **without** `id` (or with `id: null`). Never change the `id` of an existing visualization.
- `enumOptions`: use YAML multiline (`|-`), not escaped `\n`, or deploy fails.

## Creating Resources

Queries:
1. Create `0-{slug}.sql` and `0-{slug}.yaml` with `id: 0`.
2. `stmo-cli deploy` creates the query in Redash and renames the files to `{new-id}-{slug}.*`.
3. If the repo is under version control, commit the renamed files. Never commit `0-*` files.

Dashboards:
1. Create `dashboards/0-{slug}.yaml` with `id: 0`.
2. `stmo-cli dashboards deploy {slug}` creates the dashboard in Redash.
3. The local file is renamed to `{new-id}-{server-slug}.yaml`.

Snippets:
1. Create `snippets/0-{trigger}.sql` and `snippets/0-{trigger}.yaml` with `id: 0`.
2. `stmo-cli snippets deploy 0` creates the snippet and renames the files to `{new-id}-{trigger}.*`.
3. If the repo is under version control, commit the renamed files. Never commit `0-*` files.

## Snippets

Redash query snippets are reusable SQL fragments pasted into a query by typing their `trigger`. Pasting is a one-time text expansion. Editing a snippet does **not** update queries that already pasted it, so re-paste or hand-edit each affected query.

Snippet files are bare CTE lists with no leading `WITH` and no trailing `SELECT`. They cannot run on their own.

`snippets delete` removes the snippet from Redash and deletes the local files. Snippets have no archive, so this is irreversible.

## SQL Style

sqlfluff (BigQuery). Match the formatting of existing `queries/*.sql`.

If the repo has a `.pre-commit-config.yaml` (set up by `stmo-cli init` when you opt in), run `pre-commit run sqlfluff-lint --all-files` before deploying. If it includes `sqlfluff-lint-snippets`, that hook also lints snippet files by wrapping each in `WITH ... SELECT 1`.

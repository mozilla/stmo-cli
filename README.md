# stmo-cli

## How it works

stmo-cli is a CLI that AI coding assistants such as Claude Code and Codex can use on
your behalf. Install it and set your API key. It can then help you:

- **Explore** — discover data sources, find existing queries, browse schemas
- **Write** — create new Redash queries with proper BigQuery SQL
- **Deploy** — push queries, charts, and dashboards to STMO
- **Execute** — run queries and inspect results
- **Analyze** — export data for deeper analysis with other tools

For example, ask Claude Code or Codex to:
- "Find queries about Firefox DAU"
- "Write a query to track [metric] over time"
- "Fetch and run query #12345"
- "Explore what telemetry tables are available"

Claude Code users can pair it with the [mozdata plugin](https://github.com/mozilla/internal-aidev-plugins/tree/main/plugins/mozdata) for telemetry expertise and probe discovery. Codex users can use `stmo-cli` directly.

## Prerequisites

- Redash API key from https://sql.telemetry.mozilla.org

## Installation

### For people who already have the Firefox source stored locally

```
cd /path/to/your/firefox/source/folder
./mach bootstrap
```

### For anyone else
Install [cargo-binstall](https://docs.rs/crate/cargo-binstall/latest).

```
cargo binstall stmo-cli
```

### Build from source:

```bash
cargo build --release
# The binary will be at ./target/release/stmo-cli
```

## Setup

1. Get your Redash API key from your user profile

2. Provide the key:

**On macOS**, run this once in your own terminal; the command needs an interactive
prompt:
```bash
stmo-cli login
```
This stores the key encrypted in the macOS Keychain (service `stmo-cli`) and reads it
back automatically from then on — every assistant session, every worktree, no env var,
no manual export. The first `stmo-cli login` (or the first command run with no key set,
if you're in a terminal) prompts you for the key with a hidden `security` prompt; grant
"Always Allow" once when macOS asks, and later reads are silent.

**On other platforms**, or if you'd rather manage it yourself, set the environment
variable directly:
```bash
export REDASH_API_KEY="your-api-key-here"
export REDASH_URL="https://sql.telemetry.mozilla.org"  # optional, this is the default
```
`REDASH_API_KEY` always takes precedence over the Keychain when set.

For Mozilla, the key can be accessed via the following URL: https://sql.telemetry.mozilla.org/users/me

3. Create directories:
```bash
stmo-cli init                  # scaffolds the current directory
stmo-cli init ~/stmo-queries   # or scaffolds a directory of your choice (created if missing)
```
`init` refuses to scaffold into a directory that already contains unrelated files (e.g.
your home directory) — pick an empty or dedicated subdirectory instead.

`init` is an interactive wizard — it needs a real terminal, and asks before touching
anything:
```
? Initialize a git repository?           [y/N]
? Create an initial commit?               [y/N]   (only asked if you said yes above)
? Add linter configs (.sqlfluff, .yamllint)?  [Y/n]
? Install pre-commit hooks?                [y/N]   (only asked if git + linters + pre-commit are all available)
? Add AGENTS.md for AI assistants?          [Y/n]
```
`queries/` and `dashboards/` are always created; everything else is opt-in. `init` never
runs git on your behalf unless you say yes — no more surprise commits.

4. Discover available queries:
```bash
stmo-cli discover                          # List your own queries
stmo-cli discover --search "firefox dau"   # Full-text search queries + dashboards
```

5. Fetch specific queries:
```bash
stmo-cli fetch 123 456 789
```

## Usage

### Fetch Queries from Redash

```bash
stmo-cli fetch --all                       # Fetch all tracked queries
stmo-cli fetch 123 456 789                 # Fetch specific queries
stmo-cli discover                          # List your own queries
stmo-cli discover --search "firefox dau"   # Full-text search queries + dashboards (--limit, default 50)
```

This creates/updates:
- `queries/{id}-{slug}.sql` - Query SQL
- `queries/{id}-{slug}.yaml` - Query metadata (parameters, visualizations, etc.)

### Deploy to Redash

```bash
stmo-cli deploy       # Deploy queries whose local content differs from what's on Redash
stmo-cli deploy --all # Deploy all queries regardless of differences
```

Bare `deploy` compares each tracked query's local `.sql`/`.yaml` against the server before
pushing — no git required, and re-running it after a successful deploy pushes nothing.

**Warning**: This force overwrites the queries in Redash. Your local files are the source
of truth for what gets pushed; putting them under version control is optional but
recommended (`stmo-cli init` can set that up for you).

### Execute Queries

```bash
stmo-cli execute 123                                       # Run query 123 (deploys local changes first, if any)
stmo-cli execute 123 --param start_date=2026-06-15
stmo-cli execute 123 --param channels='["release","beta"]' # Multi-value enum as JSON
stmo-cli data-sources                                      # List data sources
echo 'SELECT 1' | stmo-cli execute --data-source 321       # Run arbitrary SQL against a data source
stmo-cli execute --data-source 321 --file scratch.sql
```

`execute ID` deploys the local `.sql`/`.yaml` first if it differs from what's stored on the
server (SQL, name, data source, or parameters), so it never silently runs a stale copy — then
it always runs the up-to-date server-stored query.

`execute --data-source ID` runs ad-hoc SQL directly against a data source without creating a
tracked query — useful for one-off exploration. It has no parameter schema, so multi-value
parameters can't be expanded for you — inline the values directly in the SQL (e.g.
`IN ('release', 'beta')`).

Parameters are passed as `--param name=value` (repeatable). Values are parsed as JSON when
possible, anything that isn't valid JSON is treated as a plain string.

### Manage Query Snippets

```bash
stmo-cli snippets list                     # List query snippets from Redash
stmo-cli snippets fetch 31 42              # Fetch specific snippets
stmo-cli snippets fetch --all              # Fetch all tracked snippets
stmo-cli snippets deploy                   # Deploy snippets whose local content differs from what's on Redash
stmo-cli snippets deploy --all             # Deploy all snippets
stmo-cli snippets delete 31 42             # Delete snippets in Redash and remove local files
```

Snippets don't have an archive concept in Redash — `snippets delete` removes the snippet on
the server and deletes the local files in one step; there's no separate `--cleanup` flag.

## File Structure

```
queries/
├── 123-mobile-crashes.sql
└── 123-mobile-crashes.yaml
dashboards/
└── 456-my-dashboard.yaml
snippets/
├── 31-reviewbot_e2e_action_ctcs.sql
└── 31-reviewbot_e2e_action_ctcs.yaml
```

Query IDs are embedded in filenames (`{id}-{slug}.{ext}`), so no separate config file is needed.
Snippet filenames use `{id}-{trigger}.{ext}` (Redash snippets are keyed by `trigger`, not `name`).

## Development

### Pre-commit Hooks

The project uses clippy in pedantic mode:

```bash
cargo clippy --all-targets --all-features -- -W clippy::pedantic -D warnings
```

Install pre-commit hooks:
```bash
pip install pre-commit
pre-commit install
```

### Building for Release

```bash
cargo build --release
./target/release/stmo-cli --help
```

## Architecture

See [AGENTS.md](./AGENTS.md) for detailed architecture documentation.

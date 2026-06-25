# stmo-cli

## How it works

stmo-cli is a CLI that Claude Code calls on your behalf. Install it, set your API key, and Claude Code can:

- **Explore** — discover data sources, find existing queries, browse schemas
- **Write** — create new Redash queries with proper BigQuery SQL
- **Deploy** — push queries, charts, and dashboards to STMO
- **Execute** — run queries and inspect results
- **Analyze** — export data for deeper analysis with other tools

For example, ask Claude Code to:
- "Find queries about Firefox DAU"
- "Write a query to track [metric] over time"
- "Fetch and run query #12345"
- "Explore what telemetry tables are available"

Pair it with the [mozdata plugin](https://github.com/mozilla/internal-aidev-plugins/tree/main/plugins/mozdata) for telemetry expertise and probe discovery.

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

2. Set environment variables:
```bash
export REDASH_API_KEY="your-api-key-here"
export REDASH_URL="https://sql.telemetry.mozilla.org"  # optional, this is the default
```

For Mozilla, the key can be accessed via the following URL: https://sql.telemetry.mozilla.org/users/me

3. Create directories:
```bash
stmo-cli init
```

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
stmo-cli deploy       # Deploy changed queries (detected via git status)
stmo-cli deploy --all # Deploy all queries
```

**Warning**: This force overwrites the queries in Redash. Git is the source of truth.

### Execute Queries

```bash
stmo-cli execute 123                                       # Run the local queries/123-*.sql
stmo-cli execute 123 --remote                              # Run the server-stored SQL instead
stmo-cli execute 123 --param start_date=2026-06-15
stmo-cli execute 123 --param channels='["release","beta"]' # Multi-value enum as JSON
stmo-cli data-sources                                      # List data sources
echo 'SELECT 1' | stmo-cli execute --data-source 321       # Run arbitrary SQL against a data source
stmo-cli execute --file scratch.sql --data-source 321
```

Parameters are passed as `--param name=value` (repeatable). Values are parsed as JSON when
possible, anything that isn't valid JSON is treated as a plain string.

Ad-hoc `--data-source` execution has no parameter schema (the SQL isn't a tracked query), so
multi-value parameters can't be expanded for you — inline the values directly in the SQL
(e.g. `IN ('release', 'beta')`). Running a tracked query by ID applies its parameter
definitions normally.

## File Structure

```
queries/
├── 123-mobile-crashes.sql
└── 123-mobile-crashes.yaml
dashboards/
└── 456-my-dashboard.yaml
```

Query IDs are embedded in filenames (`{id}-{slug}.{ext}`), so no separate config file is needed.

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

See [CLAUDE.md](./CLAUDE.md) for detailed architecture documentation.

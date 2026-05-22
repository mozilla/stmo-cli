# stmo-cli

Rust CLI for version controlling Redash queries and dashboards.

## Features

- Version control Redash queries (SQL + metadata) and dashboards
- Automatic query discovery from local files
- Automatic deployment to Redash
- Built in Rust for blazing fast performance
- Pedantic code quality with clippy pre-commit hooks

## Prerequisites

- Redash API key from https://sql.telemetry.mozilla.org

## Claude Integration

stmo-cli works great with the [mozdata-claude-plugin](https://github.com/mozilla/internal-aidev-plugins/tree/main/plugins/mozdata), which provides Mozilla telemetry expertise and discovery directly in Claude.

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
stmo-cli discover
```

5. Fetch specific queries:
```bash
stmo-cli fetch 123 456 789
```

## Usage

### Fetch Queries from Redash

```bash
stmo-cli fetch --all        # Fetch all tracked queries
stmo-cli fetch 123 456 789  # Fetch specific queries
stmo-cli discover            # List available queries
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

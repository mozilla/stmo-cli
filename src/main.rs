mod api;
mod commands;
mod models;

use anyhow::{Context, Result};
use api::RedashClient;
use clap::{Parser, Subcommand};
use moz_cli_version_check::VersionChecker;

#[derive(Parser)]
#[command(name = "stmo-cli", version)]
#[command(about = "Turn Claude Code into a data analyst on sql.telemetry.mozilla.org", long_about = None)]
struct Cli {
    #[command(subcommand)]
    command: Commands,
}

#[derive(Subcommand)]
enum Commands {
    #[command(
        about = "List queries and dashboards from Redash",
        long_about = "List queries and dashboards from Redash.\n\nWithout --search, lists your own queries.\nWith --search, performs a full-text search across all queries and dashboards."
    )]
    Discover {
        #[arg(long, short = 'q', help = "Search queries and dashboards by text")]
        search: Option<String>,
        #[arg(long, default_value_t = 50, help = "Max results per section")]
        limit: usize,
    },

    #[command(about = "Scaffold a new query/dashboard repository")]
    Init,

    #[command(about = "Fetch queries from Redash")]
    Fetch {
        #[arg(help = "Query IDs to fetch (e.g., 123 456 789)")]
        query_ids: Vec<u64>,
        #[arg(
            long,
            help = "Fetch all queries currently tracked in queries/ directory"
        )]
        all: bool,
    },

    #[command(about = "Deploy local changes to Redash (only changed queries by default)")]
    Deploy {
        #[arg(help = "Query IDs to deploy (e.g., 123 456 789)")]
        query_ids: Vec<u64>,
        #[arg(long, help = "Deploy all queries instead of only changed ones")]
        all: bool,
    },

    #[command(about = "Execute a tracked query, or run ad-hoc SQL against a data source")]
    Execute {
        #[arg(
            help = "Query ID to execute (must be fetched locally first); omit and use \
                    --data-source to run ad-hoc SQL"
        )]
        query_id: Option<u64>,

        #[arg(
            long,
            help = "Data source ID to run ad-hoc SQL against (no tracked query is created); \
                    SQL comes from --file or stdin"
        )]
        data_source: Option<u64>,

        #[arg(
            long,
            help = "Path to a .sql file to read the ad-hoc SQL from; pass '-' or omit to read \
                    SQL from stdin (e.g. echo 'SELECT 1' | stmo-cli execute --data-source ID)"
        )]
        file: Option<String>,

        #[arg(
            long,
            help = "Query parameter in format: name=value (can be used multiple times)"
        )]
        param: Vec<String>,

        #[arg(
            long,
            short = 'f',
            default_value = "json",
            help = "Output format: json or table"
        )]
        format: String,

        #[arg(
            long,
            short = 'i',
            help = "Prompt for missing parameters interactively"
        )]
        interactive: bool,

        #[arg(long, default_value = "300", help = "Timeout in seconds")]
        timeout: u64,

        #[arg(long, help = "Limit number of rows displayed (default: all)")]
        limit: Option<usize>,
    },

    #[command(about = "List and explore data sources")]
    DataSources {
        #[arg(help = "Optional: Data source ID to inspect")]
        data_source_id: Option<u64>,

        #[arg(long, help = "Show table schema for the data source")]
        schema: bool,

        #[arg(
            long,
            help = "Force refresh schema from data source (slower but always up-to-date)"
        )]
        refresh: bool,

        #[arg(
            long,
            short = 'f',
            default_value = "json",
            help = "Output format: json or table"
        )]
        format: String,
    },

    #[command(about = "Archive queries in Redash and remove local files")]
    Archive {
        #[arg(help = "Query IDs to archive (e.g., 123 456 789)")]
        query_ids: Vec<u64>,

        #[arg(
            long,
            help = "Remove local files for queries already archived in Redash"
        )]
        cleanup: bool,
    },

    #[command(about = "Restore archived queries")]
    Unarchive {
        #[arg(help = "Query IDs to unarchive (e.g., 123 456 789)")]
        query_ids: Vec<u64>,
    },

    #[command(about = "Manage dashboards")]
    Dashboards {
        #[command(subcommand)]
        command: DashboardCommands,
    },

    #[command(
        about = "Set or clear a query's refresh schedule (updates local YAML; run 'deploy' to push to Redash)",
        long_about = "Set or clear a query's refresh schedule.\n\nUpdates the schedule field in each query's local YAML file. The change is not pushed to Redash until you run 'stmo-cli deploy'.\n\nExamples:\n  stmo-cli schedule 123 456 --interval 86400 --time 07:15\n  stmo-cli schedule 123 --clear"
    )]
    Schedule {
        #[arg(help = "Query IDs to update (e.g., 123 456 789)")]
        query_ids: Vec<u64>,

        #[arg(
            long,
            help = "Refresh interval in seconds (e.g., 86400 for daily)",
            conflicts_with = "clear"
        )]
        interval: Option<u64>,

        #[arg(
            long,
            help = "Time of day for the refresh in HH:MM format (e.g., 07:15)",
            requires = "interval"
        )]
        time: Option<String>,

        #[arg(
            long,
            help = "Day of week for the refresh (0=Sunday through 6=Saturday)",
            requires = "interval"
        )]
        day_of_week: Option<String>,

        #[arg(long, help = "Clear the refresh schedule", conflicts_with = "interval")]
        clear: bool,
    },

    #[command(about = "Update stmo-cli to the latest version")]
    Update,
}

#[derive(Subcommand)]
enum DashboardCommands {
    #[command(about = "List all dashboards from Redash")]
    Discover,

    #[command(about = "Fetch dashboards from Redash")]
    Fetch {
        #[arg(
            help = "Dashboard slugs to fetch (e.g., firefox-desktop-on-steamos bug-2006698---ccov-build-regression)"
        )]
        slugs: Vec<String>,
    },

    #[command(about = "Deploy dashboard changes to Redash")]
    Deploy {
        #[arg(
            help = "Dashboard slugs to deploy (e.g., firefox-desktop-on-steamos bug-2006698---ccov-build-regression)"
        )]
        slugs: Vec<String>,
        #[arg(long, help = "Deploy all tracked dashboards")]
        all: bool,
    },

    #[command(about = "Archive dashboards in Redash and remove local files")]
    Archive {
        #[arg(
            help = "Dashboard slugs to archive (e.g., firefox-desktop-on-steamos bug-2006698---ccov-build-regression)"
        )]
        slugs: Vec<String>,
    },

    #[command(about = "Restore archived dashboards")]
    Unarchive {
        #[arg(
            help = "Dashboard slugs to unarchive (e.g., firefox-desktop-on-steamos bug-2006698---ccov-build-regression)"
        )]
        slugs: Vec<String>,
    },
}

#[allow(clippy::too_many_lines)]
async fn run_command(client: RedashClient, command: Commands) -> Result<()> {
    match command {
        Commands::Discover { search, limit } => {
            commands::discover::discover(&client, search.as_deref(), limit).await?;
        }
        Commands::Init | Commands::Update => unreachable!(),
        Commands::Fetch { query_ids, all } => {
            commands::fetch::fetch(&client, query_ids, all).await?;
        }
        Commands::Deploy { query_ids, all } => {
            commands::deploy::deploy(&client, query_ids, all).await?;
        }
        Commands::Execute {
            query_id,
            data_source,
            file,
            param,
            format,
            interactive,
            timeout,
            limit,
        } => {
            let output_format = format
                .parse::<commands::OutputFormat>()
                .context("Invalid output format")?;
            let args = commands::execute::ExecuteArgs {
                query_id,
                data_source,
                file,
                param_args: param,
                format: output_format,
                interactive,
                timeout_secs: timeout,
                limit_rows: limit,
            };
            commands::execute::execute(&client, args).await?;
        }
        Commands::DataSources {
            data_source_id,
            schema,
            refresh,
            format,
        } => {
            let output_format = format
                .parse::<commands::OutputFormat>()
                .context("Invalid output format")?;
            if let Some(id) = data_source_id {
                commands::datasources::show_data_source(
                    &client,
                    id,
                    schema,
                    refresh,
                    output_format,
                )
                .await?;
            } else {
                commands::datasources::list_data_sources(&client, output_format).await?;
            }
        }
        Commands::Archive { query_ids, cleanup } => {
            if cleanup {
                commands::archive::cleanup(&client).await?;
            } else if !query_ids.is_empty() {
                commands::archive::archive(&client, query_ids).await?;
            } else {
                anyhow::bail!(
                    "No query IDs specified. Use specific query IDs or --cleanup flag.\n\nExamples:\n  stmo-cli archive 123 456\n  stmo-cli archive --cleanup"
                );
            }
        }
        Commands::Unarchive { query_ids } => {
            if query_ids.is_empty() {
                anyhow::bail!(
                    "No query IDs specified. Provide query IDs to unarchive.\n\nExample:\n  stmo-cli unarchive 123 456"
                );
            }
            commands::archive::unarchive(&client, query_ids).await?;
        }
        Commands::Schedule {
            query_ids,
            interval,
            time,
            day_of_week,
            clear,
        } => {
            if query_ids.is_empty() {
                anyhow::bail!(
                    "No query IDs specified. Provide query IDs to update.\n\nExamples:\n  stmo-cli schedule 123 456 --interval 86400 --time 07:15\n  stmo-cli schedule 123 --clear"
                );
            }
            commands::schedule::schedule(
                &query_ids,
                interval,
                time.as_deref(),
                day_of_week.as_deref(),
                clear,
            )?;
        }
        Commands::Dashboards { command } => match command {
            DashboardCommands::Discover => commands::dashboards::discover(&client).await?,
            DashboardCommands::Fetch { slugs } => {
                commands::dashboards::fetch(&client, slugs).await?;
            }
            DashboardCommands::Deploy { slugs, all } => {
                commands::dashboards::deploy(&client, slugs, all).await?;
            }
            DashboardCommands::Archive { slugs } => {
                commands::dashboards::archive(&client, slugs).await?;
            }
            DashboardCommands::Unarchive { slugs } => {
                commands::dashboards::unarchive(&client, slugs).await?;
            }
        },
    }
    Ok(())
}

fn is_llm_environment() -> bool {
    std::env::var("CLAUDECODE").is_ok()
        || std::env::var("CODEX_SANDBOX").is_ok()
        || std::env::var("GEMINI_CLI").is_ok()
        || std::env::var("OPENCODE").is_ok()
}

fn print_llm_help() {
    print!(
        r#"stmo-cli — Redash CLI for sql.telemetry.mozilla.org. Explore data sources, run queries, deploy dashboards.
REDASH_API_KEY required | REDASH_URL optional (default: https://sql.telemetry.mozilla.org)
API key: https://sql.telemetry.mozilla.org/users/me → API Key section

discover [--search TEXT] [--limit N] | fetch [IDs] [--all] | deploy [IDs] [--all] | execute ID [--format table|json] [--param k=v]... [--interactive] [--limit N]
execute --data-source ID [--file PATH|-] [--param k=v]...: ad-hoc SQL from a file or stdin ('-' or omit --file = read stdin), no tracked query created (no schema, so no d_* dates or multi-value expansion — inline values in the SQL)
data-sources [ID] [--schema] [--refresh] | archive IDs | archive --cleanup | unarchive IDs | init | update
dashboards discover|fetch SLUGS|deploy SLUGS [--all]|archive SLUGS|unarchive SLUGS

schedule IDs --interval SECS [--time HH:MM] [--day-of-week N] | schedule IDs --clear (writes YAML; run deploy to push)
deploy: uses git diff by default; --all required outside a git repo
execute ID: deploys local .sql/.yaml first if it differs from the server-stored query, then always runs the up-to-date server copy
archive IDs: archives on server + deletes local | archive --cleanup: deletes local only for already-archived (does NOT archive on server)
dashboards: addressed by slug not ID; only favorited dashboards appear in dashboards discover

Files: queries/<id>-<slug>.sql + .yaml, dashboards/<id>-<slug>.yaml | id=0 for new resources, auto-renamed after first deploy
Required YAML fields: id name data_source_id options.parameters(can be []) visualizations(can be [])
Slug from name: lowercase, non-alphanum→'-', collapse dashes (e.g. "Mozilla's .rpm"→"mozilla-s-rpm")
enumOptions: use YAML multiline (|-), NOT escaped \n or deploy fails
Multi-value enum params require JSON array: --param channels='["release","beta"]'
Dynamic date tokens resolved client-side (tracked queries only): d_now/d_yesterday (date types); d_today/d_last_7_days/d_last_month/d_this_week/... (range types)
JSON export: stmo-cli execute ID --format json 2>/dev/null > data.json
"#
    );
}

#[tokio::main]
async fn main() -> Result<()> {
    let version_checker = VersionChecker::new("stmo-cli", env!("CARGO_PKG_VERSION"));
    version_checker.check_async();

    if is_llm_environment() && std::env::args().any(|arg| arg == "--help" || arg == "-h") {
        print_llm_help();
        version_checker.print_warning();
        return Ok(());
    }

    let cli = match Cli::try_parse() {
        Ok(cli) => cli,
        Err(e) => {
            e.print()?;
            if e.kind() == clap::error::ErrorKind::DisplayVersion {
                version_checker.print_warning_sync();
            } else {
                version_checker.print_warning();
            }
            std::process::exit(e.exit_code());
        }
    };

    if let Commands::Init = cli.command {
        let result = commands::init::init();
        version_checker.print_warning();
        return result;
    }

    if let Commands::Update = cli.command {
        let result = commands::update::update();
        version_checker.print_warning();
        return result;
    }

    let api_key =
        std::env::var("REDASH_API_KEY").context("REDASH_API_KEY environment variable not set")?;
    let base_url = std::env::var("REDASH_URL")
        .unwrap_or_else(|_| "https://sql.telemetry.mozilla.org".to_string());
    let client = RedashClient::new(base_url, &api_key)?;

    run_command(client, cli.command).await?;
    version_checker.print_warning();
    Ok(())
}

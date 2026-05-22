mod api;
mod commands;
mod models;

use anyhow::{Context, Result};
use api::RedashClient;
use clap::{Parser, Subcommand};
use moz_cli_version_check::VersionChecker;

#[derive(Parser)]
#[command(name = "stmo-cli", version)]
#[command(about = "CLI tool for version controlling Redash queries and dashboards", long_about = None)]
struct Cli {
    #[command(subcommand)]
    command: Commands,
}

#[derive(Subcommand)]
enum Commands {
    #[command(about = "List all queries from Redash")]
    Discover,

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

    #[command(about = "Execute a query and display results")]
    Execute {
        #[arg(help = "Query ID to execute (must be fetched locally first)")]
        query_id: u64,

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

async fn run_command(client: RedashClient, command: Commands) -> Result<()> {
    match command {
        Commands::Discover => commands::discover::discover(&client).await?,
        Commands::Init | Commands::Update => unreachable!(),
        Commands::Fetch { query_ids, all } => {
            commands::fetch::fetch(&client, query_ids, all).await?;
        }
        Commands::Deploy { query_ids, all } => {
            commands::deploy::deploy(&client, query_ids, all).await?;
        }
        Commands::Execute {
            query_id,
            param,
            format,
            interactive,
            timeout,
            limit,
        } => {
            let output_format = format
                .parse::<commands::OutputFormat>()
                .context("Invalid output format")?;
            commands::execute::execute(
                &client,
                query_id,
                param,
                output_format,
                interactive,
                timeout,
                limit,
            )
            .await?;
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

#[tokio::main]
async fn main() -> Result<()> {
    let version_checker = VersionChecker::new("stmo-cli", env!("CARGO_PKG_VERSION"));
    version_checker.check_async();

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

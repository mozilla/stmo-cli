mod changelog;

use anyhow::Result;
use clap::{Parser, Subcommand};

#[derive(Parser)]
struct Cli {
    #[command(subcommand)]
    command: Command,
}

#[derive(Subcommand)]
enum Command {
    /// Print the CHANGELOG.md section for a released version
    ExtractChangelog { version: String },
}

fn main() -> Result<()> {
    let cli = Cli::parse();
    match cli.command {
        Command::ExtractChangelog { version } => {
            let changelog = std::fs::read_to_string("CHANGELOG.md")?;
            let section = changelog::extract_changelog_section(&changelog, &version)?;
            print!("{section}");
        }
    }
    Ok(())
}

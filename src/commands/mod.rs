pub mod archive;
pub mod dashboards;
pub mod datasources;
pub mod deploy;
pub mod discover;
pub mod dynamic_dates;
pub mod execute;
pub mod fetch;
pub mod init;
pub mod update;

use anyhow::{Result, bail};

#[derive(Debug, Clone, Copy)]
pub enum OutputFormat {
    Json,
    Table,
}

impl std::str::FromStr for OutputFormat {
    type Err = anyhow::Error;

    fn from_str(s: &str) -> Result<Self> {
        match s.to_lowercase().as_str() {
            "json" => Ok(Self::Json),
            "table" => Ok(Self::Table),
            _ => bail!("Invalid format. Use: json or table"),
        }
    }
}

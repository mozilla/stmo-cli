#![allow(clippy::missing_errors_doc)]

use anyhow::Result;

use crate::api::RedashClient;

pub async fn show_query_api_key(client: &RedashClient, query_id: u64) -> Result<()> {
    let api_key = client.get_query_api_key(query_id).await?;
    println!("{api_key}");
    Ok(())
}

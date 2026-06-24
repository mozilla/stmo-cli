#![allow(clippy::missing_errors_doc)]

use crate::models::{
    CreateDashboard, CreateQuery, CreateWidget, Dashboard, DashboardSummary, DashboardsResponse,
    DataSource, DataSourceSchema, QueriesResponse, Query,
};
use anyhow::{Context, Result};
use reqwest::{Client, header};
use serde::Serialize;
use serde::de::DeserializeOwned;

pub struct RedashClient {
    client: Client,
    base_url: String,
}

impl RedashClient {
    pub fn new(base_url: String, api_key: &str) -> Result<Self> {
        let mut headers = header::HeaderMap::new();
        headers.insert(
            "Authorization",
            header::HeaderValue::from_str(&format!("Key {api_key}"))
                .context("Invalid API key format")?,
        );

        let client = Client::builder()
            .default_headers(headers)
            .build()
            .context("Failed to build HTTP client")?;

        Ok(Self { client, base_url })
    }

    #[must_use]
    pub fn base_url(&self) -> &str {
        &self.base_url
    }

    async fn get_json<T: DeserializeOwned>(&self, url: &str, ctx: &str) -> Result<T> {
        let response = self
            .client
            .get(url)
            .send()
            .await
            .with_context(|| format!("Failed to request {ctx}"))?;

        let response = ensure_success(response).await?;

        response
            .json()
            .await
            .with_context(|| format!("Failed to parse {ctx} response"))
    }

    async fn post_json<T: DeserializeOwned, B: Serialize + ?Sized>(
        &self,
        url: &str,
        body: &B,
        ctx: &str,
    ) -> Result<T> {
        let response = self
            .client
            .post(url)
            .json(body)
            .send()
            .await
            .with_context(|| format!("Failed to request {ctx}"))?;

        let response = ensure_success(response).await?;

        response
            .json()
            .await
            .with_context(|| format!("Failed to parse {ctx} response"))
    }

    pub async fn list_my_queries(&self, page: u32, page_size: u32) -> Result<QueriesResponse> {
        let url = format!(
            "{}/api/queries/my?page={page}&page_size={page_size}",
            self.base_url
        );
        self.get_json(&url, "my queries").await
    }

    pub async fn get_query(&self, query_id: u64) -> Result<Query> {
        let url = format!("{}/api/queries/{query_id}", self.base_url);
        self.get_json(&url, &format!("query {query_id}")).await
    }

    pub async fn list_data_sources(&self) -> Result<Vec<DataSource>> {
        let url = format!("{}/api/data_sources", self.base_url);
        self.get_json(&url, "data sources").await
    }

    pub async fn get_data_source(&self, data_source_id: u64) -> Result<DataSource> {
        let url = format!("{}/api/data_sources/{data_source_id}", self.base_url);
        self.get_json(&url, &format!("data source {data_source_id}"))
            .await
    }

    pub async fn get_data_source_schema(
        &self,
        data_source_id: u64,
        refresh: bool,
    ) -> Result<DataSourceSchema> {
        let url = if refresh {
            format!(
                "{}/api/data_sources/{data_source_id}/schema?refresh=true",
                self.base_url
            )
        } else {
            format!("{}/api/data_sources/{data_source_id}/schema", self.base_url)
        };

        self.get_json(&url, &format!("schema for data source {data_source_id}"))
            .await
    }

    pub async fn create_query(&self, create_query: &CreateQuery) -> Result<Query> {
        let url = format!("{}/api/queries", self.base_url);
        self.post_json(&url, create_query, "new query").await
    }

    pub async fn create_or_update_query(&self, query: &Query) -> Result<Query> {
        let url = format!("{}/api/queries/{}", self.base_url, query.id);
        self.post_json(&url, query, &format!("query {} update", query.id))
            .await
    }

    pub async fn create_visualization(
        &self,
        query_id: u64,
        viz: &crate::models::CreateVisualization,
    ) -> Result<crate::models::Visualization> {
        let url = format!("{}/api/visualizations", self.base_url);
        self.post_json(&url, viz, &format!("visualization for query {query_id}"))
            .await
    }

    pub async fn update_visualization(
        &self,
        viz: &crate::models::Visualization,
    ) -> Result<crate::models::Visualization> {
        let url = format!("{}/api/visualizations/{}", self.base_url, viz.id);
        self.post_json(&url, viz, &format!("visualization {} update", viz.id))
            .await
    }

    pub async fn fetch_all_queries(&self) -> Result<Vec<Query>> {
        let mut all_queries = Vec::new();
        let mut page = 1;
        let page_size = 100;

        loop {
            let response = self.list_my_queries(page, page_size).await?;

            if response.results.is_empty() {
                break;
            }

            all_queries.extend(response.results);
            eprintln!(
                "Fetched {} / {} queries...",
                all_queries.len(),
                response.count
            );

            #[allow(clippy::cast_possible_truncation)]
            if all_queries.len() >= response.count as usize {
                break;
            }

            page += 1;
        }

        Ok(all_queries)
    }

    pub async fn refresh_query(
        &self,
        query_id: u64,
        parameters: Option<std::collections::HashMap<String, serde_json::Value>>,
    ) -> Result<crate::models::Job> {
        let url = format!("{}/api/queries/{query_id}/results", self.base_url);

        let request = crate::models::RefreshRequest {
            max_age: 0,
            parameters,
        };

        let job_response: crate::models::JobResponse = self
            .post_json(&url, &request, &format!("query {query_id} refresh"))
            .await?;

        Ok(job_response.job)
    }

    pub async fn poll_job(&self, job_id: &str) -> Result<crate::models::Job> {
        let url = format!("{}/api/jobs/{job_id}", self.base_url);

        let job_response: crate::models::JobResponse =
            self.get_json(&url, &format!("job {job_id}")).await?;

        Ok(job_response.job)
    }

    pub async fn get_query_result(
        &self,
        query_id: u64,
        result_id: u64,
    ) -> Result<crate::models::QueryResult> {
        let url = format!(
            "{}/api/queries/{query_id}/results/{result_id}.json",
            self.base_url
        );

        let result_response: crate::models::QueryResultResponse = self
            .get_json(&url, &format!("result {result_id} for query {query_id}"))
            .await?;

        Ok(result_response.query_result)
    }

    pub async fn execute_query_with_polling(
        &self,
        query_id: u64,
        parameters: Option<std::collections::HashMap<String, serde_json::Value>>,
        timeout_secs: u64,
        poll_interval_ms: u64,
    ) -> Result<crate::models::QueryResult> {
        use crate::models::JobStatus;
        use tokio::time::{Duration, sleep};

        eprintln!("Executing query {query_id}...");
        let job = self.refresh_query(query_id, parameters).await?;

        let start = std::time::Instant::now();
        let timeout = Duration::from_secs(timeout_secs);
        let poll_interval = Duration::from_millis(poll_interval_ms);

        let mut current_job = job;
        loop {
            if start.elapsed() > timeout {
                anyhow::bail!("Query execution timed out after {timeout_secs} seconds");
            }

            let status = JobStatus::from_u8(current_job.status)?;

            match status {
                JobStatus::Success => {
                    let result_id = current_job
                        .query_result_id
                        .context("Job succeeded but no result_id returned")?;

                    eprintln!("Query completed, fetching results...");
                    return self.get_query_result(query_id, result_id).await;
                }
                JobStatus::Failure => {
                    let error = current_job
                        .error
                        .unwrap_or_else(|| "Unknown error".to_string());
                    anyhow::bail!("Query execution failed: {error}");
                }
                JobStatus::Cancelled => {
                    anyhow::bail!("Query execution was cancelled");
                }
                JobStatus::Pending | JobStatus::Started => {
                    eprint!(".");
                    sleep(poll_interval).await;
                    current_job = self.poll_job(&current_job.id).await?;
                }
            }
        }
    }

    pub async fn archive_query(&self, query_id: u64) -> Result<Query> {
        let url = format!("{}/api/queries/{query_id}", self.base_url);
        let payload = serde_json::json!({"is_archived": true});
        self.post_json(&url, &payload, &format!("query {query_id} archive"))
            .await
    }

    pub async fn unarchive_query(&self, query_id: u64) -> Result<Query> {
        let url = format!("{}/api/queries/{query_id}", self.base_url);
        let payload = serde_json::json!({"is_archived": false});
        self.post_json(&url, &payload, &format!("query {query_id} unarchive"))
            .await
    }

    pub async fn create_dashboard(&self, dashboard: &CreateDashboard) -> Result<Dashboard> {
        let url = format!("{}/api/dashboards", self.base_url);
        self.post_json(&url, dashboard, "new dashboard").await
    }

    pub async fn list_favorite_dashboards(
        &self,
        page: u32,
        page_size: u32,
    ) -> Result<DashboardsResponse> {
        let url = format!(
            "{}/api/dashboards/favorites?page={page}&page_size={page_size}",
            self.base_url
        );
        self.get_json(&url, "favorite dashboards").await
    }

    pub async fn get_dashboard(&self, slug_or_id: &str) -> Result<Dashboard> {
        let url = format!("{}/api/dashboards/{slug_or_id}", self.base_url);
        self.get_json(&url, &format!("dashboard {slug_or_id}"))
            .await
    }

    pub async fn update_dashboard(&self, dashboard: &Dashboard) -> Result<Dashboard> {
        let url = format!("{}/api/dashboards/{}", self.base_url, dashboard.id);
        self.post_json(
            &url,
            dashboard,
            &format!("dashboard {} update", dashboard.id),
        )
        .await
    }

    pub async fn archive_dashboard(&self, dashboard_id: u64) -> Result<()> {
        let url = format!("{}/api/dashboards/{dashboard_id}", self.base_url);
        let payload = serde_json::json!({"is_archived": true});
        let response = self
            .client
            .post(&url)
            .json(&payload)
            .send()
            .await
            .context(format!("Failed to archive dashboard {dashboard_id}"))?;

        ensure_success(response).await?;

        Ok(())
    }

    pub async fn unarchive_dashboard(&self, dashboard_id: u64) -> Result<Dashboard> {
        let url = format!("{}/api/dashboards/{dashboard_id}", self.base_url);
        let payload = serde_json::json!({"is_archived": false});
        self.post_json(
            &url,
            &payload,
            &format!("dashboard {dashboard_id} unarchive"),
        )
        .await
    }

    pub async fn create_widget(&self, widget: &CreateWidget) -> Result<crate::models::Widget> {
        let url = format!("{}/api/widgets", self.base_url);
        self.post_json(&url, widget, "new widget").await
    }

    pub async fn update_widget(
        &self,
        widget_id: u64,
        widget: &CreateWidget,
    ) -> Result<crate::models::Widget> {
        let url = format!("{}/api/widgets/{widget_id}", self.base_url);
        self.post_json(&url, widget, &format!("widget {widget_id} update"))
            .await
    }

    pub async fn delete_widget(&self, widget_id: u64) -> Result<()> {
        let url = format!("{}/api/widgets/{widget_id}", self.base_url);
        let response = self
            .client
            .delete(&url)
            .send()
            .await
            .context(format!("Failed to delete widget {widget_id}"))?;

        ensure_success(response).await?;

        Ok(())
    }

    pub async fn favorite_dashboard(&self, slug: &str) -> Result<()> {
        let url = format!("{}/api/dashboards/{slug}/favorite", self.base_url);
        let response = self
            .client
            .post(&url)
            .json(&serde_json::json!({}))
            .send()
            .await
            .context(format!("Failed to favorite dashboard {slug}"))?;

        ensure_success(response).await?;

        Ok(())
    }

    pub async fn fetch_favorite_dashboards(&self) -> Result<Vec<DashboardSummary>> {
        let mut all_dashboards = Vec::new();
        let mut page = 1;
        let page_size = 100;

        loop {
            let response = self.list_favorite_dashboards(page, page_size).await?;

            if response.results.is_empty() {
                break;
            }

            all_dashboards.extend(response.results);
            eprintln!(
                "Fetched {} / {} dashboards...",
                all_dashboards.len(),
                response.count
            );

            #[allow(clippy::cast_possible_truncation)]
            if all_dashboards.len() >= response.count as usize {
                break;
            }

            page += 1;
        }

        Ok(all_dashboards)
    }

    async fn get_with_retry(
        &self,
        url: &str,
        params: &[(&str, String)],
    ) -> Result<reqwest::Response> {
        use tokio::time::{Duration, sleep};

        const MAX_ATTEMPTS: u32 = 4;
        let base_delays = [250u64, 500, 1000, 2000];

        let mut last_error = anyhow::anyhow!("No attempts made");
        for attempt in 0..MAX_ATTEMPTS {
            let response = self
                .client
                .get(url)
                .query(params)
                .send()
                .await
                .with_context(|| format!("Failed to GET {url}"))?;

            let status = response.status();
            let should_retry = status.as_u16() == 429 || status.is_server_error();

            match ensure_success(response).await {
                Ok(response) => return Ok(response),
                Err(err) if !should_retry || attempt + 1 == MAX_ATTEMPTS => return Err(err),
                Err(err) => last_error = err,
            }

            let delay_ms = base_delays[attempt as usize];
            sleep(Duration::from_millis(delay_ms)).await;
        }

        Err(last_error)
    }

    async fn list_queries(&self, q: &str, page: u32, page_size: u32) -> Result<QueriesResponse> {
        let url = format!("{}/api/queries", self.base_url);
        let params = [
            ("page", page.to_string()),
            ("page_size", page_size.to_string()),
            ("q", q.to_string()),
        ];
        self.get_with_retry(&url, &params)
            .await?
            .json()
            .await
            .context("Failed to parse queries response")
    }

    async fn list_dashboards(
        &self,
        q: &str,
        page: u32,
        page_size: u32,
    ) -> Result<DashboardsResponse> {
        let url = format!("{}/api/dashboards", self.base_url);
        let params = [
            ("page", page.to_string()),
            ("page_size", page_size.to_string()),
            ("q", q.to_string()),
        ];
        self.get_with_retry(&url, &params)
            .await?
            .json()
            .await
            .context("Failed to parse dashboards response")
    }

    pub async fn search_queries(&self, q: &str, limit: usize) -> Result<Vec<Query>> {
        const PAGE_SIZE: usize = 250;

        let mut results: Vec<Query> = Vec::new();
        let mut page = 1u32;

        loop {
            let remaining = limit - results.len();
            #[allow(clippy::cast_possible_truncation)]
            let page_size = remaining.min(PAGE_SIZE) as u32;
            let response = self.list_queries(q, page, page_size).await?;

            results.extend(response.results);

            #[allow(clippy::cast_possible_truncation)]
            if results.len() >= limit || results.len() >= response.count as usize {
                break;
            }

            page += 1;
        }

        results.truncate(limit);
        Ok(results)
    }

    pub async fn search_dashboards(&self, q: &str, limit: usize) -> Result<Vec<DashboardSummary>> {
        const PAGE_SIZE: usize = 250;

        let mut results: Vec<DashboardSummary> = Vec::new();
        let mut page = 1u32;

        loop {
            let remaining = limit - results.len();
            #[allow(clippy::cast_possible_truncation)]
            let page_size = remaining.min(PAGE_SIZE) as u32;
            let response = self.list_dashboards(q, page, page_size).await?;

            results.extend(response.results);

            #[allow(clippy::cast_possible_truncation)]
            if results.len() >= limit || results.len() >= response.count as usize {
                break;
            }

            page += 1;
        }

        results.truncate(limit);
        Ok(results)
    }
}

async fn ensure_success(response: reqwest::Response) -> Result<reqwest::Response> {
    let status = response.status();
    if !status.is_success() {
        let body = response.text().await.unwrap_or_default();
        anyhow::bail!("API error {status}: {body}");
    }
    Ok(response)
}

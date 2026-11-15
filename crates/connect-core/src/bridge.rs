use anyhow::{bail, Context, Result};
use reqwest::Client;
use serde::{de::DeserializeOwned, Deserialize, Serialize};
use std::time::Duration;

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct BridgeItem {
    pub id: i64,
    pub name: String,
    #[serde(default)]
    pub enabled: bool,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct BridgeHealth {
    pub app: String,
    pub version: String,
    pub bridge_api_version: u32,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct BridgeRunAccepted {
    pub run_id: String,
    pub status: String,
    pub kind: String,
    pub target_id: i64,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct BridgeRunStatus {
    pub run_id: String,
    pub kind: String,
    pub target_id: i64,
    pub status: String,
    pub error: Option<String>,
}

#[derive(Debug, Deserialize)]
struct ItemsResponse {
    items: Vec<BridgeItem>,
}

pub struct BridgeClient {
    http: Client,
}

impl BridgeClient {
    pub fn new() -> Result<Self> {
        Ok(Self {
            http: Client::builder()
                .timeout(Duration::from_secs(15))
                .build()?,
        })
    }

    fn base(base: &str) -> String {
        base.trim().trim_end_matches('/').to_string()
    }

    async fn send<T: DeserializeOwned>(
        &self,
        request: reqwest::RequestBuilder,
    ) -> Result<T> {
        let response = request.send().await.context("bridge request failed")?;
        let status = response.status();
        let body = response.text().await.unwrap_or_default();
        if !status.is_success() {
            bail!("bridge HTTP {status}: {body}");
        }
        serde_json::from_str(&body).with_context(|| format!("invalid bridge response: {body}"))
    }

    pub async fn health(&self, base: &str, token: &str) -> Result<BridgeHealth> {
        let url = format!("{}/api/connect/v1/health", Self::base(base));
        self.send(self.http.get(url).bearer_auth(token)).await
    }

    pub async fn tasks(&self, base: &str, token: &str) -> Result<Vec<BridgeItem>> {
        let url = format!("{}/api/connect/v1/tasks", Self::base(base));
        let response: ItemsResponse = self.send(self.http.get(url).bearer_auth(token)).await?;
        Ok(response.items)
    }

    pub async fn workflows(&self, base: &str, token: &str) -> Result<Vec<BridgeItem>> {
        let url = format!("{}/api/connect/v1/workflows", Self::base(base));
        let response: ItemsResponse = self.send(self.http.get(url).bearer_auth(token)).await?;
        Ok(response.items)
    }

    pub async fn run_task(
        &self,
        base: &str,
        token: &str,
        id: i64,
    ) -> Result<BridgeRunAccepted> {
        let url = format!("{}/api/connect/v1/tasks/{id}/run", Self::base(base));
        self.send(self.http.post(url).bearer_auth(token)).await
    }

    pub async fn run_workflow(
        &self,
        base: &str,
        token: &str,
        id: i64,
    ) -> Result<BridgeRunAccepted> {
        let url = format!("{}/api/connect/v1/workflows/{id}/run", Self::base(base));
        self.send(self.http.post(url).bearer_auth(token)).await
    }

    pub async fn run_status(
        &self,
        base: &str,
        token: &str,
        run_id: &str,
    ) -> Result<BridgeRunStatus> {
        let url = format!("{}/api/connect/v1/runs/{run_id}", Self::base(base));
        self.send(self.http.get(url).bearer_auth(token)).await
    }
}

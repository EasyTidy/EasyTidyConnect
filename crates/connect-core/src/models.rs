use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize)]
pub struct TriggerView {
    pub id: i64,
    pub name: String,
    pub enabled: bool,
    pub action_type: String,
    pub action_config: String,
    pub public_id: String,
    pub tag_written: bool,
    pub verified_at: Option<String>,
    pub created_at: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct TagPayload {
    pub trigger_id: i64,
    pub trigger_name: String,
    /// NDEF URI to write to NTAG216.
    pub uri: String,
    /// LAN helper page for copying the payload on a phone.
    pub write_page: String,
    pub public_id: String,
    pub bytes: usize,
    pub tag_written: bool,
}

#[derive(Debug, Clone, Serialize)]
pub struct RunView {
    pub id: i64,
    pub trigger_id: i64,
    pub trigger_name: String,
    pub status: String,
    pub source: String,
    pub started_at: String,
    pub finished_at: Option<String>,
    pub error: Option<String>,
    pub external_run_id: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AnniversaryProject {
    pub id: i64,
    pub slug: String,
    pub name: String,
    pub event_date: String,
    pub title: String,
    pub recipient: String,
    pub opening: String,
    pub body: String,
    pub closing: String,
    pub template_id: String,
    pub theme: String,
    #[serde(default)]
    pub photos: Vec<String>,
    pub custom_css: String,
    pub created_at: String,
    pub updated_at: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProjectInput {
    pub name: String,
    pub event_date: String,
    pub title: String,
    pub recipient: String,
    pub opening: String,
    pub body: String,
    pub closing: String,
    pub template_id: String,
    pub theme: String,
    pub custom_css: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AiProvider {
    pub id: i64,
    pub name: String,
    pub kind: String,
    pub base_url: String,
    pub model: String,
    pub api_key: String,
    pub is_default: bool,
    pub created_at: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AiProviderInput {
    pub name: String,
    pub kind: String,
    pub base_url: String,
    pub model: String,
    pub api_key: String,
    #[serde(default)]
    pub is_default: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GeneratedCopy {
    pub title: String,
    pub opening: String,
    pub body: String,
    pub closing: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PublishTarget {
    pub id: i64,
    pub name: String,
    pub kind: String,
    pub endpoint: String,
    pub region: String,
    pub bucket: String,
    pub access_key: String,
    pub secret_key: String,
    pub prefix: String,
    pub public_base_url: String,
    pub is_default: bool,
    pub created_at: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PublishTargetInput {
    pub name: String,
    pub kind: String,
    pub endpoint: String,
    pub region: String,
    pub bucket: String,
    pub access_key: String,
    pub secret_key: String,
    pub prefix: String,
    pub public_base_url: String,
    #[serde(default)]
    pub is_default: bool,
}

#[derive(Debug, Clone, Serialize)]
pub struct PublishResult {
    pub url: String,
    pub slug: String,
    pub uploaded: usize,
}

#[derive(Debug, Clone, Serialize)]
pub struct ProjectVersion {
    pub id: i64,
    pub project_id: i64,
    pub version: i64,
    pub published_url: String,
    pub status: String,
    pub published_at: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Settings {
    pub device_name: String,
    pub port: u16,
    pub auto_start: bool,
    /// EasyTidy Bridge base URL, e.g. http://127.0.0.1:53401
    #[serde(default)]
    pub bridge_url: String,
    /// EasyTidy Bridge Bearer token.
    #[serde(default)]
    pub bridge_token: String,
    /// follow | independent | manual; None when EasyTidy integration is unavailable.
    #[serde(default)]
    pub integration_mode: Option<String>,
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            device_name: "EasyTidy Connect".to_string(),
            port: 53400,
            auto_start: false,
            bridge_url: String::new(),
            bridge_token: String::new(),
            integration_mode: None,
        }
    }
}

#[derive(Debug, Clone)]
pub struct TriggerRow {
    pub id: i64,
    pub name: String,
    pub enabled: bool,
    pub action_type: String,
    pub action_config: String,
    pub public_id: String,
    pub token: String,
    pub write_token: String,
    pub tag_written: bool,
    pub verified_at: Option<String>,
    pub created_at: String,
}

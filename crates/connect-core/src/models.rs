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

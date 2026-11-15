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
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Settings {
    pub device_name: String,
    pub port: u16,
    pub auto_start: bool,
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            device_name: "EasyTidy Connect".to_string(),
            port: 53400,
            auto_start: false,
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

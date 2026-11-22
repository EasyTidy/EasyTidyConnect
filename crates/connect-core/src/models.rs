use serde::{Deserialize, Serialize};

/// 照片裁剪焦点：文件名 → [x%, y%]。
pub type PhotoFocus = std::collections::BTreeMap<String, [f32; 2]>;

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
    #[serde(default)]
    pub music: String,
    #[serde(default)]
    pub video: String,
    pub custom_css: String,
    /// 照片裁剪焦点：文件名 → [x%, y%]（0–100，默认居中）。只在需要裁剪的位置（如封面）生效。
    #[serde(default)]
    pub photo_focus: PhotoFocus,
    /// 照片自动放大轮播：滚动到照片时自动全屏逐张播放（默认关闭）。
    #[serde(default)]
    pub slideshow: bool,
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
    #[serde(default)]
    pub music: String,
    #[serde(default)]
    pub video: String,
    pub custom_css: String,
    /// 照片裁剪焦点：文件名 → [x%, y%]（0–100，默认居中）。只在需要裁剪的位置（如封面）生效。
    #[serde(default)]
    pub photo_focus: PhotoFocus,
    /// 照片自动放大轮播：滚动到照片时自动全屏逐张播放（默认关闭）。
    #[serde(default)]
    pub slideshow: bool,
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

/// AI 设计的模板：只保存 CSS，套在固定的语义化 HTML 骨架（templates/custom.html）上。
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CustomTemplate {
    pub id: i64,
    pub name: String,
    pub prompt: String,
    pub css: String,
    /// 页面整体是否为深色背景（决定音乐按钮等浮层用深色毛玻璃）。
    pub dark: bool,
    pub created_at: String,
    pub updated_at: String,
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
    #[serde(default)]
    pub cloudflared_path: String,
    #[serde(default)]
    pub tunnel_token: String,
    #[serde(default)]
    pub hostname: String,
    #[serde(default)]
    pub account_id: String,
    #[serde(default)]
    pub project_name: String,
    #[serde(default)]
    pub api_token: String,
    #[serde(default)]
    pub branch: String,
    /// 内网穿透（ngrok / frp / SSH / bore）的配置，JSON 字符串，字段见 tunnel::TunnelConfig。
    #[serde(default)]
    pub tunnel_config: String,
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
    pub cloudflared_path: String,
    #[serde(default)]
    pub tunnel_token: String,
    #[serde(default)]
    pub hostname: String,
    #[serde(default)]
    pub account_id: String,
    #[serde(default)]
    pub project_name: String,
    #[serde(default)]
    pub api_token: String,
    #[serde(default)]
    pub branch: String,
    /// 内网穿透（ngrok / frp / SSH / bore）的配置，JSON 字符串，字段见 tunnel::TunnelConfig。
    #[serde(default)]
    pub tunnel_config: String,
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
    /// 发布目标类型（s3 / r2 / cloudflare_pages / lan_tunnel …），决定能否分享。
    #[serde(default)]
    pub target_kind: String,
}

/// 微信 JS-SDK（公众号）配置：AppID / AppSecret 由用户自己填写（或只保存在签名服务器上）。
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct WechatConfig {
    pub enabled: bool,
    /// 签名方式：`server`（默认）= 使用自己部署的签名服务器，AppID / AppSecret 只保存在服务器上，
    /// 本应用不需要填写；`local` = 本机签名，在本应用填写 AppID / AppSecret（配合内网穿透）。
    pub mode: String,
    pub app_id: String,
    pub app_secret: String,
    /// 签名服务地址，例如 https://sign.example.com/wx/jssdk；发布的页面打开时调用它获取 wx.config。
    pub signer_url: String,
    /// 允许签名的页面域名（逗号分隔，支持子域名）；为空表示不限制。
    pub allowed_domains: String,
}

/// 分享卡片的自定义文案；为空时用项目标题 / 正文第一句。
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct ShareSettings {
    #[serde(default)]
    pub title: String,
    #[serde(default)]
    pub description: String,
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

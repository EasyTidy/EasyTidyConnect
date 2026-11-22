//! 微信公众号 JS-SDK 签名服务。
//!
//! 静态托管的页面（S3 / R2 / Cloudflare Pages）在微信里打开时请求
//! `GET /wx/jssdk?url=<当前页面地址>`，本服务用公众号 AppID / AppSecret 换取
//! access_token → jsapi_ticket（各约 2 小时有效，缓存复用），按微信规则计算 SHA-1 签名后返回，
//! 页面据此调用 `wx.config` 并设置分享卡片。
//!
//! 微信的硬性要求：已认证的公众号；页面域名已备案并加入「JS 接口安全域名」；
//! 本服务所在服务器的公网 IP 在公众号「IP 白名单」中。

use std::sync::Arc;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use anyhow::{bail, Context, Result};
use axum::extract::{Query, State};
use axum::http::{header, HeaderMap, HeaderValue, StatusCode};
use axum::response::{IntoResponse, Response};
use axum::routing::get;
use axum::{Json, Router};
use rand::Rng;
use reqwest::Client;
use serde::{Deserialize, Serialize};
use serde_json::json;
use sha1::{Digest, Sha1};
use tokio::sync::Mutex;

pub const DEFAULT_API_BASE: &str = "https://api.weixin.qq.com";

/// 获取 access_token 的方式。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum TokenApi {
    /// `/cgi-bin/stable_token`：不会让其他系统正在使用的 access_token 失效（推荐）。
    #[default]
    Stable,
    /// `/cgi-bin/token`：旧接口，每次调用都会刷新 token，同一公众号的其他系统会受影响。
    Classic,
}

/// 签名所需的公众号配置。
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct SignerConfig {
    pub app_id: String,
    pub app_secret: String,
    /// 允许签名的页面域名（逗号分隔，支持子域名）；为空表示不限制。
    pub allowed_domains: String,
    pub token_api: TokenApi,
}

/// 签名结果，字段名与 wx.config 一致。
#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct JsSdkSignature {
    pub app_id: String,
    pub timestamp: i64,
    pub nonce_str: String,
    pub signature: String,
}

/// 按微信规则计算签名：sha1("jsapi_ticket=..&noncestr=..&timestamp=..&url=..")，url 不含 # 及之后的部分。
pub fn sign(ticket: &str, nonce: &str, timestamp: i64, url: &str) -> String {
    let url = url.split('#').next().unwrap_or(url);
    let plain = format!("jsapi_ticket={ticket}&noncestr={nonce}&timestamp={timestamp}&url={url}");
    hex::encode(Sha1::digest(plain.as_bytes()))
}

/// 解析域名规则：逗号 / 空格 / 换行分隔，`*.example.com` 与 `example.com` 等价（都包含子域名）。
pub fn domain_rules(allowed: &str) -> Vec<String> {
    allowed
        .split([',', '，', '\n', ' ', ';'])
        .map(|item| item.trim().trim_start_matches("*.").trim_start_matches('.').to_ascii_lowercase())
        .filter(|item| !item.is_empty())
        .collect()
}

/// 页面地址（或 Origin）的域名是否在允许列表里；列表为空表示不限制。
pub fn domain_allowed(allowed: &str, page_url: &str) -> bool {
    let rules = domain_rules(allowed);
    if rules.is_empty() {
        return true;
    }
    let Some(host) = url::Url::parse(page_url).ok().and_then(|u| u.host_str().map(str::to_ascii_lowercase)) else {
        return false;
    };
    rules.iter().any(|rule| host == *rule || host.ends_with(&format!(".{rule}")))
}

struct CachedTicket {
    /// AppID + AppSecret 的指纹：配置改了就重新获取
    key: String,
    ticket: String,
    expires_at: Instant,
}

/// jsapi_ticket 缓存。微信限制 access_token / ticket 的每日调用次数，必须缓存复用。
pub struct TicketCache {
    http: Client,
    api_base: String,
    cached: Mutex<Option<CachedTicket>>,
}

impl TicketCache {
    pub fn new(api_base: impl Into<String>) -> Self {
        Self {
            http: Client::builder()
                .timeout(Duration::from_secs(15))
                .user_agent(concat!("wechat-signer/", env!("CARGO_PKG_VERSION")))
                .build()
                .unwrap_or_default(),
            api_base: api_base.into().trim_end_matches('/').to_string(),
            cached: Mutex::new(None),
        }
    }

    /// 取得 jsapi_ticket（有缓存且距过期超过 5 分钟时直接复用）。
    pub async fn ticket(&self, config: &SignerConfig) -> Result<String> {
        let app_id = config.app_id.trim();
        let secret = config.app_secret.trim();
        if app_id.is_empty() || secret.is_empty() {
            bail!("未配置公众号 AppID / AppSecret");
        }
        let key = hex::encode(Sha1::digest(format!("{app_id}\n{secret}").as_bytes()));
        let mut cached = self.cached.lock().await;
        if let Some(item) = cached.as_ref() {
            if item.key == key && item.expires_at > Instant::now() + Duration::from_secs(300) {
                return Ok(item.ticket.clone());
            }
        }

        let access_token = self.access_token(app_id, secret, config.token_api).await?;
        let ticket: serde_json::Value = self
            .http
            .get(format!("{}/cgi-bin/ticket/getticket", self.api_base))
            .query(&[("access_token", access_token.as_str()), ("type", "jsapi")])
            .send()
            .await
            .context("请求 jsapi_ticket 失败")?
            .json()
            .await
            .context("jsapi_ticket 响应不是 JSON")?;
        check_error(&ticket)?;
        let value = ticket
            .get("ticket")
            .and_then(|v| v.as_str())
            .context("响应里没有 ticket")?
            .to_string();
        let expires_in = ticket.get("expires_in").and_then(|v| v.as_u64()).unwrap_or(7200);
        *cached = Some(CachedTicket {
            key,
            ticket: value.clone(),
            expires_at: Instant::now() + Duration::from_secs(expires_in),
        });
        tracing::info!("refreshed jsapi_ticket (expires in {expires_in}s)");
        Ok(value)
    }

    async fn access_token(&self, app_id: &str, secret: &str, api: TokenApi) -> Result<String> {
        let response = match api {
            TokenApi::Stable => self
                .http
                .post(format!("{}/cgi-bin/stable_token", self.api_base))
                .json(&json!({
                    "grant_type": "client_credential",
                    "appid": app_id,
                    "secret": secret,
                    "force_refresh": false,
                }))
                .send()
                .await,
            TokenApi::Classic => self
                .http
                .get(format!("{}/cgi-bin/token", self.api_base))
                .query(&[("grant_type", "client_credential"), ("appid", app_id), ("secret", secret)])
                .send()
                .await,
        };
        let token: serde_json::Value = response
            .context("请求 access_token 失败")?
            .json()
            .await
            .context("access_token 响应不是 JSON")?;
        check_error(&token)?;
        token
            .get("access_token")
            .and_then(|v| v.as_str())
            .map(str::to_string)
            .context("响应里没有 access_token")
    }

    /// 为页面地址生成 wx.config 参数。
    pub async fn signature(&self, config: &SignerConfig, page_url: &str) -> Result<JsSdkSignature> {
        if !(page_url.starts_with("https://") || page_url.starts_with("http://")) {
            bail!("页面地址无效");
        }
        if !domain_allowed(&config.allowed_domains, page_url) {
            bail!("该域名不在允许签名的列表中");
        }
        let ticket = self.ticket(config).await?;
        let nonce: String = rand::thread_rng()
            .sample_iter(&rand::distributions::Alphanumeric)
            .take(16)
            .map(char::from)
            .collect();
        let timestamp = SystemTime::now().duration_since(UNIX_EPOCH).map(|d| d.as_secs() as i64).unwrap_or(0);
        Ok(JsSdkSignature {
            app_id: config.app_id.trim().to_string(),
            timestamp,
            signature: sign(&ticket, &nonce, timestamp, page_url),
            nonce_str: nonce,
        })
    }
}

/// 把微信的错误码翻译成可操作的提示。
fn check_error(value: &serde_json::Value) -> Result<()> {
    let code = value.get("errcode").and_then(|v| v.as_i64()).unwrap_or(0);
    if code == 0 {
        return Ok(());
    }
    let msg = value.get("errmsg").and_then(|v| v.as_str()).unwrap_or_default();
    let hint = match code {
        40164 | 61004 => {
            // errmsg 形如 "invalid ip 1.2.3.4 ipv6 ::ffff:1.2.3.4, not in whitelist"
            let ip = msg.split_whitespace().nth(2).unwrap_or("当前服务器 IP");
            format!("调用 IP {ip} 不在公众号 IP 白名单中：请在「设置与开发 → 基本配置 → IP 白名单」添加")
        }
        40013 => "AppID 无效".to_string(),
        40125 | 40001 => "AppSecret 无效，请在公众号后台重置后重新填写".to_string(),
        48001 => "公众号没有该接口权限（需要已认证的公众号）".to_string(),
        45009 => "access_token 调用次数已达当日上限".to_string(),
        45011 => "调用过于频繁，请稍后再试".to_string(),
        _ => String::new(),
    };
    if hint.is_empty() {
        bail!("微信接口错误 {code}：{msg}")
    }
    bail!("{hint}（微信错误 {code}）")
}

/// 每次请求读取当前配置：独立部署时是固定配置，嵌入桌面端时从数据库读（可随时修改）。
/// 返回 `Err(原因)` 表示当前不提供签名（例如未启用）。
pub trait ConfigProvider: Send + Sync + 'static {
    fn current(&self) -> std::result::Result<SignerConfig, String>;
}

impl ConfigProvider for SignerConfig {
    fn current(&self) -> std::result::Result<SignerConfig, String> {
        Ok(self.clone())
    }
}

#[derive(Clone)]
struct AppState {
    provider: Arc<dyn ConfigProvider>,
    cache: Arc<TicketCache>,
}

#[derive(Deserialize)]
struct SignQuery {
    #[serde(default)]
    url: String,
}

/// `GET /wx/jssdk?url=...` 与 `GET /wx/health`。
pub fn router(provider: Arc<dyn ConfigProvider>, cache: Arc<TicketCache>) -> Router {
    Router::new()
        .route("/wx/jssdk", get(sign_handler))
        .route("/wx/health", get(health_handler))
        .with_state(AppState { provider, cache })
}

async fn health_handler(State(state): State<AppState>) -> impl IntoResponse {
    let configured = state
        .provider
        .current()
        .map(|c| !c.app_id.trim().is_empty() && !c.app_secret.trim().is_empty())
        .unwrap_or(false);
    Json(json!({
        "status": "ok",
        "service": "wechat-signer",
        "version": env!("CARGO_PKG_VERSION"),
        "configured": configured,
    }))
}

async fn sign_handler(State(state): State<AppState>, headers: HeaderMap, Query(query): Query<SignQuery>) -> Response {
    let config = match state.provider.current() {
        Ok(config) => config,
        Err(reason) => return error(StatusCode::SERVICE_UNAVAILABLE, &reason, None),
    };
    // 页面通过 fetch 跨域调用：只给允许的域名回 CORS 头。
    let origin = headers
        .get(header::ORIGIN)
        .and_then(|v| v.to_str().ok())
        .filter(|origin| domain_allowed(&config.allowed_domains, origin))
        .map(str::to_string);
    match state.cache.signature(&config, query.url.trim()).await {
        Ok(signature) => {
            let mut response = Json(signature).into_response();
            apply_cors(&mut response, origin.as_deref());
            response
        }
        Err(err) => {
            tracing::warn!("sign {} failed: {err:#}", query.url);
            error(StatusCode::BAD_REQUEST, &format!("{err:#}"), origin.as_deref())
        }
    }
}

fn error(status: StatusCode, message: &str, origin: Option<&str>) -> Response {
    let mut response = (status, Json(json!({ "error": message }))).into_response();
    apply_cors(&mut response, origin);
    response
}

fn apply_cors(response: &mut Response, origin: Option<&str>) {
    let headers = response.headers_mut();
    headers.insert(header::CACHE_CONTROL, HeaderValue::from_static("no-store"));
    if let Some(value) = origin.and_then(|o| HeaderValue::from_str(o).ok()) {
        headers.insert(header::ACCESS_CONTROL_ALLOW_ORIGIN, value);
        headers.insert(header::VARY, HeaderValue::from_static("Origin"));
    }
}

/// 发布页面里调用签名服务并设置分享卡片的脚本（只在微信内置浏览器里生效）。
pub fn page_script(signer_url: &str, title: &str, desc: &str, link: &str, image: &str) -> String {
    let data = json!({ "signer": signer_url.trim(), "title": title, "desc": desc, "link": link, "imgUrl": image });
    let data = data.to_string().replace("</", "<\\/");
    format!(
        concat!(
            "<script src=\"https://res.wx.qq.com/open/js/jweixin-1.6.0.js\"></script>\n",
            "<script>(function(){{var d={data};",
            "if(!/MicroMessenger/i.test(navigator.userAgent)||!window.wx||!d.signer)return;",
            "var page=location.href.split('#')[0];",
            "fetch(d.signer+(d.signer.indexOf('?')<0?'?':'&')+'url='+encodeURIComponent(page))",
            ".then(function(r){{return r.json();}}).then(function(c){{if(!c||!c.signature)return;",
            "wx.config({{debug:false,appId:c.appId,timestamp:c.timestamp,nonceStr:c.nonceStr,signature:c.signature,",
            "jsApiList:['updateAppMessageShareData','updateTimelineShareData','onMenuShareAppMessage','onMenuShareTimeline']}});",
            "wx.ready(function(){{var s={{title:d.title,desc:d.desc,link:d.link,imgUrl:d.imgUrl}},t={{title:d.title,link:d.link,imgUrl:d.imgUrl}};",
            // 新接口在测试号、部分安卓机和 PC 微信上会 permission denied：失败时改用旧接口
            "function legacy(){{if(wx.onMenuShareAppMessage){{wx.onMenuShareAppMessage(s);wx.onMenuShareTimeline(t);}}}}",
            "if(wx.updateAppMessageShareData){{wx.updateAppMessageShareData(Object.assign({{}},s,{{fail:legacy}}));wx.updateTimelineShareData(Object.assign({{}},t,{{fail:legacy}}));}}",
            "else{{legacy();}}}});",
            "}})['catch'](function(){{}});}})();</script>\n"
        ),
        data = data
    )
}

/// 运行签名服务，直到收到 Ctrl+C / SIGTERM（容器停止时优雅退出）。
pub async fn serve(provider: Arc<dyn ConfigProvider>, listen: std::net::SocketAddr) -> Result<()> {
    let app = router(provider, Arc::new(TicketCache::new(DEFAULT_API_BASE)));
    let listener = tokio::net::TcpListener::bind(listen)
        .await
        .with_context(|| format!("监听 {listen} 失败"))?;
    tracing::info!("wechat-signer listening on http://{listen}/wx/jssdk");
    axum::serve(listener, app).with_graceful_shutdown(shutdown_signal()).await?;
    tracing::info!("wechat-signer stopped");
    Ok(())
}

async fn shutdown_signal() {
    let ctrl_c = async {
        let _ = tokio::signal::ctrl_c().await;
    };
    #[cfg(unix)]
    let terminate = async {
        if let Ok(mut signal) = tokio::signal::unix::signal(tokio::signal::unix::SignalKind::terminate()) {
            signal.recv().await;
        }
    };
    #[cfg(not(unix))]
    let terminate = std::future::pending::<()>();
    tokio::select! {
        _ = ctrl_c => {},
        _ = terminate => {},
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn signature_matches_wechat_reference() {
        // 微信官方文档《JS-SDK 说明文档 · 附录 1》中的示例
        let signature = sign(
            "sM4AOVdWfPE4DxkXGEs8VMCPGGVi4C3VM0P37wVUCFvkVAy_90u5h9nbSlYy3-Sl-HhTdfl2fzFy1AOcHKP7qg",
            "Wm3WZYTPz0wzccnW",
            1414587457,
            "http://mp.weixin.qq.com?params=value#hash",
        );
        assert_eq!(signature, "0f9de62fce790f9a083d5c99e95740ceb90c27ed");
    }

    #[test]
    fn domain_rules_match_hosts() {
        assert!(domain_allowed("", "https://anything.test/a"));
        assert!(domain_allowed("love.example.com", "https://love.example.com/a/index.html"));
        assert!(domain_allowed("example.com, foo.test", "https://cdn.example.com/x"));
        assert!(domain_allowed("*.example.com", "https://example.com"));
        assert!(!domain_allowed("example.com", "https://evil-example.com/x"));
        assert!(!domain_allowed("example.com", "not a url"));
        assert_eq!(domain_rules(" a.com ;*.b.com，c.com\n"), vec!["a.com", "b.com", "c.com"]);
    }

    #[test]
    fn page_script_escapes_values() {
        let script = page_script("https://sign.test/wx/jssdk", "标题</script><b>", "描述", "https://x.test/a/", "https://x.test/a/p.jpg");
        assert!(script.contains("jweixin-1.6.0.js"));
        assert!(!script.contains("标题</script>"));
        assert!(script.contains("标题<\\/script><b>"));
        // 新接口被拒绝时退回旧接口
        assert!(script.contains("fail:legacy") && script.contains("wx.onMenuShareAppMessage(s)"));
    }
}

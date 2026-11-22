//! Cloudflare 一键授权：OAuth 2.0 授权码 + PKCE（桌面应用，无 client secret）。
//!
//! 授权一次即可用于 R2 / Pages / Tunnel：自动发现账户，自动创建 bucket、Pages 项目和命名隧道，
//! 发布目标只需要填名称。OAuth 不可用时退回到「预填权限的 API Token 链接」，用户粘贴 token 即可，
//! 两种方式最终都存成同一个 `CloudflareAuth`。
//!
//! 需要在你自己的 Cloudflare 账户里注册一个 OAuth client（Manage Account → OAuth clients）：
//! Grant type = authorization_code，Token auth method = none（PKCE），
//! Redirect URL = `http://localhost:8976/oauth/callback`，scopes 见 [`OAUTH_SCOPES`]。
//! client ID 通过设置界面、环境变量 `EASYTIDY_CF_CLIENT_ID` 或编译期同名环境变量提供。

use std::time::Duration;

use anyhow::{anyhow, bail, Context, Result};
use base64::Engine as _;
use rand::RngCore;
use reqwest::Client;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use sha2::{Digest, Sha256};

pub const API: &str = "https://api.cloudflare.com/client/v4";
pub const AUTHORIZE_URL: &str = "https://dash.cloudflare.com/oauth2/auth";
pub const TOKEN_URL: &str = "https://dash.cloudflare.com/oauth2/token";
pub const REVOKE_URL: &str = "https://dash.cloudflare.com/oauth2/revoke";
pub const CALLBACK_PORT: u16 = 8976;
pub const REDIRECT_URI: &str = "http://localhost:8976/oauth/callback";

/// OAuth 需要的权限：账户信息、Pages、R2、Tunnel、DNS（为隧道建 CNAME）、域名列表，以及刷新令牌。
pub const OAUTH_SCOPES: [&str; 8] = [
    "account-settings.read",
    "memberships.read",
    "pages.write",
    "workers-r2.write",
    "argotunnel.write",
    "zone.read",
    "dns.write",
    "offline_access",
];

/// 预填权限的 API Token 链接（OAuth 不可用时的兜底）：用户在 Cloudflare 页面点「创建」后粘贴 token。
pub fn token_template_url() -> String {
    let permissions = json!([
        { "key": "account_settings", "type": "read" },
        { "key": "page", "type": "edit" },
        { "key": "workers_r2", "type": "edit" },
        { "key": "argo_tunnel", "type": "edit" },
        { "key": "zone", "type": "read" },
        { "key": "dns", "type": "edit" }
    ]);
    format!(
        "https://dash.cloudflare.com/profile/api-tokens?permissionGroupKeys={}&accountId=*&zoneId=all&name={}",
        urlencode(&permissions.to_string()),
        urlencode("EasyTidy Connect")
    )
}

fn urlencode(value: &str) -> String {
    url::form_urlencoded::byte_serialize(value.as_bytes()).collect()
}

/// 已保存的 Cloudflare 授权（OAuth 或 API Token），所有 Cloudflare 发布目标共用。
#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq)]
#[serde(default)]
pub struct CloudflareAuth {
    /// `oauth` 或 `token`
    pub method: String,
    pub access_token: String,
    pub refresh_token: String,
    /// 访问令牌过期时间（unix 秒）；API Token 为 0（不过期）
    pub expires_at: i64,
    pub account_id: String,
    pub account_name: String,
    /// 授权时使用的 OAuth client ID（刷新令牌时需要）
    pub client_id: String,
}

impl CloudflareAuth {
    pub fn is_connected(&self) -> bool {
        !self.access_token.trim().is_empty() && !self.account_id.trim().is_empty()
    }
}

/// 设置界面展示用（不含令牌）。
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct CloudflareStatus {
    pub connected: bool,
    pub method: String,
    pub account_id: String,
    pub account_name: String,
    /// 是否配置了 OAuth client ID（没有时界面只显示 Token 方式）
    pub oauth_available: bool,
    pub client_id: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct Account {
    pub id: String,
    pub name: String,
}

/// 内置 / 环境变量里的 OAuth client ID。
pub fn default_client_id() -> String {
    std::env::var("EASYTIDY_CF_CLIENT_ID")
        .ok()
        .or_else(|| option_env!("EASYTIDY_CF_CLIENT_ID").map(str::to_string))
        .unwrap_or_default()
        .trim()
        .to_string()
}

/* ---------- OAuth + PKCE ---------- */

/// 一次授权请求：浏览器打开 `url`，回调后用 `verifier` 换取令牌。
pub struct PendingAuthorization {
    pub url: String,
    verifier: String,
    state: String,
    client_id: String,
}

fn random_urlsafe(bytes: usize) -> String {
    let mut buf = vec![0u8; bytes];
    rand::thread_rng().fill_bytes(&mut buf);
    base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(buf)
}

/// 生成授权地址（S256 PKCE + 随机 state）。
pub fn begin_authorization(client_id: &str) -> Result<PendingAuthorization> {
    let client_id = client_id.trim();
    if client_id.is_empty() {
        bail!("未配置 Cloudflare OAuth client ID");
    }
    let verifier = random_urlsafe(48);
    let challenge = base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(Sha256::digest(verifier.as_bytes()));
    let state = random_urlsafe(24);
    let url = url::Url::parse_with_params(
        AUTHORIZE_URL,
        &[
            ("response_type", "code"),
            ("client_id", client_id),
            ("redirect_uri", REDIRECT_URI),
            ("scope", &OAUTH_SCOPES.join(" ")),
            ("state", &state),
            ("code_challenge", &challenge),
            ("code_challenge_method", "S256"),
        ],
    )?
    .to_string();
    Ok(PendingAuthorization {
        url,
        verifier,
        state,
        client_id: client_id.to_string(),
    })
}

const CALLBACK_PAGE: &str = r#"<!doctype html><html lang="zh-CN"><meta charset="utf-8"><title>EasyTidy Connect</title>
<body style="font-family:system-ui,-apple-system,'Microsoft YaHei',sans-serif;display:grid;place-items:center;height:100vh;margin:0;background:#f6f7f9">
<div style="text-align:center"><h2>{{TITLE}}</h2><p style="color:#666">{{TEXT}}</p></div></body></html>"#;

/// 在本机 8976 端口等待 Cloudflare 回调，拿到授权码（最多等 5 分钟）。
pub async fn wait_for_callback(pending: &PendingAuthorization) -> Result<String> {
    use tokio::io::{AsyncReadExt, AsyncWriteExt};
    let listener = tokio::net::TcpListener::bind(("127.0.0.1", CALLBACK_PORT))
        .await
        .with_context(|| format!("端口 {CALLBACK_PORT} 被占用（可能 wrangler login 正在运行），请稍后重试"))?;
    let deadline = tokio::time::Instant::now() + Duration::from_secs(300);
    loop {
        let (mut stream, _) = tokio::time::timeout_at(deadline, listener.accept())
            .await
            .map_err(|_| anyhow!("等待授权超时（5 分钟），请重新点击授权"))??;
        let mut buf = vec![0u8; 8192];
        let n = stream.read(&mut buf).await.unwrap_or(0);
        let request = String::from_utf8_lossy(&buf[..n]);
        let path = request.split_whitespace().nth(1).unwrap_or("/");
        let Ok(url) = url::Url::parse(&format!("http://localhost{path}")) else { continue };
        if url.path() != "/oauth/callback" {
            let _ = stream.write_all(b"HTTP/1.1 404 Not Found\r\nContent-Length: 0\r\n\r\n").await;
            continue;
        }
        let params: std::collections::HashMap<String, String> = url.query_pairs().into_owned().collect();
        let outcome = if params.get("state") != Some(&pending.state) {
            Err(anyhow!("授权回调的 state 不匹配，已忽略"))
        } else if let Some(err) = params.get("error") {
            Err(anyhow!(
                "Cloudflare 拒绝了授权：{} {}",
                err,
                params.get("error_description").map(String::as_str).unwrap_or("")
            ))
        } else {
            params.get("code").cloned().ok_or_else(|| anyhow!("回调里没有授权码"))
        };
        let (title, text) = match &outcome {
            Ok(_) => ("授权成功", "可以关闭这个页面，回到 EasyTidy Connect。"),
            Err(_) => ("授权失败", "请回到 EasyTidy Connect 查看原因，或改用 API Token。"),
        };
        let body = CALLBACK_PAGE.replace("{{TITLE}}", title).replace("{{TEXT}}", text);
        let response = format!(
            "HTTP/1.1 200 OK\r\nContent-Type: text/html; charset=utf-8\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
            body.len()
        );
        let _ = stream.write_all(response.as_bytes()).await;
        return outcome;
    }
}

#[derive(Deserialize)]
struct TokenResponse {
    access_token: String,
    #[serde(default)]
    refresh_token: Option<String>,
    #[serde(default)]
    expires_in: Option<i64>,
}

async fn token_request(form: &[(&str, &str)]) -> Result<TokenResponse> {
    let response = http()?.post(TOKEN_URL).form(form).send().await.context("请求 Cloudflare 令牌失败")?;
    let status = response.status();
    let body = response.text().await.unwrap_or_default();
    if !status.is_success() {
        bail!("Cloudflare 令牌接口返回 {status}：{}", body.chars().take(300).collect::<String>());
    }
    serde_json::from_str(&body).context("Cloudflare 令牌响应格式不正确")
}

fn now() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0)
}

/// 用授权码换取令牌。
pub async fn exchange_code(pending: &PendingAuthorization, code: &str) -> Result<CloudflareAuth> {
    let token = token_request(&[
        ("grant_type", "authorization_code"),
        ("code", code),
        ("redirect_uri", REDIRECT_URI),
        ("client_id", &pending.client_id),
        ("code_verifier", &pending.verifier),
    ])
    .await?;
    Ok(CloudflareAuth {
        method: "oauth".to_string(),
        access_token: token.access_token,
        refresh_token: token.refresh_token.unwrap_or_default(),
        expires_at: token.expires_in.map(|s| now() + s).unwrap_or(0),
        client_id: pending.client_id.clone(),
        ..Default::default()
    })
}

/// OAuth 令牌快过期时自动刷新；返回 true 表示令牌有更新，调用方需保存。
pub async fn refresh_if_needed(auth: &mut CloudflareAuth) -> Result<bool> {
    if auth.method != "oauth" || auth.expires_at == 0 || auth.expires_at > now() + 120 {
        return Ok(false);
    }
    if auth.refresh_token.is_empty() {
        bail!("Cloudflare 授权已过期，请重新授权");
    }
    let token = token_request(&[
        ("grant_type", "refresh_token"),
        ("refresh_token", &auth.refresh_token),
        ("client_id", &auth.client_id),
    ])
    .await
    .context("刷新 Cloudflare 授权失败，请重新授权")?;
    auth.access_token = token.access_token;
    if let Some(refresh) = token.refresh_token {
        auth.refresh_token = refresh;
    }
    auth.expires_at = token.expires_in.map(|s| now() + s).unwrap_or(0);
    Ok(true)
}

/// 撤销 OAuth 令牌（断开连接时调用，失败不影响本地清除）。
pub async fn revoke(auth: &CloudflareAuth) {
    if auth.method != "oauth" || auth.refresh_token.is_empty() {
        return;
    }
    if let Ok(client) = http() {
        let _ = client
            .post(REVOKE_URL)
            .form(&[("token", auth.refresh_token.as_str()), ("client_id", auth.client_id.as_str())])
            .send()
            .await;
    }
}

/* ---------- REST API ---------- */

fn http() -> Result<Client> {
    Ok(Client::builder()
        .timeout(Duration::from_secs(60))
        .user_agent(concat!("EasyTidyConnect/", env!("CARGO_PKG_VERSION")))
        .build()?)
}

/// 带令牌的 Cloudflare API 客户端。`base` 可替换为测试用的模拟服务。
pub struct Api {
    client: Client,
    base: String,
    token: String,
}

impl Api {
    pub fn new(token: &str) -> Result<Self> {
        Self::with_base(API, token)
    }

    pub fn with_base(base: &str, token: &str) -> Result<Self> {
        if token.trim().is_empty() {
            bail!("缺少 Cloudflare 令牌");
        }
        Ok(Self {
            client: http()?,
            base: base.trim_end_matches('/').to_string(),
            token: token.trim().to_string(),
        })
    }

    async fn call(&self, method: reqwest::Method, path: &str, body: Option<Value>) -> Result<Value> {
        let mut request = self.client.request(method, format!("{}{path}", self.base)).bearer_auth(&self.token);
        if let Some(body) = body {
            request = request.json(&body);
        }
        let response = request.send().await.with_context(|| format!("请求 Cloudflare API {path} 失败"))?;
        let status = response.status();
        let value: Value = response.json().await.unwrap_or(Value::Null);
        if value.get("success").and_then(Value::as_bool) == Some(true) {
            return Ok(value.get("result").cloned().unwrap_or(Value::Null));
        }
        Err(api_error(status, &value, path))
    }

    pub async fn get(&self, path: &str) -> Result<Value> {
        self.call(reqwest::Method::GET, path, None).await
    }

    pub async fn post(&self, path: &str, body: Value) -> Result<Value> {
        self.call(reqwest::Method::POST, path, Some(body)).await
    }

    pub async fn put(&self, path: &str, body: Value) -> Result<Value> {
        self.call(reqwest::Method::PUT, path, Some(body)).await
    }

    /// 上传 R2 对象（Cloudflare REST API，与 wrangler 相同；不需要 S3 密钥）。
    pub async fn put_object(&self, account: &str, bucket: &str, key: &str, bytes: Vec<u8>, content_type: &str) -> Result<()> {
        let encoded: String = key
            .split('/')
            .map(|part| percent_encoding::utf8_percent_encode(part, percent_encoding::NON_ALPHANUMERIC).to_string())
            .collect::<Vec<_>>()
            .join("/");
        let path = format!("/accounts/{account}/r2/buckets/{bucket}/objects/{encoded}");
        let response = self
            .client
            .put(format!("{}{path}", self.base))
            .bearer_auth(&self.token)
            .header(reqwest::header::CONTENT_TYPE, content_type)
            .body(bytes)
            .send()
            .await
            .with_context(|| format!("上传 {key} 失败"))?;
        let status = response.status();
        if status.is_success() {
            return Ok(());
        }
        let value: Value = response.json().await.unwrap_or(Value::Null);
        Err(api_error(status, &value, &path))
    }

    /// 令牌可访问的账户。
    pub async fn accounts(&self) -> Result<Vec<Account>> {
        let result = self.get("/accounts?per_page=50").await?;
        Ok(result
            .as_array()
            .map(|items| {
                items
                    .iter()
                    .filter_map(|item| {
                        Some(Account {
                            id: item.get("id")?.as_str()?.to_string(),
                            name: item.get("name").and_then(Value::as_str).unwrap_or_default().to_string(),
                        })
                    })
                    .collect()
            })
            .unwrap_or_default())
    }
}

fn api_error(status: reqwest::StatusCode, value: &Value, path: &str) -> anyhow::Error {
    let errors = value
        .get("errors")
        .and_then(Value::as_array)
        .map(|items| {
            items
                .iter()
                .map(|e| {
                    format!(
                        "{} {}",
                        e.get("code").and_then(Value::as_i64).unwrap_or_default(),
                        e.get("message").and_then(Value::as_str).unwrap_or_default()
                    )
                })
                .collect::<Vec<_>>()
                .join("；")
        })
        .unwrap_or_default();
    let hint = match status.as_u16() {
        401 => "（授权已失效，请重新授权）",
        403 => "（授权缺少所需权限，请重新授权或在 Token 中勾选对应权限）",
        _ => "",
    };
    anyhow!("Cloudflare API {path} 返回 {status}{hint}：{errors}")
}

/// 是否已存在（Cloudflare 对重复创建返回 409 或特定错误码）。
fn already_exists(err: &anyhow::Error) -> bool {
    let text = err.to_string();
    text.contains("409") || text.contains("already exists") || text.contains("10004") || text.contains("8000002")
}

/* ---------- 发布目标：使用已授权账户 ---------- */

/// 发布目标是否使用「已授权的 Cloudflare 账户」（标记存在 tunnel_config JSON 里，不需要新增数据库列）。
pub fn uses_auth(target: &crate::models::PublishTarget) -> bool {
    serde_json::from_str::<Value>(&target.tunnel_config)
        .ok()
        .and_then(|v| v.get("cf_auth").and_then(Value::as_bool))
        .unwrap_or(false)
}

/// R2 通过授权发布：上传整个项目目录到 bucket，返回公开地址。
pub async fn publish_r2(
    api: &Api,
    account: &str,
    target: &crate::models::PublishTarget,
    project: &crate::models::AnniversaryProject,
    html: &str,
    photos_dir: &std::path::Path,
    media_dir: &std::path::Path,
) -> Result<crate::models::PublishResult> {
    let bucket = target.bucket.trim();
    let base_key = crate::publish::base_key(target, &project.slug);
    let mut uploaded = 0usize;
    for (dir, folder) in [(photos_dir, "photos"), (media_dir, "media")] {
        if !dir.exists() {
            continue;
        }
        for entry in std::fs::read_dir(dir)? {
            let entry = entry?;
            if !entry.file_type()?.is_file() {
                continue;
            }
            let name = entry.file_name().to_string_lossy().to_string();
            let content_type = crate::publish::content_type(&name);
            api.put_object(account, bucket, &format!("{base_key}/{folder}/{name}"), std::fs::read(entry.path())?, content_type)
                .await?;
            uploaded += 1;
        }
    }
    api.put_object(
        account,
        bucket,
        &format!("{base_key}/{}", crate::share::SHARE_THUMB_NAME),
        crate::share::share_thumbnail(project, photos_dir),
        "image/jpeg",
    )
    .await?;
    api.put_object(account, bucket, &format!("{base_key}/index.html"), html.as_bytes().to_vec(), "text/html; charset=utf-8")
        .await?;
    Ok(crate::models::PublishResult {
        url: crate::publish::public_url(target, &format!("{base_key}/index.html")),
        slug: project.slug.clone(),
        uploaded: uploaded + 2,
    })
}

/* ---------- 自动创建资源（幂等：已存在就复用） ---------- */

/// 资源名规范化：小写字母、数字和 -，用于 bucket / Pages 项目 / 隧道名。
pub fn slug_name(value: &str, fallback: &str) -> String {
    let mut out = String::new();
    for ch in value.trim().to_ascii_lowercase().chars() {
        if ch.is_ascii_alphanumeric() {
            out.push(ch);
        } else if !out.ends_with('-') && !out.is_empty() {
            out.push('-');
        }
    }
    let out = out.trim_matches('-').chars().take(58).collect::<String>();
    if out.len() < 3 {
        fallback.to_string()
    } else {
        out
    }
}

/// 确保 R2 bucket 存在并开启 r2.dev 公开访问，返回公开访问的基础地址（https://pub-xxx.r2.dev）。
pub async fn ensure_r2_bucket(api: &Api, account: &str, bucket: &str) -> Result<String> {
    match api.post(&format!("/accounts/{account}/r2/buckets"), json!({ "name": bucket })).await {
        Ok(_) => {}
        Err(err) if already_exists(&err) => {}
        Err(err) => return Err(err.context(format!("创建 R2 bucket {bucket} 失败"))),
    }
    let managed = api
        .put(&format!("/accounts/{account}/r2/buckets/{bucket}/domains/managed"), json!({ "enabled": true }))
        .await
        .context("开启 R2 公开访问（r2.dev）失败")?;
    let domain = managed
        .get("domain")
        .and_then(Value::as_str)
        .context("Cloudflare 没有返回 r2.dev 域名")?;
    Ok(format!("https://{}", domain.trim_end_matches('/')))
}

/// 确保 Pages 项目存在（直传项目，生产分支 main），返回 `{项目}.pages.dev` 子域名。
pub async fn ensure_pages_project(api: &Api, account: &str, project: &str) -> Result<String> {
    let existing = api.get(&format!("/accounts/{account}/pages/projects/{project}")).await;
    let result = match existing {
        Ok(value) => value,
        Err(_) => api
            .post(
                &format!("/accounts/{account}/pages/projects"),
                json!({ "name": project, "production_branch": "main" }),
            )
            .await
            .with_context(|| format!("创建 Pages 项目 {project} 失败（名称可能已被他人占用，换一个名称）"))?,
    };
    Ok(result
        .get("subdomain")
        .and_then(Value::as_str)
        .map(str::to_string)
        .unwrap_or_else(|| format!("{project}.pages.dev")))
}

#[derive(Debug, Clone, PartialEq)]
pub struct TunnelSetup {
    pub tunnel_id: String,
    /// cloudflared tunnel run 用的令牌
    pub token: String,
    pub hostname: String,
}

/// 创建（或复用）远程配置的命名隧道，把 `hostname` 指向本机 `service`，并创建 DNS CNAME。
pub async fn ensure_tunnel(api: &Api, account: &str, name: &str, hostname: &str, service: &str) -> Result<TunnelSetup> {
    let hostname = hostname.trim().trim_start_matches("https://").trim_end_matches('/').to_ascii_lowercase();
    if hostname.is_empty() || !hostname.contains('.') {
        bail!("请填写要使用的域名，例如 love.example.com（需在该 Cloudflare 账户下）");
    }

    // 1. 隧道：同名未删除的隧道直接复用
    let list = api
        .get(&format!("/accounts/{account}/cfd_tunnel?name={}&is_deleted=false", urlencode(name)))
        .await
        .unwrap_or(Value::Null);
    let tunnel_id = match list.as_array().and_then(|items| items.first()).and_then(|t| t.get("id")).and_then(Value::as_str) {
        Some(id) => id.to_string(),
        None => {
            let mut secret = [0u8; 32];
            rand::thread_rng().fill_bytes(&mut secret);
            let created = api
                .post(
                    &format!("/accounts/{account}/cfd_tunnel"),
                    json!({
                        "name": name,
                        "config_src": "cloudflare",
                        "tunnel_secret": base64::engine::general_purpose::STANDARD.encode(secret),
                    }),
                )
                .await
                .context("创建 Cloudflare Tunnel 失败")?;
            created.get("id").and_then(Value::as_str).context("创建隧道后没有返回 id")?.to_string()
        }
    };

    // 2. 路由：hostname → 本机公开预览端口，其余请求 404
    api.put(
        &format!("/accounts/{account}/cfd_tunnel/{tunnel_id}/configurations"),
        json!({ "config": { "ingress": [
            { "hostname": hostname, "service": service },
            { "service": "http_status:404" }
        ]}}),
    )
    .await
    .context("配置隧道路由失败")?;

    // 3. DNS：在对应域名下创建 CNAME → {tunnel}.cfargotunnel.com
    let zone_id = find_zone(api, account, &hostname).await?;
    let target = format!("{tunnel_id}.cfargotunnel.com");
    let records = api
        .get(&format!("/zones/{zone_id}/dns_records?name={}", urlencode(&hostname)))
        .await
        .unwrap_or(Value::Null);
    let record = json!({ "type": "CNAME", "name": hostname, "content": target, "proxied": true, "comment": "EasyTidy Connect tunnel" });
    match records.as_array().and_then(|items| items.first()) {
        Some(existing) => {
            let same = existing.get("type").and_then(Value::as_str) == Some("CNAME")
                && existing.get("content").and_then(Value::as_str) == Some(target.as_str());
            if !same {
                let id = existing.get("id").and_then(Value::as_str).unwrap_or_default();
                // 只覆盖指向其他隧道的 CNAME，不动用户已有的 A 记录等
                let is_tunnel = existing
                    .get("content")
                    .and_then(Value::as_str)
                    .is_some_and(|c| c.ends_with(".cfargotunnel.com"));
                if !is_tunnel {
                    bail!("{hostname} 已有 DNS 记录（{}），为避免覆盖请换一个子域名", existing.get("type").and_then(Value::as_str).unwrap_or("?"));
                }
                api.put(&format!("/zones/{zone_id}/dns_records/{id}"), record).await.context("更新 DNS 记录失败")?;
            }
        }
        None => {
            api.post(&format!("/zones/{zone_id}/dns_records"), record).await.context("创建 DNS 记录失败")?;
        }
    }

    // 4. 运行令牌
    let token = api
        .get(&format!("/accounts/{account}/cfd_tunnel/{tunnel_id}/token"))
        .await
        .context("获取隧道令牌失败")?
        .as_str()
        .context("隧道令牌格式不正确")?
        .to_string();
    Ok(TunnelSetup { tunnel_id, token, hostname })
}

/// 找到 hostname 所属的域名（zone）：从完整域名逐级去掉前缀匹配。
async fn find_zone(api: &Api, account: &str, hostname: &str) -> Result<String> {
    let labels: Vec<&str> = hostname.split('.').collect();
    for start in 0..labels.len().saturating_sub(1) {
        let candidate = labels[start..].join(".");
        let zones = api
            .get(&format!("/zones?name={}&account.id={account}", urlencode(&candidate)))
            .await
            .unwrap_or(Value::Null);
        if let Some(id) = zones.as_array().and_then(|z| z.first()).and_then(|z| z.get("id")).and_then(Value::as_str) {
            return Ok(id.to_string());
        }
    }
    bail!("{hostname} 不在这个 Cloudflare 账户的域名下，请先把域名添加到 Cloudflare")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn builds_pkce_authorization_url() {
        let pending = begin_authorization("client-123").unwrap();
        let url = url::Url::parse(&pending.url).unwrap();
        let q: std::collections::HashMap<_, _> = url.query_pairs().into_owned().collect();
        assert_eq!(url.as_str().split('?').next().unwrap(), AUTHORIZE_URL);
        assert_eq!(q["client_id"], "client-123");
        assert_eq!(q["redirect_uri"], REDIRECT_URI);
        assert_eq!(q["code_challenge_method"], "S256");
        let expected = base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(Sha256::digest(pending.verifier.as_bytes()));
        assert_eq!(q["code_challenge"], expected);
        assert!(q["scope"].contains("pages.write") && q["scope"].contains("offline_access"));
        assert!(begin_authorization(" ").is_err());
    }

    #[test]
    fn token_template_has_required_permissions() {
        let url = token_template_url();
        assert!(url.starts_with("https://dash.cloudflare.com/profile/api-tokens?permissionGroupKeys="));
        let decoded: String = url::form_urlencoded::parse(url.split('?').nth(1).unwrap().as_bytes())
            .find(|(k, _)| k == "permissionGroupKeys")
            .unwrap()
            .1
            .into_owned();
        let parsed: Value = serde_json::from_str(&decoded).unwrap();
        let keys: Vec<&str> = parsed.as_array().unwrap().iter().map(|p| p["key"].as_str().unwrap()).collect();
        assert_eq!(keys, ["account_settings", "page", "workers_r2", "argo_tunnel", "zone", "dns"]);
    }

    #[test]
    fn slug_names() {
        assert_eq!(slug_name("My Love Cards!", "x"), "my-love-cards");
        assert_eq!(slug_name("--A__B--", "fallback"), "a-b");
        assert_eq!(slug_name("纪念日", "easytidy-connect"), "easytidy-connect");
    }
}

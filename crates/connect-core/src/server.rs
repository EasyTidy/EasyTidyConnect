use std::net::UdpSocket;
use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;

use anyhow::Result;
use axum::{
    extract::{Path, Query, State},
    http::StatusCode,
    response::{Html, IntoResponse, Response},
    routing::get,
    Json, Router,
};
use reqwest::Client;
use serde::Deserialize;
use serde_json::json;

use crate::anniversary::render_with_db;
use crate::bridge::BridgeClient;

/// 公开预览端口：只暴露 /a/*，供隧道使用。
pub const PUBLIC_PREVIEW_PORT: u16 = 53410;
use crate::db::Database;
use crate::models::{Settings, TagPayload};

pub fn local_ip() -> String {    UdpSocket::bind("0.0.0.0:0")
        .and_then(|socket| {
            socket.connect("8.8.8.8:80")?;
            socket.local_addr()
        })
        .map(|addr| addr.ip().to_string())
        .unwrap_or_else(|_| "127.0.0.1".to_string())
}

pub fn tag_payload(db: &Database, trigger_id: i64, port: u16) -> Result<TagPayload> {    let row = db
        .find_by_id(trigger_id)?
        .ok_or_else(|| anyhow::anyhow!("trigger not found"))?;
    let host = local_ip();
    let base = format!("http://{host}:{port}");
    let uri = format!("{base}/t/{}?k={}", row.public_id, row.token);
    let write_page = format!("{base}/w/{}", row.write_token);

    Ok(TagPayload {
        trigger_id: row.id,
        trigger_name: row.name,
        bytes: crate::nfc::ndef_uri_size(&uri),
        uri,
        write_page,
        public_id: row.public_id,
        tag_written: row.tag_written,
    })
}

#[derive(Clone)]
struct ServerState {
    db: Arc<Database>,
    port: u16,
    data_dir: PathBuf,
    http: Client,
    tunnels: Arc<crate::tunnel::TunnelManager>,
}

#[derive(Deserialize)]
struct TriggerQuery {
    #[serde(default)]
    k: String,
}

pub async fn serve(
    db: Arc<Database>,
    port: u16,
    data_dir: PathBuf,
    tunnels: Arc<crate::tunnel::TunnelManager>,
) -> Result<()> {
    let state = Arc::new(ServerState {
        db,
        port,
        data_dir,
        http: Client::builder()
            .timeout(Duration::from_secs(30))
            .build()?,
        tunnels,
    });

    let app = Router::new()
        .route("/health", get(health))
        .route("/t/{public_id}", get(trigger))
        .route("/w/{write_token}", get(write_helper))
        .route("/nfc", get(nfc_link_helper))
        .route("/a/{slug}", get(anniversary_page))
        .route("/a/{slug}/photos/{file}", get(anniversary_photo))
        .route("/a/{slug}/media/{file}", get(anniversary_media))
        .with_state(state.clone())
        // 微信 JS-SDK 签名（本机签名，配合内网穿透做测试）
        .merge(crate::wechat::local_router(state.db.clone()));

    let listener = tokio::net::TcpListener::bind(("0.0.0.0", port))
        .await
        .map_err(|err| anyhow::anyhow!("bind 0.0.0.0:{port}: {err}"))?;

    tracing::info!("EasyTidy Connect listening on http://{}:{}", local_ip(), port);
    axum::serve(listener, app).await?;
    Ok(())
}

async fn health(State(state): State<Arc<ServerState>>) -> impl IntoResponse {
    Json(json!({
        "status": "ok",
        "app": "EasyTidy Connect",
        "version": env!("CARGO_PKG_VERSION"),
        "port": state.port,
        "ip": local_ip()
    }))
}

/// 公开预览服务：只暴露 /a/*，不暴露 /t 和 /w。
pub async fn serve_public(
    db: Arc<Database>,
    port: u16,
    data_dir: PathBuf,
) -> Result<()> {
    let state = Arc::new(ServerState {
        db,
        port,
        data_dir,
        http: Client::builder()
            .timeout(Duration::from_secs(30))
            .build()?,
        // 公开预览服务不启动隧道，这里只是占位。
        tunnels: Arc::new(crate::tunnel::TunnelManager::new(std::env::temp_dir())),
    });

    let app = Router::new()
        .route("/health", get(public_health))
        .route("/a/{slug}", get(anniversary_page))
        .route("/a/{slug}/photos/{file}", get(anniversary_photo))
        .route("/a/{slug}/media/{file}", get(anniversary_media))
        .with_state(state.clone())
        // 隧道暴露的公开端口也提供签名接口：签名地址填「隧道地址/wx/jssdk」即可本机签名
        .merge(crate::wechat::local_router(state.db.clone()));

    let listener = tokio::net::TcpListener::bind(("127.0.0.1", port))
        .await
        .map_err(|err| anyhow::anyhow!("bind public 127.0.0.1:{port}: {err}"))?;
    tracing::info!("public preview listening on http://127.0.0.1:{port}");
    axum::serve(listener, app).await?;
    Ok(())
}

async fn public_health() -> impl IntoResponse {
    Json(json!({"status": "ok", "public": true}))
}

#[derive(Clone)]
struct ExternalRun {
    bridge_url: String,
    bridge_token: String,
    run_id: String,
}

struct ActionOutcome {
    status: StatusCode,
    message: String,
    external: Option<ExternalRun>,
    /// 「打开网页」动作：触发成功后把手机浏览器跳转到该地址。
    redirect: Option<String>,
}

impl ActionOutcome {
    fn message(status: StatusCode, message: impl Into<String>) -> Self {
        Self {
            status,
            message: message.into(),
            external: None,
            redirect: None,
        }
    }
}

async fn trigger(
    State(state): State<Arc<ServerState>>,
    Path(public_id): Path<String>,
    Query(query): Query<TriggerQuery>,
) -> Response {
    let Some(row) = state.db.find_by_public_id(&public_id).ok().flatten() else {
        return result_page(StatusCode::NOT_FOUND, "未找到触发器", "标签内容无效或已被删除。");
    };

    if query.k != row.token {
        return result_page(StatusCode::UNAUTHORIZED, "无法触发", "标签已失效，请在 EasyTidy Connect 中重新写入。");
    }

    if !row.enabled {
        return result_page(StatusCode::FORBIDDEN, "触发器已停用", "请在 EasyTidy Connect 中启用该触发器。");
    }

    let run_id = match state.db.start_run(row.id, "nfc") {
        Ok(id) => id,
        Err(err) => {
            return result_page(StatusCode::INTERNAL_SERVER_ERROR, "执行失败", &err.to_string());
        }
    };

    let settings = match state.db.get_settings() {
        Ok(settings) => settings,
        Err(err) => {
            let _ = state.db.finish_run(run_id, "failed", Some(&err.to_string()));
            return result_page(StatusCode::INTERNAL_SERVER_ERROR, "执行失败", &err.to_string());
        }
    };

    let result = execute_action(&state, &row.action_type, &row.action_config, &settings).await;

    match result {
        Ok(outcome) => {
            if let Some(external) = outcome.external {
                let _ = state.db.set_external_run(run_id, &external.run_id);
                let _ = state.db.mark_verified(row.id);
                spawn_external_poll(state.db.clone(), external, run_id);
                result_page(outcome.status, &format!("已触发：{}", row.name), &outcome.message)
            } else {
                let _ = state.db.finish_run(run_id, "success", None);
                let _ = state.db.mark_verified(row.id);
                if let Some(url) = outcome.redirect {
                    // URL 含非 ASCII 字符时不能直接放进 Location 头，回退为结果页。
                    if let Ok(location) = axum::http::HeaderValue::from_str(&url) {
                        return (
                            StatusCode::SEE_OTHER,
                            [(axum::http::header::LOCATION, location)],
                        )
                            .into_response();
                    }
                }
                result_page(outcome.status, &format!("已触发：{}", row.name), &outcome.message)
            }
        }
        Err((status, message)) => {
            let _ = state.db.finish_run(run_id, "failed", Some(&message));
            result_page(status, "触发失败", &message)
        }
    }
}

fn spawn_external_poll(db: Arc<Database>, external: ExternalRun, run_id: i64) {
    tokio::spawn(async move {
        let client = match BridgeClient::new() {
            Ok(client) => client,
            Err(err) => {
                let _ = db.finish_run(run_id, "failed", Some(&format!("无法创建 Bridge 客户端：{err}")));
                return;
            }
        };

        // 最多轮询 10 分钟。
        for _ in 0..300 {
            tokio::time::sleep(std::time::Duration::from_secs(2)).await;
            match client
                .run_status(&external.bridge_url, &external.bridge_token, &external.run_id)
                .await
            {
                Ok(status) if status.status == "success" => {
                    let _ = db.finish_run(run_id, "success", None);
                    return;
                }
                Ok(status) if status.status == "failed" => {
                    let error = status.error.unwrap_or_else(|| "EasyTidy 运行失败".to_string());
                    let _ = db.finish_run(run_id, "failed", Some(&error));
                    return;
                }
                Ok(_) => {}
                Err(err) => {
                    tracing::debug!("bridge run status poll failed: {err}");
                }
            }
        }

        let _ = db.finish_run(run_id, "failed", Some("EasyTidy 运行超时（10 分钟）"));
    });
}

async fn execute_action(
    state: &ServerState,
    action_type: &str,
    action_config: &str,
    settings: &Settings,
) -> Result<ActionOutcome, (StatusCode, String)> {
    match action_type {
        "webhook" => {
            let config: serde_json::Value = serde_json::from_str(action_config)
                .map_err(|err| (StatusCode::BAD_REQUEST, format!("Webhook 配置 JSON 无效：{err}")))?;
            let url = config
                .get("url")
                .and_then(|value| value.as_str())
                .ok_or((StatusCode::BAD_REQUEST, "Webhook 缺少 url".to_string()))?;

            let response = state
                .http
                .post(url)
                .json(&json!({
                    "source": "easytidy-connect",
                    "event": "nfc-trigger",
                    "timestamp": chrono::Utc::now().to_rfc3339()
                }))
                .send()
                .await
                .map_err(|err| (StatusCode::BAD_GATEWAY, format!("Webhook 请求失败：{err}")))?;

            let code = response.status().as_u16();
            if response.status().is_success() {
                Ok(ActionOutcome::message(
                    StatusCode::OK,
                    format!("Webhook 已调用，HTTP {code}。"),
                ))
            } else {
                Err((StatusCode::BAD_GATEWAY, format!("Webhook 返回 HTTP {code}。")))
            }
        }
        "open_url" => {
            let config: serde_json::Value = serde_json::from_str(action_config)
                .map_err(|err| (StatusCode::BAD_REQUEST, format!("URL 配置 JSON 无效：{err}")))?;
            let url = config
                .get("url")
                .and_then(|value| value.as_str())
                .ok_or((StatusCode::BAD_REQUEST, "缺少 url".to_string()))?;
            let url = url.trim();
            if !(url.starts_with("http://") || url.starts_with("https://")) {
                return Err((
                    StatusCode::BAD_REQUEST,
                    "打开网页只支持 http:// 或 https:// 地址".to_string(),
                ));
            }
            let mut outcome = ActionOutcome::message(StatusCode::OK, format!("正在打开：{url}"));
            outcome.redirect = Some(url.to_string());
            Ok(outcome)
        }
        "easytidy_task" | "easytidy_workflow" => {
            let config: serde_json::Value = serde_json::from_str(action_config)
                .map_err(|err| (StatusCode::BAD_REQUEST, format!("EasyTidy 配置 JSON 无效：{err}")))?;
            let target_id = config
                .get("id")
                .and_then(|value| value.as_i64())
                .ok_or((StatusCode::BAD_REQUEST, "EasyTidy 动作缺少任务/工作流 id".to_string()))?;
            let name = config
                .get("name")
                .and_then(|value| value.as_str())
                .unwrap_or("未命名");

            if settings.bridge_url.trim().is_empty() || settings.bridge_token.trim().is_empty() {
                return Err((
                    StatusCode::BAD_REQUEST,
                    "还没有连接 EasyTidy，请先在设置中完成连接。".to_string(),
                ));
            }

            let client = BridgeClient::new()
                .map_err(|err| (StatusCode::INTERNAL_SERVER_ERROR, err.to_string()))?;

            let accepted = if action_type == "easytidy_task" {
                client
                    .run_task(&settings.bridge_url, &settings.bridge_token, target_id)
                    .await
            } else {
                client
                    .run_workflow(&settings.bridge_url, &settings.bridge_token, target_id)
                    .await
            }
            .map_err(|err| {
                (
                    StatusCode::BAD_GATEWAY,
                    format!("调用 EasyTidy Bridge 失败：{err}"),
                )
            })?;

            let kind = if action_type == "easytidy_task" {
                "任务"
            } else {
                "工作流"
            };
            let message = format!(
                "已提交 EasyTidy {kind}「{name}」，运行 ID {}，正在后台跟踪结果。",
                accepted.run_id
            );
            Ok(ActionOutcome {
                status: StatusCode::OK,
                message,
                external: Some(ExternalRun {
                    bridge_url: settings.bridge_url.clone(),
                    bridge_token: settings.bridge_token.clone(),
                    run_id: accepted.run_id,
                }),
                redirect: None,
            })
        }
        "publish_anniversary" => {
            let config: serde_json::Value = serde_json::from_str(action_config)
                .map_err(|err| (StatusCode::BAD_REQUEST, format!("纪念日配置 JSON 无效：{err}")))?;
            let project_id = config
                .get("project_id")
                .and_then(|value| value.as_i64())
                .ok_or((StatusCode::BAD_REQUEST, "缺少纪念日项目 id".to_string()))?;
            let target_id = config
                .get("target_id")
                .and_then(|value| value.as_i64())
                .ok_or((StatusCode::BAD_REQUEST, "缺少发布目标 id".to_string()))?;

            let project = state
                .db
                .get_project(project_id)
                .map_err(|err| (StatusCode::INTERNAL_SERVER_ERROR, err.to_string()))?
                .ok_or((StatusCode::NOT_FOUND, "纪念日项目不存在".to_string()))?;
            let target = state
                .db
                .get_publish_target(target_id)
                .map_err(|err| (StatusCode::INTERNAL_SERVER_ERROR, err.to_string()))?
                .ok_or((StatusCode::NOT_FOUND, "发布目标不存在".to_string()))?;

            if target.kind == "local" {
                let url = local_page_url(state.port, &project.slug);
                if latest_published_url(&state.db, &project, "local").as_deref() != Some(url.as_str()) {
                    let _ = state.db.add_project_version(project.id, &url, "local");
                }
                return Ok(open_page(url));
            }
            // 碰标签的目的是打开纪念日网页：隧道直接打开（内容实时）；静态托管只在内容有改动时才重新上传。
            if crate::tunnel::is_tunnel_kind(&target.kind) {
                let target = crate::resolve_cloudflare_target(&state.db, &target)
                    .await
                    .map_err(|err| (StatusCode::BAD_GATEWAY, format!("Cloudflare 授权：{err:#}")))?;
                let tunnel = state
                    .tunnels
                    .ensure(&target, PUBLIC_PREVIEW_PORT)
                    .await
                    .map_err(|err| (StatusCode::BAD_GATEWAY, format!("启动隧道失败：{err:#}")))?;
                let url = format!("{}/a/{}", tunnel.url.trim_end_matches('/'), project.slug);
                if latest_published_url(&state.db, &project, &target.kind).as_deref() != Some(url.as_str()) {
                    let _ = state.db.add_project_version(project.id, &url, &target.kind);
                }
                return Ok(open_page(url));
            }

            if let Some(url) = latest_published_url(&state.db, &project, &target.kind) {
                return Ok(open_page(url));
            }
            let result = crate::share::publish_static(&state.db, &state.data_dir, &project, &target)
                .await
                .map_err(|err| (StatusCode::BAD_GATEWAY, format!("发布失败：{err}")))?;
            let _ = state.db.add_project_version(project.id, &result.url, &target.kind);
            Ok(open_page(result.url))
        }
        "easytidy" => Err((
            StatusCode::BAD_REQUEST,
            "请把动作改为「EasyTidy 任务」或「EasyTidy 工作流」并选择目标。".to_string(),
        )),
        _ => Ok(ActionOutcome::message(
            StatusCode::OK,
            "动作已触发（消息动作仅用于测试）。",
        )),
    }
}

/// 本机发布的页面地址：同一局域网内可访问（IP 变化后需要重新发布 / 重写标签）。
pub fn local_page_url(port: u16, slug: &str) -> String {
    format!("http://{}:{port}/a/{slug}", local_ip())
}

/// 碰标签后直接跳转到网页。
fn open_page(url: String) -> ActionOutcome {
    let mut outcome = ActionOutcome::message(StatusCode::OK, format!("正在打开：{url}"));
    outcome.redirect = Some(url);
    outcome
}

/// 最近一次发布到同类目标、且之后项目没有再修改过的地址（用于跳过重复上传）。
fn latest_published_url(db: &Database, project: &crate::models::AnniversaryProject, kind: &str) -> Option<String> {
    let parse = |value: &str| chrono::DateTime::parse_from_rfc3339(value).ok();
    let updated = parse(&project.updated_at)?;
    db.list_project_versions(project.id)
        .ok()?
        .into_iter()
        .find(|version| version.target_kind == kind)
        .filter(|version| parse(&version.published_at).is_some_and(|published| published >= updated))
        .map(|version| version.published_url)
}

async fn anniversary_page(
    State(state): State<Arc<ServerState>>,
    Path(slug): Path<String>,
) -> Response {
    let Ok(Some(project)) = state.db.get_project_by_slug(&slug) else {
        return result_page(StatusCode::NOT_FOUND, "未找到纪念日", "项目不存在或已删除。");
    };
    let photo_base = format!("/a/{}/photos/", project.slug);
    let media_base = format!("/a/{}/media/", project.slug);
    Html(render_with_db(&state.db, &project, &photo_base, &media_base)).into_response()
}

async fn anniversary_photo(
    State(state): State<Arc<ServerState>>,
    Path((slug, file)): Path<(String, String)>,
) -> Response {
    if file.contains('/') || file.contains('\\') || file.contains("..") {
        return StatusCode::BAD_REQUEST.into_response();
    }
    let Ok(Some(project)) = state.db.get_project_by_slug(&slug) else {
        return StatusCode::NOT_FOUND.into_response();
    };
    let path = state
        .data_dir
        .join("anniversaries")
        .join(project.id.to_string())
        .join("photos")
        .join(&file);
    let Ok(bytes) = tokio::fs::read(&path).await else {
        return StatusCode::NOT_FOUND.into_response();
    };
    let content_type = match file
        .rsplit('.')
        .next()
        .unwrap_or_default()
        .to_ascii_lowercase()
        .as_str()
    {
        "jpg" | "jpeg" => "image/jpeg",
        "png" => "image/png",
        "webp" => "image/webp",
        "gif" => "image/gif",
        _ => "application/octet-stream",
    };
    (
        [(axum::http::header::CONTENT_TYPE, content_type)],
        bytes,
    )
        .into_response()
}

async fn anniversary_media(
    State(state): State<Arc<ServerState>>,
    Path((slug, file)): Path<(String, String)>,
    headers: axum::http::HeaderMap,
) -> Response {
    if file.contains('/') || file.contains('\\') || file.contains("..") {
        return StatusCode::BAD_REQUEST.into_response();
    }
    let Ok(Some(project)) = state.db.get_project_by_slug(&slug) else {
        return StatusCode::NOT_FOUND.into_response();
    };
    let path = state
        .data_dir
        .join("anniversaries")
        .join(project.id.to_string())
        .join("media")
        .join(&file);
    let Ok(bytes) = tokio::fs::read(&path).await else {
        return StatusCode::NOT_FOUND.into_response();
    };
    let content_type = match file
        .rsplit('.')
        .next()
        .unwrap_or_default()
        .to_ascii_lowercase()
        .as_str()
    {
        "mp3" => "audio/mpeg",
        "m4a" => "audio/mp4",
        "wav" => "audio/wav",
        "ogg" => "audio/ogg",
        "mp4" => "video/mp4",
        "webm" => "video/webm",
        "mov" => "video/quicktime",
        _ => "application/octet-stream",
    };

    // iOS Safari 播放 <video>/<audio> 时必须拿到 206 分段响应，否则视频无法播放。
    use axum::http::header;
    let bytes = axum::body::Bytes::from(bytes);
    let total = bytes.len() as u64;
    let range_header = headers.get(header::RANGE).and_then(|value| value.to_str().ok());
    let mut response = match range_header {
        None => ([(header::CONTENT_TYPE, content_type)], bytes).into_response(),
        Some(value) => match parse_range(value, total) {
            Some((start, end)) => (
                StatusCode::PARTIAL_CONTENT,
                [
                    (header::CONTENT_TYPE, content_type.to_string()),
                    (header::CONTENT_RANGE, format!("bytes {start}-{end}/{total}")),
                ],
                bytes.slice(start as usize..=end as usize),
            )
                .into_response(),
            None => (
                StatusCode::RANGE_NOT_SATISFIABLE,
                [(header::CONTENT_RANGE, format!("bytes */{total}"))],
            )
                .into_response(),
        },
    };
    response.headers_mut().insert(
        header::ACCEPT_RANGES,
        axum::http::HeaderValue::from_static("bytes"),
    );
    response
}

/// 解析单段 `Range: bytes=start-end`，返回闭区间 (start, end)。多段请求只取第一段。
fn parse_range(value: &str, total: u64) -> Option<(u64, u64)> {
    if total == 0 {
        return None;
    }
    let spec = value.trim().strip_prefix("bytes=")?;
    let first = spec.split(',').next()?.trim();
    let (start, end) = first.split_once('-')?;
    let (start, end) = if start.trim().is_empty() {
        let suffix: u64 = end.trim().parse().ok()?;
        if suffix == 0 {
            return None;
        }
        (total.saturating_sub(suffix), total - 1)
    } else {
        let start: u64 = start.trim().parse().ok()?;
        let end = if end.trim().is_empty() {
            total - 1
        } else {
            end.trim().parse::<u64>().ok()?.min(total - 1)
        };
        (start, end)
    };
    (start <= end && start < total).then_some((start, end))
}

#[cfg(test)]
mod tests {
    use super::parse_range;

    #[test]
    fn parses_byte_ranges() {
        assert_eq!(parse_range("bytes=0-1", 100), Some((0, 1)));
        assert_eq!(parse_range("bytes=10-", 100), Some((10, 99)));
        assert_eq!(parse_range("bytes=-20", 100), Some((80, 99)));
        assert_eq!(parse_range("bytes=50-500", 100), Some((50, 99)));
        assert_eq!(parse_range("bytes=0-1,5-9", 100), Some((0, 1)));
        assert_eq!(parse_range("bytes=100-", 100), None);
        assert_eq!(parse_range("bytes=5-1", 100), None);
        assert_eq!(parse_range("items=0-1", 100), None);
        assert_eq!(parse_range("bytes=0-", 0), None);
    }
}

async fn write_helper(
    State(state): State<Arc<ServerState>>,
    Path(write_token): Path<String>,
) -> Response {
    let Some(row) = state.db.find_by_write_token(&write_token).ok().flatten() else {
        return result_page(StatusCode::NOT_FOUND, "写入链接无效", "请重新生成标签。");
    };

    let uri = format!(
        "http://{}:{}/t/{}?k={}",
        local_ip(),
        state.port,
        row.public_id,
        row.token
    );
    Html(write_page_html(&row.name, &uri, WritePageKind::Trigger)).into_response()
}

#[derive(Deserialize)]
struct NfcLinkQuery {
    #[serde(default)]
    u: String,
    #[serde(default)]
    n: String,
}

/// 把任意公开链接（例如已发布的纪念日网页）写入 NFC 的辅助页：手机扫码打开后复制链接。
async fn nfc_link_helper(Query(query): Query<NfcLinkQuery>) -> Response {
    let url = query.u.trim();
    if !(url.starts_with("https://") || url.starts_with("http://")) || url.len() > 2000 {
        return result_page(StatusCode::BAD_REQUEST, "链接无效", "请在 EasyTidy Connect 里重新生成写入二维码。");
    }
    let name = if query.n.trim().is_empty() { "纪念日网页" } else { query.n.trim() };
    Html(write_page_html(name, url, WritePageKind::Link)).into_response()
}

#[derive(Clone, Copy, PartialEq)]
enum WritePageKind {
    /// 局域网触发链接（写入后需在电脑端标记已写入）
    Trigger,
    /// 公开网页链接（不依赖电脑和局域网）
    Link,
}

fn write_page_html(name: &str, uri: &str, kind: WritePageKind) -> String {
    let escaped_uri = escape_html(uri);
    let escaped_name = escape_html(name);
    let bytes = crate::nfc::ndef_uri_size(uri);
    let (link_label, copy_label, last_step) = match kind {
        WritePageKind::Trigger => ("触发链接", "复制触发链接", "回到电脑端点击“标记为已写入”"),
        WritePageKind::Link => ("网页链接", "复制网页链接", "碰一下标签，手机直接打开网页"),
    };
    let html = format!(
        r#"<!doctype html>
<html lang="zh-CN"><head><meta charset="utf-8">
<meta name="viewport" content="width=device-width,initial-scale=1,viewport-fit=cover">
<meta name="color-scheme" content="light dark">
<title>写入 NFC - {escaped_name}</title>
<style>{MOBILE_CSS}
textarea{{width:100%;box-sizing:border-box;min-height:96px;padding:14px;border:1px solid var(--line);border-radius:14px;background:var(--soft);color:var(--text);font:16px/1.5 ui-monospace,SFMono-Regular,Menlo,monospace;word-break:break-all;resize:none;-webkit-user-select:text;user-select:text}}
.meta{{display:flex;gap:10px;margin:18px 0 4px}}
.meta div{{flex:1;background:var(--soft);border-radius:14px;padding:12px 14px}}
.meta span{{display:block;font-size:12px;color:var(--muted)}}
.meta strong{{display:block;margin-top:4px;font-size:15px;word-break:break-all}}
ol{{margin:18px 0 0;padding-left:20px;color:var(--muted);line-height:1.9;font-size:14px}}
.hint{{min-height:20px;margin:10px 2px 0;font-size:13px}}.hint.ok{{color:var(--ok)}}.hint.bad{{color:var(--bad)}}
</style></head><body><main class="card">
<div class="badge">NFC</div>
<h1>写入 NFC 标签</h1>
<p>复制下面的{link_label}，在 NFC 工具中选择“写入 URL”后粘贴。</p>
<textarea id="payload" readonly>{escaped_uri}</textarea>
<button class="primary" type="button" onclick="copyPayload(this)" data-label="{copy_label}">{copy_label}</button>
<div id="hint" class="hint" aria-live="polite"></div>
<div class="meta"><div><span>标签名称</span><strong>{escaped_name}</strong></div><div><span>占用空间</span><strong>{bytes} 字节</strong></div></div>
<p>请确认你的 NFC 标签容量不小于这个数值。</p>
<ol><li>打开 NFC Tools 等写卡 App</li><li>选择“写入 → URL”，粘贴链接</li><li>把手机贴近 NFC 标签完成写入</li><li>{last_step}</li></ol>
</main>
<script>
// 手机通过 http://局域网IP 打开本页，不是安全上下文，iOS Safari 上 navigator.clipboard 不存在。
// 先在点击手势内同步执行 execCommand('copy')（iOS 需要选区落在可选中的节点上），失败再尝试 Clipboard API。
function legacyCopy(text){{
  const span=document.createElement('span');
  span.textContent=text;
  span.style.cssText='position:fixed;top:0;left:0;clip:rect(0,0,0,0);white-space:pre;-webkit-user-select:text;user-select:text;font-size:16px';
  // 同时在 copy 事件里直接写入剪贴板数据，即使选区没生效也能拿到正确内容。
  const onCopy=(e)=>{{if(e.clipboardData){{e.clipboardData.setData('text/plain',text);e.preventDefault();}}}};
  document.addEventListener('copy',onCopy);
  document.body.appendChild(span);
  const sel=window.getSelection();
  const range=document.createRange();
  range.selectNodeContents(span);
  sel.removeAllRanges();
  sel.addRange(range);
  let ok=false;
  try{{ok=document.execCommand('copy');}}catch(e){{ok=false;}}
  document.removeEventListener('copy',onCopy);
  sel.removeAllRanges();
  span.remove();
  return ok;
}}
function showResult(btn,ok){{
  const hint=document.getElementById('hint');
  btn.classList.toggle('done',ok);
  btn.textContent=ok?'已复制 ✓':btn.dataset.label;
  hint.textContent=ok?'已复制到剪贴板，去 NFC 工具里粘贴即可。':'自动复制失败，请长按上方链接，选择“全选”后“拷贝”。';
  hint.className='hint '+(ok?'ok':'bad');
  if(!ok){{const el=document.getElementById('payload');el.focus();el.setSelectionRange(0,el.value.length);}}
  if(ok)setTimeout(()=>{{btn.classList.remove('done');btn.textContent=btn.dataset.label;}},2400);
}}
function copyPayload(btn){{
  const text=document.getElementById('payload').value;
  if(legacyCopy(text)){{showResult(btn,true);return;}}
  if(window.isSecureContext&&navigator.clipboard&&navigator.clipboard.writeText){{
    navigator.clipboard.writeText(text).then(()=>showResult(btn,true),()=>showResult(btn,false));
    return;
  }}
  showResult(btn,false);
}}
</script></body></html>"#,
        MOBILE_CSS = MOBILE_CSS,
    );
    html
}

/// 手机端页面（写入辅助页、触发结果页）共用的样式，支持深色模式与刘海屏安全区。
const MOBILE_CSS: &str = r#"
:root{--bg:#eef2fb;--card:#fff;--text:#111827;--muted:#64748b;--line:#dbe3f0;--soft:#f4f7fc;--brand:#2f6bff;--brand2:#7c5cff;--ok:#059669;--bad:#dc2626;--warn:#d97706}
@media (prefers-color-scheme:dark){:root{--bg:#0b1020;--card:#141b2d;--text:#e8edf7;--muted:#94a3b8;--line:#26314a;--soft:#1b2439;--ok:#34d399;--bad:#f87171;--warn:#fbbf24}}
*{box-sizing:border-box}
body{margin:0;min-height:100vh;min-height:100dvh;display:flex;align-items:center;justify-content:center;padding:max(24px,env(safe-area-inset-top)) 18px max(24px,env(safe-area-inset-bottom));font-family:-apple-system,BlinkMacSystemFont,"PingFang SC","Segoe UI","Microsoft YaHei",sans-serif;color:var(--text);background:radial-gradient(120% 60% at 50% 0%,rgba(47,107,255,.18),transparent 70%),var(--bg);-webkit-text-size-adjust:100%}
.card{width:100%;max-width:520px;background:var(--card);border:1px solid var(--line);border-radius:24px;padding:28px 22px 24px;box-shadow:0 24px 60px -24px rgba(15,23,42,.35)}
.badge{width:52px;height:52px;border-radius:16px;display:grid;place-items:center;margin-bottom:16px;font-weight:800;font-size:15px;letter-spacing:.5px;color:#fff;background:linear-gradient(135deg,var(--brand),var(--brand2));box-shadow:0 12px 24px -10px rgba(47,107,255,.7)}
h1{font-size:22px;line-height:1.3;margin:0 0 8px;letter-spacing:-.2px}
p{color:var(--muted);line-height:1.7;margin:0 0 18px;font-size:15px}
button.primary{margin-top:14px;width:100%;padding:15px;border:0;border-radius:14px;background:linear-gradient(135deg,var(--brand),var(--brand2));color:#fff;font-family:inherit;font-size:16px;font-weight:600;box-shadow:0 12px 24px -12px rgba(47,107,255,.8);transition:transform .12s ease,filter .2s ease;-webkit-tap-highlight-color:transparent}
button.primary:active{transform:scale(.98);filter:brightness(.95)}
button.primary.done{background:var(--ok);box-shadow:none}
.foot{margin-top:22px;text-align:center;font-size:12px;color:var(--muted);opacity:.8}
"#;

fn result_page(status: StatusCode, title: &str, message: &str) -> Response {
    let (icon, tone) = if status.is_success() {
        ("✓", "var(--ok)")
    } else if status.is_client_error() {
        ("!", "var(--warn)")
    } else {
        ("×", "var(--bad)")
    };
    let title = escape_html(title);
    let message = escape_html(message);
    let html = format!(
        r#"<!doctype html>
<html lang="zh-CN"><head><meta charset="utf-8">
<meta name="viewport" content="width=device-width,initial-scale=1,viewport-fit=cover">
<meta name="color-scheme" content="light dark">
<title>{title}</title>
<style>{MOBILE_CSS}
.card{{text-align:center}}
.status{{width:72px;height:72px;margin:4px auto 18px;border-radius:50%;display:grid;place-items:center;font-size:34px;font-weight:700;color:#fff;background:{tone};box-shadow:0 0 0 10px color-mix(in srgb,{tone} 16%,transparent)}}
p{{margin:0;word-break:break-word}}
</style></head><body><main class="card">
<div class="status">{icon}</div>
<h1>{title}</h1><p>{message}</p>
<div class="foot">EasyTidy Connect</div>
</main></body></html>"#,
        MOBILE_CSS = MOBILE_CSS,
    );
    (status, Html(html)).into_response()
}

fn escape_html(value: &str) -> String {
    value
        .replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
        .replace('\'', "&#39;")
}

use std::net::UdpSocket;
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

use crate::db::Database;
use crate::models::TagPayload;

pub fn local_ip() -> String {
    UdpSocket::bind("0.0.0.0:0")
        .and_then(|socket| {
            socket.connect("8.8.8.8:80")?;
            socket.local_addr()
        })
        .map(|addr| addr.ip().to_string())
        .unwrap_or_else(|_| "127.0.0.1".to_string())
}

pub fn tag_payload(db: &Database, trigger_id: i64, port: u16) -> Result<TagPayload> {
    let row = db
        .find_by_id(trigger_id)?
        .ok_or_else(|| anyhow::anyhow!("trigger not found"))?;
    let host = local_ip();
    let base = format!("http://{host}:{port}");
    let uri = format!("{base}/t/{}?k={}", row.public_id, row.token);
    let write_page = format!("{base}/w/{}", row.write_token);

    Ok(TagPayload {
        trigger_id: row.id,
        trigger_name: row.name,
        bytes: uri.len(),
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
    http: Client,
}

#[derive(Deserialize)]
struct TriggerQuery {
    #[serde(default)]
    k: String,
}

pub async fn serve(db: Arc<Database>, port: u16) -> Result<()> {
    let state = Arc::new(ServerState {
        db,
        port,
        http: Client::builder()
            .timeout(Duration::from_secs(30))
            .build()?,
    });

    let app = Router::new()
        .route("/health", get(health))
        .route("/t/{public_id}", get(trigger))
        .route("/w/{write_token}", get(write_helper))
        .with_state(state);

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

async fn trigger(
    State(state): State<Arc<ServerState>>,
    Path(public_id): Path<String>,
    Query(query): Query<TriggerQuery>,
) -> Response {
    let Some(row) = state.db.find_by_public_id(&public_id).ok().flatten() else {
        return result_page(StatusCode::NOT_FOUND, "未找到触发器", "标签内容无效或已被删除。");
    };

    if query.k != row.token {
        return result_page(StatusCode::UNAUTHORIZED, "无法触发", "标签 token 不正确或已失效。");
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

    let result = execute_action(&state, &row.action_type, &row.action_config).await;

    match result {
        Ok((status, message)) => {
            let _ = state.db.finish_run(run_id, "success", None);
            let _ = state.db.mark_verified(row.id);
            result_page(status, &format!("已触发：{}", row.name), &message)
        }
        Err((status, message)) => {
            let _ = state.db.finish_run(run_id, "failed", Some(&message));
            result_page(status, "触发失败", &message)
        }
    }
}

async fn execute_action(
    state: &ServerState,
    action_type: &str,
    action_config: &str,
) -> Result<(StatusCode, String), (StatusCode, String)> {
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
                Ok((StatusCode::OK, format!("Webhook 已调用，HTTP {code}。")))
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
            Ok((StatusCode::OK, format!("请打开：{url}")))
        }
        "easytidy" => Err((
            StatusCode::NOT_IMPLEMENTED,
            "尚未连接 EasyTidy。请在 EasyTidy 中启用 Connect 集成后再使用。".to_string(),
        )),
        _ => Ok((
            StatusCode::OK,
            "动作已触发（消息动作仅用于测试）。".to_string(),
        )),
    }
}

async fn write_helper(
    State(state): State<Arc<ServerState>>,
    Path(write_token): Path<String>,
) -> Response {
    let Some(row) = state.db.find_by_write_token(&write_token).ok().flatten() else {
        return result_page(StatusCode::NOT_FOUND, "写入链接无效", "请重新生成标签。");
    };

    let host = local_ip();
    let uri = format!(
        "http://{host}:{}/t/{}?k={}",
        state.port, row.public_id, row.token
    );
    let escaped_uri = escape_html(&uri);
    let escaped_name = escape_html(&row.name);
    let html = format!(
        r#"<!doctype html>
<html lang="zh-CN"><head><meta charset="utf-8">
<meta name="viewport" content="width=device-width,initial-scale=1">
<title>写入 NFC - {escaped_name}</title>
<style>
body{{font-family:-apple-system,BlinkMacSystemFont,"Segoe UI",sans-serif;background:#f6f7fb;margin:0;padding:24px;color:#1f2430}}
.card{{max-width:680px;margin:0 auto;background:#fff;border-radius:16px;padding:24px;box-shadow:0 8px 30px rgba(0,0,0,.08)}}
h1{{font-size:20px;margin:0 0 8px}}p{{color:#5b6475;line-height:1.6}}
textarea{{width:100%;box-sizing:border-box;height:88px;padding:12px;border:1px solid #d8dce6;border-radius:10px;font-size:14px;word-break:break-all}}
button{{margin-top:12px;width:100%;padding:14px;border:0;border-radius:10px;background:#2563eb;color:#fff;font-size:16px;font-weight:600}}
code{{background:#eef2ff;padding:2px 6px;border-radius:6px}}
</style></head><body><div class="card">
<h1>写入 NFC 标签</h1>
<p>在手机 NFC 工具中选择“写入 URL / 文本”，粘贴下面的内容。</p>
<textarea id="payload" readonly>{escaped_uri}</textarea>
<button onclick="copyPayload()">复制触发链接</button>
<p style="margin-top:18px">标签名称：<strong>{escaped_name}</strong><br>大小：<code>{bytes} / 888 字节</code></p>
<p>写入后回到 EasyTidy Connect 点击“已写入”。第一次碰标签成功即自动验证。</p>
</div>
<script>
function copyPayload(){{
  const el=document.getElementById('payload');
  el.select(); el.setSelectionRange(0,99999);
  navigator.clipboard?.writeText(el.value).catch(()=>document.execCommand('copy'));
  event.target.textContent='已复制';
}}
</script></body></html>"#,
        bytes = uri.len()
    );

    Html(html).into_response()
}

fn result_page(status: StatusCode, title: &str, message: &str) -> Response {
    let title = escape_html(title);
    let message = escape_html(message);
    let html = format!(
        r#"<!doctype html>
<html lang="zh-CN"><head><meta charset="utf-8">
<meta name="viewport" content="width=device-width,initial-scale=1">
<title>{title}</title>
<style>
body{{font-family:-apple-system,BlinkMacSystemFont,"Segoe UI",sans-serif;background:#f6f7fb;min-height:100vh;display:flex;align-items:center;justify-content:center;margin:0;padding:24px}}
.card{{max-width:560px;background:#fff;border-radius:18px;padding:28px;box-shadow:0 10px 40px rgba(0,0,0,.08);text-align:center}}
h1{{font-size:22px;margin:0 0 12px}}p{{color:#5b6475;line-height:1.7;margin:0}}
</style></head><body><div class="card">
<h1>{title}</h1><p>{message}</p>
<p style="margin-top:20px"><a href="/">EasyTidy Connect</a></p>
</div></body></html>"#
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

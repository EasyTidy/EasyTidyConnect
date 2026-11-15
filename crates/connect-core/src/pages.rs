use std::path::Path;
use std::time::Duration;

use anyhow::{bail, Context, Result};
use base64::Engine as _;
use reqwest::Client;
use serde_json::{json, Value};

use crate::models::{AnniversaryProject, PublishResult, PublishTarget};

const CF_API: &str = "https://api.cloudflare.com/client/v4";

fn api_base(target: &PublishTarget) -> &str {
    let endpoint = target.endpoint.trim();
    if endpoint.starts_with("http://") || endpoint.starts_with("https://") {
        endpoint.trim_end_matches('/')
    } else {
        CF_API
    }
}

struct Asset {
    path: String,
    hash: String,
    base64: String,
    content_type: String,
}

pub async fn publish_project(
    target: &PublishTarget,
    project: &AnniversaryProject,
    html: &str,
    photos_dir: &Path,
    media_dir: &Path,
) -> Result<PublishResult> {
    validate_target(target)?;

    let client = Client::builder()
        .timeout(Duration::from_secs(180))
        .build()?;

    let prefix = target.prefix.trim().trim_matches('/');
    let prefix = if prefix.is_empty() { "anniversary" } else { prefix };
    let base_path = format!("{prefix}/{}", project.slug);

    let mut assets = vec![make_asset(
        &format!("{base_path}/index.html"),
        html.as_bytes(),
        "text/html; charset=utf-8",
    )];

    if photos_dir.exists() {
        for entry in std::fs::read_dir(photos_dir)
            .with_context(|| format!("read photos dir {}", photos_dir.display()))?
        {
            let entry = entry?;
            if !entry.file_type()?.is_file() {
                continue;
            }
            let name = entry.file_name().to_string_lossy().to_string();
            let bytes = std::fs::read(entry.path())?;
            assets.push(make_asset(
                &format!("{base_path}/photos/{name}"),
                &bytes,
                content_type(&name),
            ));
        }
    }

    if media_dir.exists() {
        for entry in std::fs::read_dir(media_dir)
            .with_context(|| format!("read media dir {}", media_dir.display()))?
        {
            let entry = entry?;
            if !entry.file_type()?.is_file() {
                continue;
            }
            let name = entry.file_name().to_string_lossy().to_string();
            let bytes = std::fs::read(entry.path())?;
            assets.push(make_asset(
                &format!("{base_path}/media/{name}"),
                &bytes,
                content_type(&name),
            ));
        }
    }

    let api = api_base(target);
    let hashes = assets.iter().map(|asset| asset.hash.clone()).collect::<Vec<_>>();
    let jwt = upload_token(&client, target).await?;

    let missing = check_missing(&client, api, &jwt, &hashes).await?;
    let to_upload = assets
        .iter()
        .filter(|asset| missing.iter().any(|hash| hash == &asset.hash))
        .collect::<Vec<_>>();

    for chunk in to_upload.chunks(20) {
        upload_assets(&client, api, &jwt, chunk).await?;
    }

    let manifest = assets
        .iter()
        .map(|asset| (format!("/{}", asset.path), asset.hash.clone()))
        .collect::<std::collections::BTreeMap<_, _>>();
    let deployment = create_deployment(&client, target, &manifest).await?;
    let deployment_url = deployment
        .get("url")
        .and_then(|value| value.as_str())
        .unwrap_or_default()
        .trim_end_matches('/')
        .to_string();
    if deployment_url.is_empty() {
        bail!("Cloudflare Pages 未返回部署 URL");
    }

    let deployment_id = deployment
        .get("id")
        .and_then(|value| value.as_str())
        .unwrap_or_default();
    poll_deployment(&client, target, deployment_id).await?;

    let base = if target.public_base_url.trim().is_empty() {
        deployment_url
    } else {
        target.public_base_url.trim().trim_end_matches('/').to_string()
    };
    let url = format!("{base}/{base_path}/");

    Ok(PublishResult {
        url,
        slug: project.slug.clone(),
        uploaded: to_upload.len(),
    })
}

pub async fn test_target(target: &PublishTarget) -> Result<()> {
    validate_target(target)?;
    let client = Client::builder()
        .timeout(Duration::from_secs(20))
        .build()?;
    let url = format!(
        "{}/accounts/{}/pages/projects/{}",
        api_base(target),
        target.account_id.trim(),
        target.project_name.trim()
    );
    let value = send_json(
        client
            .get(url)
            .bearer_auth(target.api_token.trim())
            .header("content-type", "application/json"),
    )
    .await?;
    ensure_success(&value, "Cloudflare Pages 项目检查")?;
    Ok(())
}

fn validate_target(target: &PublishTarget) -> Result<()> {
    if target.account_id.trim().is_empty()
        || target.project_name.trim().is_empty()
        || target.api_token.trim().is_empty()
    {
        bail!("Cloudflare Pages 缺少 account id / project / API token");
    }
    Ok(())
}

async fn upload_token(client: &Client, target: &PublishTarget) -> Result<String> {
    let url = format!(
        "{}/accounts/{}/pages/projects/{}/upload-token",
        api_base(target),
        target.account_id.trim(),
        target.project_name.trim()
    );
    let value = send_json(
        client
            .post(url)
            .bearer_auth(target.api_token.trim())
            .header("content-type", "application/json"),
    )
    .await?;
    ensure_success(&value, "获取 Pages upload token")?;
    value
        .pointer("/result/jwt")
        .and_then(|v| v.as_str())
        .map(|v| v.to_string())
        .context("Cloudflare 未返回 upload JWT")
}

async fn check_missing(
    client: &Client,
    api: &str,
    jwt: &str,
    hashes: &[String],
) -> Result<Vec<String>> {
    let value = send_json(
        client
            .post(format!("{api}/pages/assets/check-missing"))
            .bearer_auth(jwt)
            .json(&json!({ "hashes": hashes })),
    )
    .await?;
    ensure_success(&value, "检查 Pages 资源")?;

    let result = value.get("result").context("缺少 result")?;
    // 兼容 result 直接数组或 { hashes: [...] } 两种返回。
    let array = result
        .as_array()
        .or_else(|| result.get("hashes").and_then(|v| v.as_array()))
        .context("result 不是数组")?;
    Ok(array
        .iter()
        .filter_map(|item| item.as_str().map(|v| v.to_string()))
        .collect())
}

async fn upload_assets(client: &Client, api: &str, jwt: &str, assets: &[&Asset]) -> Result<()> {
    let payload = assets
        .iter()
        .map(|asset| {
            json!({
                "key": asset.hash,
                "value": asset.base64,
                "metadata": { "contentType": asset.content_type },
                "base64": true
            })
        })
        .collect::<Vec<_>>();

    let value = send_json(
        client
            .post(format!("{api}/pages/assets/upload"))
            .bearer_auth(jwt)
            .json(&payload),
    )
    .await?;
    ensure_success(&value, "上传 Pages 资源")
}

async fn create_deployment(
    client: &Client,
    target: &PublishTarget,
    manifest: &std::collections::BTreeMap<String, String>,
) -> Result<Value> {
    let manifest = serde_json::to_string(manifest)?;
    let branch = if target.branch.trim().is_empty() {
        "main"
    } else {
        target.branch.trim()
    };
    let form = reqwest::multipart::Form::new()
        .text("manifest", manifest)
        .text("branch", branch.to_string());

    let value = send_json(
        client
            .post(format!(
                "{}/accounts/{}/pages/projects/{}/deployments",
                api_base(target),
                target.account_id.trim(),
                target.project_name.trim()
            ))
            .bearer_auth(target.api_token.trim())
            .multipart(form),
    )
    .await?;
    ensure_success(&value, "创建 Pages deployment")?;
    value.get("result").cloned().context("deployment 缺少 result")
}

async fn poll_deployment(client: &Client, target: &PublishTarget, deployment_id: &str) -> Result<()> {
    if deployment_id.is_empty() {
        return Ok(());
    }
    let url = format!(
        "{}/accounts/{}/pages/projects/{}/deployments/{}",
        api_base(target),
        target.account_id.trim(),
        target.project_name.trim(),
        deployment_id
    );

    for _ in 0..20 {
        tokio::time::sleep(Duration::from_secs(2)).await;
        let value = send_json(
            client
                .get(&url)
                .bearer_auth(target.api_token.trim())
                .header("content-type", "application/json"),
        )
        .await?;
        ensure_success(&value, "查询 Pages deployment")?;
        let count = value
            .pointer("/result/file_count")
            .and_then(|v| v.as_i64())
            .unwrap_or(0);
        if count > 0 {
            return Ok(());
        }
    }
    bail!("Cloudflare Pages 部署已创建，但资源尚未生效（file_count=0）")
}

async fn send_json(request: reqwest::RequestBuilder) -> Result<Value> {
    let response = request.send().await.context("Cloudflare API 请求失败")?;
    let status = response.status();
    let text = response.text().await.unwrap_or_default();
    if !status.is_success() {
        bail!("Cloudflare API HTTP {status}: {}", truncate(&text, 800));
    }
    serde_json::from_str(&text).with_context(|| format!("Cloudflare 返回非 JSON：{}", truncate(&text, 300)))
}

fn ensure_success(value: &Value, action: &str) -> Result<()> {
    if value.get("success").and_then(|v| v.as_bool()).unwrap_or(false) {
        return Ok(());
    }
    let message = value
        .pointer("/errors/0/message")
        .and_then(|v| v.as_str())
        .unwrap_or("unknown error");
    bail!("{action} 失败：{message}")
}

fn make_asset(path: &str, bytes: &[u8], content_type: &str) -> Asset {
    let base64 = base64::engine::general_purpose::STANDARD.encode(bytes);
    let extension = Path::new(path)
        .extension()
        .and_then(|v| v.to_str())
        .unwrap_or_default()
        .to_ascii_lowercase();
    let input = format!("{base64}{extension}");
    let hash = blake3::hash(input.as_bytes()).to_hex().to_string()[..32].to_string();
    Asset {
        path: path.to_string(),
        hash,
        base64,
        content_type: content_type.to_string(),
    }
}

fn content_type(name: &str) -> &'static str {
    match Path::new(name)
        .extension()
        .and_then(|v| v.to_str())
        .unwrap_or_default()
        .to_ascii_lowercase()
        .as_str()
    {
        "jpg" | "jpeg" => "image/jpeg",
        "png" => "image/png",
        "webp" => "image/webp",
        "gif" => "image/gif",
        "mp3" => "audio/mpeg",
        "m4a" => "audio/mp4",
        "wav" => "audio/wav",
        "ogg" => "audio/ogg",
        "mp4" => "video/mp4",
        "webm" => "video/webm",
        "mov" => "video/quicktime",
        "html" => "text/html; charset=utf-8",
        "css" => "text/css; charset=utf-8",
        "js" => "application/javascript",
        _ => "application/octet-stream",
    }
}

fn truncate(value: &str, limit: usize) -> String {
    value.chars().take(limit).collect()
}

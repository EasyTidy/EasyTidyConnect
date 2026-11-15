use std::path::Path;
use std::time::Duration;

use anyhow::{bail, Context, Result};
use chrono::Utc;
use hmac::{Hmac, Mac};
use percent_encoding::{utf8_percent_encode, AsciiSet, CONTROLS};
use reqwest::Client;
use serde_json::json;
use sha2::{Digest, Sha256};

use crate::models::{AnniversaryProject, PublishResult, PublishTarget};

const PATH_SEGMENT_ENCODE: &AsciiSet = &CONTROLS
    .add(b' ')
    .add(b'"')
    .add(b'#')
    .add(b'%')
    .add(b'&')
    .add(b'+')
    .add(b',')
    .add(b'/')
    .add(b':')
    .add(b';')
    .add(b'<')
    .add(b'=')
    .add(b'>')
    .add(b'?')
    .add(b'@')
    .add(b'[')
    .add(b'\\')
    .add(b']')
    .add(b'^')
    .add(b'`')
    .add(b'{')
    .add(b'|')
    .add(b'}');

type HmacSha256 = Hmac<Sha256>;

pub async fn publish_project(
    target: &PublishTarget,
    project: &AnniversaryProject,
    html: &str,
    photos_dir: &Path,
    media_dir: &Path,
) -> Result<PublishResult> {
    if target.kind == "cloudflare_pages" {
        bail!("Cloudflare Pages Direct Upload 尚未实现；请先使用 S3/R2 目标。");
    }
    if target.kind != "s3" && target.kind != "r2" {
        bail!("不支持的发布目标类型：{}", target.kind);
    }

    let client = Client::builder()
        .timeout(Duration::from_secs(120))
        .build()?;

    let prefix = target
        .prefix
        .trim()
        .trim_matches('/')
        .to_string();
    let slug = project.slug.trim_matches('/');
    let base_key = if prefix.is_empty() {
        slug.to_string()
    } else {
        format!("{prefix}/{slug}")
    };

    let mut uploaded = 0usize;
    for (dir, folder) in [(photos_dir, "photos"), (media_dir, "media")] {
        if !dir.exists() {
            continue;
        }
        for entry in std::fs::read_dir(dir)
            .with_context(|| format!("read media dir {}", dir.display()))?
        {
            let entry = entry?;
            if !entry.file_type()?.is_file() {
                continue;
            }
            let name = entry.file_name().to_string_lossy().to_string();
            let bytes = std::fs::read(entry.path())?;
            let key = format!("{base_key}/{folder}/{name}");
            put_object(&client, target, &key, &bytes, content_type(&name)).await?;
            uploaded += 1;
        }
    }

    let index_key = format!("{base_key}/index.html");
    put_object(&client, target, &index_key, html.as_bytes(), "text/html; charset=utf-8").await?;

    let url = public_url(target, &index_key);
    Ok(PublishResult {
        url,
        slug: project.slug.clone(),
        uploaded,
    })
}

pub async fn test_target(target: &PublishTarget) -> Result<()> {
    if target.kind == "cloudflare_pages" {
        bail!("Cloudflare Pages Direct Upload 尚未实现。");
    }
    let client = Client::builder()
        .timeout(Duration::from_secs(30))
        .build()?;
    let key = format!("{}/.easytidy-connect-test", target.prefix.trim_matches('/'));
    let body = json!({"test": true, "at": Utc::now().to_rfc3339()}).to_string();
    put_object(&client, target, &key, body.as_bytes(), "application/json").await
}

async fn put_object(
    client: &Client,
    target: &PublishTarget,
    key: &str,
    body: &[u8],
    content_type: &str,
) -> Result<()> {
    let endpoint = target.endpoint.trim().trim_end_matches('/');
    if endpoint.is_empty() || target.bucket.trim().is_empty() {
        bail!("S3 endpoint/bucket 未配置。");
    }

    let host = endpoint
        .split("://")
        .nth(1)
        .unwrap_or(endpoint)
        .split('/')
        .next()
        .unwrap_or_default()
        .to_string();
    if host.is_empty() {
        bail!("S3 endpoint 无效。");
    }

    let encoded_key = key
        .split('/')
        .map(|segment| utf8_percent_encode(segment, PATH_SEGMENT_ENCODE).to_string())
        .collect::<Vec<_>>()
        .join("/");
    let encoded_bucket = utf8_percent_encode(target.bucket.trim(), PATH_SEGMENT_ENCODE).to_string();
    let canonical_uri = format!("/{encoded_bucket}/{encoded_key}");
    let url = format!("{endpoint}{canonical_uri}");

    let now = Utc::now();
    let amz_date = now.format("%Y%m%dT%H%M%SZ").to_string();
    let date_stamp = now.format("%Y%m%d").to_string();
    let payload_hash = hex::encode(Sha256::digest(body));

    let canonical_headers = format!(
        "content-type:{content_type}\nhost:{host}\nx-amz-content-sha256:{payload_hash}\nx-amz-date:{amz_date}\n"
    );
    let signed_headers = "content-type;host;x-amz-content-sha256;x-amz-date";
    let canonical_request = format!(
        "PUT\n{canonical_uri}\n\n{canonical_headers}{signed_headers}\n{payload_hash}"
    );
    let scope = format!(
        "{date_stamp}/{}/s3/aws4_request",
        if target.region.trim().is_empty() {
            "auto"
        } else {
            target.region.trim()
        }
    );
    let string_to_sign = format!(
        "AWS4-HMAC-SHA256\n{amz_date}\n{scope}\n{}",
        hex::encode(Sha256::digest(canonical_request.as_bytes()))
    );

    let signing_key = signing_key(&target.secret_key, &date_stamp, target.region.trim());
    let signature = hex::encode(hmac(&signing_key, string_to_sign.as_bytes())?);
    let authorization = format!(
        "AWS4-HMAC-SHA256 Credential={}/{scope}, SignedHeaders={signed_headers}, Signature={signature}",
        target.access_key
    );

    let response = client
        .put(url)
        .header("content-type", content_type)
        .header("x-amz-content-sha256", &payload_hash)
        .header("x-amz-date", &amz_date)
        .header("authorization", authorization)
        .body(body.to_vec())
        .send()
        .await?;

    let status = response.status();
    if !status.is_success() {
        let text = response.text().await.unwrap_or_default();
        bail!("S3 PUT {key} 失败：HTTP {status} {}", truncate(&text, 500));
    }
    Ok(())
}

fn signing_key(secret: &str, date_stamp: &str, region: &str) -> Vec<u8> {
    let region = if region.trim().is_empty() {
        "auto"
    } else {
        region.trim()
    };
    let k_date = hmac(format!("AWS4{secret}").as_bytes(), date_stamp.as_bytes())
        .expect("hmac key");
    let k_region = hmac(&k_date, region.as_bytes()).expect("hmac region");
    let k_service = hmac(&k_region, b"s3").expect("hmac service");
    hmac(&k_service, b"aws4_request").expect("hmac signing")
}

fn hmac(key: &[u8], data: &[u8]) -> Result<Vec<u8>> {
    let mut mac = HmacSha256::new_from_slice(key).context("invalid HMAC key")?;
    mac.update(data);
    Ok(mac.finalize().into_bytes().to_vec())
}

fn public_url(target: &PublishTarget, key: &str) -> String {
    if !target.public_base_url.trim().is_empty() {
        return format!(
            "{}/{}",
            target.public_base_url.trim().trim_end_matches('/'),
            key
        );
    }
    format!(
        "{}/{}/{}",
        target.endpoint.trim().trim_end_matches('/'),
        target.bucket.trim(),
        key
    )
}

fn content_type(name: &str) -> &'static str {
    match Path::new(name)
        .extension()
        .and_then(|ext| ext.to_str())
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

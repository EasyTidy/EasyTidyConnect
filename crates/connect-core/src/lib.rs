pub mod ai;
pub mod anniversary;
pub mod bridge;
pub mod cloudflare;
pub mod db;
pub mod models;
pub mod nfc;
pub mod publish;
pub mod pages;
pub mod server;
pub mod share;
pub mod tools;
pub mod tunnel;
pub mod wechat;

pub use bridge::{BridgeClient, BridgeHealth, BridgeItem, BridgeRunAccepted};
pub use db::Database;
pub use models::{
    AiProvider, AiProviderInput, AnniversaryProject, CustomTemplate, GeneratedCopy, ProjectInput,
    ProjectVersion,
    PublishResult, PublishTarget, PublishTargetInput, RunView, Settings, ShareSettings, TagPayload,
    TriggerView, WechatConfig,
};
pub use server::{local_ip, tag_payload};
pub use tunnel::TunnelManager;

/// 读取 Cloudflare 授权；OAuth 令牌快过期时自动刷新并保存。
pub async fn cloudflare_auth(db: &Database) -> anyhow::Result<cloudflare::CloudflareAuth> {
    let mut auth = db.get_cloudflare_auth()?;
    if !auth.is_connected() {
        anyhow::bail!("还没有授权 Cloudflare：请在「设置 → Cloudflare」点击授权");
    }
    if cloudflare::refresh_if_needed(&mut auth).await? {
        db.save_cloudflare_auth(&auth)?;
    }
    Ok(auth)
}

/// 使用授权的 Cloudflare 发布目标：把账户 ID、令牌和自动创建的资源填进目标，供后续发布使用。
/// 返回补全后的目标（R2 填 r2.dev 公开地址，Pages 填账户与令牌，Tunnel 填运行令牌）。
pub async fn resolve_cloudflare_target(db: &Database, target: &PublishTarget) -> anyhow::Result<PublishTarget> {
    if !cloudflare::uses_auth(target) {
        return Ok(target.clone());
    }
    let auth = cloudflare_auth(db).await?;
    let api = cloudflare::Api::new(&auth.access_token)?;
    let account = auth.account_id.as_str();
    let mut resolved = target.clone();
    match target.kind.as_str() {
        "r2" => {
            let bucket = cloudflare::slug_name(&target.bucket, "easytidy-connect");
            let public = cloudflare::ensure_r2_bucket(&api, account, &bucket).await?;
            resolved.bucket = bucket;
            resolved.account_id = account.to_string();
            if resolved.public_base_url.trim().is_empty() {
                resolved.public_base_url = public;
            }
        }
        "cloudflare_pages" => {
            let project = cloudflare::slug_name(&target.project_name, "easytidy-connect");
            cloudflare::ensure_pages_project(&api, account, &project).await?;
            resolved.project_name = project;
            resolved.account_id = account.to_string();
            resolved.api_token = auth.access_token.clone();
            if resolved.branch.trim().is_empty() {
                resolved.branch = "main".to_string();
            }
        }
        "cloudflare_tunnel" => {
            let setup = cloudflare::ensure_tunnel(
                &api,
                account,
                &format!("easytidy-connect-{}", target.id),
                &target.hostname,
                &format!("http://127.0.0.1:{}", server::PUBLIC_PREVIEW_PORT),
            )
            .await?;
            resolved.tunnel_token = setup.token;
            resolved.hostname = setup.hostname;
        }
        _ => {}
    }
    Ok(resolved)
}

//! wechat-signer：可独立部署的微信 JS-SDK 签名服务。
//!
//! ```text
//! wechat-signer                    启动服务（默认 0.0.0.0:8080）
//! wechat-signer check [--url URL]  检查配置：换取 ticket 并签名一次（排查 IP 白名单等）
//! wechat-signer --config FILE      从 KEY=VALUE 配置文件读取（环境变量优先）
//! ```

use std::collections::HashMap;
use std::sync::Arc;

use anyhow::{bail, Context, Result};
use wechat_signer::{SignerConfig, TicketCache, TokenApi, DEFAULT_API_BASE};

const HELP: &str = "\
wechat-signer —— 微信公众号 JS-SDK 签名服务

用法：
  wechat-signer [--config FILE] [--listen ADDR]     启动服务
  wechat-signer check [--config FILE] [--url URL]   检查配置是否可用
  wechat-signer health [--url URL]                  健康检查（供 Docker HEALTHCHECK 使用）
  wechat-signer --version

配置（环境变量，或 --config 指定的 KEY=VALUE 文件；环境变量优先）：
  WECHAT_APP_ID            公众号 AppID（必填）
  WECHAT_APP_SECRET        公众号 AppSecret（与 WECHAT_APP_SECRET_FILE 二选一）
  WECHAT_APP_SECRET_FILE   从文件读取 AppSecret（适合 Docker secrets）
  WECHAT_ALLOWED_DOMAINS   允许签名的页面域名，逗号分隔（强烈建议设置）
  WECHAT_LISTEN            监听地址，默认 0.0.0.0:8080
  WECHAT_TOKEN_API         stable（默认，不影响其他系统的 token）或 classic
  RUST_LOG                 日志级别，默认 info

接口：
  GET /wx/jssdk?url=<页面地址>   返回 wx.config 所需的 appId / timestamp / nonceStr / signature
  GET /wx/health                 健康检查
";

struct Settings {
    config: SignerConfig,
    listen: std::net::SocketAddr,
}

#[tokio::main]
async fn main() -> Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter(tracing_subscriber::EnvFilter::try_from_default_env().unwrap_or_else(|_| "info".into()))
        .init();

    let args: Vec<String> = std::env::args().skip(1).collect();
    if args.iter().any(|a| a == "--help" || a == "-h") {
        print!("{HELP}");
        return Ok(());
    }
    if args.iter().any(|a| a == "--version" || a == "-V") {
        println!("wechat-signer {}", env!("CARGO_PKG_VERSION"));
        return Ok(());
    }

    // 健康检查不需要读取配置，供容器镜像（无 curl）的 HEALTHCHECK 使用。
    if args.first().map(String::as_str) == Some("health") {
        let url = flag(&args, "--url").unwrap_or_else(|| "http://127.0.0.1:8080/wx/health".to_string());
        let ok = reqwest::Client::new()
            .get(&url)
            .timeout(std::time::Duration::from_secs(3))
            .send()
            .await
            .map(|r| r.status().is_success())
            .unwrap_or(false);
        std::process::exit(if ok { 0 } else { 1 });
    }

    let check = args.first().map(String::as_str) == Some("check");
    let settings = load(&args)?;
    if check {
        let url = flag(&args, "--url").unwrap_or_else(|| {
            let domain = wechat_signer::domain_rules(&settings.config.allowed_domains)
                .into_iter()
                .next()
                .unwrap_or_else(|| "example.com".to_string());
            format!("https://{domain}/")
        });
        return run_check(&settings.config, &url).await;
    }

    if wechat_signer::domain_rules(&settings.config.allowed_domains).is_empty() {
        tracing::warn!("未设置 WECHAT_ALLOWED_DOMAINS：任何网页都能用你的公众号获取签名，建议限制为自己的域名");
    }
    tracing::info!(
        "AppID {}，token 接口 {:?}，允许域名：{}",
        settings.config.app_id,
        settings.config.token_api,
        if settings.config.allowed_domains.is_empty() { "不限" } else { &settings.config.allowed_domains }
    );
    wechat_signer::serve(Arc::new(settings.config), settings.listen).await
}

/// 读取配置：--config 文件 < 环境变量 < 命令行参数。
fn load(args: &[String]) -> Result<Settings> {
    let mut values: HashMap<String, String> = HashMap::new();
    if let Some(path) = flag(args, "--config") {
        let text = std::fs::read_to_string(&path).with_context(|| format!("读取配置文件 {path} 失败"))?;
        values.extend(parse_env_file(&text));
    }
    for key in [
        "WECHAT_APP_ID",
        "WECHAT_APP_SECRET",
        "WECHAT_APP_SECRET_FILE",
        "WECHAT_ALLOWED_DOMAINS",
        "WECHAT_LISTEN",
        "WECHAT_TOKEN_API",
    ] {
        if let Ok(value) = std::env::var(key) {
            values.insert(key.to_string(), value);
        }
    }
    let get = |key: &str| values.get(key).map(|v| v.trim().to_string()).unwrap_or_default();

    let mut secret = get("WECHAT_APP_SECRET");
    let secret_file = get("WECHAT_APP_SECRET_FILE");
    if secret.is_empty() && !secret_file.is_empty() {
        secret = std::fs::read_to_string(&secret_file)
            .with_context(|| format!("读取 AppSecret 文件 {secret_file} 失败"))?
            .trim()
            .to_string();
    }
    let app_id = get("WECHAT_APP_ID");
    if app_id.is_empty() || secret.is_empty() {
        bail!("请设置 WECHAT_APP_ID 与 WECHAT_APP_SECRET（或 WECHAT_APP_SECRET_FILE），运行 wechat-signer --help 查看说明");
    }
    let token_api = match get("WECHAT_TOKEN_API").to_ascii_lowercase().as_str() {
        "" | "stable" => TokenApi::Stable,
        "classic" => TokenApi::Classic,
        other => bail!("WECHAT_TOKEN_API 只能是 stable 或 classic，当前为 {other}"),
    };
    let listen = flag(args, "--listen")
        .or_else(|| Some(get("WECHAT_LISTEN")).filter(|v| !v.is_empty()))
        .unwrap_or_else(|| "0.0.0.0:8080".to_string());
    Ok(Settings {
        config: SignerConfig {
            app_id,
            app_secret: secret,
            allowed_domains: get("WECHAT_ALLOWED_DOMAINS"),
            token_api,
        },
        listen: listen.parse().with_context(|| format!("监听地址无效：{listen}"))?,
    })
}

/// 解析 KEY=VALUE 文件：忽略空行和 # 注释，值两侧的引号会被去掉。
fn parse_env_file(text: &str) -> HashMap<String, String> {
    text.lines()
        .map(str::trim)
        .filter(|line| !line.is_empty() && !line.starts_with('#'))
        .filter_map(|line| {
            let line = line.strip_prefix("export ").unwrap_or(line);
            let (key, value) = line.split_once('=')?;
            let value = value.trim();
            let value = value
                .strip_prefix('"')
                .and_then(|v| v.strip_suffix('"'))
                .or_else(|| value.strip_prefix('\'').and_then(|v| v.strip_suffix('\'')))
                .unwrap_or(value);
            Some((key.trim().to_string(), value.to_string()))
        })
        .collect()
}

fn flag(args: &[String], name: &str) -> Option<String> {
    args.iter().position(|a| a == name).and_then(|i| args.get(i + 1)).cloned()
}

async fn run_check(config: &SignerConfig, url: &str) -> Result<()> {
    println!("AppID：{}", config.app_id);
    println!("token 接口：{:?}", config.token_api);
    println!("测试页面：{url}");
    let cache = TicketCache::new(DEFAULT_API_BASE);
    match cache.signature(config, url).await {
        Ok(signature) => {
            println!("✓ 获取 jsapi_ticket 成功，签名：{}", signature.signature);
            println!("  页面域名还需加入公众号「JS 接口安全域名」才能在微信里生效。");
            Ok(())
        }
        Err(err) => {
            println!("✗ {err:#}");
            std::process::exit(1);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_env_files() {
        let values = parse_env_file(
            "# 注释\nWECHAT_APP_ID=wx123\nexport WECHAT_APP_SECRET=\"s3 cret\"\n\nWECHAT_ALLOWED_DOMAINS = 'a.com,b.com'\nbroken line\n",
        );
        assert_eq!(values["WECHAT_APP_ID"], "wx123");
        assert_eq!(values["WECHAT_APP_SECRET"], "s3 cret");
        assert_eq!(values["WECHAT_ALLOWED_DOMAINS"], "a.com,b.com");
        assert_eq!(values.len(), 3);
    }
}

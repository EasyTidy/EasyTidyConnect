use std::collections::HashMap;
use std::process::Stdio;
use std::time::Duration;

use anyhow::{bail, Context, Result};
use tokio::io::{AsyncBufReadExt, BufReader};
use tokio::process::{Child, Command};
use tokio::sync::Mutex;

use crate::models::PublishTarget;

pub struct TunnelManager {
    running: Mutex<HashMap<i64, RunningTunnel>>,
}

struct RunningTunnel {
    child: Child,
    url: String,
}

#[derive(Debug, Clone)]
pub struct TunnelInfo {
    pub url: String,
}

impl TunnelManager {
    pub fn new() -> Self {
        Self {
            running: Mutex::new(HashMap::new()),
        }
    }

    pub async fn ensure(&self, target: &PublishTarget, public_port: u16) -> Result<TunnelInfo> {
        let mut running = self.running.lock().await;

        if let Some(existing) = running.get_mut(&target.id) {
            if existing.child.try_wait()?.is_none() {
                return Ok(TunnelInfo {
                    url: existing.url.clone(),
                });
            }
            running.remove(&target.id);
        }

        let tunnel = match target.kind.as_str() {
            "lan_tunnel" => start_quick(target, public_port).await?,
            "cloudflare_tunnel" => start_named(target).await?,
            other => bail!("不是隧道类型：{other}"),
        };
        let url = tunnel.url.clone();
        running.insert(target.id, tunnel);
        Ok(TunnelInfo { url })
    }

    pub async fn stop(&self, target_id: i64) {
        if let Some(mut item) = self.running.lock().await.remove(&target_id) {
            let _ = item.child.kill().await;
        }
    }

    pub async fn stop_all(&self) {
        let mut running = self.running.lock().await;
        for (_, mut item) in running.drain() {
            let _ = item.child.kill().await;
        }
    }
}

async fn start_quick(target: &PublishTarget, public_port: u16) -> Result<RunningTunnel> {
    let executable = cloudflared(target);
    let mut child = Command::new(&executable)
        .args([
            "tunnel",
            "--url",
            &format!("http://127.0.0.1:{public_port}"),
            "--no-autoupdate",
        ])
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .kill_on_drop(true)
        .spawn()
        .with_context(|| format!("启动 cloudflared 失败：{executable}"))?;

    let stdout = child.stdout.take().context("cloudflared stdout 不可用")?;
    let stderr = child.stderr.take();
    let (tx, mut rx) = tokio::sync::mpsc::channel::<String>(2);

    let stdout_tx = tx.clone();
    tokio::spawn(async move {
        let mut lines = BufReader::new(stdout).lines();
        while let Ok(Some(line)) = lines.next_line().await {
            if let Some(url) = extract_url(&line) {
                let _ = stdout_tx.send(url).await;
            }
        }
    });

    if let Some(stderr) = stderr {
        tokio::spawn(async move {
            let mut lines = BufReader::new(stderr).lines();
            while let Ok(Some(line)) = lines.next_line().await {
                tracing::debug!("cloudflared: {line}");
                // cloudflared 将 quick tunnel URL 打印到 stderr。
                if let Some(url) = extract_url(&line) {
                    let _ = tx.send(url).await;
                }
            }
        });
    }

    match tokio::time::timeout(Duration::from_secs(35), rx.recv()).await {
        Ok(Some(url)) => Ok(RunningTunnel { child, url }),
        Ok(None) => {
            let _ = child.kill().await;
            bail!("cloudflared 未返回公网地址")
        }
        Err(_) => {
            let _ = child.kill().await;
            bail!("cloudflared 启动超时（35 秒）")
        }
    }
}

async fn start_named(target: &PublishTarget) -> Result<RunningTunnel> {
    let token = target.tunnel_token.trim();
    let hostname = target.hostname.trim();
    if token.is_empty() {
        bail!("Cloudflare Tunnel 缺少 tunnel token");
    }
    if hostname.is_empty() {
        bail!("Cloudflare Tunnel 缺少公开 hostname");
    }

    let executable = cloudflared(target);
    let mut child = Command::new(&executable)
        .args(["tunnel", "run"])
        .env("TUNNEL_TOKEN", token)
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .kill_on_drop(true)
        .spawn()
        .with_context(|| format!("启动 cloudflared 失败：{executable}"))?;

    tokio::time::sleep(Duration::from_secs(2)).await;
    if let Some(status) = child.try_wait()? {
        bail!("cloudflared 退出：{status}");
    }

    let url = if hostname.starts_with("http") {
        hostname.trim_end_matches('/').to_string()
    } else {
        format!("https://{}", hostname.trim_end_matches('/'))
    };
    Ok(RunningTunnel { child, url })
}

pub async fn test_cloudflared(target: &PublishTarget) -> Result<String> {
    let executable = cloudflared(target);
    let output = tokio::time::timeout(
        Duration::from_secs(10),
        Command::new(&executable).arg("--version").output(),
    )
    .await
    .context("cloudflared --version 超时")?
    .with_context(|| format!("找不到 cloudflared：{executable}"))?;

    if !output.status.success() {
        bail!("cloudflared 版本检查失败：{}", output.status);
    }
    Ok(String::from_utf8_lossy(&output.stdout).trim().to_string())
}

fn cloudflared(target: &PublishTarget) -> String {
    if target.cloudflared_path.trim().is_empty() {
        "cloudflared".to_string()
    } else {
        target.cloudflared_path.trim().to_string()
    }
}

fn extract_url(line: &str) -> Option<String> {
    let start = line.find("https://")?;
    let rest = &line[start..];
    let end = rest
        .find(|ch: char| ch.is_whitespace() || ch == '|' || ch == '"')
        .unwrap_or(rest.len());
    let url = rest[..end].trim_end_matches(['.', ',', ')']).to_string();
    if url.contains("trycloudflare.com") {
        Some(url)
    } else {
        None
    }
}

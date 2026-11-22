//! 内网穿透：把公开预览端口（只暴露 /a/*）映射到公网。
//!
//! | kind                | 实现                                   | 需要安装？            |
//! |---------------------|----------------------------------------|-----------------------|
//! | `ngrok`             | 官方 Rust SDK，进程内运行              | 否                    |
//! | `bore`              | bore 客户端（Rust），进程内运行        | 否                    |
//! | `ssh_tunnel`        | SSH 反向端口转发（russh），进程内运行  | 否                    |
//! | `frp`               | frpc 子进程                            | 首次使用自动下载      |
//! | `lan_tunnel`        | Cloudflare Quick Tunnel（cloudflared） | 本机没有时自动下载    |
//! | `cloudflare_tunnel` | Cloudflare 命名隧道（cloudflared）     | 本机没有时自动下载    |

use std::collections::HashMap;
use std::path::PathBuf;
use std::process::Stdio;
use std::sync::Arc;
use std::time::Duration;

use anyhow::{anyhow, bail, Context, Result};
use serde::{Deserialize, Serialize};
use tokio::io::{AsyncBufReadExt, BufReader};
use tokio::process::{Child, Command};
use tokio::sync::{mpsc, Mutex};
use tokio::task::JoinHandle;

use crate::models::PublishTarget;
use crate::tools::{self, Tool};

/// 所有隧道类发布目标。隧道地址是临时的，不允许分享（见 share::is_shareable_kind）。
pub const TUNNEL_KINDS: [&str; 6] = ["ngrok", "frp", "ssh_tunnel", "bore", "lan_tunnel", "cloudflare_tunnel"];

pub fn is_tunnel_kind(kind: &str) -> bool {
    TUNNEL_KINDS.contains(&kind)
}

/// 隧道配置（存于 publish_targets.tunnel_config，JSON）。各模式只用到自己的字段，其余留空。
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct TunnelConfig {
    /// 手动指定公网地址（覆盖自动推断）。
    pub public_url: String,

    // ngrok
    pub ngrok_authtoken: String,
    /// 固定域名（ngrok 后台申请的免费静态域名或自有域名），留空为随机地址。
    pub ngrok_domain: String,

    // frp
    pub frp_server_addr: String,
    pub frp_server_port: u16,
    pub frp_token: String,
    /// http（按域名）或 tcp（按端口）
    pub frp_proxy_type: String,
    pub frp_custom_domain: String,
    pub frp_remote_port: u16,
    /// frps 的 vhostHTTPPort，默认 80
    pub frp_http_port: u16,
    pub frpc_path: String,

    // SSH 反向隧道
    pub ssh_host: String,
    pub ssh_port: u16,
    pub ssh_user: String,
    pub ssh_password: String,
    /// 私钥内容（-----BEGIN ...）或私钥文件路径
    pub ssh_key: String,
    pub ssh_key_passphrase: String,
    /// 服务器上监听的端口，0 表示由服务器分配
    pub ssh_remote_port: u16,

    // bore
    pub bore_server: String,
    /// 远端端口，0 表示随机
    pub bore_port: u16,
    pub bore_secret: String,
}

impl TunnelConfig {
    pub fn parse(target: &PublishTarget) -> Self {
        serde_json::from_str(&target.tunnel_config).unwrap_or_default()
    }
}

pub struct TunnelManager {
    running: Mutex<HashMap<i64, RunningTunnel>>,
    /// 自动下载的 cloudflared / frpc 存放目录
    tools_dir: PathBuf,
}

struct RunningTunnel {
    handle: TunnelHandle,
    url: String,
    /// 配置指纹：发布目标被修改后重新建立隧道
    fingerprint: String,
}

enum TunnelHandle {
    Process(Child),
    Task(JoinHandle<()>),
}

impl TunnelHandle {
    fn alive(&mut self) -> bool {
        match self {
            TunnelHandle::Process(child) => matches!(child.try_wait(), Ok(None)),
            TunnelHandle::Task(task) => !task.is_finished(),
        }
    }

    async fn stop(self) {
        match self {
            TunnelHandle::Process(mut child) => {
                let _ = child.kill().await;
            }
            TunnelHandle::Task(task) => task.abort(),
        }
    }
}

#[derive(Debug, Clone)]
pub struct TunnelInfo {
    pub url: String,
}

impl TunnelManager {
    pub fn new(tools_dir: PathBuf) -> Self {
        Self {
            running: Mutex::new(HashMap::new()),
            tools_dir,
        }
    }

    /// 确保隧道在运行并返回公网地址；配置未变且仍存活时复用。
    pub async fn ensure(&self, target: &PublishTarget, public_port: u16) -> Result<TunnelInfo> {
        let fingerprint = fingerprint(target);
        let mut running = self.running.lock().await;
        if let Some(existing) = running.get_mut(&target.id) {
            if existing.fingerprint == fingerprint && existing.handle.alive() {
                return Ok(TunnelInfo { url: existing.url.clone() });
            }
            if let Some(old) = running.remove(&target.id) {
                old.handle.stop().await;
            }
        }
        let (handle, url) = self.start(target, public_port).await?;
        running.insert(target.id, RunningTunnel { handle, url: url.clone(), fingerprint });
        Ok(TunnelInfo { url })
    }

    /// 测试连接：临时建立隧道拿到公网地址后立即关闭（不影响已在运行的隧道）。
    pub async fn test(&self, target: &PublishTarget, public_port: u16) -> Result<String> {
        if let Some(existing) = self.running.lock().await.get_mut(&target.id) {
            if existing.fingerprint == fingerprint(target) && existing.handle.alive() {
                return Ok(existing.url.clone());
            }
        }
        let (handle, url) = self.start(target, public_port).await?;
        handle.stop().await;
        Ok(url)
    }

    pub async fn stop(&self, target_id: i64) {
        if let Some(item) = self.running.lock().await.remove(&target_id) {
            item.handle.stop().await;
        }
    }

    pub async fn stop_all(&self) {
        let mut running = self.running.lock().await;
        for (_, item) in running.drain() {
            item.handle.stop().await;
        }
    }

    async fn start(&self, target: &PublishTarget, port: u16) -> Result<(TunnelHandle, String)> {
        let cfg = TunnelConfig::parse(target);
        let (handle, url) = match target.kind.as_str() {
            "ngrok" => start_ngrok(&cfg, port).await?,
            "bore" => start_bore(&cfg, port).await?,
            "ssh_tunnel" => start_ssh(&cfg, port).await?,
            "frp" => start_frp(target, &cfg, port, &self.tools_dir).await?,
            "lan_tunnel" => start_quick(target, port, &self.tools_dir).await?,
            "cloudflare_tunnel" => start_named(target, &self.tools_dir).await?,
            other => bail!("不是内网穿透类型：{other}"),
        };
        let url = match cfg.public_url.trim() {
            "" => url,
            custom => custom.trim_end_matches('/').to_string(),
        };
        Ok((handle, url))
    }
}

fn fingerprint(target: &PublishTarget) -> String {
    format!(
        "{}|{}|{}|{}|{}",
        target.kind, target.tunnel_config, target.tunnel_token, target.hostname, target.cloudflared_path
    )
}

fn required<'a>(value: &'a str, label: &str) -> Result<&'a str> {
    let value = value.trim();
    if value.is_empty() {
        bail!("请填写{label}");
    }
    Ok(value)
}

/* ---------- ngrok：官方 SDK ---------- */

async fn start_ngrok(cfg: &TunnelConfig, port: u16) -> Result<(TunnelHandle, String)> {
    use ngrok::config::ForwarderBuilder;
    use ngrok::tunnel::EndpointInfo;

    let token = required(&cfg.ngrok_authtoken, " ngrok Authtoken")?;
    let session = ngrok::Session::builder()
        .authtoken(token)
        .connect()
        .await
        .map_err(|err| anyhow!("连接 ngrok 失败：{err}"))?;
    let mut endpoint = session.http_endpoint();
    if !cfg.ngrok_domain.trim().is_empty() {
        endpoint.domain(cfg.ngrok_domain.trim());
    }
    let upstream = url::Url::parse(&format!("http://127.0.0.1:{port}"))?;
    let mut forwarder = endpoint
        .listen_and_forward(upstream)
        .await
        .map_err(|err| anyhow!("建立 ngrok 隧道失败：{err}"))?;
    let url = forwarder.url().to_string();
    let task = tokio::spawn(async move {
        // session 必须和转发器一起存活
        let _session = session;
        if let Ok(Err(err)) = forwarder.join().await {
            tracing::warn!("ngrok forwarder stopped: {err}");
        }
    });
    Ok((TunnelHandle::Task(task), url))
}

/* ---------- bore ---------- */

async fn start_bore(cfg: &TunnelConfig, port: u16) -> Result<(TunnelHandle, String)> {
    let server = match cfg.bore_server.trim() {
        "" => "bore.pub",
        value => value,
    };
    let secret = Some(cfg.bore_secret.trim()).filter(|s| !s.is_empty());
    let client = tokio::time::timeout(
        Duration::from_secs(20),
        bore_cli::client::Client::new("127.0.0.1", port, server, cfg.bore_port, secret),
    )
    .await
    .context("连接 bore 服务器超时（20 秒）")?
    .with_context(|| format!("连接 bore 服务器 {server} 失败"))?;
    let url = format!("http://{server}:{}", client.remote_port());
    let task = tokio::spawn(async move {
        if let Err(err) = client.listen().await {
            tracing::warn!("bore tunnel stopped: {err}");
        }
    });
    Ok((TunnelHandle::Task(task), url))
}

/* ---------- SSH 反向隧道：自有 VPS / localhost.run / Pinggy ---------- */

struct SshHandler {
    local_port: u16,
    /// 服务器在会话输出中打印的公网地址（localhost.run、Pinggy 会这样做）
    url_tx: mpsc::Sender<String>,
}

impl SshHandler {
    fn capture(&self, data: &[u8]) {
        let text = String::from_utf8_lossy(data);
        for line in text.lines() {
            tracing::debug!("ssh: {line}");
            if let Some(url) = find_https_url(line, |_| true) {
                let _ = self.url_tx.try_send(url);
            }
        }
    }
}

impl russh::client::Handler for SshHandler {
    type Error = russh::Error;

    // 反向隧道只转发公开预览端口（只读的 /a/* 页面），不在本机执行任何远程命令，
    // 因此不做 known_hosts 校验，避免首次连接需要手工确认指纹。
    async fn check_server_key(&mut self, _key: &russh::keys::PublicKeyOrCertificate) -> Result<bool, Self::Error> {
        Ok(true)
    }

    async fn server_channel_open_forwarded_tcpip(
        &mut self,
        channel: russh::Channel<russh::client::Msg>,
        _connected_address: &str,
        _connected_port: u32,
        _originator_address: &str,
        _originator_port: u32,
        reply: russh::client::ChannelOpenHandle,
        _session: &mut russh::client::Session,
    ) -> Result<(), Self::Error> {
        reply.accept().await;
        let port = self.local_port;
        tokio::spawn(async move {
            match tokio::net::TcpStream::connect(("127.0.0.1", port)).await {
                Ok(mut local) => {
                    let mut remote = channel.into_stream();
                    let _ = tokio::io::copy_bidirectional(&mut local, &mut remote).await;
                }
                Err(err) => tracing::warn!("ssh forward to 127.0.0.1:{port} failed: {err}"),
            }
        });
        Ok(())
    }

    async fn data(
        &mut self,
        _channel: russh::ChannelId,
        data: &[u8],
        _session: &mut russh::client::Session,
    ) -> Result<(), Self::Error> {
        self.capture(data);
        Ok(())
    }

    async fn extended_data(
        &mut self,
        _channel: russh::ChannelId,
        _ext: u32,
        data: &[u8],
        _session: &mut russh::client::Session,
    ) -> Result<(), Self::Error> {
        self.capture(data);
        Ok(())
    }
}

async fn start_ssh(cfg: &TunnelConfig, port: u16) -> Result<(TunnelHandle, String)> {
    let host = required(&cfg.ssh_host, " SSH 服务器")?.to_string();
    let ssh_port = if cfg.ssh_port == 0 { 22 } else { cfg.ssh_port };
    let user = match cfg.ssh_user.trim() {
        "" => "nokey".to_string(),
        value => value.to_string(),
    };

    let (url_tx, mut url_rx) = mpsc::channel(4);
    let config = Arc::new(russh::client::Config {
        keepalive_interval: Some(Duration::from_secs(30)),
        ..Default::default()
    });
    let mut session = tokio::time::timeout(
        Duration::from_secs(20),
        russh::client::connect(config, (host.as_str(), ssh_port), SshHandler { local_port: port, url_tx }),
    )
    .await
    .context("连接 SSH 服务器超时（20 秒）")?
    .with_context(|| format!("连接 SSH 服务器 {host}:{ssh_port} 失败"))?;

    authenticate_ssh(&mut session, &user, cfg).await?;

    let requested = u32::from(cfg.ssh_remote_port);
    let assigned = session
        .tcpip_forward("", requested)
        .await
        .context("服务器拒绝了端口转发（检查 sshd 的 AllowTcpForwarding / GatewayPorts）")?;
    let remote_port = if requested == 0 { assigned } else { requested };

    // localhost.run / Pinggy 会在 shell 会话里打印分配的地址。
    let mut shell = session.channel_open_session().await.ok();
    if let Some(channel) = shell.as_mut() {
        let _ = channel.request_shell(false).await;
    }
    let printed = tokio::time::timeout(Duration::from_secs(8), url_rx.recv()).await.ok().flatten();
    let url = printed.unwrap_or_else(|| format!("http://{host}:{remote_port}"));

    let task = tokio::spawn(async move {
        let _shell = shell;
        // 会话断开（网络中断、服务器关闭）时任务结束，下次发布会重新连接；
        // 任务被中止时 session 随之释放，连接关闭。
        while !session.is_closed() {
            tokio::time::sleep(Duration::from_secs(3)).await;
        }
        tracing::warn!("ssh tunnel closed");
    });
    Ok((TunnelHandle::Task(task), url))
}

async fn authenticate_ssh(
    session: &mut russh::client::Handle<SshHandler>,
    user: &str,
    cfg: &TunnelConfig,
) -> Result<()> {
    let key = cfg.ssh_key.trim();
    if !key.is_empty() {
        let passphrase = Some(cfg.ssh_key_passphrase.as_str()).filter(|p| !p.is_empty());
        let private = if key.contains("PRIVATE KEY") {
            russh::keys::decode_secret_key(key, passphrase)
        } else {
            russh::keys::load_secret_key(key, passphrase)
        }
        .context("无法读取 SSH 私钥（检查内容或口令）")?;
        let hash = session.best_supported_rsa_hash().await.ok().flatten().flatten();
        let result = session
            .authenticate_publickey(user, russh::keys::PrivateKeyWithHashAlg::new(Arc::new(private), hash))
            .await?;
        if result.success() {
            return Ok(());
        }
        bail!("SSH 私钥认证失败");
    }
    if !cfg.ssh_password.is_empty() {
        if session.authenticate_password(user, &cfg.ssh_password).await?.success() {
            return Ok(());
        }
        bail!("SSH 密码认证失败");
    }
    // localhost.run / Pinggy 这类免费服务允许无密码登录。
    if session.authenticate_none(user).await?.success() {
        return Ok(());
    }
    use russh::client::KeyboardInteractiveAuthResponse as Kbd;
    let mut response = session.authenticate_keyboard_interactive_start(user, None).await?;
    for _ in 0..3 {
        match response {
            Kbd::Success => return Ok(()),
            Kbd::Failure { .. } => break,
            Kbd::InfoRequest { prompts, .. } => {
                response = session
                    .authenticate_keyboard_interactive_respond(vec![String::new(); prompts.len()])
                    .await?;
            }
        }
    }
    bail!("SSH 认证失败：请填写密码或私钥")
}

/* ---------- frp：frpc 子进程（首次使用自动下载） ---------- */

/// 生成 frpc.toml（frp ≥ 0.52 的 TOML 格式）。字符串用 JSON 转义，和 TOML 基本字符串兼容。
pub fn frpc_config(cfg: &TunnelConfig, target_id: i64, port: u16) -> Result<(String, String)> {
    let q = |value: &str| serde_json::to_string(value).unwrap_or_else(|_| "\"\"".to_string());
    let server = required(&cfg.frp_server_addr, " frps 服务器地址")?;
    let server_port = if cfg.frp_server_port == 0 { 7000 } else { cfg.frp_server_port };
    let mut toml = format!("serverAddr = {}\nserverPort = {server_port}\nloginFailExit = true\n", q(server));
    if !cfg.frp_token.trim().is_empty() {
        toml.push_str(&format!("auth.method = \"token\"\nauth.token = {}\n", q(cfg.frp_token.trim())));
    }
    toml.push_str(&format!(
        "\n[[proxies]]\nname = {}\nlocalIP = \"127.0.0.1\"\nlocalPort = {port}\n",
        q(&format!("easytidy-connect-{target_id}"))
    ));
    let url = if cfg.frp_proxy_type.trim() == "tcp" {
        if cfg.frp_remote_port == 0 {
            bail!("frp TCP 模式请填写远端端口");
        }
        toml.push_str(&format!("type = \"tcp\"\nremotePort = {}\n", cfg.frp_remote_port));
        format!("http://{server}:{}", cfg.frp_remote_port)
    } else {
        let domain = required(&cfg.frp_custom_domain, " frp 自定义域名（已解析到 frps 服务器）")?;
        toml.push_str(&format!("type = \"http\"\ncustomDomains = [{}]\n", q(domain)));
        match cfg.frp_http_port {
            0 | 80 => format!("http://{domain}"),
            p => format!("http://{domain}:{p}"),
        }
    };
    Ok((toml, url))
}

async fn start_frp(
    target: &PublishTarget,
    cfg: &TunnelConfig,
    port: u16,
    tools_dir: &std::path::Path,
) -> Result<(TunnelHandle, String)> {
    let (toml, url) = frpc_config(cfg, target.id, port)?;
    let frpc = tools::resolve(Tool::Frpc, &cfg.frpc_path, tools_dir)
        .await
        .context("准备 frpc 失败")?;
    let run_dir = tools_dir.join("frpc").join("run");
    std::fs::create_dir_all(&run_dir)?;
    let config_path = run_dir.join(format!("target-{}.toml", target.id));
    std::fs::write(&config_path, toml)?;

    let mut child = Command::new(&frpc)
        .arg("-c")
        .arg(&config_path)
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .kill_on_drop(true)
        .spawn()
        .with_context(|| format!("启动 frpc 失败：{}", frpc.display()))?;

    // frpc 把日志写到 stdout：成功时打印 "start proxy success"，失败时打印 [E] 行。
    let (tx, mut rx) = mpsc::channel::<Result<(), String>>(4);
    for stream in [
        child.stdout.take().map(|s| Box::new(s) as Box<dyn tokio::io::AsyncRead + Unpin + Send>),
        child.stderr.take().map(|s| Box::new(s) as Box<dyn tokio::io::AsyncRead + Unpin + Send>),
    ]
    .into_iter()
    .flatten()
    {
        let tx = tx.clone();
        tokio::spawn(async move {
            let mut lines = BufReader::new(stream).lines();
            while let Ok(Some(line)) = lines.next_line().await {
                tracing::debug!("frpc: {line}");
                if line.contains("start proxy success") {
                    let _ = tx.send(Ok(())).await;
                } else if line.contains("[E]") || line.contains("login to the server failed") {
                    let _ = tx.send(Err(line)).await;
                }
            }
        });
    }
    drop(tx);

    match tokio::time::timeout(Duration::from_secs(25), rx.recv()).await {
        Ok(Some(Ok(()))) => Ok((TunnelHandle::Process(child), url)),
        Ok(Some(Err(line))) => {
            let _ = child.kill().await;
            bail!("frpc 连接失败：{}", line.trim())
        }
        Ok(None) => {
            let status = child.wait().await.ok();
            bail!("frpc 已退出{}", status.map(|s| format!("（{s}）")).unwrap_or_default())
        }
        Err(_) => {
            let _ = child.kill().await;
            bail!("frpc 启动超时（25 秒），请检查 frps 地址、端口和 token")
        }
    }
}

/* ---------- Cloudflare：本机没有 cloudflared 时自动下载 ---------- */

async fn start_quick(target: &PublishTarget, port: u16, tools_dir: &std::path::Path) -> Result<(TunnelHandle, String)> {
    let executable = tools::resolve(Tool::Cloudflared, &target.cloudflared_path, tools_dir)
        .await
        .context("准备 cloudflared 失败")?;
    let mut child = Command::new(&executable)
        .args(["tunnel", "--url", &format!("http://127.0.0.1:{port}"), "--no-autoupdate"])
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .kill_on_drop(true)
        .spawn()
        .with_context(|| format!("启动 cloudflared 失败：{}", executable.display()))?;

    let (tx, mut rx) = mpsc::channel::<String>(2);
    for stream in [
        child.stdout.take().map(|s| Box::new(s) as Box<dyn tokio::io::AsyncRead + Unpin + Send>),
        child.stderr.take().map(|s| Box::new(s) as Box<dyn tokio::io::AsyncRead + Unpin + Send>),
    ]
    .into_iter()
    .flatten()
    {
        let tx = tx.clone();
        tokio::spawn(async move {
            let mut lines = BufReader::new(stream).lines();
            while let Ok(Some(line)) = lines.next_line().await {
                tracing::debug!("cloudflared: {line}");
                // Quick Tunnel 的地址打印在 stderr
                if let Some(url) = find_https_url(&line, |url| url.contains("trycloudflare.com")) {
                    let _ = tx.send(url).await;
                }
            }
        });
    }
    drop(tx);

    match tokio::time::timeout(Duration::from_secs(35), rx.recv()).await {
        Ok(Some(url)) => Ok((TunnelHandle::Process(child), url)),
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

async fn start_named(target: &PublishTarget, tools_dir: &std::path::Path) -> Result<(TunnelHandle, String)> {
    let token = required(&target.tunnel_token, " Cloudflare Tunnel Token")?;
    let hostname = required(&target.hostname, "公开 hostname")?;
    let executable = tools::resolve(Tool::Cloudflared, &target.cloudflared_path, tools_dir)
        .await
        .context("准备 cloudflared 失败")?;
    let mut child = Command::new(&executable)
        .args(["tunnel", "--no-autoupdate", "run"])
        .env("TUNNEL_TOKEN", token)
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .kill_on_drop(true)
        .spawn()
        .with_context(|| format!("启动 cloudflared 失败：{}", executable.display()))?;

    tokio::time::sleep(Duration::from_secs(2)).await;
    if let Some(status) = child.try_wait()? {
        bail!("cloudflared 退出：{status}（检查 Tunnel Token）");
    }
    let url = if hostname.starts_with("http") {
        hostname.trim_end_matches('/').to_string()
    } else {
        format!("https://{}", hostname.trim_end_matches('/'))
    };
    Ok((TunnelHandle::Process(child), url))
}

/// 从一行日志里找出第一个满足条件的 https 地址。
fn find_https_url(line: &str, accept: impl Fn(&str) -> bool) -> Option<String> {
    let mut rest = line;
    while let Some(start) = rest.find("https://") {
        let candidate = &rest[start..];
        let end = candidate
            .find(|ch: char| ch.is_whitespace() || matches!(ch, '|' | '"' | '\'' | '<' | '>'))
            .unwrap_or(candidate.len());
        let url = candidate[..end].trim_end_matches(['.', ',', ')', ';']).to_string();
        if url.len() > "https://".len() && accept(&url) {
            return Some(url);
        }
        rest = &candidate[end..];
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn finds_urls_in_tunnel_output() {
        let line = "2026-11-21 INF |  https://quiet-sky-1234.trycloudflare.com  |";
        assert_eq!(
            find_https_url(line, |u| u.contains("trycloudflare.com")).as_deref(),
            Some("https://quiet-sky-1234.trycloudflare.com")
        );
        let lhr = "Connect to http://abc.lhr.life or https://abc.lhr.life.";
        assert_eq!(find_https_url(lhr, |_| true).as_deref(), Some("https://abc.lhr.life"));
        assert!(find_https_url("no url here", |_| true).is_none());
    }

    #[test]
    fn builds_frpc_config() {
        let cfg = TunnelConfig {
            frp_server_addr: "frp.example.com".into(),
            frp_token: "se\"cret".into(),
            frp_custom_domain: "love.example.com".into(),
            frp_http_port: 8080,
            ..Default::default()
        };
        let (toml, url) = frpc_config(&cfg, 3, 53410).unwrap();
        assert!(toml.contains("serverAddr = \"frp.example.com\"\nserverPort = 7000"));
        assert!(toml.contains("auth.token = \"se\\\"cret\""));
        assert!(toml.contains("name = \"easytidy-connect-3\""));
        assert!(toml.contains("localPort = 53410"));
        assert!(toml.contains("customDomains = [\"love.example.com\"]"));
        assert_eq!(url, "http://love.example.com:8080");

        let tcp = TunnelConfig {
            frp_server_addr: "1.2.3.4".into(),
            frp_proxy_type: "tcp".into(),
            frp_remote_port: 6000,
            ..Default::default()
        };
        let (toml, url) = frpc_config(&tcp, 1, 53410).unwrap();
        assert!(toml.contains("type = \"tcp\"\nremotePort = 6000"));
        assert!(!toml.contains("auth.token"));
        assert_eq!(url, "http://1.2.3.4:6000");

        assert!(frpc_config(&TunnelConfig::default(), 1, 1).is_err());
    }

    #[test]
    fn tunnel_config_tolerates_missing_fields() {
        let target_json = r#"{"ngrok_authtoken":"abc","unknown":1}"#;
        let cfg: TunnelConfig = serde_json::from_str(target_json).unwrap();
        assert_eq!(cfg.ngrok_authtoken, "abc");
        assert_eq!(cfg.ssh_port, 0);
        assert!(is_tunnel_kind("frp") && !is_tunnel_kind("s3"));
    }
}

//! 外部工具（cloudflared、frpc）的自动安装：
//! 自定义路径 > 系统 PATH > 已下载的副本 > 首次使用时按当前平台自动下载。
//! 下载优先走 GitHub 官方 release，失败后依次尝试国内加速镜像；能拿到 SHA-256 时会校验。

use std::path::{Path, PathBuf};
use std::sync::OnceLock;
use std::time::Duration;

use anyhow::{bail, Context, Result};
use reqwest::Client;
use sha2::{Digest, Sha256};
use tokio::process::Command;
use tokio::sync::Mutex;

/// GitHub 加速镜像：在原始地址前加前缀。官方地址失败时按顺序尝试。
pub const GITHUB_MIRRORS: [&str; 2] = ["https://ghfast.top/", "https://gh-proxy.com/"];

/// 拿不到 GitHub API（限流 / 网络不通）时使用的 frp 版本。
pub const FRP_FALLBACK_VERSION: &str = "0.61.1";

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Tool {
    Cloudflared,
    Frpc,
}

impl Tool {
    fn name(self) -> &'static str {
        match self {
            Tool::Cloudflared => "cloudflared",
            Tool::Frpc => "frpc",
        }
    }

    /// 可执行文件名（Windows 带 .exe）。
    pub fn file_name(self) -> String {
        format!("{}{}", self.name(), std::env::consts::EXE_SUFFIX)
    }

    fn version_arg(self) -> &'static str {
        match self {
            Tool::Cloudflared => "--version",
            Tool::Frpc => "-v",
        }
    }
}

/// 下载用的平台标识（cloudflared 与 frp 的 release 都用 Go 的命名：windows/linux/darwin + amd64/arm64/386/arm）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Platform {
    pub os: &'static str,
    pub arch: &'static str,
}

impl Platform {
    pub fn current() -> Result<Self> {
        Self::from_rust(std::env::consts::OS, std::env::consts::ARCH)
    }

    pub fn from_rust(os: &str, arch: &str) -> Result<Self> {
        let os = match os {
            "windows" => "windows",
            "linux" => "linux",
            "macos" => "darwin",
            other => bail!("暂不支持在 {other} 上自动下载内网穿透工具"),
        };
        let arch = match arch {
            "x86_64" => "amd64",
            "aarch64" => "arm64",
            "x86" => "386",
            "arm" => "arm",
            other => bail!("暂不支持 {other} 架构自动下载内网穿透工具"),
        };
        Ok(Self { os, arch })
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ArchiveFormat {
    /// 直接就是可执行文件
    Raw,
    TarGz,
    Zip,
}

/// 某个平台对应的 release 资源。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ReleaseAsset {
    /// GitHub 上的相对路径，例如 `cloudflare/cloudflared/releases/latest/download/cloudflared-linux-amd64`
    pub path: String,
    /// 资源文件名（用于在 release 信息里查 SHA-256）
    pub name: String,
    pub format: ArchiveFormat,
}

/// cloudflared 各平台的 release 文件。Windows ARM64 没有原生版本，使用 x64 版本（系统自带转译）。
pub fn cloudflared_asset(platform: Platform) -> Result<ReleaseAsset> {
    let (name, format) = match (platform.os, platform.arch) {
        ("windows", "amd64" | "arm64") => ("cloudflared-windows-amd64.exe".to_string(), ArchiveFormat::Raw),
        ("windows", "386") => ("cloudflared-windows-386.exe".to_string(), ArchiveFormat::Raw),
        ("linux", arch @ ("amd64" | "arm64" | "386" | "arm")) => (format!("cloudflared-linux-{arch}"), ArchiveFormat::Raw),
        ("darwin", arch @ ("amd64" | "arm64")) => (format!("cloudflared-darwin-{arch}.tgz"), ArchiveFormat::TarGz),
        (os, arch) => bail!("cloudflared 没有提供 {os}/{arch} 的版本，请手动安装后填写路径"),
    };
    Ok(ReleaseAsset {
        path: format!("cloudflare/cloudflared/releases/latest/download/{name}"),
        name,
        format,
    })
}

/// frp 各平台的 release 文件（Windows 为 zip，其余为 tar.gz）。
pub fn frp_asset(platform: Platform, version: &str) -> Result<ReleaseAsset> {
    let version = version.trim_start_matches('v');
    let supported = match platform.os {
        "windows" => matches!(platform.arch, "amd64" | "arm64" | "386"),
        "linux" => matches!(platform.arch, "amd64" | "arm64" | "386" | "arm"),
        "darwin" => matches!(platform.arch, "amd64" | "arm64"),
        _ => false,
    };
    if !supported {
        bail!("frp 没有提供 {}/{} 的版本，请手动安装后填写 frpc 路径", platform.os, platform.arch);
    }
    let (ext, format) = if platform.os == "windows" {
        ("zip", ArchiveFormat::Zip)
    } else {
        ("tar.gz", ArchiveFormat::TarGz)
    };
    let name = format!("frp_{version}_{}_{}.{ext}", platform.os, platform.arch);
    Ok(ReleaseAsset {
        path: format!("fatedier/frp/releases/download/v{version}/{name}"),
        name,
        format,
    })
}

fn install_lock() -> &'static Mutex<()> {
    static LOCK: OnceLock<Mutex<()>> = OnceLock::new();
    LOCK.get_or_init(|| Mutex::new(()))
}

/// 找到可用的工具；本机没有时自动下载到 `tools_dir`。
pub async fn resolve(tool: Tool, custom_path: &str, tools_dir: &Path) -> Result<PathBuf> {
    let custom = custom_path.trim();
    if !custom.is_empty() {
        let path = PathBuf::from(custom);
        version(tool, &path)
            .await
            .with_context(|| format!("无法运行 {}：{custom}", tool.name()))?;
        return Ok(path);
    }
    // 系统 PATH 里已经装好的版本
    let in_path = PathBuf::from(tool.name());
    if version(tool, &in_path).await.is_ok() {
        return Ok(in_path);
    }
    let installed = tools_dir.join(tool.name()).join(tool.file_name());

    let _guard = install_lock().lock().await;
    if installed.exists() && version(tool, &installed).await.is_ok() {
        return Ok(installed);
    }
    install(tool, &installed).await?;
    version(tool, &installed)
        .await
        .with_context(|| format!("{} 已下载但无法运行（可能被安全软件拦截）", tool.name()))?;
    Ok(installed)
}

/// 运行 `--version` 检查工具是否可用，返回版本信息。
pub async fn version(tool: Tool, path: &Path) -> Result<String> {
    let output = tokio::time::timeout(
        Duration::from_secs(10),
        Command::new(path).arg(tool.version_arg()).kill_on_drop(true).output(),
    )
    .await
    .context("版本检查超时")??;
    if !output.status.success() {
        bail!("版本检查失败：{}", output.status);
    }
    let text = String::from_utf8_lossy(&output.stdout).trim().to_string();
    Ok(if text.is_empty() {
        String::from_utf8_lossy(&output.stderr).trim().to_string()
    } else {
        text
    })
}

struct ReleaseInfo {
    tag: String,
    /// 资源名 → SHA-256（十六进制，小写）
    digests: Vec<(String, String)>,
}

/// 读 GitHub release 信息（版本号与资源的 SHA-256）。限流或网络不通时返回 None，不影响下载。
async fn release_info(client: &Client, repo: &str, tag: Option<&str>) -> Option<ReleaseInfo> {
    let url = match tag {
        Some(tag) => format!("https://api.github.com/repos/{repo}/releases/tags/{tag}"),
        None => format!("https://api.github.com/repos/{repo}/releases/latest"),
    };
    let value: serde_json::Value = client
        .get(url)
        .header("Accept", "application/vnd.github+json")
        .timeout(Duration::from_secs(10))
        .send()
        .await
        .ok()?
        .error_for_status()
        .ok()?
        .json()
        .await
        .ok()?;
    let tag = value.get("tag_name")?.as_str()?.to_string();
    let digests = value
        .get("assets")
        .and_then(|assets| assets.as_array())
        .map(|assets| {
            assets
                .iter()
                .filter_map(|asset| {
                    let name = asset.get("name")?.as_str()?;
                    let digest = asset.get("digest")?.as_str()?.strip_prefix("sha256:")?;
                    Some((name.to_string(), digest.to_ascii_lowercase()))
                })
                .collect()
        })
        .unwrap_or_default();
    Some(ReleaseInfo { tag, digests })
}

/// 依次尝试官方地址和镜像，返回第一个成功（且通过校验）的内容。
async fn download(client: &Client, path: &str, sha256: Option<&str>) -> Result<Vec<u8>> {
    let official = format!("https://github.com/{path}");
    let candidates = std::iter::once(official.clone())
        .chain(GITHUB_MIRRORS.iter().map(|mirror| format!("{mirror}{official}")));
    let mut errors = Vec::new();
    for url in candidates {
        let attempt = async {
            let mut response = client.get(&url).send().await?.error_for_status()?;
            // 不限总时长（慢速网络也能下完），但 20 秒收不到数据就换下一个地址。
            let mut bytes = Vec::with_capacity(response.content_length().unwrap_or(0) as usize);
            loop {
                match tokio::time::timeout(Duration::from_secs(20), response.chunk()).await {
                    Ok(Ok(Some(chunk))) => bytes.extend_from_slice(&chunk),
                    Ok(Ok(None)) => break,
                    Ok(Err(err)) => return Err(err.into()),
                    Err(_) => bail!("20 秒内没有收到数据"),
                }
            }
            if bytes.len() < 1024 * 1024 {
                bail!("文件过小（{} 字节），可能是错误页面", bytes.len());
            }
            if let Some(expected) = sha256 {
                let actual = hex::encode(Sha256::digest(&bytes));
                if actual != expected {
                    bail!("SHA-256 校验失败");
                }
            }
            Ok::<_, anyhow::Error>(bytes)
        };
        match attempt.await {
            Ok(bytes) => {
                tracing::info!("downloaded {url}");
                return Ok(bytes);
            }
            Err(err) => {
                tracing::warn!("download {url} failed: {err}");
                errors.push(format!("{url}：{err}"));
            }
        }
    }
    bail!("下载失败（官方地址和镜像都不可用）：\n{}", errors.join("\n"))
}

async fn install(tool: Tool, dest: &Path) -> Result<()> {
    let platform = Platform::current()?;
    let client = Client::builder()
        .user_agent(concat!("EasyTidyConnect/", env!("CARGO_PKG_VERSION")))
        .connect_timeout(Duration::from_secs(10))
        .build()?;

    let (asset, info) = match tool {
        Tool::Cloudflared => {
            let info = release_info(&client, "cloudflare/cloudflared", None).await;
            (cloudflared_asset(platform)?, info)
        }
        Tool::Frpc => {
            let info = release_info(&client, "fatedier/frp", None).await;
            let version = info.as_ref().map(|i| i.tag.clone()).unwrap_or_else(|| FRP_FALLBACK_VERSION.to_string());
            (frp_asset(platform, &version)?, info)
        }
    };
    let sha256 = info
        .as_ref()
        .and_then(|i| i.digests.iter().find(|(name, _)| name == &asset.name))
        .map(|(_, digest)| digest.as_str());
    tracing::info!(
        "installing {} for {}/{} from {} (sha256 {})",
        tool.name(),
        platform.os,
        platform.arch,
        asset.path,
        if sha256.is_some() { "verified" } else { "unavailable" }
    );

    let bytes = download(&client, &asset.path, sha256).await?;
    let binary = extract(&bytes, asset.format, &tool.file_name())?;

    std::fs::create_dir_all(dest.parent().context("invalid tool path")?)?;
    let tmp = dest.with_extension("download");
    std::fs::write(&tmp, binary)?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(&tmp, std::fs::Permissions::from_mode(0o755))?;
    }
    std::fs::rename(&tmp, dest)?;
    Ok(())
}

/// 从下载内容里取出可执行文件（压缩包里按文件名匹配，忽略目录层级）。
pub fn extract(bytes: &[u8], format: ArchiveFormat, file_name: &str) -> Result<Vec<u8>> {
    use std::io::Read;
    match format {
        ArchiveFormat::Raw => Ok(bytes.to_vec()),
        ArchiveFormat::TarGz => {
            let mut archive = tar::Archive::new(flate2::read::GzDecoder::new(bytes));
            for entry in archive.entries()? {
                let mut entry = entry?;
                let matches = entry
                    .path()?
                    .file_name()
                    .is_some_and(|name| name.to_string_lossy() == file_name);
                if matches {
                    let mut out = Vec::new();
                    entry.read_to_end(&mut out)?;
                    return Ok(out);
                }
            }
            bail!("压缩包里没有找到 {file_name}")
        }
        ArchiveFormat::Zip => {
            let mut archive = zip::ZipArchive::new(std::io::Cursor::new(bytes))?;
            for index in 0..archive.len() {
                let mut file = archive.by_index(index)?;
                let name = file.name()?.to_string();
                let matches = Path::new(&name)
                    .file_name()
                    .is_some_and(|name| name.to_string_lossy() == file_name);
                if matches && file.is_file() {
                    let mut out = Vec::new();
                    file.read_to_end(&mut out)?;
                    return Ok(out);
                }
            }
            bail!("压缩包里没有找到 {file_name}")
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn maps_rust_platforms_to_release_names() {
        assert_eq!(Platform::from_rust("macos", "aarch64").unwrap(), Platform { os: "darwin", arch: "arm64" });
        assert!(Platform::from_rust("freebsd", "x86_64").is_err());

        let win = Platform::from_rust("windows", "x86_64").unwrap();
        assert_eq!(cloudflared_asset(win).unwrap().name, "cloudflared-windows-amd64.exe");
        let win_arm = Platform::from_rust("windows", "aarch64").unwrap();
        assert_eq!(cloudflared_asset(win_arm).unwrap().name, "cloudflared-windows-amd64.exe");
        let mac = Platform::from_rust("macos", "aarch64").unwrap();
        let asset = cloudflared_asset(mac).unwrap();
        assert_eq!(asset.name, "cloudflared-darwin-arm64.tgz");
        assert_eq!(asset.format, ArchiveFormat::TarGz);
        let linux = Platform::from_rust("linux", "aarch64").unwrap();
        assert_eq!(cloudflared_asset(linux).unwrap().path, "cloudflare/cloudflared/releases/latest/download/cloudflared-linux-arm64");

        let frp = frp_asset(win, "v0.61.1").unwrap();
        assert_eq!(frp.name, "frp_0.61.1_windows_amd64.zip");
        assert_eq!(frp.path, "fatedier/frp/releases/download/v0.61.1/frp_0.61.1_windows_amd64.zip");
        assert_eq!(frp_asset(mac, "0.61.1").unwrap().name, "frp_0.61.1_darwin_arm64.tar.gz");
        assert!(frp_asset(Platform { os: "darwin", arch: "386" }, "0.61.1").is_err());
    }

    #[test]
    fn extracts_binary_from_archives() {
        // tar.gz：frp_x/frpc
        let mut tar_bytes = Vec::new();
        {
            let gz = flate2::write::GzEncoder::new(&mut tar_bytes, flate2::Compression::fast());
            let mut builder = tar::Builder::new(gz);
            for (path, data) in [("frp_x/LICENSE", b"license".as_slice()), ("frp_x/frpc", b"binary".as_slice())] {
                let mut header = tar::Header::new_gnu();
                header.set_size(data.len() as u64);
                header.set_mode(0o755);
                header.set_cksum();
                builder.append_data(&mut header, path, data).unwrap();
            }
            builder.into_inner().unwrap().finish().unwrap();
        }
        assert_eq!(extract(&tar_bytes, ArchiveFormat::TarGz, "frpc").unwrap(), b"binary");
        assert!(extract(&tar_bytes, ArchiveFormat::TarGz, "frps").is_err());

        // zip：frp_x/frpc.exe
        let mut zip_bytes = std::io::Cursor::new(Vec::new());
        {
            use std::io::Write;
            let mut writer = zip::ZipWriter::new(&mut zip_bytes);
            writer.start_file("frp_x/frpc.exe", zip::write::SimpleFileOptions::default()).unwrap();
            writer.write_all(b"exe").unwrap();
            writer.finish().unwrap();
        }
        assert_eq!(extract(zip_bytes.get_ref(), ArchiveFormat::Zip, "frpc.exe").unwrap(), b"exe");
    }
}

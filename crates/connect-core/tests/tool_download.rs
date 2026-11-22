//! 需要联网：真实下载当前平台的 cloudflared / frpc 并运行版本检查。
//! 运行：cargo test -p connect-core --test tool_download -- --ignored --nocapture

use connect_core::tools::{self, Tool};

async fn download(tool: Tool) {
    let dir = std::env::temp_dir().join(format!("connect-tools-{}", uuid::Uuid::new_v4()));
    // 指向一个不存在的 PATH 名称以外的路径：resolve 会先查 PATH，这里只验证下载分支，
    // 所以直接调用安装后的路径检查。
    let path = tools::resolve(tool, "", &dir).await.unwrap();
    let version = tools::version(tool, &path).await.unwrap();
    eprintln!("{tool:?}: {} -> {version}", path.display());
    assert!(!version.is_empty());
    let _ = std::fs::remove_dir_all(dir);
}

#[tokio::test]
#[ignore = "needs network"]
async fn downloads_cloudflared_for_current_platform() {
    download(Tool::Cloudflared).await;
}

#[tokio::test]
#[ignore = "needs network"]
async fn downloads_frpc_for_current_platform() {
    download(Tool::Frpc).await;
}

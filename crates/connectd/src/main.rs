use std::path::PathBuf;
use std::sync::Arc;

use anyhow::Result;
use connect_core::Database;

#[tokio::main]
async fn main() -> Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| "info".into()),
        )
        .init();

    let mut port = 53400u16;
    let mut data_dir = dirs::data_dir()
        .unwrap_or_else(|| PathBuf::from("."))
        .join("EasyTidyConnect");

    let mut args = std::env::args().skip(1);
    while let Some(arg) = args.next() {
        match arg.as_str() {
            "--port" => {
                if let Some(value) = args.next() {
                    port = value.parse().unwrap_or(port);
                }
            }
            "--data-dir" => {
                if let Some(value) = args.next() {
                    data_dir = PathBuf::from(value);
                }
            }
            "--help" | "-h" => {
                println!("connectd --port 53400 --data-dir <dir>");
                println!("微信 JS-SDK 签名服务请使用独立程序 wechat-signer（wechat-signer --help）");
                return Ok(());
            }
            _ => {}
        }
    }

    std::fs::create_dir_all(&data_dir)?;
    let db = Arc::new(Database::open(&data_dir.join("connect.db"))?);
    let public_db = db.clone();
    let public_data = data_dir.clone();
    tokio::spawn(async move {
        if let Err(err) = connect_core::server::serve_public(
            public_db,
            connect_core::server::PUBLIC_PREVIEW_PORT,
            public_data,
        )
        .await
        {
            tracing::error!("public preview server stopped: {err}");
        }
    });
    let tools_dir = data_dir.join("tools");
    connect_core::server::serve(
        db,
        port,
        data_dir,
        Arc::new(connect_core::TunnelManager::new(tools_dir)),
    )
    .await
}


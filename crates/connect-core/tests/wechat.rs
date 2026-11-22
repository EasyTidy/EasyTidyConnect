//! 桌面端「本机签名」接口：选择签名服务器时本机没有密钥，不应签名。
//! 签名本身（换票、缓存、CORS 等）的测试在 crates/wechat-signer/tests 里。

use std::sync::Arc;

use connect_core::WechatConfig;
use serde_json::Value;

async fn spawn(app: axum::Router) -> String {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });
    format!("http://{addr}")
}

#[tokio::test]
async fn local_router_follows_mode() {
    let root = std::env::temp_dir().join(format!("connect-wx-{}", uuid::Uuid::new_v4()));
    let db = Arc::new(connect_core::Database::open(&root.join("connect.db")).unwrap());
    let base = WechatConfig {
        enabled: true,
        mode: "server".to_string(),
        app_id: "wx123".to_string(),
        app_secret: String::new(),
        signer_url: "https://sign.example.com/wx/jssdk".to_string(),
        allowed_domains: String::new(),
    };
    db.save_wechat_config(&base).unwrap();
    let signer = spawn(connect_core::wechat::local_router(db.clone())).await;

    // 签名服务器模式：本机拒绝签名
    let response = reqwest::get(format!("{signer}/wx/jssdk?url=https://love.example.com/")).await.unwrap();
    assert_eq!(response.status(), 503);
    let health: Value = reqwest::get(format!("{signer}/wx/health")).await.unwrap().json().await.unwrap();
    assert_eq!(health["configured"], false);

    // 切到本机签名并填写密钥后，配置即时生效（无需重启）
    db.save_wechat_config(&WechatConfig { mode: "local".to_string(), app_secret: "s".to_string(), ..base })
        .unwrap();
    let health: Value = reqwest::get(format!("{signer}/wx/health")).await.unwrap().json().await.unwrap();
    assert_eq!(health["configured"], true);

    drop(db);
    let _ = std::fs::remove_dir_all(root);
}

//! 内网穿透端到端测试：本机起一个 bore 服务端，经 TunnelManager 建立隧道后从远端端口访问本地页面。

use axum::{routing::get, Router};
use connect_core::models::PublishTarget;
use connect_core::TunnelManager;

fn bore_target(port: u16) -> PublishTarget {
    PublishTarget {
        id: 42,
        name: "bore".to_string(),
        kind: "bore".to_string(),
        endpoint: String::new(),
        region: String::new(),
        bucket: String::new(),
        access_key: String::new(),
        secret_key: String::new(),
        prefix: String::new(),
        public_base_url: String::new(),
        cloudflared_path: String::new(),
        tunnel_token: String::new(),
        hostname: String::new(),
        account_id: String::new(),
        project_name: String::new(),
        api_token: String::new(),
        branch: String::new(),
        is_default: false,
        created_at: String::new(),
        tunnel_config: format!(r#"{{"bore_server":"127.0.0.1","bore_port":{port},"bore_secret":"s3cret"}}"#),
    }
}

#[tokio::test]
async fn bore_tunnel_forwards_to_local_preview() {
    // bore 服务端固定使用控制端口 7835；被占用时跳过（例如本机正在运行 bore server）。
    if std::net::TcpListener::bind(("127.0.0.1", 7835)).is_err() {
        eprintln!("port 7835 busy, skip");
        return;
    }
    let server = bore_cli::server::Server::new(41000..=41010, Some("s3cret"));
    tokio::spawn(server.listen());

    // 本地「公开预览」服务
    let app = Router::new().route("/a/demo", get(|| async { "hello from preview" }));
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let local_port = listener.local_addr().unwrap().port();
    tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });
    tokio::time::sleep(std::time::Duration::from_millis(200)).await;

    let tunnels = TunnelManager::new(std::env::temp_dir().join("connect-tunnel-test"));
    let target = bore_target(41005);
    let info = tunnels.ensure(&target, local_port).await.unwrap();
    assert_eq!(info.url, "http://127.0.0.1:41005");

    // 同一配置再次 ensure 复用现有隧道
    assert_eq!(tunnels.ensure(&target, local_port).await.unwrap().url, info.url);

    let body = reqwest::get(format!("{}/a/demo", info.url)).await.unwrap().text().await.unwrap();
    assert_eq!(body, "hello from preview");

    // 错误的 secret 应该失败
    let mut wrong = bore_target(41006);
    wrong.id = 43;
    wrong.tunnel_config = r#"{"bore_server":"127.0.0.1","bore_port":41006,"bore_secret":"nope"}"#.to_string();
    assert!(tunnels.ensure(&wrong, local_port).await.is_err());

    tunnels.stop_all().await;
}

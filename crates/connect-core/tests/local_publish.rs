//! 本机发布 + 碰标签打开网页 + 公开链接的 NFC 写入辅助页。

use std::sync::Arc;

use connect_core::models::{ProjectInput, PublishTargetInput};
use connect_core::{Database, TunnelManager};

fn target(kind: &str) -> PublishTargetInput {
    PublishTargetInput {
        name: "本机".to_string(),
        kind: kind.to_string(),
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
        tunnel_config: String::new(),
        is_default: true,
    }
}

#[tokio::test]
async fn tapping_a_local_anniversary_trigger_opens_the_page() {
    let root = std::env::temp_dir().join(format!("connect-local-{}", uuid::Uuid::new_v4()));
    let db = Arc::new(Database::open(&root.join("connect.db")).unwrap());
    let project = db
        .create_project(&ProjectInput {
            name: "周年".to_string(),
            event_date: "2026-12-24".to_string(),
            title: "三年前的今天".to_string(),
            recipient: "小雨".to_string(),
            opening: "亲爱的小雨：".to_string(),
            body: "第一段".to_string(),
            closing: "永远爱你".to_string(),
            template_id: "minimal".to_string(),
            theme: "#2563eb".to_string(),
            music: String::new(),
            video: String::new(),
            custom_css: String::new(),
            photo_focus: Default::default(),
            slideshow: false,
        })
        .unwrap();
    let local = db.create_publish_target(&target("local")).unwrap();
    let trigger = db
        .create_trigger(
            "纪念日",
            "publish_anniversary",
            &format!(r#"{{"project_id":{},"target_id":{}}}"#, project.id, local.id),
        )
        .unwrap();
    let row = db.find_by_id(trigger.id).unwrap().unwrap();

    let port = std::net::TcpListener::bind("127.0.0.1:0").unwrap().local_addr().unwrap().port();
    let server_db = db.clone();
    let data_dir = root.clone();
    tokio::spawn(async move {
        let _ = connect_core::server::serve(server_db, port, data_dir, Arc::new(TunnelManager::new(std::env::temp_dir()))).await;
    });
    tokio::time::sleep(std::time::Duration::from_millis(300)).await;
    let base = format!("http://127.0.0.1:{port}");
    let http = reqwest::Client::builder().redirect(reqwest::redirect::Policy::none()).build().unwrap();

    // 碰标签：跳转到本机发布的页面（而不是显示「已发布」）
    let page_url = connect_core::server::local_page_url(port, &project.slug);
    for _ in 0..2 {
        let tapped = http.get(format!("{base}/t/{}?k={}", row.public_id, row.token)).send().await.unwrap();
        assert_eq!(tapped.status(), 303);
        assert_eq!(tapped.headers()["location"], page_url.as_str());
    }
    // 第二次碰标签不会重复记录版本
    let versions = db.list_project_versions(project.id).unwrap();
    assert_eq!(versions.len(), 1);
    assert_eq!((versions[0].target_kind.as_str(), versions[0].published_url.as_str()), ("local", page_url.as_str()));

    // 本机页面可以直接打开
    let page = http.get(format!("{base}/a/{}", project.slug)).send().await.unwrap().text().await.unwrap();
    assert!(page.contains("三年前的今天"));

    // 公开链接的写入辅助页
    let helper = http
        .get(format!("{base}/nfc"))
        .query(&[("u", "https://love.example.com/anniversary/ab12/?a=1&b=<x>"), ("n", "周年")])
        .send()
        .await
        .unwrap();
    assert_eq!(helper.status(), 200);
    let html = helper.text().await.unwrap();
    assert!(html.contains("复制网页链接") && html.contains("周年"));
    assert!(html.contains("https://love.example.com/anniversary/ab12/?a=1&amp;b=&lt;x&gt;"));
    let bad = http.get(format!("{base}/nfc")).query(&[("u", "javascript:alert(1)")]).send().await.unwrap();
    assert_eq!(bad.status(), 400);

    drop(db);
    let _ = std::fs::remove_dir_all(root);
}

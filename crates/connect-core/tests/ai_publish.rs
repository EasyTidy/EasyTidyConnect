use std::sync::{Arc, Mutex};

use axum::{
    body::Bytes,
    extract::{Path, State},
    http::{HeaderMap, StatusCode},
    routing::{post, put},
    Router,
};
use connect_core::models::{AiProvider, AnniversaryProject, PublishTarget};
use serde_json::json;

#[derive(Default)]
struct Recorder {
    auth: Mutex<Vec<String>>,
    paths: Mutex<Vec<String>>,
    bodies: Mutex<Vec<(String, Vec<u8>)>>,
}

async fn mock_ai() -> axum::Json<serde_json::Value> {
    axum::Json(json!({
        "choices": [{
            "message": {
                "content": "{\"title\":\"给未来的我们\",\"opening\":\"开场\",\"body\":\"正文\",\"closing\":\"结尾\"}"
            }
        }]
    }))
}

async fn mock_put(
    State(recorder): State<Arc<Recorder>>,
    Path(path): Path<String>,
    headers: HeaderMap,
    body: Bytes,
) -> StatusCode {
    if let Some(auth) = headers.get("authorization").and_then(|v| v.to_str().ok()) {
        recorder.auth.lock().unwrap().push(auth.to_string());
    }
    recorder.bodies.lock().unwrap().push((path.clone(), body.to_vec()));
    recorder.paths.lock().unwrap().push(path);
    StatusCode::OK
}

async fn spawn_mock() -> (String, Arc<Recorder>) {
    let recorder = Arc::new(Recorder::default());
    let app = Router::new()
        .route("/v1/chat/completions", post(mock_ai))
        .route("/{*path}", put(mock_put))
        .with_state(recorder.clone());
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    tokio::spawn(async move {
        axum::serve(listener, app).await.unwrap();
    });
    (format!("http://{addr}"), recorder)
}

#[tokio::test]
async fn ai_generation_uses_openai_compatible_payload() {
    let (base, _) = spawn_mock().await;
    let provider = AiProvider {
        id: 1,
        name: "Mock".to_string(),
        kind: "openai".to_string(),
        base_url: base,
        model: "mock-model".to_string(),
        api_key: "test-key".to_string(),
        is_default: true,
        created_at: String::new(),
    };
    let input = connect_core::ai::AiGenerateInput {
        relationship: "伴侣".to_string(),
        tone: "温柔".to_string(),
        keywords: "海边".to_string(),
        language: "中文".to_string(),
        length: "中等".to_string(),
    };
    let copy = connect_core::ai::generate_copy(&provider, &input).await.unwrap();
    assert_eq!(copy.title, "给未来的我们");
    assert_eq!(copy.closing, "结尾");
}

#[tokio::test]
async fn s3_publish_uploads_html_and_photos_with_sigv4() {
    let (base, recorder) = spawn_mock().await;
    let root = std::env::temp_dir().join(format!("connect-s3-test-{}", uuid::Uuid::new_v4()));
    let photos = root.join("photos");
    std::fs::create_dir_all(&photos).unwrap();
    std::fs::write(photos.join("a.jpg"), b"fake-jpeg").unwrap();

    let project = AnniversaryProject {
        id: 1,
        slug: "testslug".to_string(),
        name: "测试".to_string(),
        event_date: "2026-11-15".to_string(),
        title: "标题".to_string(),
        recipient: "小满".to_string(),
        opening: "开场".to_string(),
        body: "正文".to_string(),
        closing: "结尾".to_string(),
        template_id: "minimal".to_string(),
        theme: "#2563eb".to_string(),
        photos: vec!["a.jpg".to_string()],
        music: String::new(),
        video: String::new(),
        custom_css: String::new(),
        photo_focus: Default::default(),
        slideshow: false,
        created_at: String::new(),
        updated_at: String::new(),
    };
    let target = PublishTarget {
        id: 1,
        name: "Mock S3".to_string(),
        kind: "s3".to_string(),
        endpoint: base.clone(),
        region: "us-east-1".to_string(),
        bucket: "bucket".to_string(),
        access_key: "AK".to_string(),
        secret_key: "SK".to_string(),
        prefix: "anniversary".to_string(),
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
        created_at: String::new(),
    };

    let html = connect_core::anniversary::render_project(&project, "photos/", "media/");
    let result = connect_core::publish::publish_project(
        &target,
        &project,
        &html,
        &photos,
        std::path::Path::new("/nonexistent"),
    )
        .await
        .unwrap();

    // 照片 + 分享缩略图 share.jpg
    assert_eq!(result.uploaded, 2);
    assert!(result.url.ends_with("/anniversary/testslug/index.html"));
    let paths = recorder.paths.lock().unwrap().clone();
    assert_eq!(paths.len(), 3);
    assert!(paths.iter().any(|path| path.ends_with("testslug/share.jpg")));
    assert!(recorder
        .auth
        .lock()
        .unwrap()
        .iter()
        .all(|value| value.starts_with("AWS4-HMAC-SHA256 ")));

    let _ = std::fs::remove_dir_all(root);
}

#[tokio::test]
async fn static_publish_writes_share_card_meta() {
    let (base, recorder) = spawn_mock().await;
    let root = std::env::temp_dir().join(format!("connect-share-test-{}", uuid::Uuid::new_v4()));
    let db = connect_core::Database::open(&root.join("connect.db")).unwrap();
    let project = db
        .create_project(&connect_core::ProjectInput {
            name: "周年".to_string(),
            event_date: "2026-12-24".to_string(),
            title: "三年前的今天".to_string(),
            recipient: "小雨".to_string(),
            opening: "亲爱的小雨：".to_string(),
            body: "第一段".to_string(),
            closing: "结尾".to_string(),
            template_id: "xitie".to_string(),
            theme: "#2563eb".to_string(),
            music: String::new(),
            video: String::new(),
            custom_css: String::new(),
            photo_focus: Default::default(),
            slideshow: false,
        })
        .unwrap();
    let photos = root.join("anniversaries").join(project.id.to_string()).join("photos");
    std::fs::create_dir_all(&photos).unwrap();
    std::fs::write(photos.join("cover.jpg"), b"fake-jpeg").unwrap();
    db.set_project_photos(project.id, &["cover.jpg".to_string()]).unwrap();
    db.save_share_settings(
        project.id,
        &connect_core::ShareSettings { title: "给小雨的喜帖".to_string(), description: String::new() },
    )
    .unwrap();
    let project = db.get_project(project.id).unwrap().unwrap();

    let target = PublishTarget {
        id: 1,
        name: "Mock S3".to_string(),
        kind: "s3".to_string(),
        endpoint: base.clone(),
        region: "us-east-1".to_string(),
        bucket: "bucket".to_string(),
        access_key: "AK".to_string(),
        secret_key: "SK".to_string(),
        prefix: "anniversary".to_string(),
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
        created_at: String::new(),
    };
    let result = connect_core::share::publish_static(&db, &root, &project, &target).await.unwrap();
    let page = format!("{base}/bucket/anniversary/{}/", project.slug);
    assert_eq!(result.url, format!("{page}index.html"));

    let bodies = recorder.bodies.lock().unwrap().clone();
    let (_, html) = bodies.iter().find(|(path, _)| path.ends_with("/index.html")).expect("index.html uploaded");
    let html = String::from_utf8(html.clone()).unwrap();
    assert!(html.contains("<title>给小雨的喜帖</title>"));
    assert!(html.contains(&format!("<meta property=\"og:image\" content=\"{page}share.jpg\">")));
    assert!(html.contains(&format!("<meta property=\"og:url\" content=\"{page}index.html\">")));
    assert!(html.contains("<meta name=\"description\" content=\"亲爱的小雨：\">"));
    assert!(bodies.iter().any(|(path, _)| path.ends_with("/share.jpg")));

    // 隧道类目标不允许走静态发布（不能分享）。
    let mut tunnel = target.clone();
    tunnel.kind = "lan_tunnel".to_string();
    assert!(connect_core::share::publish_static(&db, &root, &project, &tunnel).await.is_err());

    drop(db);
    let _ = std::fs::remove_dir_all(root);
}

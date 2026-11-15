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
    _body: Bytes,
) -> StatusCode {
    if let Some(auth) = headers.get("authorization").and_then(|v| v.to_str().ok()) {
        recorder.auth.lock().unwrap().push(auth.to_string());
    }
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

    assert_eq!(result.uploaded, 1);
    assert!(result.url.ends_with("/anniversary/testslug/index.html"));
    assert_eq!(recorder.paths.lock().unwrap().len(), 2);
    assert!(recorder
        .auth
        .lock()
        .unwrap()
        .iter()
        .all(|value| value.starts_with("AWS4-HMAC-SHA256 ")));

    let _ = std::fs::remove_dir_all(root);
}

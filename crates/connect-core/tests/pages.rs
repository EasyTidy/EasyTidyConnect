use std::sync::{Arc, Mutex};

use axum::{
    extract::{Multipart, State},
    routing::{get, post},
    Json, Router,
};
use connect_core::models::{AnniversaryProject, PublishTarget};
use serde_json::{json, Value};

#[derive(Default)]
struct Recorder {
    manifest: Mutex<String>,
}

async fn mock_project(State(_r): State<Arc<Recorder>>) -> Json<Value> {
    Json(json!({"success": true, "result": {"name": "proj"}}))
}

async fn upload_token(State(_r): State<Arc<Recorder>>) -> Json<Value> {
    Json(json!({"success": true, "result": {"jwt": "test-jwt"}}))
}

async fn check_missing(State(_r): State<Arc<Recorder>>) -> Json<Value> {
    Json(json!({"success": true, "result": []}))
}

async fn deployment(
    State(recorder): State<Arc<Recorder>>,
    mut multipart: Multipart,
) -> Json<Value> {
    while let Some(field) = multipart.next_field().await.unwrap() {
        if field.name() == Some("manifest") {
            *recorder.manifest.lock().unwrap() = field.text().await.unwrap();
        }
    }
    Json(json!({"success": true, "result": {"id": "dep1", "url": "https://test.pages.dev"}}))
}

async fn deployment_status(State(_r): State<Arc<Recorder>>) -> Json<Value> {
    Json(json!({"success": true, "result": {"file_count": 2}}))
}

async fn spawn_mock() -> (String, Arc<Recorder>) {
    let recorder = Arc::new(Recorder::default());
    let app = Router::new()
        .route("/accounts/acc/pages/projects/proj", get(mock_project))
        .route(
            "/accounts/acc/pages/projects/proj/upload-token",
            post(upload_token),
        )
        .route("/pages/assets/check-missing", post(check_missing))
        .route(
            "/accounts/acc/pages/projects/proj/deployments",
            post(deployment),
        )
        .route(
            "/accounts/acc/pages/projects/proj/deployments/dep1",
            get(deployment_status),
        )
        .with_state(recorder.clone());
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    tokio::spawn(async move {
        axum::serve(listener, app).await.unwrap();
    });
    (format!("http://{addr}"), recorder)
}

fn project() -> AnniversaryProject {
    AnniversaryProject {
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
        photos: vec![],
        custom_css: String::new(),
        created_at: String::new(),
        updated_at: String::new(),
    }
}

fn target(endpoint: String) -> PublishTarget {
    PublishTarget {
        id: 1,
        name: "Pages".to_string(),
        kind: "cloudflare_pages".to_string(),
        endpoint,
        region: String::new(),
        bucket: String::new(),
        access_key: String::new(),
        secret_key: String::new(),
        prefix: "anniversary".to_string(),
        public_base_url: String::new(),
        cloudflared_path: String::new(),
        tunnel_token: String::new(),
        hostname: String::new(),
        account_id: "acc".to_string(),
        project_name: "proj".to_string(),
        api_token: "api-token".to_string(),
        branch: "main".to_string(),
        is_default: true,
        created_at: String::new(),
    }
}

#[tokio::test]
async fn pages_direct_upload_creates_deployment() {
    let (base, recorder) = spawn_mock().await;
    let target = target(base);
    let project = project();
    let html = "<html>test</html>";

    connect_core::pages::test_target(&target).await.unwrap();

    let result = connect_core::pages::publish_project(
        &target,
        &project,
        html,
        std::path::Path::new("/nonexistent"),
    )
    .await
    .unwrap();

    assert_eq!(result.url, "https://test.pages.dev/anniversary/testslug/");
    let manifest = recorder.manifest.lock().unwrap().clone();
    assert!(manifest.contains("/anniversary/testslug/index.html"));
}

//! Cloudflare 一键授权后的自动创建资源：用本地模拟的 Cloudflare API 验证 R2 / Pages / Tunnel 流程。

use std::collections::HashMap;
use std::sync::{Arc, Mutex};

use axum::body::Bytes;
use axum::extract::{Path, Query, State};
use axum::http::{HeaderMap, StatusCode};
use axum::response::IntoResponse;
use axum::routing::{get, post, put};
use axum::{Json, Router};
use connect_core::cloudflare::{self, Api};
use serde_json::{json, Value};

#[derive(Default)]
struct Cf {
    buckets: Mutex<Vec<String>>,
    objects: Mutex<HashMap<String, (String, Vec<u8>)>>,
    projects: Mutex<Vec<String>>,
    tunnels: Mutex<Vec<String>>,
    ingress: Mutex<Option<Value>>,
    /// (zone, name, type, content, id)
    records: Mutex<Vec<(String, String, String, String, String)>>,
    calls: Mutex<Vec<String>>,
}

type S = State<Arc<Cf>>;

fn ok(result: Value) -> Json<Value> {
    Json(json!({ "success": true, "errors": [], "result": result }))
}

fn fail(status: StatusCode, code: i64, message: &str) -> axum::response::Response {
    (status, Json(json!({ "success": false, "errors": [{ "code": code, "message": message }] }))).into_response()
}

fn authed(headers: &HeaderMap) -> bool {
    headers.get("authorization").and_then(|v| v.to_str().ok()) == Some("Bearer test-token")
}

async fn spawn() -> (String, Arc<Cf>) {
    let cf = Arc::new(Cf::default());
    cf.records.lock().unwrap().push(("zone1".into(), "www.example.com".into(), "A".into(), "1.2.3.4".into(), "rec-a".into()));
    let app = Router::new()
        .route("/accounts", get(|h: HeaderMap| async move {
            if !authed(&h) { return fail(StatusCode::UNAUTHORIZED, 10000, "Authentication error"); }
            ok(json!([{ "id": "acc1", "name": "我的账户" }])).into_response()
        }))
        .route("/accounts/{acc}/r2/buckets", post(|State(cf): S, Json(body): Json<Value>| async move {
            let name = body["name"].as_str().unwrap().to_string();
            let mut buckets = cf.buckets.lock().unwrap();
            if buckets.contains(&name) { return fail(StatusCode::CONFLICT, 10004, "The bucket you tried to create already exists"); }
            buckets.push(name.clone());
            ok(json!({ "name": name })).into_response()
        }))
        .route("/accounts/{acc}/r2/buckets/{bucket}/domains/managed", put(|Path((_, bucket)): Path<(String, String)>, Json(body): Json<Value>| async move {
            assert_eq!(body["enabled"], true);
            ok(json!({ "bucketId": bucket, "domain": format!("pub-{bucket}.r2.dev"), "enabled": true }))
        }))
        .route("/accounts/{acc}/r2/buckets/{bucket}/objects/{*key}", put(|State(cf): S, Path((_, bucket, key)): Path<(String, String, String)>, h: HeaderMap, body: Bytes| async move {
            let ct = h.get("content-type").and_then(|v| v.to_str().ok()).unwrap_or_default().to_string();
            cf.objects.lock().unwrap().insert(format!("{bucket}/{key}"), (ct, body.to_vec()));
            ok(json!({}))
        }))
        .route("/accounts/{acc}/pages/projects/{name}", get(|State(cf): S, Path((_, name)): Path<(String, String)>| async move {
            if cf.projects.lock().unwrap().contains(&name) {
                ok(json!({ "name": name, "subdomain": format!("{name}.pages.dev") })).into_response()
            } else {
                fail(StatusCode::NOT_FOUND, 8000007, "Project not found")
            }
        }))
        .route("/accounts/{acc}/pages/projects", post(|State(cf): S, Json(body): Json<Value>| async move {
            let name = body["name"].as_str().unwrap().to_string();
            assert_eq!(body["production_branch"], "main");
            cf.projects.lock().unwrap().push(name.clone());
            cf.calls.lock().unwrap().push(format!("create-pages:{name}"));
            ok(json!({ "name": name, "subdomain": format!("{name}.pages.dev") }))
        }))
        .route("/accounts/{acc}/cfd_tunnel", get(|State(cf): S, Query(q): Query<HashMap<String, String>>| async move {
            let name = q.get("name").cloned().unwrap_or_default();
            let found: Vec<Value> = cf.tunnels.lock().unwrap().iter().filter(|t| **t == name).map(|t| json!({ "id": format!("tid-{t}"), "name": t })).collect();
            ok(json!(found))
        }).post(|State(cf): S, Json(body): Json<Value>| async move {
            assert_eq!(body["config_src"], "cloudflare");
            assert!(body["tunnel_secret"].as_str().unwrap().len() >= 40);
            let name = body["name"].as_str().unwrap().to_string();
            cf.tunnels.lock().unwrap().push(name.clone());
            cf.calls.lock().unwrap().push(format!("create-tunnel:{name}"));
            ok(json!({ "id": format!("tid-{name}"), "name": name }))
        }))
        .route("/accounts/{acc}/cfd_tunnel/{id}/configurations", put(|State(cf): S, Json(body): Json<Value>| async move {
            *cf.ingress.lock().unwrap() = Some(body["config"]["ingress"].clone());
            ok(json!({}))
        }))
        .route("/accounts/{acc}/cfd_tunnel/{id}/token", get(|Path((_, id)): Path<(String, String)>| async move { ok(json!(format!("run-token-{id}"))) }))
        .route("/zones", get(|Query(q): Query<HashMap<String, String>>| async move {
            let name = q.get("name").cloned().unwrap_or_default();
            assert_eq!(q.get("account.id").map(String::as_str), Some("acc1"));
            ok(if name == "example.com" { json!([{ "id": "zone1", "name": name }]) } else { json!([]) })
        }))
        .route("/zones/{zone}/dns_records", get(|State(cf): S, Path(zone): Path<String>, Query(q): Query<HashMap<String, String>>| async move {
            let name = q.get("name").cloned().unwrap_or_default();
            let found: Vec<Value> = cf.records.lock().unwrap().iter()
                .filter(|r| r.0 == zone && r.1 == name)
                .map(|r| json!({ "id": r.4, "name": r.1, "type": r.2, "content": r.3 }))
                .collect();
            ok(json!(found))
        }).post(|State(cf): S, Path(zone): Path<String>, Json(body): Json<Value>| async move {
            assert_eq!(body["proxied"], true);
            let mut records = cf.records.lock().unwrap();
            let id = format!("rec-{}", records.len());
            records.push((zone, body["name"].as_str().unwrap().into(), body["type"].as_str().unwrap().into(), body["content"].as_str().unwrap().into(), id.clone()));
            ok(json!({ "id": id }))
        }))
        .with_state(cf.clone());
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });
    (format!("http://{addr}"), cf)
}

#[tokio::test]
async fn provisions_r2_pages_and_tunnel_idempotently() {
    let (base, cf) = spawn().await;
    let api = Api::with_base(&base, "test-token").unwrap();

    // 账户发现；错误令牌给出可操作的提示
    assert_eq!(api.accounts().await.unwrap()[0].id, "acc1");
    let bad = Api::with_base(&base, "wrong").unwrap().accounts().await.unwrap_err().to_string();
    assert!(bad.contains("401") && bad.contains("重新授权"), "{bad}");

    // R2：创建 bucket 并开启 r2.dev；重复调用复用已有 bucket
    let public = cloudflare::ensure_r2_bucket(&api, "acc1", "love-cards").await.unwrap();
    assert_eq!(public, "https://pub-love-cards.r2.dev");
    assert_eq!(cloudflare::ensure_r2_bucket(&api, "acc1", "love-cards").await.unwrap(), public);
    assert_eq!(cf.buckets.lock().unwrap().len(), 1);

    // R2 对象上传（REST API，key 中的中文与空格要编码，目录分隔符保留）
    api.put_object("acc1", "love-cards", "anniversary/ab12/photos/我的 照片.jpg", b"jpg".to_vec(), "image/jpeg").await.unwrap();
    let objects = cf.objects.lock().unwrap();
    let (ct, body) = objects.get("love-cards/anniversary/ab12/photos/我的 照片.jpg").expect("object stored");
    assert_eq!((ct.as_str(), body.as_slice()), ("image/jpeg", b"jpg".as_slice()));
    drop(objects);

    // Pages：不存在时创建，存在时复用
    assert_eq!(cloudflare::ensure_pages_project(&api, "acc1", "love-cards").await.unwrap(), "love-cards.pages.dev");
    cloudflare::ensure_pages_project(&api, "acc1", "love-cards").await.unwrap();
    assert_eq!(cf.calls.lock().unwrap().iter().filter(|c| c.starts_with("create-pages")).count(), 1);

    // Tunnel：创建远程配置隧道、写入路由、建 CNAME、取运行令牌；重复调用复用
    let setup = cloudflare::ensure_tunnel(&api, "acc1", "easytidy-connect-7", "https://Love.Example.com/", "http://127.0.0.1:53410").await.unwrap();
    assert_eq!(setup.hostname, "love.example.com");
    assert_eq!(setup.token, "run-token-tid-easytidy-connect-7");
    let ingress = cf.ingress.lock().unwrap().clone().unwrap();
    assert_eq!(ingress[0], json!({ "hostname": "love.example.com", "service": "http://127.0.0.1:53410" }));
    assert_eq!(ingress[1]["service"], "http_status:404");
    cloudflare::ensure_tunnel(&api, "acc1", "easytidy-connect-7", "love.example.com", "http://127.0.0.1:53410").await.unwrap();
    assert_eq!(cf.tunnels.lock().unwrap().len(), 1);
    let cnames: Vec<_> = cf.records.lock().unwrap().iter().filter(|r| r.1 == "love.example.com").cloned().collect();
    assert_eq!(cnames.len(), 1);
    assert_eq!((cnames[0].2.as_str(), cnames[0].3.as_str()), ("CNAME", "tid-easytidy-connect-7.cfargotunnel.com"));

    // 不覆盖用户已有的非隧道记录
    let err = cloudflare::ensure_tunnel(&api, "acc1", "t2", "www.example.com", "http://127.0.0.1:53410").await.unwrap_err().to_string();
    assert!(err.contains("已有 DNS 记录"), "{err}");
    // 域名不在账户下
    let err = cloudflare::ensure_tunnel(&api, "acc1", "t3", "love.other.org", "http://127.0.0.1:53410").await.unwrap_err().to_string();
    assert!(err.contains("不在这个 Cloudflare 账户"), "{err}");
}

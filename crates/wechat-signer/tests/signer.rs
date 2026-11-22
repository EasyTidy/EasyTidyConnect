//! 用本地模拟的微信接口验证：stable_token / 旧 token 接口、ticket 缓存、签名、CORS 与错误提示。

use std::collections::HashMap;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;

use axum::extract::Query;
use axum::routing::{get, post};
use axum::{Json, Router};
use serde_json::{json, Value};
use wechat_signer::{router, sign, SignerConfig, TicketCache, TokenApi};

const TICKET: &str = "mock-jsapi-ticket";

async fn spawn(app: Router) -> String {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });
    format!("http://{addr}")
}

#[derive(Default)]
struct Calls {
    stable: AtomicUsize,
    classic: AtomicUsize,
}

fn token_reply(appid: &str, secret: &str) -> Value {
    if secret == "bad-ip" {
        return json!({"errcode": 40164, "errmsg": "invalid ip 203.0.113.9 ipv6 ::ffff:203.0.113.9, not in whitelist"});
    }
    assert_eq!(appid, "wx123");
    json!({"access_token": "AT", "expires_in": 7200})
}

/// 模拟 api.weixin.qq.com
async fn mock_wechat(calls: Arc<Calls>) -> String {
    let stable_calls = calls.clone();
    let app = Router::new()
        .route(
            "/cgi-bin/stable_token",
            post(move |Json(body): Json<Value>| {
                let calls = stable_calls.clone();
                async move {
                    calls.stable.fetch_add(1, Ordering::SeqCst);
                    assert_eq!(body["grant_type"], "client_credential");
                    assert_eq!(body["force_refresh"], false);
                    Json(token_reply(body["appid"].as_str().unwrap(), body["secret"].as_str().unwrap()))
                }
            }),
        )
        .route(
            "/cgi-bin/token",
            get(move |Query(q): Query<HashMap<String, String>>| {
                let calls = calls.clone();
                async move {
                    calls.classic.fetch_add(1, Ordering::SeqCst);
                    Json(token_reply(&q["appid"], &q["secret"]))
                }
            }),
        )
        .route(
            "/cgi-bin/ticket/getticket",
            get(|Query(q): Query<HashMap<String, String>>| async move {
                assert_eq!(q["access_token"], "AT");
                assert_eq!(q["type"], "jsapi");
                Json(json!({"errcode": 0, "errmsg": "ok", "ticket": TICKET, "expires_in": 7200}))
            }),
        );
    spawn(app).await
}

fn config(secret: &str, token_api: TokenApi) -> SignerConfig {
    SignerConfig {
        app_id: "wx123".to_string(),
        app_secret: secret.to_string(),
        allowed_domains: "love.example.com".to_string(),
        token_api,
    }
}

#[tokio::test]
async fn signs_caches_and_restricts() {
    let calls = Arc::new(Calls::default());
    let wx = mock_wechat(calls.clone()).await;
    let signer = spawn(router(Arc::new(config("good", TokenApi::Stable)), Arc::new(TicketCache::new(wx)))).await;
    let http = reqwest::Client::new();
    let page = "https://love.example.com/anniversary/ab12/index.html";

    for _ in 0..3 {
        let response = http
            .get(format!("{signer}/wx/jssdk"))
            .query(&[("url", format!("{page}#section"))])
            .header("Origin", "https://love.example.com")
            .send()
            .await
            .unwrap();
        assert_eq!(response.status(), 200);
        assert_eq!(response.headers()["access-control-allow-origin"], "https://love.example.com");
        assert_eq!(response.headers()["cache-control"], "no-store");
        let body: Value = response.json().await.unwrap();
        assert_eq!(body["appId"], "wx123");
        assert_eq!(body["nonceStr"].as_str().unwrap().len(), 16);
        let expected = sign(TICKET, body["nonceStr"].as_str().unwrap(), body["timestamp"].as_i64().unwrap(), page);
        assert_eq!(body["signature"], expected);
    }
    // 默认用 stable_token，且 ticket 被缓存：三次签名只换取一次 token
    assert_eq!(calls.stable.load(Ordering::SeqCst), 1);
    assert_eq!(calls.classic.load(Ordering::SeqCst), 0);

    let denied = http
        .get(format!("{signer}/wx/jssdk"))
        .query(&[("url", "https://evil.test/x")])
        .header("Origin", "https://evil.test")
        .send()
        .await
        .unwrap();
    assert_eq!(denied.status(), 400);
    assert!(denied.headers().get("access-control-allow-origin").is_none());

    let health: Value = http.get(format!("{signer}/wx/health")).send().await.unwrap().json().await.unwrap();
    assert_eq!(health["configured"], true);
    assert_eq!(health["service"], "wechat-signer");
}

#[tokio::test]
async fn classic_token_api_and_whitelist_error() {
    let calls = Arc::new(Calls::default());
    let wx = mock_wechat(calls.clone()).await;
    let cache = TicketCache::new(wx);

    cache.ticket(&config("good", TokenApi::Classic)).await.unwrap();
    assert_eq!(calls.classic.load(Ordering::SeqCst), 1);

    let err = cache.ticket(&config("bad-ip", TokenApi::Stable)).await.unwrap_err().to_string();
    assert!(err.contains("203.0.113.9") && err.contains("IP 白名单"), "{err}");
}

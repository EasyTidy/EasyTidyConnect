//! 微信 JS-SDK：签名逻辑在独立的 `wechat-signer` crate 里（也可单独部署到服务器）。
//! 这里只负责桌面端的配置读取与「本机签名」接口。

use std::sync::{Arc, OnceLock};

use axum::Router;
pub use wechat_signer::{
    domain_allowed, page_script, sign, ConfigProvider, JsSdkSignature, SignerConfig, TicketCache, TokenApi,
    DEFAULT_API_BASE,
};

use crate::db::Database;
use crate::models::WechatConfig;

impl WechatConfig {
    /// 是否使用独立的签名服务器（密钥只在服务器上，本机不签名）。
    pub fn uses_signing_server(&self) -> bool {
        self.mode.trim() != "local"
    }

    pub fn to_signer(&self) -> SignerConfig {
        SignerConfig {
            app_id: self.app_id.trim().to_string(),
            app_secret: self.app_secret.trim().to_string(),
            allowed_domains: self.allowed_domains.clone(),
            token_api: TokenApi::Stable,
        }
    }
}

/// 进程内共享的 ticket 缓存（局域网服务和公开预览服务共用，避免重复换取 access_token）。
pub fn shared_cache() -> Arc<TicketCache> {
    static CACHE: OnceLock<Arc<TicketCache>> = OnceLock::new();
    CACHE.get_or_init(|| Arc::new(TicketCache::new(DEFAULT_API_BASE))).clone()
}

/// 本机签名：每次请求从数据库读取配置，设置界面修改后立即生效。
struct DatabaseConfig(Arc<Database>);

impl ConfigProvider for DatabaseConfig {
    fn current(&self) -> Result<SignerConfig, String> {
        let config = self.0.get_wechat_config().map_err(|err| err.to_string())?;
        if !config.enabled {
            return Err("微信分享未启用".to_string());
        }
        if config.uses_signing_server() {
            return Err("本机签名未启用（当前使用签名服务器）".to_string());
        }
        Ok(config.to_signer())
    }
}

/// 桌面端的 `/wx/jssdk` 与 `/wx/health`（配合内网穿透做本机签名）。
pub fn local_router(db: Arc<Database>) -> Router {
    wechat_signer::router(Arc::new(DatabaseConfig(db)), shared_cache())
}

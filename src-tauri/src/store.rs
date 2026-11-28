use crate::database::Database;
use crate::proxy::providers::codex_oauth_auth::CodexOAuthManager;
use crate::proxy::providers::xai_oauth_auth::XaiOAuthManager;
use crate::services::{ProxyService, UsageCache};
use std::sync::{Arc, Weak};

/// `AppState` 的弱引用视图，供后台服务（如故障转移路由回写）在无 `tauri::AppHandle`
/// 时安全还原 `AppState` 而不形成强引用环。
#[derive(Clone)]
pub(crate) struct WeakAppState {
    db: Weak<Database>,
    usage_cache: Weak<UsageCache>,
    codex_oauth_manager: Weak<CodexOAuthManager>,
    xai_oauth_manager: Weak<tokio::sync::RwLock<XaiOAuthManager>>,
}

impl WeakAppState {
    pub(crate) fn from_state(state: &AppState) -> Self {
        Self {
            db: Arc::downgrade(&state.db),
            usage_cache: Arc::downgrade(&state.usage_cache),
            codex_oauth_manager: Arc::downgrade(&state.codex_oauth_manager),
            xai_oauth_manager: Arc::downgrade(&state.xai_oauth_manager),
        }
    }

    pub(crate) fn upgrade(&self, proxy_service: ProxyService) -> Option<AppState> {
        Some(AppState {
            db: self.db.upgrade()?,
            proxy_service,
            usage_cache: self.usage_cache.upgrade()?,
            codex_oauth_manager: self.codex_oauth_manager.upgrade()?,
            xai_oauth_manager: self.xai_oauth_manager.upgrade()?,
        })
    }
}

/// 全局应用状态
#[derive(Clone)]
pub struct AppState {
    pub db: Arc<Database>,
    pub proxy_service: ProxyService,
    pub usage_cache: Arc<UsageCache>,
    // 内部已使用细粒度锁（accounts/access_tokens/refresh_locks），所有方法均为
    // `&self`，无需外层 RwLock；避免持有粗粒度锁跨网络刷新导致的连锁阻塞。
    pub codex_oauth_manager: Arc<CodexOAuthManager>,
    /// 授权中心的 xAI 账号；切 Grok 官方卡时换 Grok CLI 登录也用这一个实例
    pub xai_oauth_manager: Arc<tokio::sync::RwLock<XaiOAuthManager>>,
}

impl AppState {
    /// 创建新的应用状态
    pub fn new(db: Arc<Database>) -> Self {
        let codex_oauth_manager =
            Arc::new(CodexOAuthManager::new(crate::config::get_app_config_dir()));
        let xai_oauth_manager = Arc::new(tokio::sync::RwLock::new(XaiOAuthManager::new(
            crate::config::get_app_config_dir(),
        )));
        let proxy_service = ProxyService::new(db.clone());

        let state = Self {
            db,
            proxy_service,
            usage_cache: Arc::new(UsageCache::new()),
            codex_oauth_manager,
            xai_oauth_manager,
        };
        state.proxy_service.bind_app_state(&state);
        state
    }
}

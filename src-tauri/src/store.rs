use crate::database::Database;
use crate::proxy::providers::codex_oauth_auth::{ChatgptProxyConfig, CodexOAuthManager};
use crate::services::{ProxyService, UsageCache};
use std::sync::{Arc, Mutex};

const CHATGPT_PROXY_CONFIG_KEY: &str = "chatgpt_account_proxy_config";

/// 全局应用状态
#[derive(Clone)]
pub struct AppState {
    pub db: Arc<Database>,
    pub proxy_service: ProxyService,
    pub usage_cache: Arc<UsageCache>,
    // 内部已使用细粒度锁（accounts/access_tokens/refresh_locks），所有方法均为
    // `&self`，无需外层 RwLock；避免持有粗粒度锁跨网络刷新导致的连锁阻塞。
    pub codex_oauth_manager: Arc<CodexOAuthManager>,
    // Serialize persistence and runtime updates across saves and post-import reloads.
    chatgpt_proxy_update_lock: Arc<Mutex<()>>,
}

impl AppState {
    /// 创建新的应用状态
    pub fn new(db: Arc<Database>) -> Self {
        let codex_oauth_manager =
            Arc::new(CodexOAuthManager::new(crate::config::get_app_config_dir()));
        let proxy_service = ProxyService::new(db.clone());

        let state = Self {
            db,
            proxy_service,
            usage_cache: Arc::new(UsageCache::new()),
            codex_oauth_manager,
            chatgpt_proxy_update_lock: Arc::new(Mutex::new(())),
        };
        if let Err(error) = state.reload_chatgpt_proxy_config() {
            log::warn!("[CodexOAuth] Failed to load account proxy: {error}");
        }
        state
    }

    pub(crate) fn chatgpt_proxy_config(&self) -> Result<ChatgptProxyConfig, String> {
        self.db
            .get_setting(CHATGPT_PROXY_CONFIG_KEY)
            .map_err(|error| error.to_string())?
            .map(|value| serde_json::from_str(&value).map_err(|error| error.to_string()))
            .transpose()
            .map(Option::unwrap_or_default)
    }

    pub(crate) fn save_chatgpt_proxy_config(
        &self,
        mut config: ChatgptProxyConfig,
    ) -> Result<(), String> {
        config.url = config.url.trim().to_owned();
        let client = config.build_client()?;
        let value = serde_json::to_string(&config).map_err(|error| error.to_string())?;
        let _update = self
            .chatgpt_proxy_update_lock
            .lock()
            .map_err(|error| error.to_string())?;
        self.db
            .set_setting(CHATGPT_PROXY_CONFIG_KEY, &value)
            .map_err(|error| error.to_string())?;
        self.codex_oauth_manager.replace_account_proxy(client);
        Ok(())
    }

    pub(crate) fn reload_chatgpt_proxy_config(&self) -> Result<(), String> {
        let _update = self
            .chatgpt_proxy_update_lock
            .lock()
            .map_err(|error| error.to_string())?;
        match self
            .chatgpt_proxy_config()
            .and_then(|config| config.build_client())
        {
            Ok(client) => {
                self.codex_oauth_manager.replace_account_proxy(client);
                Ok(())
            }
            Err(error) => {
                self.codex_oauth_manager.replace_account_proxy(None);
                Err(error)
            }
        }
    }
}

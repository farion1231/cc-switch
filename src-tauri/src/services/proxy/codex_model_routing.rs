use super::ProxyService;
use crate::{
    app_config::AppType,
    error::AppError,
    provider::Provider,
    proxy::codex_model_routing::{self as routing, CodexModelRoutingConfig},
};
use indexmap::IndexMap;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::path::PathBuf;

/// Only the two files owned by this feature. Never snapshot/restore auth.json.
struct RouterFiles(Vec<(PathBuf, Option<String>)>);

impl RouterFiles {
    fn capture() -> Result<Self, String> {
        let paths = [
            crate::codex_config::get_codex_config_path(),
            crate::codex_config::get_codex_config_dir().join(routing::CATALOG_FILENAME),
        ];
        let mut files = Vec::new();
        for path in paths {
            if std::fs::symlink_metadata(&path).is_ok_and(|meta| meta.file_type().is_symlink()) {
                return Err(format!(
                    "为避免覆盖其它文件，模型路由不写入符号链接：{}",
                    path.display()
                ));
            }
            let contents = match std::fs::read_to_string(&path) {
                Ok(text) => Some(text),
                Err(e) if e.kind() == std::io::ErrorKind::NotFound => None,
                Err(e) => return Err(format!("读取 {} 失败: {e}", path.display())),
            };
            files.push((path, contents));
        }
        Ok(Self(files))
    }

    fn restore(&self) -> Result<(), String> {
        for (path, text) in &self.0 {
            if let Some(text) = text {
                crate::config::write_text_file(path, text).map_err(|e| e.to_string())?;
            } else if path.exists() {
                std::fs::remove_file(path).map_err(|e| e.to_string())?;
            }
        }
        Ok(())
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ModelRoutingSaveResult {
    pub config: CodexModelRoutingConfig,
    /// Menu/capability changes may require Codex to reload its model catalog.
    pub catalog_changed: bool,
}

impl ProxyService {
    pub(crate) fn codex_model_routing_config(&self) -> Result<CodexModelRoutingConfig, String> {
        self.db.get_codex_model_routing().map_err(|e| e.to_string())
    }

    pub(crate) fn codex_model_routing_enabled(&self) -> Result<bool, String> {
        Ok(self.codex_model_routing_config()?.enabled)
    }

    fn validate_model_routing_targets(
        &self,
        config: &CodexModelRoutingConfig,
        providers: &IndexMap<String, Provider>,
        proxy_url: &str,
    ) -> Result<(), String> {
        config
            .validate(providers, true)
            .map_err(|e| e.to_string())?;
        let adapter = crate::proxy::providers::get_adapter(&AppType::Codex)
            .ok_or("Codex adapter unavailable")?;
        let local_url = url::Url::parse(proxy_url).map_err(|e| e.to_string())?;
        for selection in &config.models {
            let provider = &providers[&selection.provider_id];
            let base = adapter
                .extract_base_url(provider)
                .map_err(|e| e.to_string())?;
            let upstream =
                url::Url::parse(&base).map_err(|e| format!("{} 地址无效: {e}", provider.name))?;
            if !matches!(upstream.scheme(), "http" | "https") {
                return Err(format!("{} 必须使用 HTTP/HTTPS 地址", provider.name));
            }
            let local_host = matches!(
                upstream.host_str(),
                Some("127.0.0.1" | "localhost" | "0.0.0.0" | "[::1]" | "[::]")
            );
            if upstream.port_or_known_default() == local_url.port_or_known_default()
                && (local_host || upstream.host_str() == local_url.host_str())
            {
                return Err(format!(
                    "{} 指向本地路由自身，请修正供应商地址",
                    provider.name
                ));
            }
            if adapter.extract_auth(provider).is_none() {
                return Err(format!("{} 缺少 API Key，请先编辑供应商", provider.name));
            }
        }
        Ok(())
    }

    /// Caller owns the Codex switch lock. DB is not changed here.
    async fn project_codex_model_routing(
        &self,
        config: &CodexModelRoutingConfig,
        providers: &IndexMap<String, Provider>,
    ) -> Result<bool, String> {
        let (_, base_url) = self.build_proxy_urls().await?;
        self.validate_model_routing_targets(config, providers, &base_url)?;
        let catalog = config.catalog(providers).map_err(|e| e.to_string())?;
        let existing = crate::codex_config::read_codex_config_text().map_err(|e| e.to_string())?;
        let mut projected =
            routing::project_config(&existing, config, &base_url).map_err(|e| e.to_string())?;
        // Preserve the login-aware desktop compatibility behavior of ordinary
        // takeover without reading, rewriting or exposing the login itself.
        if let Some(has_login) = Self::codex_live_login_state(&projected) {
            projected =
                crate::codex_config::align_codex_requires_openai_auth_with_login_preservation(
                    &projected, has_login,
                )
                .map_err(|e| e.to_string())?;
        }
        let path = crate::codex_config::get_codex_config_dir().join(routing::CATALOG_FILENAME);
        let old_catalog = std::fs::read_to_string(&path)
            .ok()
            .and_then(|text| serde_json::from_str::<Value>(&text).ok());
        let catalog_changed = old_catalog.as_ref() != Some(&catalog);
        let snapshot = RouterFiles::capture()?;
        let result = (|| {
            if catalog_changed {
                crate::config::write_json_file(&path, &catalog).map_err(|e| e.to_string())?;
            }
            if existing != projected {
                crate::codex_config::write_codex_live_config_atomic(Some(&projected))
                    .map_err(|e| e.to_string())?;
            }
            Ok::<_, String>(())
        })();
        if let Err(error) = result {
            snapshot
                .restore()
                .map_err(|rollback| format!("{error}; 回滚失败: {rollback}"))?;
            return Err(error);
        }
        Ok(catalog_changed)
    }

    pub(crate) async fn refresh_codex_model_routing_live(&self) -> Result<(), String> {
        let config = self.codex_model_routing_config()?;
        let providers = self
            .db
            .get_all_providers("codex")
            .map_err(|e| e.to_string())?;
        self.project_codex_model_routing(&config, &providers)
            .await?;
        Ok(())
    }

    /// Saving a draft never activates the service or changes the mode flag.
    pub async fn save_codex_model_routing_config(
        &self,
        mut config: CodexModelRoutingConfig,
    ) -> Result<ModelRoutingSaveResult, String> {
        let _guard = self.switch_locks.lock_for_app("codex").await;
        let old = self.codex_model_routing_config()?;
        config.enabled = old.enabled;
        config.provider_name = config.provider_name.trim().to_string();
        let providers = self
            .db
            .get_all_providers("codex")
            .map_err(|e| e.to_string())?;
        let taken_over = self
            .db
            .get_proxy_config_for_app("codex")
            .await
            .map_err(|e| e.to_string())?
            .enabled;
        let active = config.enabled && taken_over;
        config
            .validate(&providers, active)
            .map_err(|e| e.to_string())?;
        let files = if active {
            Some(RouterFiles::capture()?)
        } else {
            None
        };
        let catalog_changed = if active {
            self.project_codex_model_routing(&config, &providers)
                .await?
        } else {
            false
        };
        if let Err(error) = self.db.save_codex_model_routing(&config) {
            if let Some(files) = files {
                files
                    .restore()
                    .map_err(|rollback| format!("{error}; 回滚失败: {rollback}"))?;
            }
            return Err(error.to_string());
        }
        if active {
            self.refresh_active_target_from_current_provider(&AppType::Codex)
                .await;
        }
        Ok(ModelRoutingSaveResult {
            config,
            catalog_changed,
        })
    }

    /// Shares the normal takeover transaction, backup and per-app lock.
    /// Enabling from ordinary takeover preserves its original restore backup.
    pub async fn set_codex_model_routing_enabled(&self, enabled: bool) -> Result<(), String> {
        let _guard = self.switch_locks.lock_for_app("codex").await;
        let old = self.codex_model_routing_config()?;
        if !enabled && !old.enabled {
            // Routing is already off. In particular, do not disable an
            // otherwise ordinary Codex takeover that happens to be active.
            return Ok(());
        }
        let mut config = old.clone();
        config.enabled = enabled;
        if enabled {
            let providers = self
                .db
                .get_all_providers("codex")
                .map_err(|e| e.to_string())?;
            let (_, url) = self.build_proxy_urls().await?;
            self.validate_model_routing_targets(&config, &providers, &url)?;
            config.catalog(&providers).map_err(|e| e.to_string())?;
            let snapshot = RouterFiles::capture()?;
            self.db
                .save_codex_model_routing(&config)
                .map_err(|e| e.to_string())?;
            if let Err(error) = self.set_takeover_for_app_inner("codex", true).await {
                let db_rollback = self.db.save_codex_model_routing(&old);
                let file_rollback = snapshot.restore();
                return Err(format!(
                    "{error}; 配置回滚: {db_rollback:?}; 文件回滚: {file_rollback:?}"
                ));
            }
        } else {
            // Restore while the mode flag is still true, so router auth is
            // never mistaken for a station's native-login credentials.
            if old.enabled {
                self.set_takeover_for_app_inner("codex", false).await?;
            }
            self.db
                .save_codex_model_routing(&config)
                .map_err(|e| e.to_string())?;
        }
        Ok(())
    }

    /// Provider edit fast path while routing owns Live. Do not replace the
    /// original takeover backup, switch accounts, or backfill a merged catalog.
    /// The caller already holds the Codex switch lock.
    pub(crate) async fn update_codex_provider_in_model_routing(
        &self,
        provider: &Provider,
    ) -> Result<(), AppError> {
        let config = self.db.get_codex_model_routing()?;
        let mut providers = self.db.get_all_providers("codex")?;
        let old = providers.insert(provider.id.clone(), provider.clone());
        config.validate(&providers, true)?;
        let files = RouterFiles::capture().map_err(AppError::Message)?;
        self.project_codex_model_routing(&config, &providers)
            .await
            .map_err(AppError::Message)?;
        if let Err(error) = self.db.save_provider("codex", provider) {
            files.restore().map_err(AppError::Message)?;
            return Err(error);
        }
        // `old` documents that this operation only replaces an existing row;
        // callers perform their standard provider existence validation.
        debug_assert!(old.is_some());
        Ok(())
    }
}

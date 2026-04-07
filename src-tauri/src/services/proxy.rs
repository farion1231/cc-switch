//! 代理服务业务逻辑层
//!
//! 提供代理服务器的启动、停止和配置管理

use crate::app_config::AppType;
use crate::config::{get_claude_settings_path, read_json_file, write_json_file};
use crate::database::Database;
use crate::provider::Provider;
use crate::proxy::server::ProxyServer;
use crate::proxy::switch_lock::SwitchLockManager;
use crate::proxy::types::*;
use crate::services::provider::{
    build_effective_settings_with_common_config, provider_uses_common_config,
    remove_common_config_from_settings, write_live_with_common_config,
};
use serde_json::{json, Value};
use std::str::FromStr;
use std::sync::Arc;
use tokio::sync::RwLock;

/// 用于接管 Live 配置时的占位符（避免客户端提示缺少 key，同时不泄露真实 Token）
const PROXY_TOKEN_PLACEHOLDER: &str = "PROXY_MANAGED";
const CLAUDE_PROVIDER_ENV_KEYS: &[&str] = &[
    "ANTHROPIC_AUTH_TOKEN",
    "ANTHROPIC_API_KEY",
    "OPENROUTER_API_KEY",
    "OPENAI_API_KEY",
    "ANTHROPIC_BASE_URL",
    "ANTHROPIC_MODEL",
    "ANTHROPIC_REASONING_MODEL",
    "ANTHROPIC_SMALL_FAST_MODEL",
    "ANTHROPIC_DEFAULT_HAIKU_MODEL",
    "ANTHROPIC_DEFAULT_SONNET_MODEL",
    "ANTHROPIC_DEFAULT_OPUS_MODEL",
];
const CLAUDE_PROVIDER_TOP_LEVEL_KEYS: &[&str] = &["apiBaseUrl", "primaryModel", "smallFastModel"];
const CODEX_PROVIDER_TOP_LEVEL_KEYS: &[&str] = &[
    "model_provider",
    "model",
    "model_reasoning_effort",
    "base_url",
];

#[derive(Clone)]
pub struct ProxyService {
    db: Arc<Database>,
    server: Arc<RwLock<Option<ProxyServer>>>,
    /// AppHandle，用于传递给 ProxyServer 以支持故障转移时的 UI 更新
    app_handle: Arc<RwLock<Option<tauri::AppHandle>>>,
    switch_locks: SwitchLockManager,
}

#[derive(Debug, Clone, Copy, Default)]
pub struct HotSwitchOutcome {
    pub logical_target_changed: bool,
}

impl ProxyService {
    pub fn new(db: Arc<Database>) -> Self {
        Self {
            db,
            server: Arc::new(RwLock::new(None)),
            app_handle: Arc::new(RwLock::new(None)),
            switch_locks: SwitchLockManager::new(),
        }
    }

    fn apply_claude_takeover_fields(config: &mut Value, proxy_url: &str) {
        if !config.is_object() {
            *config = json!({});
        }

        let root = config
            .as_object_mut()
            .expect("Claude config should be normalized to an object");
        let env = root.entry("env".to_string()).or_insert_with(|| json!({}));
        if !env.is_object() {
            *env = json!({});
        }

        let env = env
            .as_object_mut()
            .expect("Claude env should be normalized to an object");
        env.insert("ANTHROPIC_BASE_URL".to_string(), json!(proxy_url));

        let token_keys = [
            "ANTHROPIC_AUTH_TOKEN",
            "ANTHROPIC_API_KEY",
            "OPENROUTER_API_KEY",
            "OPENAI_API_KEY",
        ];

        let mut replaced_any = false;
        for key in token_keys {
            if env.contains_key(key) {
                env.insert(key.to_string(), json!(PROXY_TOKEN_PLACEHOLDER));
                replaced_any = true;
            }
        }

        if !replaced_any {
            env.insert(
                "ANTHROPIC_AUTH_TOKEN".to_string(),
                json!(PROXY_TOKEN_PLACEHOLDER),
            );
        }
    }

    fn apply_codex_takeover_fields(config: &mut Value, proxy_codex_base_url: &str) {
        if !config.is_object() {
            *config = json!({});
        }

        let root = config
            .as_object_mut()
            .expect("Codex config should be normalized to an object");
        let auth = root.entry("auth".to_string()).or_insert_with(|| json!({}));
        if !auth.is_object() {
            *auth = json!({});
        }
        auth.as_object_mut()
            .expect("Codex auth should be normalized to an object")
            .insert("OPENAI_API_KEY".to_string(), json!(PROXY_TOKEN_PLACEHOLDER));

        let config_str = root.get("config").and_then(|v| v.as_str()).unwrap_or("");
        let updated_config = Self::update_toml_base_url(config_str, proxy_codex_base_url);
        root.insert("config".to_string(), json!(updated_config));
    }

    fn apply_gemini_takeover_fields(config: &mut Value, proxy_url: &str) {
        if !config.is_object() {
            *config = json!({});
        }

        let root = config
            .as_object_mut()
            .expect("Gemini config should be normalized to an object");
        let env = root.entry("env".to_string()).or_insert_with(|| json!({}));
        if !env.is_object() {
            *env = json!({});
        }

        let env = env
            .as_object_mut()
            .expect("Gemini env should be normalized to an object");
        env.insert("GOOGLE_GEMINI_BASE_URL".to_string(), json!(proxy_url));
        env.insert("GEMINI_API_KEY".to_string(), json!(PROXY_TOKEN_PLACEHOLDER));
    }

    fn apply_takeover_overlay(
        app_type: &AppType,
        config: &mut Value,
        proxy_url: &str,
        proxy_codex_base_url: &str,
    ) -> Result<(), String> {
        match app_type {
            AppType::Claude => {
                Self::apply_claude_takeover_fields(config, proxy_url);
                Ok(())
            }
            AppType::Codex => {
                Self::apply_codex_takeover_fields(config, proxy_codex_base_url);
                Ok(())
            }
            AppType::Gemini => {
                Self::apply_gemini_takeover_fields(config, proxy_url);
                Ok(())
            }
            AppType::OpenCode => Err("OpenCode 不支持代理功能".to_string()),
            AppType::OpenClaw => Err("OpenClaw 不支持代理功能".to_string()),
        }
    }

    pub async fn sync_live_from_provider_while_proxy_active(
        &self,
        app_type: &AppType,
        provider: &Provider,
    ) -> Result<(), String> {
        let _guard = self.switch_locks.lock_for_app(app_type.as_str()).await;
        self.sync_live_from_provider_while_proxy_active_inner(app_type, provider)
            .await
    }

    async fn sync_live_from_provider_while_proxy_active_inner(
        &self,
        app_type: &AppType,
        provider: &Provider,
    ) -> Result<(), String> {
        let effective_settings = self
            .build_takeover_live_config_from_provider(app_type, provider)
            .await?;
        self.write_live_config_for_app(app_type, &effective_settings)?;
        Ok(())
    }

    async fn sync_live_from_current_provider_while_proxy_active_inner(
        &self,
        app_type: &AppType,
    ) -> Result<bool, String> {
        let current_id = crate::settings::get_effective_current_provider(&self.db, app_type)
            .map_err(|e| format!("读取当前 {} 供应商失败: {e}", app_type.as_str()))?;
        let Some(current_id) = current_id else {
            return Ok(false);
        };

        let provider = self
            .db
            .get_provider_by_id(&current_id, app_type.as_str())
            .map_err(|e| format!("读取当前 {} 供应商配置失败: {e}", app_type.as_str()))?;
        let Some(provider) = provider else {
            return Ok(false);
        };

        self.sync_live_from_provider_while_proxy_active_inner(app_type, &provider)
            .await?;
        Ok(true)
    }

    pub async fn refresh_takeover_state_from_provider(
        &self,
        app_type: &AppType,
        provider: &Provider,
    ) -> Result<(), String> {
        let _guard = self.switch_locks.lock_for_app(app_type.as_str()).await;
        self.refresh_takeover_state_from_provider_inner(app_type, provider)
            .await
    }

    pub async fn refresh_restore_state_from_provider(
        &self,
        app_type: &AppType,
        provider: &Provider,
    ) -> Result<(), String> {
        let _guard = self.switch_locks.lock_for_app(app_type.as_str()).await;
        self.refresh_restore_state_from_provider_inner(app_type, provider)
            .await
    }

    pub async fn sync_takeover_common_config_transition(
        &self,
        app_type: &AppType,
        provider: &Provider,
        previous_common_snippet: Option<&str>,
    ) -> Result<(), String> {
        self.sync_common_config_transition(app_type, provider, previous_common_snippet, true)
            .await
    }

    pub async fn sync_restore_common_config_transition(
        &self,
        app_type: &AppType,
        provider: &Provider,
        previous_common_snippet: Option<&str>,
    ) -> Result<(), String> {
        self.sync_common_config_transition(app_type, provider, previous_common_snippet, false)
            .await
    }

    async fn sync_common_config_transition(
        &self,
        app_type: &AppType,
        provider: &Provider,
        previous_common_snippet: Option<&str>,
        apply_takeover_overlay_in_live: bool,
    ) -> Result<(), String> {
        let current_snippet = self
            .db
            .get_config_snippet(app_type.as_str())
            .map_err(|e| format!("读取 {} 通用配置失败: {e}", app_type.as_str()))?;
        let previous_common_snippet = previous_common_snippet
            .map(str::trim)
            .filter(|snippet| !snippet.is_empty());

        let had_previous_common_config = previous_common_snippet
            .is_some_and(|snippet| provider_uses_common_config(app_type, provider, Some(snippet)));
        let has_current_common_config =
            provider_uses_common_config(app_type, provider, current_snippet.as_deref());

        if !had_previous_common_config && !has_current_common_config {
            return Ok(());
        }

        let _guard = self.switch_locks.lock_for_app(app_type.as_str()).await;
        self.sync_takeover_common_config_transition_inner(
            app_type,
            provider,
            previous_common_snippet,
            apply_takeover_overlay_in_live,
        )
        .await
    }

    async fn refresh_takeover_state_from_provider_inner(
        &self,
        app_type: &AppType,
        provider: &Provider,
    ) -> Result<(), String> {
        let provider_settings = self.build_effective_provider_settings(app_type, provider)?;
        self.refresh_proxy_active_backup_from_provider_settings_inner(
            app_type,
            provider,
            &provider_settings,
        )
        .await?;
        self.refresh_proxy_active_live_from_provider_settings_inner(
            app_type,
            provider,
            &provider_settings,
        )
        .await?;
        Ok(())
    }

    async fn refresh_restore_state_from_provider_inner(
        &self,
        app_type: &AppType,
        provider: &Provider,
    ) -> Result<(), String> {
        let provider_settings = self.build_effective_provider_settings(app_type, provider)?;
        let live_snapshot = self
            .build_proxy_refresh_snapshot_from_candidates(
                app_type,
                &provider_settings,
                self.collect_live_refresh_candidates(app_type).await,
                false,
            )
            .await?;
        let mut restore_snapshot = self
            .apply_common_config_transition_to_snapshot(
                app_type,
                provider,
                live_snapshot,
                None,
                false,
            )
            .await?;
        self.write_live_config_for_app(app_type, &restore_snapshot)?;

        if matches!(app_type, AppType::Gemini) {
            restore_snapshot = json!({
                "env": restore_snapshot.get("env").cloned().unwrap_or_else(|| json!({}))
            });
        }

        let backup_json = serde_json::to_string(&restore_snapshot)
            .map_err(|e| format!("序列化 {} 恢复快照失败: {e}", app_type.as_str()))?;
        self.db
            .save_live_backup(app_type.as_str(), &backup_json)
            .await
            .map_err(|e| format!("更新 {} 恢复备份失败: {e}", app_type.as_str()))?;

        Ok(())
    }

    async fn sync_takeover_common_config_transition_inner(
        &self,
        app_type: &AppType,
        provider: &Provider,
        previous_common_snippet: Option<&str>,
        apply_takeover_overlay_in_live: bool,
    ) -> Result<(), String> {
        if let Ok(existing_live) = self.read_live_snapshot_for_app(app_type) {
            let live_base = if self.detect_takeover_in_live_config_for_app(app_type) {
                Self::strip_takeover_fields_from_taken_over_live(app_type, &existing_live)
            } else {
                existing_live
            };
            let updated_live = self
                .apply_common_config_transition_to_snapshot(
                    app_type,
                    provider,
                    live_base,
                    previous_common_snippet,
                    apply_takeover_overlay_in_live,
                )
                .await?;
            self.write_live_config_for_app(app_type, &updated_live)?;
        }

        if let Some(existing_backup) = self
            .db
            .get_live_backup(app_type.as_str())
            .await
            .map_err(|e| format!("读取 {} 备份失败: {e}", app_type.as_str()))?
        {
            let backup_value: Value = match serde_json::from_str(&existing_backup.original_config) {
                Ok(value) => value,
                Err(err) => {
                    log::warn!(
                        "解析 {} 备份失败，将跳过通用配置备份刷新并继续更新 Live: {err}",
                        app_type.as_str()
                    );
                    return Ok(());
                }
            };
            let mut updated_backup = self
                .apply_common_config_transition_to_snapshot(
                    app_type,
                    provider,
                    backup_value,
                    previous_common_snippet,
                    false,
                )
                .await?;

            if matches!(app_type, AppType::Gemini) {
                updated_backup = json!({
                    "env": updated_backup.get("env").cloned().unwrap_or_else(|| json!({}))
                });
            }

            let backup_json = serde_json::to_string(&updated_backup)
                .map_err(|e| format!("序列化 {} 备份失败: {e}", app_type.as_str()))?;
            self.db
                .save_live_backup(app_type.as_str(), &backup_json)
                .await
                .map_err(|e| format!("更新 {} 备份失败: {e}", app_type.as_str()))?;
        }

        Ok(())
    }

    async fn apply_common_config_transition_to_snapshot(
        &self,
        app_type: &AppType,
        provider: &Provider,
        mut snapshot: Value,
        previous_common_snippet: Option<&str>,
        apply_takeover_overlay: bool,
    ) -> Result<Value, String> {
        if let Some(previous_common_snippet) = previous_common_snippet
            .filter(|snippet| provider_uses_common_config(app_type, provider, Some(*snippet)))
        {
            snapshot =
                remove_common_config_from_settings(app_type, &snapshot, previous_common_snippet)
                    .map_err(|e| format!("移除 {} 旧通用配置失败: {e}", app_type.as_str()))?;
        }

        let mut snapshot_provider = provider.clone();
        snapshot_provider.settings_config = snapshot;
        let mut updated_snapshot =
            self.build_effective_provider_settings(app_type, &snapshot_provider)?;

        if apply_takeover_overlay {
            let (proxy_url, proxy_codex_base_url) = self.build_proxy_urls().await?;
            Self::apply_takeover_overlay(
                app_type,
                &mut updated_snapshot,
                &proxy_url,
                &proxy_codex_base_url,
            )?;
        }

        Ok(updated_snapshot)
    }

    fn build_effective_provider_settings(
        &self,
        app_type: &AppType,
        provider: &Provider,
    ) -> Result<Value, String> {
        build_effective_settings_with_common_config(self.db.as_ref(), app_type, provider)
            .map_err(|e| format!("构建 {} 有效配置失败: {e}", app_type.as_str()))
    }

    async fn refresh_proxy_active_backup_from_provider_settings_inner(
        &self,
        app_type: &AppType,
        provider: &Provider,
        provider_settings: &Value,
    ) -> Result<(), String> {
        let mut effective_settings = self
            .build_proxy_refresh_snapshot_from_candidates(
                app_type,
                provider_settings,
                self.collect_backup_refresh_candidates(app_type).await,
                false,
            )
            .await?;

        // Backup should preserve live-only sections from the chosen base snapshot while still
        // reflecting common-config changes applied to the current effective provider.
        let mut backup_provider = provider.clone();
        backup_provider.settings_config = effective_settings;
        effective_settings = self.build_effective_provider_settings(app_type, &backup_provider)?;

        if matches!(app_type, AppType::Gemini) {
            effective_settings = json!({
                "env": effective_settings.get("env").cloned().unwrap_or_else(|| json!({}))
            });
        }

        let backup_json = serde_json::to_string(&effective_settings)
            .map_err(|e| format!("序列化 {} 配置失败: {e}", app_type.as_str()))?;
        self.db
            .save_live_backup(app_type.as_str(), &backup_json)
            .await
            .map_err(|e| format!("更新 {} 备份失败: {e}", app_type.as_str()))?;

        Ok(())
    }

    async fn refresh_proxy_active_live_from_provider_settings_inner(
        &self,
        app_type: &AppType,
        provider: &Provider,
        provider_settings: &Value,
    ) -> Result<(), String> {
        let live_snapshot = self
            .build_proxy_refresh_snapshot_from_candidates(
                app_type,
                provider_settings,
                self.collect_live_refresh_candidates(app_type).await,
                false,
            )
            .await?;
        let mut live_provider = provider.clone();
        live_provider.settings_config = live_snapshot;
        let mut effective_settings =
            self.build_effective_provider_settings(app_type, &live_provider)?;
        let (proxy_url, proxy_codex_base_url) = self.build_proxy_urls().await?;
        Self::apply_takeover_overlay(
            app_type,
            &mut effective_settings,
            &proxy_url,
            &proxy_codex_base_url,
        )?;
        self.write_live_config_for_app(app_type, &effective_settings)?;
        Ok(())
    }

    async fn build_proxy_refresh_snapshot_from_candidates(
        &self,
        app_type: &AppType,
        provider_settings: &Value,
        candidates: Vec<(&'static str, Value)>,
        apply_takeover_overlay: bool,
    ) -> Result<Value, String> {
        for (label, mut candidate) in candidates {
            match Self::patch_provider_owned_fields(app_type, &mut candidate, provider_settings) {
                Ok(()) => {
                    if apply_takeover_overlay {
                        let (proxy_url, proxy_codex_base_url) = self.build_proxy_urls().await?;
                        Self::apply_takeover_overlay(
                            app_type,
                            &mut candidate,
                            &proxy_url,
                            &proxy_codex_base_url,
                        )?;
                    }
                    return Ok(candidate);
                }
                Err(err) => {
                    log::warn!(
                        "基于现有 {} {}刷新接管配置失败，将尝试下一个候选基线: {err}",
                        app_type.as_str(),
                        label
                    );
                }
            }
        }

        let mut effective_settings = provider_settings.clone();
        if apply_takeover_overlay {
            let (proxy_url, proxy_codex_base_url) = self.build_proxy_urls().await?;
            Self::apply_takeover_overlay(
                app_type,
                &mut effective_settings,
                &proxy_url,
                &proxy_codex_base_url,
            )?;
        }
        Ok(effective_settings)
    }

    async fn collect_backup_refresh_candidates(
        &self,
        app_type: &AppType,
    ) -> Vec<(&'static str, Value)> {
        let mut candidates = Vec::new();

        if self.detect_takeover_in_live_config_for_app(app_type) {
            match self.read_live_snapshot_for_app(app_type) {
                Ok(existing_live) => candidates.push((
                    "当前 Live 快照",
                    Self::strip_takeover_fields_from_taken_over_live(app_type, &existing_live),
                )),
                Err(err) => log::warn!(
                    "读取当前 {} Live 配置失败，将跳过该基线: {err}",
                    app_type.as_str()
                ),
            }
        }

        match self.db.get_live_backup(app_type.as_str()).await {
            Ok(Some(backup)) => match serde_json::from_str::<Value>(&backup.original_config) {
                Ok(existing_backup) => candidates.push(("备份快照", existing_backup)),
                Err(err) => log::warn!(
                    "解析现有 {} 备份失败，将跳过该基线: {err}",
                    app_type.as_str()
                ),
            },
            Ok(None) => {}
            Err(err) => log::warn!("读取 {} 现有备份失败: {err}", app_type.as_str()),
        }

        candidates
    }

    async fn collect_live_refresh_candidates(
        &self,
        app_type: &AppType,
    ) -> Vec<(&'static str, Value)> {
        let mut candidates = Vec::new();

        match self.read_live_snapshot_for_app(app_type) {
            Ok(existing_live) => {
                let live_base = if self.detect_takeover_in_live_config_for_app(app_type) {
                    Self::strip_takeover_fields_from_taken_over_live(app_type, &existing_live)
                } else {
                    existing_live
                };
                candidates.push(("当前 Live 快照", live_base));
            }
            Err(err) => log::warn!(
                "读取当前 {} Live 配置失败，将尝试其它基线: {err}",
                app_type.as_str()
            ),
        }

        match self.db.get_live_backup(app_type.as_str()).await {
            Ok(Some(backup)) => match serde_json::from_str::<Value>(&backup.original_config) {
                Ok(existing_backup) => candidates.push(("备份快照", existing_backup)),
                Err(err) => log::warn!(
                    "解析现有 {} 备份失败，将跳过该基线: {err}",
                    app_type.as_str()
                ),
            },
            Ok(None) => {}
            Err(err) => log::warn!("读取 {} 现有备份失败: {err}", app_type.as_str()),
        }

        candidates
    }

    fn patch_provider_owned_fields(
        app_type: &AppType,
        base_snapshot: &mut Value,
        provider_settings: &Value,
    ) -> Result<(), String> {
        match app_type {
            AppType::Claude => {
                Self::patch_claude_provider_owned_fields(base_snapshot, provider_settings)
            }
            AppType::Codex => {
                Self::patch_codex_provider_owned_fields(base_snapshot, provider_settings)
            }
            AppType::Gemini => {
                Self::patch_gemini_provider_owned_fields(base_snapshot, provider_settings)
            }
            AppType::OpenCode => Err("OpenCode 不支持代理功能".to_string()),
            AppType::OpenClaw => Err("OpenClaw 不支持代理功能".to_string()),
        }
    }

    fn patch_claude_provider_owned_fields(
        base_snapshot: &mut Value,
        provider_settings: &Value,
    ) -> Result<(), String> {
        if !base_snapshot.is_object() {
            *base_snapshot = json!({});
        }

        Self::patch_json_provider_env_fields(
            base_snapshot,
            provider_settings,
            CLAUDE_PROVIDER_ENV_KEYS,
        );

        let root = base_snapshot
            .as_object_mut()
            .expect("Claude snapshot should be normalized to an object");
        for key in CLAUDE_PROVIDER_TOP_LEVEL_KEYS {
            root.remove(*key);
        }

        let provider_root = provider_settings
            .as_object()
            .ok_or_else(|| "Claude provider settings should be a JSON object".to_string())?;
        for key in CLAUDE_PROVIDER_TOP_LEVEL_KEYS {
            if let Some(value) = provider_root.get(*key) {
                root.insert((*key).to_string(), value.clone());
            }
        }

        Ok(())
    }

    fn patch_gemini_provider_owned_fields(
        base_snapshot: &mut Value,
        provider_settings: &Value,
    ) -> Result<(), String> {
        if !base_snapshot.is_object() {
            *base_snapshot = json!({});
        }

        let env = provider_settings
            .get("env")
            .cloned()
            .unwrap_or_else(|| json!({}));
        if !env.is_object() {
            return Err("Gemini provider settings env should be a JSON object".to_string());
        }

        base_snapshot
            .as_object_mut()
            .expect("Gemini snapshot should be normalized to an object")
            .insert("env".to_string(), env);

        Ok(())
    }

    fn patch_json_provider_env_fields(
        base_snapshot: &mut Value,
        provider_settings: &Value,
        keys: &[&str],
    ) {
        if !base_snapshot.is_object() {
            *base_snapshot = json!({});
        }

        let root = base_snapshot
            .as_object_mut()
            .expect("JSON snapshot should be normalized to an object");
        let env = root.entry("env".to_string()).or_insert_with(|| json!({}));
        if !env.is_object() {
            *env = json!({});
        }

        let env_obj = env
            .as_object_mut()
            .expect("JSON env should be normalized to an object");
        for key in keys {
            env_obj.remove(*key);
        }

        if let Some(provider_env) = provider_settings.get("env").and_then(|v| v.as_object()) {
            for key in keys {
                if let Some(value) = provider_env.get(*key) {
                    env_obj.insert((*key).to_string(), value.clone());
                }
            }
        }
    }

    fn patch_codex_provider_owned_fields(
        base_snapshot: &mut Value,
        provider_settings: &Value,
    ) -> Result<(), String> {
        if !base_snapshot.is_object() {
            *base_snapshot = json!({});
        }

        let root = base_snapshot
            .as_object_mut()
            .expect("Codex snapshot should be normalized to an object");
        let auth = root.entry("auth".to_string()).or_insert_with(|| json!({}));
        if !auth.is_object() {
            *auth = json!({});
        }
        let auth_obj = auth
            .as_object_mut()
            .expect("Codex auth should be normalized to an object");
        auth_obj.remove("OPENAI_API_KEY");
        if let Some(value) = provider_settings
            .get("auth")
            .and_then(|v| v.get("OPENAI_API_KEY"))
        {
            auth_obj.insert("OPENAI_API_KEY".to_string(), value.clone());
        }

        let base_config = root.get("config").and_then(|v| v.as_str()).unwrap_or("");
        let provider_config = provider_settings
            .get("config")
            .and_then(|v| v.as_str())
            .unwrap_or("");
        let patched_config = Self::patch_codex_provider_owned_config(base_config, provider_config)?;
        root.insert("config".to_string(), json!(patched_config));

        Ok(())
    }

    fn patch_codex_provider_owned_config(
        base_config: &str,
        provider_config: &str,
    ) -> Result<String, String> {
        let mut base_doc = if base_config.trim().is_empty() {
            toml_edit::DocumentMut::new()
        } else {
            base_config
                .parse::<toml_edit::DocumentMut>()
                .map_err(|e| format!("解析现有 Codex config.toml 失败: {e}"))?
        };
        let provider_doc = if provider_config.trim().is_empty() {
            toml_edit::DocumentMut::new()
        } else {
            provider_config
                .parse::<toml_edit::DocumentMut>()
                .map_err(|e| format!("解析新的 Codex config.toml 失败: {e}"))?
        };

        let previous_provider_key = base_doc
            .get("model_provider")
            .and_then(|v| v.as_str())
            .map(str::to_string);
        let current_provider_key = provider_doc
            .get("model_provider")
            .and_then(|v| v.as_str())
            .map(str::to_string);

        for key in CODEX_PROVIDER_TOP_LEVEL_KEYS {
            base_doc.as_table_mut().remove(key);
        }

        if let Some(model_providers) = base_doc
            .get_mut("model_providers")
            .and_then(|v| v.as_table_mut())
        {
            if let Some(key) = previous_provider_key.as_deref() {
                model_providers.remove(key);
            }
            if let Some(key) = current_provider_key.as_deref() {
                model_providers.remove(key);
            }
            if model_providers.is_empty() {
                base_doc.as_table_mut().remove("model_providers");
            }
        }

        for key in CODEX_PROVIDER_TOP_LEVEL_KEYS {
            if let Some(item) = provider_doc.get(key) {
                base_doc[key] = item.clone();
            }
        }

        if let Some(key) = current_provider_key.as_deref() {
            if let Some(provider_item) = provider_doc
                .get("model_providers")
                .and_then(|v| v.as_table_like())
                .and_then(|table| table.get(key))
            {
                if base_doc.get("model_providers").is_none() {
                    base_doc["model_providers"] = toml_edit::table();
                }

                if let Some(model_providers) = base_doc["model_providers"].as_table_mut() {
                    model_providers.insert(key, provider_item.clone());
                }
            }
        }

        Ok(base_doc.to_string())
    }

    fn strip_takeover_fields_from_taken_over_live(app_type: &AppType, snapshot: &Value) -> Value {
        let mut stripped = snapshot.clone();
        match app_type {
            AppType::Claude => {
                if let Some(env) = stripped.get_mut("env").and_then(|v| v.as_object_mut()) {
                    for key in [
                        "ANTHROPIC_AUTH_TOKEN",
                        "ANTHROPIC_API_KEY",
                        "OPENROUTER_API_KEY",
                        "OPENAI_API_KEY",
                    ] {
                        if env.get(key).and_then(|v| v.as_str()) == Some(PROXY_TOKEN_PLACEHOLDER) {
                            env.remove(key);
                        }
                    }

                    if env
                        .get("ANTHROPIC_BASE_URL")
                        .and_then(|v| v.as_str())
                        .map(Self::is_local_proxy_url)
                        .unwrap_or(false)
                    {
                        env.remove("ANTHROPIC_BASE_URL");
                    }
                }
            }
            AppType::Codex => {
                if let Some(auth) = stripped.get_mut("auth").and_then(|v| v.as_object_mut()) {
                    if auth.get("OPENAI_API_KEY").and_then(|v| v.as_str())
                        == Some(PROXY_TOKEN_PLACEHOLDER)
                    {
                        auth.remove("OPENAI_API_KEY");
                    }
                }

                if let Some(cfg_str) = stripped.get("config").and_then(|v| v.as_str()) {
                    stripped["config"] = json!(Self::remove_local_toml_base_url(cfg_str));
                }
            }
            AppType::Gemini => {
                if let Some(env) = stripped.get_mut("env").and_then(|v| v.as_object_mut()) {
                    if env.get("GEMINI_API_KEY").and_then(|v| v.as_str())
                        == Some(PROXY_TOKEN_PLACEHOLDER)
                    {
                        env.remove("GEMINI_API_KEY");
                    }
                    if env
                        .get("GOOGLE_GEMINI_BASE_URL")
                        .and_then(|v| v.as_str())
                        .map(Self::is_local_proxy_url)
                        .unwrap_or(false)
                    {
                        env.remove("GOOGLE_GEMINI_BASE_URL");
                    }
                }
            }
            AppType::OpenCode | AppType::OpenClaw => {}
        }

        stripped
    }

    async fn build_takeover_live_config_from_provider(
        &self,
        app_type: &AppType,
        provider: &Provider,
    ) -> Result<Value, String> {
        let mut effective_settings = self.build_effective_provider_settings(app_type, provider)?;

        if matches!(app_type, AppType::Codex) {
            if let Ok(existing_live) = self.read_codex_live() {
                Self::preserve_codex_mcp_servers(&mut effective_settings, &existing_live)?;
            }
        }

        let (proxy_url, proxy_codex_base_url) = self.build_proxy_urls().await?;
        Self::apply_takeover_overlay(
            app_type,
            &mut effective_settings,
            &proxy_url,
            &proxy_codex_base_url,
        )?;
        Ok(effective_settings)
    }

    async fn overlay_takeover_on_existing_live(&self, app_type: &AppType) -> Result<(), String> {
        let (proxy_url, proxy_codex_base_url) = self.build_proxy_urls().await?;
        let mut live_config = match app_type {
            AppType::Claude => self.read_claude_live()?,
            AppType::Codex => self.read_codex_live()?,
            AppType::Gemini => self.read_gemini_live()?,
            AppType::OpenCode => return Err("OpenCode 不支持代理功能".to_string()),
            AppType::OpenClaw => return Err("OpenClaw 不支持代理功能".to_string()),
        };

        Self::apply_takeover_overlay(
            app_type,
            &mut live_config,
            &proxy_url,
            &proxy_codex_base_url,
        )?;
        self.write_live_config_for_app(app_type, &live_config)?;
        Ok(())
    }

    /// 设置 AppHandle（在应用初始化时调用）
    pub fn set_app_handle(&self, handle: tauri::AppHandle) {
        futures::executor::block_on(async {
            *self.app_handle.write().await = Some(handle);
        });
    }

    /// 启动代理服务器
    pub async fn start(&self) -> Result<ProxyServerInfo, String> {
        // 1. 启动时自动设置 proxy_enabled = true
        let mut global_config = self
            .db
            .get_global_proxy_config()
            .await
            .map_err(|e| format!("获取全局代理配置失败: {e}"))?;

        if !global_config.proxy_enabled {
            global_config.proxy_enabled = true;
            self.db
                .update_global_proxy_config(global_config.clone())
                .await
                .map_err(|e| format!("更新代理总开关失败: {e}"))?;
        }

        // 2. 获取配置
        let config = self
            .db
            .get_proxy_config()
            .await
            .map_err(|e| format!("获取代理配置失败: {e}"))?;

        // 3. 若已在运行：确保持久化状态（如需要）并返回当前信息
        if let Some(server) = self.server.read().await.as_ref() {
            let status = server.get_status().await;
            return Ok(ProxyServerInfo {
                address: status.address,
                port: status.port,
                // 无法精确取回首次启动时间，返回当前时间用于 UI 展示即可
                started_at: chrono::Utc::now().to_rfc3339(),
            });
        }

        // 4. 创建并启动服务器
        let app_handle = self.app_handle.read().await.clone();
        let server = ProxyServer::new(config.clone(), self.db.clone(), app_handle);
        let info = server
            .start()
            .await
            .map_err(|e| format!("启动代理服务器失败: {e}"))?;

        // 5. 保存服务器实例
        *self.server.write().await = Some(server);

        log::info!("代理服务器已启动: {}:{}", info.address, info.port);
        Ok(info)
    }

    /// 启动代理服务器（带 Live 配置接管）
    pub async fn start_with_takeover(&self) -> Result<ProxyServerInfo, String> {
        // 1. 备份各应用的 Live 配置
        self.backup_live_configs().await?;

        // 2. 同步 Live 配置中的 Token 到数据库（确保代理能读到最新的 Token）
        if let Err(e) = self.sync_live_to_providers().await {
            // 同步失败时尚未写入接管配置，但备份可能包含敏感信息，尽量清理
            if let Err(clean_err) = self.db.delete_all_live_backups().await {
                log::warn!("清理 Live 备份失败: {clean_err}");
            }
            return Err(e);
        }

        // 3. 在写入接管配置之前先落盘接管标志：
        //    这样即使在接管过程中断电/kill，下次启动也能检测到并自动恢复。
        if let Err(e) = self.db.set_live_takeover_active(true).await {
            if let Err(clean_err) = self.db.delete_all_live_backups().await {
                log::warn!("清理 Live 备份失败: {clean_err}");
            }
            return Err(format!("设置接管状态失败: {e}"));
        }

        // 4. 接管各应用的 Live 配置（写入代理地址，清空 Token）
        if let Err(e) = self.takeover_live_configs().await {
            // 接管失败（可能是部分写入），尝试恢复原始配置；若恢复失败则保留标志与备份，等待下次启动自动恢复。
            log::error!("接管 Live 配置失败，尝试恢复原始配置: {e}");
            match self.restore_live_configs().await {
                Ok(()) => {
                    let _ = self.db.set_live_takeover_active(false).await;
                    let _ = self.db.delete_all_live_backups().await;
                }
                Err(restore_err) => {
                    log::error!("恢复原始配置失败，将保留备份以便下次启动恢复: {restore_err}");
                }
            }
            return Err(e);
        }

        // 5. 启动代理服务器
        match self.start().await {
            Ok(info) => Ok(info),
            Err(e) => {
                // 启动失败，恢复原始配置
                log::error!("代理启动失败，尝试恢复原始配置: {e}");
                match self.restore_live_configs().await {
                    Ok(()) => {
                        let _ = self.db.set_live_takeover_active(false).await;
                        let _ = self.db.delete_all_live_backups().await;
                    }
                    Err(restore_err) => {
                        log::error!("恢复原始配置失败，将保留备份以便下次启动恢复: {restore_err}");
                    }
                }
                Err(e)
            }
        }
    }

    /// 获取各应用的接管状态（是否改写该应用的 Live 配置指向本地代理）
    pub async fn get_takeover_status(&self) -> Result<ProxyTakeoverStatus, String> {
        // 从 proxy_config.enabled 读取（优先），兼容旧的 live_backup 备份检测
        let claude_enabled = self
            .db
            .get_proxy_config_for_app("claude")
            .await
            .map(|c| c.enabled)
            .unwrap_or(false);
        let codex_enabled = self
            .db
            .get_proxy_config_for_app("codex")
            .await
            .map(|c| c.enabled)
            .unwrap_or(false);
        let gemini_enabled = self
            .db
            .get_proxy_config_for_app("gemini")
            .await
            .map(|c| c.enabled)
            .unwrap_or(false);
        // OpenCode and OpenClaw don't support proxy features, always return false
        let opencode_enabled = false;
        let openclaw_enabled = false;

        Ok(ProxyTakeoverStatus {
            claude: claude_enabled,
            codex: codex_enabled,
            gemini: gemini_enabled,
            opencode: opencode_enabled,
            openclaw: openclaw_enabled,
        })
    }

    /// 为指定应用开启/关闭 Live 接管
    ///
    /// - 开启：自动启动代理服务，仅接管当前 app 的 Live 配置
    /// - 关闭：仅恢复当前 app 的 Live 配置；若无其它接管，则自动停止代理服务
    pub async fn set_takeover_for_app(&self, app_type: &str, enabled: bool) -> Result<(), String> {
        let app = AppType::from_str(app_type).map_err(|e| format!("无效的应用类型: {e}"))?;
        let app_type_str = app.as_str();

        if enabled {
            // 1) 代理服务未运行则自动启动
            if !self.is_running().await {
                self.start().await?;
            }

            // 2) 已接管则直接返回（幂等）；但如果缺少备份或占位符残留，需要重建接管
            let current_config = self
                .db
                .get_proxy_config_for_app(app_type_str)
                .await
                .map_err(|e| format!("获取 {app_type_str} 配置失败: {e}"))?;

            if current_config.enabled {
                let has_backup = match self.db.get_live_backup(app_type_str).await {
                    Ok(v) => v.is_some(),
                    Err(e) => {
                        log::warn!("读取 {app_type_str} 备份失败（将继续重建接管）: {e}");
                        false
                    }
                };
                let live_taken_over = self.detect_takeover_in_live_config_for_app(&app);

                if has_backup || live_taken_over {
                    return Ok(());
                }

                log::warn!(
                    "{app_type_str} 标记为已接管，但缺少备份或占位符，正在重新接管并补齐备份"
                );
            }

            // 3) 备份 Live 配置（严格：目标 app 不存在则报错）
            self.backup_live_config_strict(&app).await?;

            // 4) 同步 Live Token 到数据库（仅当前 app）
            if let Err(e) = self.sync_live_to_provider(&app).await {
                let _ = self.db.delete_live_backup(app_type_str).await;
                return Err(e);
            }

            // 5) 写入接管配置（仅当前 app）
            if let Err(e) = self.takeover_live_config_strict(&app).await {
                log::error!("{app_type_str} 接管 Live 配置失败，尝试恢复: {e}");
                match self.restore_live_config_for_app(&app).await {
                    Ok(()) => {
                        // 恢复成功才清理备份，避免失败场景下丢失唯一可回滚来源
                        let _ = self.db.delete_live_backup(app_type_str).await;
                    }
                    Err(restore_err) => {
                        log::error!(
                            "{app_type_str} 恢复 Live 配置失败，将保留备份以便下次启动恢复: {restore_err}"
                        );
                    }
                }
                return Err(e);
            }

            // 6) 设置 proxy_config.enabled = true
            let mut updated_config = self
                .db
                .get_proxy_config_for_app(app_type_str)
                .await
                .map_err(|e| format!("获取 {app_type_str} 配置失败: {e}"))?;
            updated_config.enabled = true;
            self.db
                .update_proxy_config_for_app(updated_config)
                .await
                .map_err(|e| format!("设置 {app_type_str} enabled 状态失败: {e}"))?;

            // 7) 兼容旧逻辑：写入 any-of 标志（失败不影响功能）
            let _ = self.db.set_live_takeover_active(true).await;
            return Ok(());
        }

        // 关闭接管：检查 enabled 状态
        let current_config = self
            .db
            .get_proxy_config_for_app(app_type_str)
            .await
            .map_err(|e| format!("获取 {app_type_str} 配置失败: {e}"))?;

        if !current_config.enabled {
            return Ok(()); // 未接管，幂等返回
        }

        // 1) 恢复 Live 配置
        self.restore_live_config_for_app(&app).await?;

        // 2) 删除该 app 的备份（避免长期存储敏感 Token）
        self.db
            .delete_live_backup(app_type_str)
            .await
            .map_err(|e| format!("删除 {app_type_str} Live 备份失败: {e}"))?;

        // 3) 设置 proxy_config.enabled = false
        let mut updated_config = self
            .db
            .get_proxy_config_for_app(app_type_str)
            .await
            .map_err(|e| format!("获取 {app_type_str} 配置失败: {e}"))?;
        updated_config.enabled = false;
        self.db
            .update_proxy_config_for_app(updated_config)
            .await
            .map_err(|e| format!("清除 {app_type_str} enabled 状态失败: {e}"))?;

        // 4) 清除该应用的健康状态（关闭代理时重置队列状态）
        self.db
            .clear_provider_health_for_app(app_type_str)
            .await
            .map_err(|e| format!("清除 {app_type_str} 健康状态失败: {e}"))?;

        // 5) 若无其它接管，更新旧标志，并停止代理服务
        // 检查是否还有其它 app 的 enabled = true
        let any_enabled = self
            .db
            .is_live_takeover_active()
            .await
            .map_err(|e| format!("检查接管状态失败: {e}"))?;

        if !any_enabled {
            let _ = self.db.set_live_takeover_active(false).await;

            if self.is_running().await {
                // 此时没有任何 app 处于接管状态，停止服务即可
                let _ = self.stop().await;
            }
        }

        Ok(())
    }

    /// 同步 Live 配置中的 Token 到数据库
    ///
    /// 在清空 Live Token 之前调用，确保数据库中的 Provider 配置有最新的 Token。
    /// 这样代理才能从数据库读取到正确的认证信息。
    async fn sync_live_to_provider(&self, app_type: &AppType) -> Result<(), String> {
        let live_config = match app_type {
            AppType::Claude => self.read_claude_live()?,
            AppType::Codex => self.read_codex_live()?,
            AppType::Gemini => self.read_gemini_live()?,
            AppType::OpenCode => {
                // OpenCode doesn't support proxy features
                return Err("OpenCode 不支持代理功能".to_string());
            }
            AppType::OpenClaw => {
                // OpenClaw doesn't support proxy features
                return Err("OpenClaw 不支持代理功能".to_string());
            }
        };

        self.sync_live_config_to_provider(app_type, &live_config)
            .await
    }

    async fn sync_live_config_to_provider(
        &self,
        app_type: &AppType,
        live_config: &Value,
    ) -> Result<(), String> {
        match app_type {
            AppType::Claude => {
                let provider_id =
                    crate::settings::get_effective_current_provider(&self.db, &AppType::Claude)
                        .map_err(|e| format!("获取 Claude 当前供应商失败: {e}"))?;

                if let Some(provider_id) = provider_id {
                    if let Ok(Some(mut provider)) =
                        self.db.get_provider_by_id(&provider_id, "claude")
                    {
                        if let Some(env) = live_config.get("env").and_then(|v| v.as_object()) {
                            let token_pair = [
                                "ANTHROPIC_AUTH_TOKEN",
                                "ANTHROPIC_API_KEY",
                                "OPENROUTER_API_KEY",
                                "OPENAI_API_KEY",
                            ]
                            .into_iter()
                            .find_map(|key| {
                                env.get(key)
                                    .and_then(|v| v.as_str())
                                    .map(|s| (key, s.trim()))
                            })
                            .filter(|(_, token)| {
                                !token.is_empty() && *token != PROXY_TOKEN_PLACEHOLDER
                            });

                            if let Some((token_key, token)) = token_pair {
                                let env_obj = provider
                                    .settings_config
                                    .get_mut("env")
                                    .and_then(|v| v.as_object_mut());

                                match env_obj {
                                    Some(obj) => {
                                        if token_key == "ANTHROPIC_AUTH_TOKEN"
                                            || token_key == "ANTHROPIC_API_KEY"
                                        {
                                            let mut updated = false;
                                            if obj.contains_key("ANTHROPIC_AUTH_TOKEN") {
                                                obj.insert(
                                                    "ANTHROPIC_AUTH_TOKEN".to_string(),
                                                    json!(token),
                                                );
                                                updated = true;
                                            }
                                            if obj.contains_key("ANTHROPIC_API_KEY") {
                                                obj.insert(
                                                    "ANTHROPIC_API_KEY".to_string(),
                                                    json!(token),
                                                );
                                                updated = true;
                                            }
                                            if !updated {
                                                obj.insert(token_key.to_string(), json!(token));
                                            }
                                        } else {
                                            obj.insert(token_key.to_string(), json!(token));
                                        }
                                    }
                                    None => {
                                        // 至少写入一份可用的 Token
                                        if provider.settings_config.is_null() {
                                            provider.settings_config = json!({});
                                        }

                                        if let Some(root) = provider.settings_config.as_object_mut()
                                        {
                                            root.insert(
                                                "env".to_string(),
                                                json!({ token_key: token }),
                                            );
                                        } else {
                                            log::warn!(
                                                "Claude provider settings_config 格式异常（非对象），跳过写入 Token (provider: {provider_id})"
                                            );
                                        }
                                    }
                                }

                                if let Err(e) = self.db.update_provider_settings_config(
                                    "claude",
                                    &provider_id,
                                    &provider.settings_config,
                                ) {
                                    log::warn!("同步 Claude Token 到数据库失败: {e}");
                                } else {
                                    log::info!(
                                        "已同步 Claude Token 到数据库 (provider: {provider_id})"
                                    );
                                }
                            }
                        }
                    }
                }
            }
            AppType::Codex => {
                let provider_id =
                    crate::settings::get_effective_current_provider(&self.db, &AppType::Codex)
                        .map_err(|e| format!("获取 Codex 当前供应商失败: {e}"))?;

                if let Some(provider_id) = provider_id {
                    if let Ok(Some(mut provider)) =
                        self.db.get_provider_by_id(&provider_id, "codex")
                    {
                        if let Some(token) = live_config
                            .get("auth")
                            .and_then(|v| v.get("OPENAI_API_KEY"))
                            .and_then(|v| v.as_str())
                            .map(|s| s.trim())
                            .filter(|s| !s.is_empty() && *s != PROXY_TOKEN_PLACEHOLDER)
                        {
                            if let Some(auth_obj) = provider
                                .settings_config
                                .get_mut("auth")
                                .and_then(|v| v.as_object_mut())
                            {
                                auth_obj.insert("OPENAI_API_KEY".to_string(), json!(token));
                            } else {
                                if provider.settings_config.is_null() {
                                    provider.settings_config = json!({});
                                }

                                if let Some(root) = provider.settings_config.as_object_mut() {
                                    root.insert(
                                        "auth".to_string(),
                                        json!({ "OPENAI_API_KEY": token }),
                                    );
                                } else {
                                    log::warn!(
                                        "Codex provider settings_config 格式异常（非对象），跳过写入 Token (provider: {provider_id})"
                                    );
                                }
                            }

                            if let Err(e) = self.db.update_provider_settings_config(
                                "codex",
                                &provider_id,
                                &provider.settings_config,
                            ) {
                                log::warn!("同步 Codex Token 到数据库失败: {e}");
                            } else {
                                log::info!("已同步 Codex Token 到数据库 (provider: {provider_id})");
                            }
                        }
                    }
                }
            }
            AppType::Gemini => {
                let provider_id =
                    crate::settings::get_effective_current_provider(&self.db, &AppType::Gemini)
                        .map_err(|e| format!("获取 Gemini 当前供应商失败: {e}"))?;

                if let Some(provider_id) = provider_id {
                    if let Ok(Some(mut provider)) =
                        self.db.get_provider_by_id(&provider_id, "gemini")
                    {
                        if let Some(token) = live_config
                            .get("env")
                            .and_then(|v| v.get("GEMINI_API_KEY"))
                            .and_then(|v| v.as_str())
                            .map(|s| s.trim())
                            .filter(|s| !s.is_empty() && *s != PROXY_TOKEN_PLACEHOLDER)
                        {
                            if let Some(env_obj) = provider
                                .settings_config
                                .get_mut("env")
                                .and_then(|v| v.as_object_mut())
                            {
                                env_obj.insert("GEMINI_API_KEY".to_string(), json!(token));
                            } else {
                                if provider.settings_config.is_null() {
                                    provider.settings_config = json!({});
                                }

                                if let Some(root) = provider.settings_config.as_object_mut() {
                                    root.insert(
                                        "env".to_string(),
                                        json!({ "GEMINI_API_KEY": token }),
                                    );
                                } else {
                                    log::warn!(
                                        "Gemini provider settings_config 格式异常（非对象），跳过写入 Token (provider: {provider_id})"
                                    );
                                }
                            }

                            if let Err(e) = self.db.update_provider_settings_config(
                                "gemini",
                                &provider_id,
                                &provider.settings_config,
                            ) {
                                log::warn!("同步 Gemini Token 到数据库失败: {e}");
                            } else {
                                log::info!(
                                    "已同步 Gemini Token 到数据库 (provider: {provider_id})"
                                );
                            }
                        }
                    }
                }
            }
            AppType::OpenCode => {
                // OpenCode doesn't support proxy features, skip silently
            }
            AppType::OpenClaw => {
                // OpenClaw doesn't support proxy features, skip silently
            }
        }

        Ok(())
    }

    async fn sync_live_to_providers(&self) -> Result<(), String> {
        if let Ok(live_config) = self.read_claude_live() {
            self.sync_live_config_to_provider(&AppType::Claude, &live_config)
                .await?;
        }

        if let Ok(live_config) = self.read_codex_live() {
            self.sync_live_config_to_provider(&AppType::Codex, &live_config)
                .await?;
        }

        if let Ok(live_config) = self.read_gemini_live() {
            self.sync_live_config_to_provider(&AppType::Gemini, &live_config)
                .await?;
        }

        log::info!("Live 配置 Token 同步完成");
        Ok(())
    }

    /// 停止代理服务器
    pub async fn stop(&self) -> Result<(), String> {
        if let Some(server) = self.server.write().await.take() {
            server
                .stop()
                .await
                .map_err(|e| format!("停止代理服务器失败: {e}"))?;

            // 停止时设置 proxy_enabled = false
            let mut global_config = self
                .db
                .get_global_proxy_config()
                .await
                .map_err(|e| format!("获取全局代理配置失败: {e}"))?;

            if global_config.proxy_enabled {
                global_config.proxy_enabled = false;
                if let Err(e) = self.db.update_global_proxy_config(global_config).await {
                    log::warn!("更新代理总开关失败: {e}");
                }
            }

            log::info!("代理服务器已停止");
            Ok(())
        } else {
            Err("代理服务器未运行".to_string())
        }
    }

    /// 停止代理服务器（恢复 Live 配置，用户手动关闭时使用）
    ///
    /// 会清除 settings 表中的代理状态，下次启动不会自动恢复。
    pub async fn stop_with_restore(&self) -> Result<(), String> {
        // 1. 停止代理服务器（即使未运行也继续执行恢复逻辑）
        if let Err(e) = self.stop().await {
            log::warn!("停止代理服务器失败（将继续恢复 Live 配置）: {e}");
        }

        // 2. 恢复原始 Live 配置
        self.restore_live_configs().await?;

        // 3. 清除 proxy_config 表中的接管状态（兼容旧版）
        self.db
            .set_live_takeover_active(false)
            .await
            .map_err(|e| format!("清除接管状态失败: {e}"))?;

        // 4. 清除所有应用的 enabled 状态（用户手动关闭，不需要下次自动恢复）
        for app_type in ["claude", "codex", "gemini"] {
            if let Ok(mut config) = self.db.get_proxy_config_for_app(app_type).await {
                if config.enabled {
                    config.enabled = false;
                    if let Err(e) = self.db.update_proxy_config_for_app(config).await {
                        log::warn!("清除 {app_type} enabled 状态失败: {e}");
                    }
                }
            }
        }

        // 5. 删除备份
        self.db
            .delete_all_live_backups()
            .await
            .map_err(|e| format!("删除备份失败: {e}"))?;

        // 6. 重置健康状态（让健康徽章恢复为正常）
        self.db
            .clear_all_provider_health()
            .await
            .map_err(|e| format!("重置健康状态失败: {e}"))?;

        // 注意：不清除故障转移队列和开关状态，保留供下次开启代理时使用
        log::info!("代理已停止，Live 配置已恢复");
        Ok(())
    }

    /// 停止代理服务器（恢复 Live 配置，但保留 settings 表中的代理状态）
    ///
    /// 用于程序正常退出时，保留代理状态以便下次启动时自动恢复
    pub async fn stop_with_restore_keep_state(&self) -> Result<(), String> {
        // 1. 停止代理服务器（即使未运行也继续执行恢复逻辑）
        if let Err(e) = self.stop().await {
            log::warn!("停止代理服务器失败（将继续恢复 Live 配置）: {e}");
        }

        // 2. 恢复原始 Live 配置
        self.restore_live_configs().await?;

        // 3. 更新 proxy_config 表中的 live_takeover_active 标志（兼容旧版）
        //    注意：保留 proxy_config.enabled 状态，下次启动时自动恢复
        if let Ok(mut config) = self.db.get_proxy_config().await {
            config.live_takeover_active = false;
            let _ = self.db.update_proxy_config(config).await;
        }

        // 4. 删除备份（Live 配置已恢复，备份不再需要）
        self.db
            .delete_all_live_backups()
            .await
            .map_err(|e| format!("删除备份失败: {e}"))?;

        // 5. 重置健康状态
        self.db
            .clear_all_provider_health()
            .await
            .map_err(|e| format!("重置健康状态失败: {e}"))?;

        log::info!("代理已停止，Live 配置已恢复（保留代理状态，下次启动将自动恢复）");
        Ok(())
    }

    /// 备份各应用的 Live 配置
    async fn backup_live_configs(&self) -> Result<(), String> {
        // Claude
        if let Ok(config) = self.read_claude_live() {
            let json_str = serde_json::to_string(&config)
                .map_err(|e| format!("序列化 Claude 配置失败: {e}"))?;
            self.db
                .save_live_backup("claude", &json_str)
                .await
                .map_err(|e| format!("备份 Claude 配置失败: {e}"))?;
        }

        // Codex
        if let Ok(config) = self.read_codex_live() {
            let json_str = serde_json::to_string(&config)
                .map_err(|e| format!("序列化 Codex 配置失败: {e}"))?;
            self.db
                .save_live_backup("codex", &json_str)
                .await
                .map_err(|e| format!("备份 Codex 配置失败: {e}"))?;
        }

        // Gemini
        if let Ok(config) = self.read_gemini_live() {
            let json_str = serde_json::to_string(&config)
                .map_err(|e| format!("序列化 Gemini 配置失败: {e}"))?;
            self.db
                .save_live_backup("gemini", &json_str)
                .await
                .map_err(|e| format!("备份 Gemini 配置失败: {e}"))?;
        }

        log::info!("已备份所有应用的 Live 配置");
        Ok(())
    }

    /// 备份指定应用的 Live 配置（严格模式：目标配置不存在则返回错误）
    async fn backup_live_config_strict(&self, app_type: &AppType) -> Result<(), String> {
        let (app_type_str, config) = match app_type {
            AppType::Claude => ("claude", self.read_claude_live()?),
            AppType::Codex => ("codex", self.read_codex_live()?),
            AppType::Gemini => ("gemini", self.read_gemini_live()?),
            AppType::OpenCode => {
                // OpenCode doesn't support proxy features
                return Err("OpenCode 不支持代理功能".to_string());
            }
            AppType::OpenClaw => {
                // OpenClaw doesn't support proxy features
                return Err("OpenClaw 不支持代理功能".to_string());
            }
        };

        let json_str = serde_json::to_string(&config)
            .map_err(|e| format!("序列化 {app_type_str} 配置失败: {e}"))?;
        self.db
            .save_live_backup(app_type_str, &json_str)
            .await
            .map_err(|e| format!("备份 {app_type_str} 配置失败: {e}"))?;

        Ok(())
    }

    /// 构造写入 Live 的代理地址（处理 0.0.0.0 / IPv6 等特殊情况）
    async fn build_proxy_urls(&self) -> Result<(String, String), String> {
        let config = self
            .db
            .get_proxy_config()
            .await
            .map_err(|e| format!("获取代理配置失败: {e}"))?;

        // listen_address 可能是 0.0.0.0（用于监听所有网卡），但客户端无法用 0.0.0.0 连接；
        // 因此写回到各应用配置时，优先使用本机回环地址。
        let connect_host = match config.listen_address.as_str() {
            "0.0.0.0" => "127.0.0.1".to_string(),
            "::" => "::1".to_string(),
            _ => config.listen_address.clone(),
        };
        let connect_host_for_url = if connect_host.contains(':') && !connect_host.starts_with('[') {
            format!("[{connect_host}]")
        } else {
            connect_host
        };

        let proxy_origin = format!("http://{}:{}", connect_host_for_url, config.listen_port);
        let proxy_url = proxy_origin.clone();
        let proxy_codex_base_url = format!("{}/v1", proxy_origin.trim_end_matches('/'));

        Ok((proxy_url, proxy_codex_base_url))
    }

    /// 接管各应用的 Live 配置（写入代理地址）
    ///
    /// 代理服务器的路由已经根据 API 端点自动区分应用类型：
    /// - `/v1/messages` → Claude
    /// - `/v1/chat/completions`, `/v1/responses` → Codex
    /// - `/v1beta/*` → Gemini
    ///
    /// 因此不需要在 URL 中添加应用前缀。
    async fn takeover_live_configs(&self) -> Result<(), String> {
        for app_type in [AppType::Claude, AppType::Codex, AppType::Gemini] {
            self.takeover_live_config_best_effort(&app_type).await?;
        }

        Ok(())
    }

    /// 接管指定应用的 Live 配置（严格模式：目标配置不存在则返回错误）
    async fn takeover_live_config_strict(&self, app_type: &AppType) -> Result<(), String> {
        let _guard = self.switch_locks.lock_for_app(app_type.as_str()).await;
        match self
            .sync_live_from_current_provider_while_proxy_active_inner(app_type)
            .await
        {
            Ok(true) => {}
            Ok(false) => {
                self.overlay_takeover_on_existing_live(app_type).await?;
            }
            Err(err) => {
                log::warn!(
                    "基于当前 {} 供应商重建接管配置失败，将回退到现有 Live 配置: {err}",
                    app_type.as_str()
                );
                self.overlay_takeover_on_existing_live(app_type).await?;
            }
        }

        let (proxy_url, proxy_codex_base_url) = self.build_proxy_urls().await?;
        match app_type {
            AppType::Claude => log::info!("Claude Live 配置已接管，代理地址: {proxy_url}"),
            AppType::Codex => {
                log::info!("Codex Live 配置已接管，代理地址: {proxy_codex_base_url}")
            }
            AppType::Gemini => log::info!("Gemini Live 配置已接管，代理地址: {proxy_url}"),
            AppType::OpenCode => return Err("OpenCode 不支持代理功能".to_string()),
            AppType::OpenClaw => return Err("OpenClaw 不支持代理功能".to_string()),
        }

        Ok(())
    }

    /// 接管指定应用的 Live 配置（尽力而为：配置不存在/读取失败则跳过）
    async fn takeover_live_config_best_effort(&self, app_type: &AppType) -> Result<(), String> {
        let _guard = self.switch_locks.lock_for_app(app_type.as_str()).await;
        if let Err(err) = self.overlay_takeover_on_existing_live(app_type).await {
            log::debug!(
                "{} Live 配置不可用，跳过 best-effort 接管: {err}",
                app_type.as_str()
            );
        }

        Ok(())
    }

    /// 恢复指定应用的 Live 配置（若无备份则不做任何操作）
    async fn restore_live_config_for_app(&self, app_type: &AppType) -> Result<(), String> {
        let _guard = self.switch_locks.lock_for_app(app_type.as_str()).await;
        self.restore_live_config_for_app_inner(app_type).await
    }

    async fn restore_live_config_for_app_inner(&self, app_type: &AppType) -> Result<(), String> {
        match app_type {
            AppType::Claude => {
                if let Ok(Some(backup)) = self.db.get_live_backup("claude").await {
                    let config: Value = serde_json::from_str(&backup.original_config)
                        .map_err(|e| format!("解析 Claude 备份失败: {e}"))?;
                    self.write_claude_live(&config)?;
                    log::info!("Claude Live 配置已恢复");
                }
            }
            AppType::Codex => {
                if let Ok(Some(backup)) = self.db.get_live_backup("codex").await {
                    let config: Value = serde_json::from_str(&backup.original_config)
                        .map_err(|e| format!("解析 Codex 备份失败: {e}"))?;
                    self.write_codex_live(&config)?;
                    log::info!("Codex Live 配置已恢复");
                }
            }
            AppType::Gemini => {
                if let Ok(Some(backup)) = self.db.get_live_backup("gemini").await {
                    let config: Value = serde_json::from_str(&backup.original_config)
                        .map_err(|e| format!("解析 Gemini 备份失败: {e}"))?;
                    self.write_gemini_live(&config)?;
                    log::info!("Gemini Live 配置已恢复");
                }
            }
            AppType::OpenCode => {
                // OpenCode doesn't support proxy features, skip silently
            }
            AppType::OpenClaw => {
                // OpenClaw doesn't support proxy features, skip silently
            }
        }

        Ok(())
    }

    /// 恢复原始 Live 配置
    async fn restore_live_configs(&self) -> Result<(), String> {
        let mut errors = Vec::new();

        for app_type in [AppType::Claude, AppType::Codex, AppType::Gemini] {
            if let Err(e) = self
                .restore_live_config_for_app_with_fallback(&app_type)
                .await
            {
                errors.push(e);
            }
        }

        if errors.is_empty() {
            Ok(())
        } else {
            Err(errors.join("；"))
        }
    }

    async fn restore_live_config_for_app_with_fallback(
        &self,
        app_type: &AppType,
    ) -> Result<(), String> {
        let _guard = self.switch_locks.lock_for_app(app_type.as_str()).await;
        self.restore_live_config_for_app_with_fallback_inner(app_type)
            .await
    }

    async fn restore_live_config_for_app_with_fallback_inner(
        &self,
        app_type: &AppType,
    ) -> Result<(), String> {
        let app_type_str = app_type.as_str();

        // 1) 优先从 Live 备份恢复（这是"原始 Live"的唯一可靠来源）
        let backup = self
            .db
            .get_live_backup(app_type_str)
            .await
            .map_err(|e| format!("获取 {app_type_str} Live 备份失败: {e}"))?;
        if let Some(backup) = backup {
            let config: Value = serde_json::from_str(&backup.original_config)
                .map_err(|e| format!("解析 {app_type_str} 备份失败: {e}"))?;
            self.write_live_config_for_app(app_type, &config)?;
            log::info!("{app_type_str} Live 配置已从备份恢复");
            return Ok(());
        }

        // 2) 兜底：备份缺失，但 Live 仍包含接管占位符（异常退出/历史 bug 场景）
        if !self.detect_takeover_in_live_config_for_app(app_type) {
            return Ok(());
        }

        // 2.1) 优先从 SSOT（当前供应商）重建 Live（比"清理字段"更可用）
        match self.restore_live_from_ssot_for_app(app_type) {
            Ok(true) => {
                log::info!("{app_type_str} Live 配置已从 SSOT 恢复（无备份兜底）");
                return Ok(());
            }
            Ok(false) => {
                log::warn!(
                    "{app_type_str} Live 备份缺失，且无法从 SSOT 恢复，将尝试清理接管占位符"
                );
            }
            Err(e) => {
                log::error!(
                    "{app_type_str} Live 备份缺失，SSOT 恢复失败，将尝试清理接管占位符: {e}"
                );
            }
        }

        // 2.2) 最后兜底：尽力清理占位符与本地代理地址，避免长期卡在代理占位符状态
        self.cleanup_takeover_placeholders_in_live_for_app(app_type)?;
        log::info!("{app_type_str} Live 接管占位符已清理（无备份兜底）");
        Ok(())
    }

    fn write_live_config_for_app(&self, app_type: &AppType, config: &Value) -> Result<(), String> {
        match app_type {
            AppType::Claude => self.write_claude_live(config),
            AppType::Codex => self.write_codex_live(config),
            AppType::Gemini => self.write_gemini_live(config),
            AppType::OpenCode => {
                // OpenCode doesn't support proxy features
                Err("OpenCode 不支持代理功能".to_string())
            }
            AppType::OpenClaw => {
                // OpenClaw doesn't support proxy features
                Err("OpenClaw 不支持代理功能".to_string())
            }
        }
    }

    fn read_live_snapshot_for_app(&self, app_type: &AppType) -> Result<Value, String> {
        match app_type {
            AppType::Claude => self.read_claude_live(),
            AppType::Codex => self.read_codex_live(),
            AppType::Gemini => self.read_gemini_live(),
            AppType::OpenCode => Err("OpenCode 不支持代理功能".to_string()),
            AppType::OpenClaw => Err("OpenClaw 不支持代理功能".to_string()),
        }
    }

    pub fn detect_takeover_in_live_config_for_app(&self, app_type: &AppType) -> bool {
        match app_type {
            AppType::Claude => match self.read_claude_live() {
                Ok(config) => Self::is_claude_live_taken_over(&config),
                Err(_) => false,
            },
            AppType::Codex => match self.read_codex_live() {
                Ok(config) => Self::is_codex_live_taken_over(&config),
                Err(_) => false,
            },
            AppType::Gemini => match self.read_gemini_live() {
                Ok(config) => Self::is_gemini_live_taken_over(&config),
                Err(_) => false,
            },
            AppType::OpenCode => {
                // OpenCode doesn't support proxy takeover
                false
            }
            AppType::OpenClaw => {
                // OpenClaw doesn't support proxy takeover
                false
            }
        }
    }

    /// 当 Live 备份缺失时，尝试用 SSOT（当前供应商）写回 Live，以解除占位符接管。
    ///
    /// 返回值：
    /// - Ok(true)：已成功写回
    /// - Ok(false)：缺少当前供应商/供应商不存在，无法写回
    fn restore_live_from_ssot_for_app(&self, app_type: &AppType) -> Result<bool, String> {
        let current_id = crate::settings::get_effective_current_provider(&self.db, app_type)
            .map_err(|e| format!("获取 {app_type:?} 当前供应商失败: {e}"))?;

        let Some(current_id) = current_id else {
            return Ok(false);
        };

        let providers = self
            .db
            .get_all_providers(app_type.as_str())
            .map_err(|e| format!("读取 {app_type:?} 供应商列表失败: {e}"))?;

        let Some(provider) = providers.get(&current_id) else {
            return Ok(false);
        };

        write_live_with_common_config(self.db.as_ref(), app_type, provider)
            .map_err(|e| format!("写入 {app_type:?} Live 配置失败: {e}"))?;

        Ok(true)
    }

    fn cleanup_takeover_placeholders_in_live_for_app(
        &self,
        app_type: &AppType,
    ) -> Result<(), String> {
        match app_type {
            AppType::Claude => self.cleanup_claude_takeover_placeholders_in_live(),
            AppType::Codex => self.cleanup_codex_takeover_placeholders_in_live(),
            AppType::Gemini => self.cleanup_gemini_takeover_placeholders_in_live(),
            AppType::OpenCode => {
                // OpenCode doesn't support proxy features
                Ok(())
            }
            AppType::OpenClaw => {
                // OpenClaw doesn't support proxy features
                Ok(())
            }
        }
    }

    fn is_local_proxy_url(url: &str) -> bool {
        let url = url.trim();
        if !url.starts_with("http://") {
            return false;
        }
        let rest = &url["http://".len()..];
        rest.starts_with("127.0.0.1")
            || rest.starts_with("localhost")
            || rest.starts_with("0.0.0.0")
            || rest.starts_with("[::1]")
            || rest.starts_with("[::]")
            || rest.starts_with("::1")
            || rest.starts_with("::")
    }

    fn cleanup_claude_takeover_placeholders_in_live(&self) -> Result<(), String> {
        let mut config = self.read_claude_live()?;

        let Some(env) = config.get_mut("env").and_then(|v| v.as_object_mut()) else {
            return Ok(());
        };

        for key in [
            "ANTHROPIC_AUTH_TOKEN",
            "ANTHROPIC_API_KEY",
            "OPENROUTER_API_KEY",
            "OPENAI_API_KEY",
        ] {
            if env.get(key).and_then(|v| v.as_str()) == Some(PROXY_TOKEN_PLACEHOLDER) {
                env.remove(key);
            }
        }

        if env
            .get("ANTHROPIC_BASE_URL")
            .and_then(|v| v.as_str())
            .map(Self::is_local_proxy_url)
            .unwrap_or(false)
        {
            env.remove("ANTHROPIC_BASE_URL");
        }

        self.write_claude_live(&config)?;
        Ok(())
    }

    fn cleanup_codex_takeover_placeholders_in_live(&self) -> Result<(), String> {
        let mut config = self.read_codex_live()?;

        if let Some(auth) = config.get_mut("auth").and_then(|v| v.as_object_mut()) {
            if auth.get("OPENAI_API_KEY").and_then(|v| v.as_str()) == Some(PROXY_TOKEN_PLACEHOLDER)
            {
                auth.remove("OPENAI_API_KEY");
            }
        }

        if let Some(cfg_str) = config.get("config").and_then(|v| v.as_str()) {
            let updated = Self::remove_local_toml_base_url(cfg_str);
            config["config"] = json!(updated);
        }

        self.write_codex_live(&config)?;
        Ok(())
    }

    /// Remove local proxy base_url from TOML（委托给 codex_config 共享实现）
    fn remove_local_toml_base_url(toml_str: &str) -> String {
        crate::codex_config::remove_codex_toml_base_url_if(toml_str, Self::is_local_proxy_url)
    }

    fn cleanup_gemini_takeover_placeholders_in_live(&self) -> Result<(), String> {
        let mut config = self.read_gemini_live()?;

        let Some(env) = config.get_mut("env").and_then(|v| v.as_object_mut()) else {
            return Ok(());
        };

        if env.get("GEMINI_API_KEY").and_then(|v| v.as_str()) == Some(PROXY_TOKEN_PLACEHOLDER) {
            env.remove("GEMINI_API_KEY");
        }

        if env
            .get("GOOGLE_GEMINI_BASE_URL")
            .and_then(|v| v.as_str())
            .map(Self::is_local_proxy_url)
            .unwrap_or(false)
        {
            env.remove("GOOGLE_GEMINI_BASE_URL");
        }

        self.write_gemini_live(&config)?;
        Ok(())
    }

    /// 检查是否处于 Live 接管模式
    pub async fn is_takeover_active(&self) -> Result<bool, String> {
        let status = self.get_takeover_status().await?;
        Ok(status.claude || status.codex || status.gemini)
    }

    /// 从异常退出中恢复（启动时调用）
    ///
    /// 检测到 Live 备份残留时调用此方法。
    /// 会恢复 Live 配置、清除接管标志、删除备份。
    pub async fn recover_from_crash(&self) -> Result<(), String> {
        // 1. 恢复 Live 配置
        self.restore_live_configs().await?;

        // 2. 清除接管标志
        self.db
            .set_live_takeover_active(false)
            .await
            .map_err(|e| format!("清除接管状态失败: {e}"))?;

        // 3. 删除备份
        self.db
            .delete_all_live_backups()
            .await
            .map_err(|e| format!("删除备份失败: {e}"))?;

        log::info!("已从异常退出中恢复 Live 配置");
        Ok(())
    }

    /// 检测 Live 配置是否处于"被接管"的残留状态
    ///
    /// 用于兜底处理：当数据库备份缺失但 Live 文件已经写成代理占位符时，
    /// 启动流程可以据此触发恢复逻辑。
    pub fn detect_takeover_in_live_configs(&self) -> bool {
        if let Ok(config) = self.read_claude_live() {
            if Self::is_claude_live_taken_over(&config) {
                return true;
            }
        }

        if let Ok(config) = self.read_codex_live() {
            if Self::is_codex_live_taken_over(&config) {
                return true;
            }
        }

        if let Ok(config) = self.read_gemini_live() {
            if Self::is_gemini_live_taken_over(&config) {
                return true;
            }
        }

        false
    }

    fn is_claude_live_taken_over(config: &Value) -> bool {
        let env = match config.get("env").and_then(|v| v.as_object()) {
            Some(env) => env,
            None => return false,
        };

        for key in [
            "ANTHROPIC_AUTH_TOKEN",
            "ANTHROPIC_API_KEY",
            "OPENROUTER_API_KEY",
            "OPENAI_API_KEY",
        ] {
            if env.get(key).and_then(|v| v.as_str()) == Some(PROXY_TOKEN_PLACEHOLDER) {
                return true;
            }
        }

        false
    }

    fn is_codex_live_taken_over(config: &Value) -> bool {
        let auth = match config.get("auth").and_then(|v| v.as_object()) {
            Some(auth) => auth,
            None => return false,
        };
        auth.get("OPENAI_API_KEY").and_then(|v| v.as_str()) == Some(PROXY_TOKEN_PLACEHOLDER)
    }

    fn is_gemini_live_taken_over(config: &Value) -> bool {
        let env = match config.get("env").and_then(|v| v.as_object()) {
            Some(env) => env,
            None => return false,
        };
        env.get("GEMINI_API_KEY").and_then(|v| v.as_str()) == Some(PROXY_TOKEN_PLACEHOLDER)
    }

    /// 从供应商配置更新 Live 备份（用于代理模式下的热切换）
    ///
    /// 与 backup_live_configs() 不同，此方法从供应商的 settings_config 生成备份，
    /// 而不是从 Live 文件读取（因为 Live 文件已被代理接管）。
    pub async fn update_live_backup_from_provider(
        &self,
        app_type: &str,
        provider: &Provider,
    ) -> Result<(), String> {
        let _guard = self.switch_locks.lock_for_app(app_type).await;
        self.update_live_backup_from_provider_inner(app_type, provider)
            .await
    }

    /// 仅供已持有 per-app 切换锁的调用方使用。
    async fn update_live_backup_from_provider_inner(
        &self,
        app_type: &str,
        provider: &Provider,
    ) -> Result<(), String> {
        let app_type_enum =
            AppType::from_str(app_type).map_err(|_| format!("未知的应用类型: {app_type}"))?;
        let mut effective_settings =
            build_effective_settings_with_common_config(self.db.as_ref(), &app_type_enum, provider)
                .map_err(|e| format!("构建 {app_type} 有效配置失败: {e}"))?;

        if matches!(app_type_enum, AppType::Codex) {
            let existing_backup = self
                .db
                .get_live_backup(app_type)
                .await
                .map_err(|e| format!("读取 {app_type} 现有备份失败: {e}"))?;

            if let Some(existing_backup) = existing_backup {
                let existing_value: Value = serde_json::from_str(&existing_backup.original_config)
                    .map_err(|e| format!("解析 {app_type} 现有备份失败: {e}"))?;
                Self::preserve_codex_mcp_servers(&mut effective_settings, &existing_value)?;
            }
        }

        let backup_json = match app_type_enum {
            AppType::Claude => serde_json::to_string(&effective_settings)
                .map_err(|e| format!("序列化 Claude 配置失败: {e}"))?,
            AppType::Codex => serde_json::to_string(&effective_settings)
                .map_err(|e| format!("序列化 Codex 配置失败: {e}"))?,
            AppType::Gemini => {
                // Gemini takeover 仅修改 .env；settings.json（含 mcpServers）保持原样。
                let env_backup = if let Some(env) = effective_settings.get("env") {
                    json!({ "env": env })
                } else {
                    json!({ "env": {} })
                };
                serde_json::to_string(&env_backup)
                    .map_err(|e| format!("序列化 Gemini 配置失败: {e}"))?
            }
            AppType::OpenCode | AppType::OpenClaw => {
                return Err(format!("未知的应用类型: {app_type}"));
            }
        };

        self.db
            .save_live_backup(app_type, &backup_json)
            .await
            .map_err(|e| format!("更新 {app_type} 备份失败: {e}"))?;

        log::info!("已更新 {app_type} Live 备份（热切换）");
        Ok(())
    }

    pub async fn hot_switch_provider(
        &self,
        app_type: &str,
        provider_id: &str,
    ) -> Result<HotSwitchOutcome, String> {
        let _guard = self.switch_locks.lock_for_app(app_type).await;

        let app_type_enum =
            AppType::from_str(app_type).map_err(|_| format!("无效的应用类型: {app_type}"))?;
        let provider = self
            .db
            .get_provider_by_id(provider_id, app_type)
            .map_err(|e| format!("读取供应商失败: {e}"))?
            .ok_or_else(|| format!("供应商不存在: {provider_id}"))?;

        let logical_target_changed =
            crate::settings::get_effective_current_provider(&self.db, &app_type_enum)
                .map_err(|e| format!("读取当前供应商失败: {e}"))?
                .as_deref()
                != Some(provider_id);

        let has_backup = self
            .db
            .get_live_backup(app_type_enum.as_str())
            .await
            .map_err(|e| format!("读取 {app_type} 备份失败: {e}"))?
            .is_some();
        let live_taken_over = self.detect_takeover_in_live_config_for_app(&app_type_enum);
        let should_sync_backup = has_backup || live_taken_over;

        self.db
            .set_current_provider(app_type_enum.as_str(), provider_id)
            .map_err(|e| format!("更新当前供应商失败: {e}"))?;
        crate::settings::set_current_provider(&app_type_enum, Some(provider_id))
            .map_err(|e| format!("更新本地当前供应商失败: {e}"))?;

        if should_sync_backup {
            self.refresh_takeover_state_from_provider_inner(&app_type_enum, &provider)
                .await?;
        }

        if let Some(server) = self.server.read().await.as_ref() {
            server
                .set_active_target(app_type_enum.as_str(), &provider.id, &provider.name)
                .await;
        }

        Ok(HotSwitchOutcome {
            logical_target_changed,
        })
    }

    #[cfg(test)]
    async fn lock_switch_for_test(&self, app_type: &str) -> tokio::sync::OwnedMutexGuard<()> {
        self.switch_locks.lock_for_app(app_type).await
    }

    #[cfg(test)]
    pub(crate) async fn mark_running_for_test(&self) {
        let server = ProxyServer::new(ProxyConfig::default(), self.db.clone(), None);
        *self.server.write().await = Some(server);
    }

    fn preserve_codex_mcp_servers(
        target_settings: &mut Value,
        existing_snapshot: &Value,
    ) -> Result<(), String> {
        let target_obj = target_settings
            .as_object_mut()
            .ok_or_else(|| "Codex 备份必须是 JSON 对象".to_string())?;

        let target_config = target_obj
            .get("config")
            .and_then(|v| v.as_str())
            .unwrap_or("");
        let mut target_doc = if target_config.trim().is_empty() {
            toml_edit::DocumentMut::new()
        } else {
            target_config
                .parse::<toml_edit::DocumentMut>()
                .map_err(|e| format!("解析新的 Codex config.toml 失败: {e}"))?
        };

        let existing_config = existing_snapshot
            .get("config")
            .and_then(|v| v.as_str())
            .unwrap_or("");
        if existing_config.trim().is_empty() {
            target_obj.insert("config".to_string(), json!(target_doc.to_string()));
            return Ok(());
        }

        let existing_doc = existing_config
            .parse::<toml_edit::DocumentMut>()
            .map_err(|e| format!("解析现有 Codex 备份失败: {e}"))?;

        if let Some(existing_mcp_servers) = existing_doc.get("mcp_servers") {
            match target_doc.get_mut("mcp_servers") {
                Some(target_mcp_servers) => {
                    if let (Some(target_table), Some(existing_table)) = (
                        target_mcp_servers.as_table_like_mut(),
                        existing_mcp_servers.as_table_like(),
                    ) {
                        for (server_id, server_item) in existing_table.iter() {
                            if target_table.get(server_id).is_none() {
                                target_table.insert(server_id, server_item.clone());
                            }
                        }
                    } else {
                        log::warn!(
                            "Codex config contains a non-table mcp_servers section; skipping backup MCP merge"
                        );
                    }
                }
                None => {
                    target_doc["mcp_servers"] = existing_mcp_servers.clone();
                }
            }
        }

        target_obj.insert("config".to_string(), json!(target_doc.to_string()));
        Ok(())
    }

    /// 代理模式下切换供应商（热切换，不写 Live）
    pub async fn switch_proxy_target(
        &self,
        app_type: &str,
        provider_id: &str,
    ) -> Result<(), String> {
        let outcome = self.hot_switch_provider(app_type, provider_id).await?;

        if outcome.logical_target_changed {
            log::info!("代理模式：已切换 {app_type} 的目标供应商为 {provider_id}");
        } else {
            log::debug!("代理模式：{app_type} 已对齐到目标供应商 {provider_id}");
        }
        Ok(())
    }

    // ==================== Live 配置读写辅助方法 ====================

    /// 更新 TOML 字符串中的 base_url（委托给 codex_config 共享实现）
    fn update_toml_base_url(toml_str: &str, new_url: &str) -> String {
        crate::codex_config::update_codex_toml_field(toml_str, "base_url", new_url)
            .unwrap_or_else(|_| toml_str.to_string())
    }

    fn read_claude_live(&self) -> Result<Value, String> {
        let path = get_claude_settings_path();
        if !path.exists() {
            return Err("Claude 配置文件不存在".to_string());
        }

        let mut value: Value =
            read_json_file(&path).map_err(|e| format!("读取 Claude 配置失败: {e}"))?;

        if value.is_null() {
            value = json!({});
        }

        if !value.is_object() {
            let kind = match &value {
                Value::Null => "null",
                Value::Bool(_) => "boolean",
                Value::Number(_) => "number",
                Value::String(_) => "string",
                Value::Array(_) => "array",
                Value::Object(_) => "object",
            };
            return Err(format!(
                "Claude 配置文件格式错误：根节点必须是 JSON 对象（当前为 {kind}），路径: {}",
                path.display()
            ));
        }

        Ok(value)
    }

    fn write_claude_live(&self, config: &Value) -> Result<(), String> {
        let path = get_claude_settings_path();
        let settings = crate::services::provider::sanitize_claude_settings_for_live(config);
        write_json_file(&path, &settings).map_err(|e| format!("写入 Claude 配置失败: {e}"))
    }

    fn read_codex_live(&self) -> Result<Value, String> {
        use crate::codex_config::{get_codex_auth_path, get_codex_config_path};

        let auth_path = get_codex_auth_path();
        if !auth_path.exists() {
            return Err("Codex auth.json 不存在".to_string());
        }

        let auth: Value =
            read_json_file(&auth_path).map_err(|e| format!("读取 Codex auth 失败: {e}"))?;

        let config_path = get_codex_config_path();
        let config_str = if config_path.exists() {
            std::fs::read_to_string(&config_path)
                .map_err(|e| format!("读取 Codex config 失败: {e}"))?
        } else {
            String::new()
        };

        Ok(json!({
            "auth": auth,
            "config": config_str
        }))
    }

    fn write_codex_live(&self, config: &Value) -> Result<(), String> {
        use crate::codex_config::{
            get_codex_auth_path, get_codex_config_path, write_codex_live_atomic,
        };

        let auth = config.get("auth");
        let config_str = config.get("config").and_then(|v| v.as_str());

        match (auth, config_str) {
            (Some(auth), Some(cfg)) => write_codex_live_atomic(auth, Some(cfg))
                .map_err(|e| format!("写入 Codex 配置失败: {e}"))?,
            (Some(auth), None) => {
                let auth_path = get_codex_auth_path();
                write_json_file(&auth_path, auth)
                    .map_err(|e| format!("写入 Codex auth 失败: {e}"))?;
            }
            (None, Some(cfg)) => {
                let config_path = get_codex_config_path();
                crate::config::write_text_file(&config_path, cfg)
                    .map_err(|e| format!("写入 Codex config 失败: {e}"))?;
            }
            (None, None) => {}
        }

        Ok(())
    }

    fn read_gemini_live(&self) -> Result<Value, String> {
        use crate::gemini_config::{env_to_json, get_gemini_env_path, read_gemini_env};

        let env_path = get_gemini_env_path();
        if !env_path.exists() {
            return Err("Gemini .env 文件不存在".to_string());
        }

        let env_map = read_gemini_env().map_err(|e| format!("读取 Gemini env 失败: {e}"))?;
        Ok(env_to_json(&env_map))
    }

    fn write_gemini_live(&self, config: &Value) -> Result<(), String> {
        use crate::gemini_config::{json_to_env, write_gemini_env_atomic};

        let env_map = json_to_env(config).map_err(|e| format!("转换 Gemini 配置失败: {e}"))?;
        write_gemini_env_atomic(&env_map).map_err(|e| format!("写入 Gemini env 失败: {e}"))?;
        Ok(())
    }

    // ==================== 原有方法 ====================

    /// 获取服务器状态
    pub async fn get_status(&self) -> Result<ProxyStatus, String> {
        if let Some(server) = self.server.read().await.as_ref() {
            Ok(server.get_status().await)
        } else {
            // 服务器未运行时返回默认状态
            Ok(ProxyStatus {
                running: false,
                ..Default::default()
            })
        }
    }

    /// 获取代理配置
    pub async fn get_config(&self) -> Result<ProxyConfig, String> {
        self.db
            .get_proxy_config()
            .await
            .map_err(|e| format!("获取代理配置失败: {e}"))
    }

    /// 更新代理配置
    pub async fn update_config(&self, config: &ProxyConfig) -> Result<(), String> {
        // 记录旧配置用于判定是否需要重启
        let previous = self
            .db
            .get_proxy_config()
            .await
            .map_err(|e| format!("获取代理配置失败: {e}"))?;

        // 保存到数据库（保持 live_takeover_active 状态不变）
        let mut new_config = config.clone();
        new_config.live_takeover_active = previous.live_takeover_active;

        self.db
            .update_proxy_config(new_config.clone())
            .await
            .map_err(|e| format!("保存代理配置失败: {e}"))?;

        // 检查服务器当前状态
        let mut server_guard = self.server.write().await;
        if server_guard.is_none() {
            return Ok(());
        }

        // 判断是否需要重启（地址或端口变更）
        let require_restart = new_config.listen_address != previous.listen_address
            || new_config.listen_port != previous.listen_port;

        if require_restart {
            if let Some(server) = server_guard.take() {
                server
                    .stop()
                    .await
                    .map_err(|e| format!("重启前停止代理服务器失败: {e}"))?;
            }

            let app_handle = self.app_handle.read().await.clone();
            let new_server = ProxyServer::new(new_config, self.db.clone(), app_handle);
            new_server
                .start()
                .await
                .map_err(|e| format!("重启代理服务器失败: {e}"))?;

            *server_guard = Some(new_server);
            log::info!("代理配置已更新，服务器已自动重启应用最新配置");

            // 如果当前存在任意 app 的 Live 接管，需要同步更新 Live 中的代理地址（否则客户端仍指向旧端口）
            drop(server_guard);
            if let Ok(takeover) = self.get_takeover_status().await {
                let mut updated_any = false;

                if takeover.claude {
                    self.takeover_live_config_best_effort(&AppType::Claude)
                        .await?;
                    updated_any = true;
                }
                if takeover.codex {
                    self.takeover_live_config_best_effort(&AppType::Codex)
                        .await?;
                    updated_any = true;
                }
                if takeover.gemini {
                    self.takeover_live_config_best_effort(&AppType::Gemini)
                        .await?;
                    updated_any = true;
                }

                if updated_any {
                    log::info!("已同步更新 Live 配置中的代理地址");
                }
            }

            return Ok(());
        } else if let Some(server) = server_guard.as_ref() {
            server.apply_runtime_config(&new_config).await;
            log::info!("代理配置已实时应用，无需重启代理服务器");
        }

        Ok(())
    }

    /// 检查服务器是否正在运行
    pub async fn is_running(&self) -> bool {
        self.server.read().await.is_some()
    }

    /// 热更新熔断器配置
    ///
    /// 如果代理服务器正在运行，将新配置应用到所有已创建的熔断器实例
    pub async fn update_circuit_breaker_configs(
        &self,
        config: crate::proxy::CircuitBreakerConfig,
    ) -> Result<(), String> {
        if let Some(server) = self.server.read().await.as_ref() {
            server.update_circuit_breaker_configs(config).await;
            log::info!("已热更新运行中的熔断器配置");
        } else {
            log::debug!("代理服务器未运行，熔断器配置将在下次启动时生效");
        }
        Ok(())
    }

    /// 重置指定 Provider 的熔断器
    ///
    /// 如果代理服务器正在运行，立即重置内存中的熔断器状态
    pub async fn reset_provider_circuit_breaker(
        &self,
        provider_id: &str,
        app_type: &str,
    ) -> Result<(), String> {
        if let Some(server) = self.server.read().await.as_ref() {
            server
                .reset_provider_circuit_breaker(provider_id, app_type)
                .await;
            log::info!("已重置 Provider {provider_id} (app: {app_type}) 的熔断器");
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::provider::ProviderMeta;
    use serial_test::serial;
    use std::env;
    use tempfile::TempDir;

    struct TempHome {
        #[allow(dead_code)]
        dir: TempDir,
        original_home: Option<String>,
        original_userprofile: Option<String>,
        original_test_home: Option<String>,
    }

    impl TempHome {
        fn new() -> Self {
            let dir = TempDir::new().expect("failed to create temp home");
            let original_home = env::var("HOME").ok();
            let original_userprofile = env::var("USERPROFILE").ok();
            let original_test_home = env::var("CC_SWITCH_TEST_HOME").ok();

            env::set_var("HOME", dir.path());
            env::set_var("USERPROFILE", dir.path());
            env::set_var("CC_SWITCH_TEST_HOME", dir.path());

            Self {
                dir,
                original_home,
                original_userprofile,
                original_test_home,
            }
        }
    }

    impl Drop for TempHome {
        fn drop(&mut self) {
            match &self.original_home {
                Some(value) => env::set_var("HOME", value),
                None => env::remove_var("HOME"),
            }

            match &self.original_userprofile {
                Some(value) => env::set_var("USERPROFILE", value),
                None => env::remove_var("USERPROFILE"),
            }

            match &self.original_test_home {
                Some(value) => env::set_var("CC_SWITCH_TEST_HOME", value),
                None => env::remove_var("CC_SWITCH_TEST_HOME"),
            }
        }
    }

    #[test]
    fn update_toml_base_url_updates_active_model_provider_base_url() {
        let input = r#"
model_provider = "any"
model = "gpt-5.1-codex"
disable_response_storage = true

[model_providers.any]
name = "any"
base_url = "https://anyrouter.top/v1"
wire_api = "responses"
requires_openai_auth = true
"#;

        let new_url = "http://127.0.0.1:5000/v1";
        let output = ProxyService::update_toml_base_url(input, new_url);

        let parsed: toml::Value =
            toml::from_str(&output).expect("updated config should be valid TOML");

        let base_url = parsed
            .get("model_providers")
            .and_then(|v| v.get("any"))
            .and_then(|v| v.get("base_url"))
            .and_then(|v| v.as_str())
            .expect("model_providers.any.base_url should exist");

        assert_eq!(base_url, new_url);
        assert!(
            parsed.get("base_url").is_none(),
            "should not write top-level base_url"
        );

        let wire_api = parsed
            .get("model_providers")
            .and_then(|v| v.get("any"))
            .and_then(|v| v.get("wire_api"))
            .and_then(|v| v.as_str())
            .expect("model_providers.any.wire_api should exist");
        assert_eq!(wire_api, "responses");
    }

    #[test]
    fn update_toml_base_url_falls_back_to_top_level_base_url() {
        let input = r#"
model = "gpt-5.1-codex"
"#;

        let new_url = "http://127.0.0.1:5000/v1";
        let output = ProxyService::update_toml_base_url(input, new_url);

        let parsed: toml::Value =
            toml::from_str(&output).expect("updated config should be valid TOML");

        let base_url = parsed
            .get("base_url")
            .and_then(|v| v.as_str())
            .expect("base_url should exist");

        assert_eq!(base_url, new_url);
    }

    #[tokio::test]
    #[serial]
    async fn sync_claude_token_does_not_add_anthropic_api_key() {
        let _home = TempHome::new();
        crate::settings::reload_settings().expect("reload settings");

        let db = Arc::new(Database::memory().expect("init db"));
        let service = ProxyService::new(db.clone());

        let provider = Provider::with_id(
            "p1".to_string(),
            "P1".to_string(),
            json!({
                "env": {
                    "ANTHROPIC_BASE_URL": "https://api.anthropic.com",
                    "ANTHROPIC_AUTH_TOKEN": "stale"
                }
            }),
            None,
        );
        db.save_provider("claude", &provider)
            .expect("save provider");
        db.set_current_provider("claude", "p1")
            .expect("set current provider");

        let live_config = json!({
            "env": {
                "ANTHROPIC_AUTH_TOKEN": "fresh"
            }
        });

        service
            .sync_live_config_to_provider(&AppType::Claude, &live_config)
            .await
            .expect("sync");

        let updated = db
            .get_provider_by_id("p1", "claude")
            .expect("get provider")
            .expect("provider exists");
        let env = updated
            .settings_config
            .get("env")
            .and_then(|v| v.as_object())
            .expect("env object");

        assert_eq!(
            env.get("ANTHROPIC_AUTH_TOKEN").and_then(|v| v.as_str()),
            Some("fresh")
        );
        assert!(
            !env.contains_key("ANTHROPIC_API_KEY"),
            "should not add ANTHROPIC_API_KEY when absent"
        );
    }

    #[tokio::test]
    #[serial]
    async fn sync_claude_token_respects_existing_api_key_field() {
        let _home = TempHome::new();
        crate::settings::reload_settings().expect("reload settings");

        let db = Arc::new(Database::memory().expect("init db"));
        let service = ProxyService::new(db.clone());

        let provider = Provider::with_id(
            "p1".to_string(),
            "P1".to_string(),
            json!({
                "env": {
                    "ANTHROPIC_BASE_URL": "https://api.anthropic.com",
                    "ANTHROPIC_API_KEY": "stale"
                }
            }),
            None,
        );
        db.save_provider("claude", &provider)
            .expect("save provider");
        db.set_current_provider("claude", "p1")
            .expect("set current provider");

        let live_config = json!({
            "env": {
                "ANTHROPIC_AUTH_TOKEN": "fresh"
            }
        });

        service
            .sync_live_config_to_provider(&AppType::Claude, &live_config)
            .await
            .expect("sync");

        let updated = db
            .get_provider_by_id("p1", "claude")
            .expect("get provider")
            .expect("provider exists");
        let env = updated
            .settings_config
            .get("env")
            .and_then(|v| v.as_object())
            .expect("env object");

        assert_eq!(
            env.get("ANTHROPIC_API_KEY").and_then(|v| v.as_str()),
            Some("fresh")
        );
        assert!(
            !env.contains_key("ANTHROPIC_AUTH_TOKEN"),
            "should not add ANTHROPIC_AUTH_TOKEN when absent"
        );
    }

    #[tokio::test]
    #[serial]
    async fn switch_proxy_target_updates_live_backup_when_taken_over() {
        let _home = TempHome::new();
        crate::settings::reload_settings().expect("reload settings");

        let db = Arc::new(Database::memory().expect("init db"));
        let service = ProxyService::new(db.clone());

        let provider_a = Provider::with_id(
            "a".to_string(),
            "A".to_string(),
            json!({
                "env": {
                    "ANTHROPIC_API_KEY": "a-key"
                }
            }),
            None,
        );
        let provider_b = Provider::with_id(
            "b".to_string(),
            "B".to_string(),
            json!({
                "env": {
                    "ANTHROPIC_API_KEY": "b-key"
                }
            }),
            None,
        );
        db.save_provider("claude", &provider_a)
            .expect("save provider a");
        db.save_provider("claude", &provider_b)
            .expect("save provider b");
        db.set_current_provider("claude", "a")
            .expect("set current provider");

        // 模拟"已接管"状态：存在 Live 备份（内容不重要，会被热切换更新）
        db.save_live_backup("claude", "{\"env\":{}}")
            .await
            .expect("seed live backup");

        service
            .switch_proxy_target("claude", "b")
            .await
            .expect("switch proxy target");

        // 断言：本地 settings 的 current provider 已同步
        assert_eq!(
            crate::settings::get_current_provider(&AppType::Claude).as_deref(),
            Some("b")
        );

        // 断言：Live 备份已更新为目标供应商配置（用于 stop_with_restore 恢复）
        let backup = db
            .get_live_backup("claude")
            .await
            .expect("get live backup")
            .expect("backup exists");
        let expected = serde_json::to_string(&provider_b.settings_config).expect("serialize");
        assert_eq!(backup.original_config, expected);
    }

    #[tokio::test]
    #[serial]
    async fn hot_switch_provider_updates_claude_live_while_preserving_takeover_fields() {
        let _home = TempHome::new();
        crate::settings::reload_settings().expect("reload settings");

        let db = Arc::new(Database::memory().expect("init db"));
        let service = ProxyService::new(db.clone());

        let provider_a = Provider::with_id(
            "a".to_string(),
            "A".to_string(),
            json!({
                "env": {
                    "ANTHROPIC_API_KEY": "a-key",
                    "ANTHROPIC_BASE_URL": "https://api.a.example",
                    "ANTHROPIC_MODEL": "claude-old"
                },
                "permissions": { "allow": ["Bash"] }
            }),
            None,
        );
        let provider_b = Provider::with_id(
            "b".to_string(),
            "B".to_string(),
            json!({
                "env": {
                    "OPENROUTER_API_KEY": "b-key",
                    "ANTHROPIC_BASE_URL": "https://openrouter.example/api",
                    "ANTHROPIC_MODEL": "claude-new"
                }
            }),
            None,
        );

        db.save_provider("claude", &provider_a)
            .expect("save provider a");
        db.save_provider("claude", &provider_b)
            .expect("save provider b");
        db.set_current_provider("claude", "a")
            .expect("set current provider");
        crate::settings::set_current_provider(&AppType::Claude, Some("a"))
            .expect("set local current provider");
        db.save_live_backup(
            "claude",
            &serde_json::to_string(&provider_a.settings_config).expect("serialize provider a"),
        )
        .await
        .expect("seed live backup");
        service
            .write_claude_live(&json!({
                "env": {
                    "ANTHROPIC_BASE_URL": "http://127.0.0.1:15721",
                    "ANTHROPIC_API_KEY": PROXY_TOKEN_PLACEHOLDER,
                    "ANTHROPIC_MODEL": "stale-model"
                },
                "permissions": { "allow": ["Bash"] }
            }))
            .expect("seed taken-over live file");

        service
            .hot_switch_provider("claude", "b")
            .await
            .expect("hot switch provider");

        let live = service.read_claude_live().expect("read live config");
        assert_eq!(
            live.get("permissions"),
            provider_a.settings_config.get("permissions"),
            "live-only Claude settings should remain intact during takeover refresh"
        );
        assert_eq!(
            live.get("env")
                .and_then(|env| env.get("OPENROUTER_API_KEY"))
                .and_then(|v| v.as_str()),
            Some(PROXY_TOKEN_PLACEHOLDER),
            "takeover token placeholder should follow the updated Claude credential family"
        );
        assert_eq!(
            live.get("env")
                .and_then(|env| env.get("ANTHROPIC_BASE_URL"))
                .and_then(|v| v.as_str()),
            Some("http://127.0.0.1:15721"),
            "takeover proxy URL should remain active"
        );
        assert_eq!(
            live.get("env")
                .and_then(|env| env.get("ANTHROPIC_MODEL"))
                .and_then(|v| v.as_str()),
            Some("claude-new"),
            "Claude takeover live config should refresh to the current provider model"
        );
        assert!(
            live.get("env")
                .and_then(|env| env.get("ANTHROPIC_API_KEY"))
                .is_none(),
            "stale Claude credential families should be removed from live"
        );

        let backup = db
            .get_live_backup("claude")
            .await
            .expect("get live backup")
            .expect("backup exists");
        let backup_value: Value =
            serde_json::from_str(&backup.original_config).expect("parse backup json");
        assert_eq!(
            backup_value.get("permissions"),
            provider_a.settings_config.get("permissions"),
            "backup should keep the existing live-only Claude settings"
        );
        assert_eq!(
            backup_value
                .get("env")
                .and_then(|env| env.get("OPENROUTER_API_KEY"))
                .and_then(|v| v.as_str()),
            Some("b-key"),
            "backup should store the updated Claude provider credential"
        );
        assert!(
            backup_value
                .get("env")
                .and_then(|env| env.get("ANTHROPIC_API_KEY"))
                .is_none(),
            "backup should not retain stale Claude credential families"
        );
    }

    #[tokio::test]
    #[serial]
    async fn hot_switch_provider_keeps_current_claude_live_authority_over_richer_backup() {
        let _home = TempHome::new();
        crate::settings::reload_settings().expect("reload settings");

        let db = Arc::new(Database::memory().expect("init db"));
        let service = ProxyService::new(db.clone());

        let provider_a = Provider::with_id(
            "a".to_string(),
            "A".to_string(),
            json!({
                "env": {
                    "ANTHROPIC_API_KEY": "a-key",
                    "ANTHROPIC_BASE_URL": "https://api.a.example",
                    "ANTHROPIC_MODEL": "claude-old"
                },
                "permissions": { "allow": ["Bash"] }
            }),
            None,
        );
        let provider_b = Provider::with_id(
            "b".to_string(),
            "B".to_string(),
            json!({
                "env": {
                    "OPENROUTER_API_KEY": "b-key",
                    "ANTHROPIC_BASE_URL": "https://openrouter.example/api",
                    "ANTHROPIC_MODEL": "claude-new"
                }
            }),
            None,
        );

        db.save_provider("claude", &provider_a)
            .expect("save provider a");
        db.save_provider("claude", &provider_b)
            .expect("save provider b");
        db.set_current_provider("claude", "a")
            .expect("set current provider");
        crate::settings::set_current_provider(&AppType::Claude, Some("a"))
            .expect("set local current provider");
        db.save_live_backup(
            "claude",
            &serde_json::to_string(&provider_a.settings_config).expect("serialize provider a"),
        )
        .await
        .expect("seed live backup");
        service
            .write_claude_live(&json!({
                "env": {
                    "ANTHROPIC_BASE_URL": "http://127.0.0.1:15721",
                    "ANTHROPIC_API_KEY": PROXY_TOKEN_PLACEHOLDER,
                    "ANTHROPIC_MODEL": "stale-model"
                }
            }))
            .expect("seed partial taken-over live file");

        service
            .hot_switch_provider("claude", "b")
            .await
            .expect("hot switch provider");

        let live = service.read_claude_live().expect("read live config");
        assert!(
            live.get("permissions").is_none(),
            "live refresh should keep the current Claude live shape instead of resurrecting richer backup-only settings"
        );

        let backup = db
            .get_live_backup("claude")
            .await
            .expect("get live backup")
            .expect("backup exists");
        let backup_value: Value =
            serde_json::from_str(&backup.original_config).expect("parse backup json");
        assert!(
            backup_value.get("permissions").is_none(),
            "backup refresh should follow the current Claude live shape so restore does not resurrect stale backup-only settings"
        );
    }

    #[tokio::test]
    #[serial]
    async fn hot_switch_provider_updates_codex_live_while_preserving_takeover_fields() {
        let _home = TempHome::new();
        crate::settings::reload_settings().expect("reload settings");

        let db = Arc::new(Database::memory().expect("init db"));
        let service = ProxyService::new(db.clone());

        let provider_a = Provider::with_id(
            "a".to_string(),
            "A".to_string(),
            json!({
                "auth": {
                    "OPENAI_API_KEY": "a-key"
                },
                "config": r#"model_provider = "any"
model = "gpt-old"
model_reasoning_effort = "low"

[model_providers.any]
base_url = "https://api.a.example/v1"
wire_api = "responses"
"#
            }),
            None,
        );
        let provider_b = Provider::with_id(
            "b".to_string(),
            "B".to_string(),
            json!({
                "auth": {
                    "OPENAI_API_KEY": "b-key"
                },
                "config": r#"model_provider = "any"
model = "gpt-new"

[model_providers.any]
base_url = "https://api.b.example/v1"
wire_api = "responses"
"#
            }),
            None,
        );

        db.save_provider("codex", &provider_a)
            .expect("save provider a");
        db.save_provider("codex", &provider_b)
            .expect("save provider b");
        db.set_current_provider("codex", "a")
            .expect("set current provider");
        crate::settings::set_current_provider(&AppType::Codex, Some("a"))
            .expect("set local current provider");
        db.save_live_backup(
            "codex",
            &serde_json::to_string(&json!({
                "auth": {
                    "OPENAI_API_KEY": "a-key"
                },
                "config": r#"model_provider = "any"
model = "gpt-old"
model_reasoning_effort = "low"

[model_providers.any]
base_url = "https://api.a.example/v1"
wire_api = "responses"

[mcp_servers.echo]
command = "npx"
args = ["echo-server"]
"#
            }))
            .expect("serialize provider a backup"),
        )
        .await
        .expect("seed live backup");
        service
            .write_codex_live(&json!({
                "auth": {
                    "OPENAI_API_KEY": PROXY_TOKEN_PLACEHOLDER
                },
                "config": r#"model_provider = "any"
model = "stale-model"
model_reasoning_effort = "minimal"

[model_providers.any]
base_url = "http://127.0.0.1:15721/v1"
wire_api = "responses"

[mcp_servers.echo]
command = "npx"
args = ["echo-server"]
"#
            }))
            .expect("seed taken-over live file");

        service
            .hot_switch_provider("codex", "b")
            .await
            .expect("hot switch provider");

        let live = service.read_codex_live().expect("read live config");
        assert_eq!(
            live.get("auth")
                .and_then(|auth| auth.get("OPENAI_API_KEY"))
                .and_then(|v| v.as_str()),
            Some(PROXY_TOKEN_PLACEHOLDER),
            "takeover token placeholder should be preserved"
        );

        let config = live
            .get("config")
            .and_then(|v| v.as_str())
            .expect("config string");
        let parsed: toml::Value = toml::from_str(config).expect("parse live codex config");
        assert_eq!(
            parsed.get("model").and_then(|v| v.as_str()),
            Some("gpt-new"),
            "Codex takeover live config should refresh to the current provider model"
        );
        assert!(
            parsed.get("model_reasoning_effort").is_none(),
            "provider-owned Codex fields removed by the new provider should not survive in live"
        );
        assert_eq!(
            parsed
                .get("model_providers")
                .and_then(|v| v.get("any"))
                .and_then(|v| v.get("base_url"))
                .and_then(|v| v.as_str()),
            Some("http://127.0.0.1:15721/v1"),
            "takeover proxy URL should remain active"
        );
        assert!(
            parsed
                .get("mcp_servers")
                .and_then(|v| v.get("echo"))
                .is_some(),
            "Codex takeover live config should preserve MCP servers from the current live config"
        );

        let backup = db
            .get_live_backup("codex")
            .await
            .expect("get live backup")
            .expect("backup exists");
        let backup_value: Value =
            serde_json::from_str(&backup.original_config).expect("parse backup json");
        let backup_config = backup_value
            .get("config")
            .and_then(|v| v.as_str())
            .expect("backup config string");
        let parsed_backup: toml::Value =
            toml::from_str(backup_config).expect("parse backup codex config");
        assert_eq!(
            parsed_backup.get("model").and_then(|v| v.as_str()),
            Some("gpt-new"),
            "backup should refresh the current Codex model"
        );
        assert!(
            parsed_backup.get("model_reasoning_effort").is_none(),
            "backup should drop provider-owned Codex fields removed by the new provider"
        );
        assert_eq!(
            parsed_backup
                .get("model_providers")
                .and_then(|v| v.get("any"))
                .and_then(|v| v.get("base_url"))
                .and_then(|v| v.as_str()),
            Some("https://api.b.example/v1"),
            "backup should keep the provider base URL instead of the proxy URL"
        );
        assert!(
            parsed_backup
                .get("mcp_servers")
                .and_then(|v| v.get("echo"))
                .is_some(),
            "backup should preserve existing Codex MCP servers"
        );
    }

    #[tokio::test]
    #[serial]
    async fn hot_switch_provider_updates_codex_live_when_model_provider_alias_changes() {
        let _home = TempHome::new();
        crate::settings::reload_settings().expect("reload settings");

        let db = Arc::new(Database::memory().expect("init db"));
        let service = ProxyService::new(db.clone());

        let provider_a = Provider::with_id(
            "a".to_string(),
            "A".to_string(),
            json!({
                "auth": {
                    "OPENAI_API_KEY": "a-key"
                },
                "config": r#"model_provider = "openai"
model = "gpt-old"

[model_providers.openai]
base_url = "https://api.a.example/v1"
wire_api = "responses"
"#
            }),
            None,
        );
        let provider_b = Provider::with_id(
            "b".to_string(),
            "B".to_string(),
            json!({
                "auth": {
                    "OPENAI_API_KEY": "b-key"
                },
                "config": r#"model_provider = "azure"
model = "gpt-new"

[model_providers.azure]
base_url = "https://api.azure.example/v1"
wire_api = "responses"
"#
            }),
            None,
        );

        db.save_provider("codex", &provider_a)
            .expect("save provider a");
        db.save_provider("codex", &provider_b)
            .expect("save provider b");
        db.set_current_provider("codex", "a")
            .expect("set current provider");
        crate::settings::set_current_provider(&AppType::Codex, Some("a"))
            .expect("set local current provider");
        db.save_live_backup(
            "codex",
            &serde_json::to_string(&json!({
                "auth": {
                    "OPENAI_API_KEY": "a-key"
                },
                "config": r#"model_provider = "openai"
model = "gpt-old"

[model_providers.openai]
base_url = "https://api.a.example/v1"
wire_api = "responses"

[mcp_servers.echo]
command = "npx"
args = ["echo-server"]
"#
            }))
            .expect("serialize provider a backup"),
        )
        .await
        .expect("seed live backup");
        service
            .write_codex_live(&json!({
                "auth": {
                    "OPENAI_API_KEY": PROXY_TOKEN_PLACEHOLDER
                },
                "config": r#"model_provider = "openai"
model = "stale-model"

[model_providers.openai]
base_url = "http://127.0.0.1:15721/v1"
wire_api = "responses"

[mcp_servers.echo]
command = "npx"
args = ["echo-server"]
"#
            }))
            .expect("seed taken-over live file");

        service
            .hot_switch_provider("codex", "b")
            .await
            .expect("hot switch provider");

        let live = service.read_codex_live().expect("read live config");
        let live_config = live
            .get("config")
            .and_then(|v| v.as_str())
            .expect("live config string");
        let parsed_live: toml::Value =
            toml::from_str(live_config).expect("parse live codex config");
        assert_eq!(
            parsed_live.get("model_provider").and_then(|v| v.as_str()),
            Some("azure"),
            "live should refresh Codex model_provider when switching aliases"
        );
        assert_eq!(
            parsed_live.get("model").and_then(|v| v.as_str()),
            Some("gpt-new"),
            "live should refresh the current Codex model when switching aliases"
        );
        assert!(
            parsed_live
                .get("model_providers")
                .and_then(|v| v.get("openai"))
                .is_none(),
            "live should remove the stale provider-owned Codex alias table"
        );
        assert_eq!(
            parsed_live
                .get("model_providers")
                .and_then(|v| v.get("azure"))
                .and_then(|v| v.get("base_url"))
                .and_then(|v| v.as_str()),
            Some("http://127.0.0.1:15721/v1"),
            "live should rewrite the new active Codex alias to the proxy URL"
        );
        assert!(
            parsed_live
                .get("mcp_servers")
                .and_then(|v| v.get("echo"))
                .is_some(),
            "live should preserve existing Codex MCP servers when switching aliases"
        );

        let backup = db
            .get_live_backup("codex")
            .await
            .expect("get live backup")
            .expect("backup exists");
        let backup_value: Value =
            serde_json::from_str(&backup.original_config).expect("parse backup json");
        let backup_config = backup_value
            .get("config")
            .and_then(|v| v.as_str())
            .expect("backup config string");
        let parsed_backup: toml::Value =
            toml::from_str(backup_config).expect("parse backup codex config");
        assert_eq!(
            parsed_backup.get("model_provider").and_then(|v| v.as_str()),
            Some("azure"),
            "backup should refresh Codex model_provider when switching aliases"
        );
        assert!(
            parsed_backup
                .get("model_providers")
                .and_then(|v| v.get("openai"))
                .is_none(),
            "backup should remove the stale provider-owned Codex alias table"
        );
        assert_eq!(
            parsed_backup
                .get("model_providers")
                .and_then(|v| v.get("azure"))
                .and_then(|v| v.get("base_url"))
                .and_then(|v| v.as_str()),
            Some("https://api.azure.example/v1"),
            "backup should keep the provider URL for the new active Codex alias"
        );
    }

    #[tokio::test]
    #[serial]
    async fn hot_switch_provider_refreshes_codex_backup_from_current_live_when_live_only_sections_exist(
    ) {
        let _home = TempHome::new();
        crate::settings::reload_settings().expect("reload settings");

        let db = Arc::new(Database::memory().expect("init db"));
        let service = ProxyService::new(db.clone());

        let provider_a = Provider::with_id(
            "a".to_string(),
            "A".to_string(),
            json!({
                "auth": {
                    "OPENAI_API_KEY": "a-key"
                },
                "config": r#"model_provider = "any"
model = "gpt-old"

[model_providers.any]
base_url = "https://api.a.example/v1"
wire_api = "responses"
"#
            }),
            None,
        );
        let provider_b = Provider::with_id(
            "b".to_string(),
            "B".to_string(),
            json!({
                "auth": {
                    "OPENAI_API_KEY": "b-key"
                },
                "config": r#"model_provider = "any"
model = "gpt-new"

[model_providers.any]
base_url = "https://api.b.example/v1"
wire_api = "responses"
"#
            }),
            None,
        );

        db.save_provider("codex", &provider_a)
            .expect("save provider a");
        db.save_provider("codex", &provider_b)
            .expect("save provider b");
        db.set_current_provider("codex", "a")
            .expect("set current provider");
        crate::settings::set_current_provider(&AppType::Codex, Some("a"))
            .expect("set local current provider");
        db.save_live_backup(
            "codex",
            &serde_json::to_string(&json!({
                "auth": {
                    "OPENAI_API_KEY": "a-key"
                },
                "config": r#"model_provider = "any"
model = "gpt-old"

[model_providers.any]
base_url = "https://api.a.example/v1"
wire_api = "responses"
"#
            }))
            .expect("serialize partial backup"),
        )
        .await
        .expect("seed partial live backup");
        service
            .write_codex_live(&json!({
                "auth": {
                    "OPENAI_API_KEY": PROXY_TOKEN_PLACEHOLDER
                },
                "config": r#"model_provider = "any"
model = "stale-model"

[model_providers.any]
base_url = "http://127.0.0.1:15721/v1"
wire_api = "responses"

[mcp_servers.echo]
command = "npx"
args = ["echo-server"]
"#
            }))
            .expect("seed taken-over live file");

        service
            .hot_switch_provider("codex", "b")
            .await
            .expect("hot switch provider");

        let backup = db
            .get_live_backup("codex")
            .await
            .expect("get live backup")
            .expect("backup exists");
        let backup_value: Value =
            serde_json::from_str(&backup.original_config).expect("parse backup json");
        let backup_config = backup_value
            .get("config")
            .and_then(|v| v.as_str())
            .expect("backup config string");
        let parsed_backup: toml::Value =
            toml::from_str(backup_config).expect("parse backup codex config");

        assert!(
            parsed_backup
                .get("mcp_servers")
                .and_then(|v| v.get("echo"))
                .is_some(),
            "backup refresh should preserve current live-only Codex sections so restore stays in sync"
        );
    }

    #[tokio::test]
    #[serial]
    async fn hot_switch_provider_tolerates_malformed_codex_live_config_during_takeover_refresh() {
        let _home = TempHome::new();
        crate::settings::reload_settings().expect("reload settings");

        let db = Arc::new(Database::memory().expect("init db"));
        let service = ProxyService::new(db.clone());

        let provider_a = Provider::with_id(
            "a".to_string(),
            "A".to_string(),
            json!({
                "auth": {
                    "OPENAI_API_KEY": "a-key"
                },
                "config": r#"model_provider = "any"
model = "gpt-old"

[model_providers.any]
base_url = "https://api.a.example/v1"
wire_api = "responses"
"#
            }),
            None,
        );
        let provider_b = Provider::with_id(
            "b".to_string(),
            "B".to_string(),
            json!({
                "auth": {
                    "OPENAI_API_KEY": "b-key"
                },
                "config": r#"model_provider = "any"
model = "gpt-new"

[model_providers.any]
base_url = "https://api.b.example/v1"
wire_api = "responses"
"#
            }),
            None,
        );

        db.save_provider("codex", &provider_a)
            .expect("save provider a");
        db.save_provider("codex", &provider_b)
            .expect("save provider b");
        db.set_current_provider("codex", "a")
            .expect("set current provider");
        crate::settings::set_current_provider(&AppType::Codex, Some("a"))
            .expect("set local current provider");
        db.save_live_backup(
            "codex",
            &serde_json::to_string(&json!({
                "auth": {
                    "OPENAI_API_KEY": "a-key"
                },
                "config": r#"model_provider = "any"
model = "gpt-old"

[model_providers.any]
base_url = "https://api.a.example/v1"
wire_api = "responses"

[mcp_servers.echo]
command = "npx"
args = ["echo-server"]
"#
            }))
            .expect("serialize provider a backup"),
        )
        .await
        .expect("seed live backup");
        write_json_file(
            &crate::codex_config::get_codex_auth_path(),
            &json!({
                "OPENAI_API_KEY": PROXY_TOKEN_PLACEHOLDER
            }),
        )
        .expect("seed codex auth file");
        std::fs::write(
            crate::codex_config::get_codex_config_path(),
            "[mcp_servers.echo]\ncommand = ",
        )
        .expect("seed malformed codex config");

        service
            .hot_switch_provider("codex", "b")
            .await
            .expect("hot switch should ignore malformed Codex live config");

        let live = service.read_codex_live().expect("read live config");
        assert_eq!(
            live.get("auth")
                .and_then(|auth| auth.get("OPENAI_API_KEY"))
                .and_then(|v| v.as_str()),
            Some(PROXY_TOKEN_PLACEHOLDER),
            "takeover placeholder should remain intact after fallback refresh"
        );

        let live_config = live
            .get("config")
            .and_then(|v| v.as_str())
            .expect("live config string");
        let parsed_live: toml::Value =
            toml::from_str(live_config).expect("parse recovered live codex config");
        assert_eq!(
            parsed_live.get("model").and_then(|v| v.as_str()),
            Some("gpt-new"),
            "live config should still refresh to the new provider after fallback"
        );
        assert!(
            parsed_live
                .get("mcp_servers")
                .and_then(|v| v.get("echo"))
                .is_some(),
            "live config should fall back to the backup baseline when malformed live TOML cannot be patched"
        );
    }

    #[tokio::test]
    #[serial]
    async fn takeover_live_config_strict_rebuilds_claude_live_from_current_provider() {
        let _home = TempHome::new();
        crate::settings::reload_settings().expect("reload settings");

        let db = Arc::new(Database::memory().expect("init db"));
        let service = ProxyService::new(db.clone());

        let provider = Provider::with_id(
            "current".to_string(),
            "Current".to_string(),
            json!({
                "env": {
                    "ANTHROPIC_API_KEY": "provider-key",
                    "ANTHROPIC_BASE_URL": "https://api.current.example",
                    "ANTHROPIC_MODEL": "claude-current"
                },
                "permissions": { "allow": ["Read"] }
            }),
            None,
        );

        db.save_provider("claude", &provider)
            .expect("save current provider");
        db.set_current_provider("claude", "current")
            .expect("set current provider");
        crate::settings::set_current_provider(&AppType::Claude, Some("current"))
            .expect("set local current provider");

        service
            .write_claude_live(&json!({
                "env": {
                    "ANTHROPIC_API_KEY": "live-key",
                    "ANTHROPIC_BASE_URL": "https://live.example"
                },
                "permissions": { "allow": ["Bash"] }
            }))
            .expect("seed live config");

        service
            .takeover_live_config_strict(&AppType::Claude)
            .await
            .expect("take over claude live config");

        let live = service.read_claude_live().expect("read live config");
        assert_eq!(
            live.get("env")
                .and_then(|env| env.get("ANTHROPIC_API_KEY"))
                .and_then(|v| v.as_str()),
            Some(PROXY_TOKEN_PLACEHOLDER),
            "takeover should still mask provider credentials"
        );
        assert!(
            live.get("env")
                .and_then(|env| env.get("ANTHROPIC_BASE_URL"))
                .and_then(|v| v.as_str())
                .is_some_and(ProxyService::is_local_proxy_url),
            "takeover should still rewrite Claude base URL to the local proxy"
        );
        assert_eq!(
            live.get("env")
                .and_then(|env| env.get("ANTHROPIC_MODEL"))
                .and_then(|v| v.as_str()),
            Some("claude-current"),
            "takeover should write the current provider model into Claude live config"
        );
        assert_eq!(
            live.get("permissions"),
            provider.settings_config.get("permissions"),
            "takeover should rebuild live config from the current provider"
        );
    }

    #[tokio::test]
    #[serial]
    async fn takeover_live_config_strict_rebuilds_codex_live_from_current_provider() {
        let _home = TempHome::new();
        crate::settings::reload_settings().expect("reload settings");

        let db = Arc::new(Database::memory().expect("init db"));
        let service = ProxyService::new(db.clone());

        let provider = Provider::with_id(
            "current".to_string(),
            "Current".to_string(),
            json!({
                "auth": {
                    "OPENAI_API_KEY": "provider-key"
                },
                "config": r#"model_provider = "any"
model = "gpt-current"
model_reasoning_effort = "medium"

[model_providers.any]
base_url = "https://api.current.example/v1"
wire_api = "responses"
"#
            }),
            None,
        );

        db.save_provider("codex", &provider)
            .expect("save current provider");
        db.set_current_provider("codex", "current")
            .expect("set current provider");
        crate::settings::set_current_provider(&AppType::Codex, Some("current"))
            .expect("set local current provider");

        service
            .write_codex_live(&json!({
                "auth": {
                    "OPENAI_API_KEY": "live-key"
                },
                "config": r#"model_provider = "any"
model = "stale-model"
model_reasoning_effort = "low"

[model_providers.any]
base_url = "https://live.example/v1"
wire_api = "responses"

[mcp_servers.echo]
command = "npx"
args = ["echo-server"]
"#
            }))
            .expect("seed live config");

        service
            .takeover_live_config_strict(&AppType::Codex)
            .await
            .expect("take over codex live config");

        let live = service.read_codex_live().expect("read live config");
        assert_eq!(
            live.get("auth")
                .and_then(|auth| auth.get("OPENAI_API_KEY"))
                .and_then(|v| v.as_str()),
            Some(PROXY_TOKEN_PLACEHOLDER),
            "takeover should still mask provider credentials"
        );

        let config = live
            .get("config")
            .and_then(|v| v.as_str())
            .expect("config string");
        let parsed: toml::Value = toml::from_str(config).expect("parse live codex config");
        assert_eq!(
            parsed.get("model").and_then(|v| v.as_str()),
            Some("gpt-current"),
            "takeover should write the current provider model into Codex live config"
        );
        assert_eq!(
            parsed
                .get("model_reasoning_effort")
                .and_then(|v| v.as_str()),
            Some("medium"),
            "takeover should rebuild Codex live config from the current provider"
        );
        assert_eq!(
            parsed
                .get("model_providers")
                .and_then(|v| v.get("any"))
                .and_then(|v| v.get("base_url"))
                .and_then(|v| v.as_str()),
            Some("http://127.0.0.1:15721/v1"),
            "takeover should still rewrite Codex base URL to the local proxy"
        );
        assert!(
            parsed
                .get("mcp_servers")
                .and_then(|v| v.get("echo"))
                .is_some(),
            "takeover should preserve Codex MCP servers while rebuilding the current provider config"
        );
    }

    #[tokio::test]
    #[serial]
    async fn takeover_live_config_best_effort_overlays_existing_claude_live_without_provider_rebuild(
    ) {
        let _home = TempHome::new();
        crate::settings::reload_settings().expect("reload settings");

        let db = Arc::new(Database::memory().expect("init db"));
        let service = ProxyService::new(db.clone());

        let provider = Provider::with_id(
            "current".to_string(),
            "Current".to_string(),
            json!({
                "env": {
                    "ANTHROPIC_API_KEY": "provider-key",
                    "ANTHROPIC_BASE_URL": "https://api.current.example",
                    "ANTHROPIC_MODEL": "claude-current"
                },
                "permissions": { "allow": ["Read"] }
            }),
            None,
        );

        db.save_provider("claude", &provider)
            .expect("save current provider");
        db.set_current_provider("claude", "current")
            .expect("set current provider");
        crate::settings::set_current_provider(&AppType::Claude, Some("current"))
            .expect("set local current provider");

        service
            .write_claude_live(&json!({
                "env": {
                    "ANTHROPIC_API_KEY": "live-key",
                    "ANTHROPIC_BASE_URL": "https://live.example",
                    "ANTHROPIC_MODEL": "claude-live"
                },
                "permissions": { "allow": ["Bash"] }
            }))
            .expect("seed live config");

        service
            .takeover_live_config_best_effort(&AppType::Claude)
            .await
            .expect("best-effort rewrite");

        let live = service.read_claude_live().expect("read live config");
        assert_eq!(
            live.get("env")
                .and_then(|env| env.get("ANTHROPIC_MODEL"))
                .and_then(|v| v.as_str()),
            Some("claude-live"),
            "best-effort takeover should keep existing live-only Claude model fields"
        );
        assert_eq!(
            live.get("permissions"),
            Some(&json!({ "allow": ["Bash"] })),
            "best-effort takeover should preserve existing live-only top-level Claude settings"
        );
        assert_ne!(
            live.get("permissions"),
            provider.settings_config.get("permissions"),
            "best-effort takeover must not rebuild Claude live config from provider-only top-level settings"
        );
        assert_eq!(
            live.get("env")
                .and_then(|env| env.get("ANTHROPIC_API_KEY"))
                .and_then(|v| v.as_str()),
            Some(PROXY_TOKEN_PLACEHOLDER),
            "best-effort takeover should still mask Claude credentials in the live file"
        );
        assert!(
            live.get("env")
                .and_then(|env| env.get("ANTHROPIC_BASE_URL"))
                .and_then(|v| v.as_str())
                .is_some_and(ProxyService::is_local_proxy_url),
            "best-effort takeover should still rewrite Claude base URL to the local proxy"
        );
    }

    #[tokio::test]
    #[serial]
    async fn takeover_live_config_best_effort_skips_missing_claude_live_without_creating_file() {
        let _home = TempHome::new();
        crate::settings::reload_settings().expect("reload settings");

        let db = Arc::new(Database::memory().expect("init db"));
        let service = ProxyService::new(db.clone());

        let provider = Provider::with_id(
            "current".to_string(),
            "Current".to_string(),
            json!({
                "env": {
                    "ANTHROPIC_API_KEY": "provider-key",
                    "ANTHROPIC_BASE_URL": "https://api.current.example",
                    "ANTHROPIC_MODEL": "claude-current"
                }
            }),
            None,
        );

        db.save_provider("claude", &provider)
            .expect("save current provider");
        db.set_current_provider("claude", "current")
            .expect("set current provider");
        crate::settings::set_current_provider(&AppType::Claude, Some("current"))
            .expect("set local current provider");

        let live_path = get_claude_settings_path();
        assert!(
            !live_path.exists(),
            "test precondition: missing Claude live file should remain missing"
        );

        service
            .takeover_live_config_best_effort(&AppType::Claude)
            .await
            .expect("best-effort takeover should skip missing live files");

        assert!(
            !live_path.exists(),
            "best-effort takeover must not create a new Claude live file when none existed"
        );
    }

    #[tokio::test]
    #[serial]
    async fn takeover_live_config_best_effort_skips_partial_codex_live_without_creating_auth() {
        let _home = TempHome::new();
        crate::settings::reload_settings().expect("reload settings");

        let db = Arc::new(Database::memory().expect("init db"));
        let service = ProxyService::new(db.clone());

        let provider = Provider::with_id(
            "current".to_string(),
            "Current".to_string(),
            json!({
                "auth": {
                    "OPENAI_API_KEY": "provider-key"
                },
                "config": r#"model_provider = "any"
model = "gpt-current"

[model_providers.any]
base_url = "https://api.current.example/v1"
wire_api = "responses"
"#
            }),
            None,
        );

        db.save_provider("codex", &provider)
            .expect("save current provider");
        db.set_current_provider("codex", "current")
            .expect("set current provider");
        crate::settings::set_current_provider(&AppType::Codex, Some("current"))
            .expect("set local current provider");

        let auth_path = crate::codex_config::get_codex_auth_path();
        let config_path = crate::codex_config::get_codex_config_path();
        std::fs::create_dir_all(config_path.parent().expect("config dir"))
            .expect("create codex dir");
        std::fs::write(&config_path, "model = \"partial\"\n").expect("seed partial codex config");
        assert!(
            !auth_path.exists(),
            "test precondition: partial Codex live should not have auth.json"
        );

        service
            .takeover_live_config_best_effort(&AppType::Codex)
            .await
            .expect("best-effort takeover should skip partial Codex live");

        assert!(
            !auth_path.exists(),
            "best-effort takeover must not create Codex auth.json when only config.toml exists"
        );
        assert_eq!(
            std::fs::read_to_string(&config_path).expect("read codex config"),
            "model = \"partial\"\n",
            "skipped best-effort takeover should leave the existing partial Codex config untouched"
        );
    }

    #[tokio::test]
    #[serial]
    async fn takeover_live_config_best_effort_overlays_auth_only_codex_live_and_creates_proxy_config(
    ) {
        let _home = TempHome::new();
        crate::settings::reload_settings().expect("reload settings");

        let db = Arc::new(Database::memory().expect("init db"));
        let service = ProxyService::new(db.clone());

        let provider = Provider::with_id(
            "current".to_string(),
            "Current".to_string(),
            json!({
                "auth": {
                    "OPENAI_API_KEY": "provider-key"
                },
                "config": r#"model_provider = "any"
model = "gpt-current"

[model_providers.any]
base_url = "https://api.current.example/v1"
wire_api = "responses"
"#
            }),
            None,
        );

        db.save_provider("codex", &provider)
            .expect("save current provider");
        db.set_current_provider("codex", "current")
            .expect("set current provider");
        crate::settings::set_current_provider(&AppType::Codex, Some("current"))
            .expect("set local current provider");

        let auth_path = crate::codex_config::get_codex_auth_path();
        let config_path = crate::codex_config::get_codex_config_path();
        std::fs::create_dir_all(auth_path.parent().expect("auth dir")).expect("create codex dir");
        write_json_file(
            &auth_path,
            &json!({
                "OPENAI_API_KEY": PROXY_TOKEN_PLACEHOLDER
            }),
        )
        .expect("seed auth-only codex live");
        assert!(
            !config_path.exists(),
            "test precondition: auth-only Codex live should not have config.toml"
        );

        service
            .takeover_live_config_best_effort(&AppType::Codex)
            .await
            .expect("best-effort takeover should overlay auth-only Codex live");

        assert!(
            config_path.exists(),
            "best-effort takeover should create Codex config.toml when auth-only live needs a proxy base_url"
        );

        let live = service.read_codex_live().expect("read codex live");
        assert_eq!(
            live.get("auth")
                .and_then(|auth| auth.get("OPENAI_API_KEY"))
                .and_then(|v| v.as_str()),
            Some(PROXY_TOKEN_PLACEHOLDER),
            "best-effort takeover should keep the auth placeholder for auth-only Codex live"
        );

        let config = live
            .get("config")
            .and_then(|v| v.as_str())
            .expect("config string");
        let parsed: toml::Value = toml::from_str(config).expect("parse generated codex config");
        assert_eq!(
            parsed.get("base_url").and_then(|v| v.as_str()),
            Some("http://127.0.0.1:15721/v1"),
            "best-effort takeover should create a proxy base_url for auth-only Codex live"
        );
    }

    #[tokio::test]
    #[serial]
    async fn takeover_live_config_best_effort_preserves_malformed_codex_config_without_rebuild() {
        let _home = TempHome::new();
        crate::settings::reload_settings().expect("reload settings");

        let db = Arc::new(Database::memory().expect("init db"));
        let service = ProxyService::new(db.clone());

        let provider = Provider::with_id(
            "current".to_string(),
            "Current".to_string(),
            json!({
                "auth": {
                    "OPENAI_API_KEY": "provider-key"
                },
                "config": r#"model_provider = "any"
model = "gpt-current"

[model_providers.any]
base_url = "https://api.current.example/v1"
wire_api = "responses"
"#
            }),
            None,
        );

        db.save_provider("codex", &provider)
            .expect("save current provider");
        db.set_current_provider("codex", "current")
            .expect("set current provider");
        crate::settings::set_current_provider(&AppType::Codex, Some("current"))
            .expect("set local current provider");

        let config_path = crate::codex_config::get_codex_config_path();
        write_json_file(
            &crate::codex_config::get_codex_auth_path(),
            &json!({
                "OPENAI_API_KEY": PROXY_TOKEN_PLACEHOLDER
            }),
        )
        .expect("seed codex auth file");
        let malformed = "[mcp_servers.echo]\ncommand = ";
        std::fs::write(&config_path, malformed).expect("seed malformed codex config");

        service
            .takeover_live_config_best_effort(&AppType::Codex)
            .await
            .expect("best-effort takeover should preserve malformed Codex live");

        assert_eq!(
            std::fs::read_to_string(&config_path).expect("read malformed codex config"),
            malformed,
            "best-effort takeover should keep the existing malformed Codex config instead of rebuilding it from provider state"
        );
    }

    #[tokio::test]
    #[serial]
    async fn takeover_live_config_strict_fallback_preserves_existing_claude_model_fields() {
        let _home = TempHome::new();
        crate::settings::reload_settings().expect("reload settings");

        let db = Arc::new(Database::memory().expect("init db"));
        let service = ProxyService::new(db.clone());

        service
            .write_claude_live(&json!({
                "env": {
                    "ANTHROPIC_API_KEY": "live-key",
                    "ANTHROPIC_BASE_URL": "https://live.example",
                    "ANTHROPIC_MODEL": "stale-model"
                }
            }))
            .expect("seed live config");

        service
            .takeover_live_config_strict(&AppType::Claude)
            .await
            .expect("take over claude live config");

        let live = service.read_claude_live().expect("read live config");
        assert_eq!(
            live.get("env")
                .and_then(|env| env.get("ANTHROPIC_MODEL"))
                .and_then(|v| v.as_str()),
            Some("stale-model"),
            "fallback takeover should preserve provider-owned model fields when no current provider exists"
        );
    }

    #[tokio::test]
    #[serial]
    async fn hot_switch_provider_serializes_same_app_switches() {
        use tokio::time::{sleep, Duration};

        let _home = TempHome::new();
        crate::settings::reload_settings().expect("reload settings");

        let db = Arc::new(Database::memory().expect("init db"));
        let service = ProxyService::new(db.clone());

        let provider_a = Provider::with_id(
            "a".to_string(),
            "A".to_string(),
            json!({ "env": { "ANTHROPIC_API_KEY": "a-key" } }),
            None,
        );
        let provider_b = Provider::with_id(
            "b".to_string(),
            "B".to_string(),
            json!({ "env": { "ANTHROPIC_API_KEY": "b-key" } }),
            None,
        );
        let provider_c = Provider::with_id(
            "c".to_string(),
            "C".to_string(),
            json!({ "env": { "ANTHROPIC_API_KEY": "c-key" } }),
            None,
        );

        db.save_provider("claude", &provider_a)
            .expect("save provider a");
        db.save_provider("claude", &provider_b)
            .expect("save provider b");
        db.save_provider("claude", &provider_c)
            .expect("save provider c");
        db.set_current_provider("claude", "a")
            .expect("set current provider");
        crate::settings::set_current_provider(&AppType::Claude, Some("a"))
            .expect("set local current provider");
        db.save_live_backup("claude", "{\"env\":{}}")
            .await
            .expect("seed live backup");

        let guard = service.lock_switch_for_test("claude").await;
        let service_for_b = service.clone();
        let service_for_c = service.clone();

        let switch_b = tokio::spawn(async move {
            service_for_b
                .hot_switch_provider("claude", "b")
                .await
                .expect("switch to b")
        });
        sleep(Duration::from_millis(20)).await;
        let switch_c = tokio::spawn(async move {
            service_for_c
                .hot_switch_provider("claude", "c")
                .await
                .expect("switch to c")
        });

        sleep(Duration::from_millis(20)).await;
        drop(guard);

        let outcome_b = switch_b.await.expect("join switch b");
        let outcome_c = switch_c.await.expect("join switch c");
        assert!(outcome_b.logical_target_changed);
        assert!(outcome_c.logical_target_changed);

        assert_eq!(
            crate::settings::get_effective_current_provider(&db, &AppType::Claude)
                .expect("effective current"),
            Some("c".to_string())
        );
        assert_eq!(
            crate::settings::get_current_provider(&AppType::Claude).as_deref(),
            Some("c")
        );
        assert_eq!(
            db.get_current_provider("claude").expect("db current"),
            Some("c".to_string())
        );

        let backup = db
            .get_live_backup("claude")
            .await
            .expect("get live backup")
            .expect("backup exists");
        let expected = serde_json::to_string(&provider_c.settings_config).expect("serialize");
        assert_eq!(backup.original_config, expected);
    }

    #[tokio::test]
    #[serial]
    async fn restore_waits_for_hot_switch_and_restores_latest_backup() {
        use tokio::time::{sleep, Duration};

        let _home = TempHome::new();
        crate::settings::reload_settings().expect("reload settings");

        let db = Arc::new(Database::memory().expect("init db"));
        let service = ProxyService::new(db.clone());

        let provider_a = Provider::with_id(
            "a".to_string(),
            "A".to_string(),
            json!({ "env": { "ANTHROPIC_API_KEY": "a-key" } }),
            None,
        );
        let provider_b = Provider::with_id(
            "b".to_string(),
            "B".to_string(),
            json!({ "env": { "ANTHROPIC_API_KEY": "b-key" } }),
            None,
        );

        db.save_provider("claude", &provider_a)
            .expect("save provider a");
        db.save_provider("claude", &provider_b)
            .expect("save provider b");
        db.set_current_provider("claude", "a")
            .expect("set current provider");
        crate::settings::set_current_provider(&AppType::Claude, Some("a"))
            .expect("set local current provider");
        db.save_live_backup(
            "claude",
            &serde_json::to_string(&provider_a.settings_config).expect("serialize provider a"),
        )
        .await
        .expect("seed live backup");
        service
            .write_claude_live(&json!({ "env": { "ANTHROPIC_API_KEY": "stale" } }))
            .expect("seed live file");

        let guard = service.lock_switch_for_test("claude").await;
        let service_for_switch = service.clone();
        let service_for_restore = service.clone();

        let switch_to_b = tokio::spawn(async move {
            service_for_switch
                .hot_switch_provider("claude", "b")
                .await
                .expect("switch to b")
        });
        sleep(Duration::from_millis(20)).await;
        let restore = tokio::spawn(async move {
            service_for_restore
                .restore_live_config_for_app_with_fallback(&AppType::Claude)
                .await
                .expect("restore claude live")
        });

        sleep(Duration::from_millis(20)).await;
        drop(guard);

        let outcome = switch_to_b.await.expect("join switch");
        restore.await.expect("join restore");
        assert!(outcome.logical_target_changed);

        assert_eq!(
            crate::settings::get_effective_current_provider(&db, &AppType::Claude)
                .expect("effective current"),
            Some("b".to_string())
        );

        let backup = db
            .get_live_backup("claude")
            .await
            .expect("get live backup")
            .expect("backup exists");
        let expected = serde_json::to_string(&provider_b.settings_config).expect("serialize");
        assert_eq!(backup.original_config, expected);
        assert_eq!(
            service.read_claude_live().expect("read live"),
            provider_b.settings_config
        );
    }

    #[tokio::test]
    #[serial]
    async fn sync_live_from_provider_while_proxy_active_waits_for_app_lock() {
        use tokio::time::{sleep, Duration};

        let _home = TempHome::new();
        crate::settings::reload_settings().expect("reload settings");

        let db = Arc::new(Database::memory().expect("init db"));
        let service = ProxyService::new(db.clone());

        let provider = Provider::with_id(
            "p1".to_string(),
            "P1".to_string(),
            json!({
                "env": {
                    "ANTHROPIC_API_KEY": "token",
                    "ANTHROPIC_BASE_URL": "https://claude.example",
                    "ANTHROPIC_MODEL": "claude-locked"
                }
            }),
            None,
        );

        let guard = service.lock_switch_for_test("claude").await;
        let service_for_sync = service.clone();
        let provider_for_sync = provider.clone();

        let sync_task = tokio::spawn(async move {
            service_for_sync
                .sync_live_from_provider_while_proxy_active(&AppType::Claude, &provider_for_sync)
                .await
                .expect("sync live while proxy active");
        });

        sleep(Duration::from_millis(20)).await;
        assert!(
            !sync_task.is_finished(),
            "proxy-active live rebuild should wait for the per-app switch lock"
        );

        drop(guard);
        sync_task.await.expect("join sync task");

        let live = service.read_claude_live().expect("read live config");
        assert_eq!(
            live.get("env")
                .and_then(|env| env.get("ANTHROPIC_API_KEY"))
                .and_then(|v| v.as_str()),
            Some(PROXY_TOKEN_PLACEHOLDER),
            "takeover placeholder should still be applied after the lock is released"
        );
        assert_eq!(
            live.get("env")
                .and_then(|env| env.get("ANTHROPIC_MODEL"))
                .and_then(|v| v.as_str()),
            Some("claude-locked"),
            "live config should still refresh from the provider once the lock is available"
        );
    }

    #[tokio::test]
    #[serial]
    async fn refresh_takeover_state_from_provider_waits_for_app_lock_and_updates_backup_and_live() {
        use tokio::time::{sleep, Duration};

        let _home = TempHome::new();
        crate::settings::reload_settings().expect("reload settings");

        let db = Arc::new(Database::memory().expect("init db"));
        let service = ProxyService::new(db.clone());

        let provider = Provider::with_id(
            "p1".to_string(),
            "P1".to_string(),
            json!({
                "env": {
                    "ANTHROPIC_API_KEY": "token",
                    "ANTHROPIC_BASE_URL": "https://claude.example",
                    "ANTHROPIC_MODEL": "claude-refresh"
                }
            }),
            None,
        );

        let guard = service.lock_switch_for_test("claude").await;
        let service_for_refresh = service.clone();
        let provider_for_refresh = provider.clone();

        let refresh_task = tokio::spawn(async move {
            service_for_refresh
                .refresh_takeover_state_from_provider(&AppType::Claude, &provider_for_refresh)
                .await
                .expect("refresh takeover state");
        });

        sleep(Duration::from_millis(20)).await;
        assert!(
            !refresh_task.is_finished(),
            "the combined takeover refresh should wait for the per-app switch lock"
        );

        drop(guard);
        refresh_task.await.expect("join refresh task");

        let backup = db
            .get_live_backup("claude")
            .await
            .expect("get live backup")
            .expect("backup exists");
        let backup_value: Value =
            serde_json::from_str(&backup.original_config).expect("parse backup json");
        assert_eq!(
            backup_value
                .get("env")
                .and_then(|env| env.get("ANTHROPIC_MODEL"))
                .and_then(|v| v.as_str()),
            Some("claude-refresh"),
            "the combined helper should update the restore backup from the provider"
        );

        let live = service.read_claude_live().expect("read live config");
        assert_eq!(
            live.get("env")
                .and_then(|env| env.get("ANTHROPIC_API_KEY"))
                .and_then(|v| v.as_str()),
            Some(PROXY_TOKEN_PLACEHOLDER),
            "the combined helper should keep takeover credentials masked in live config"
        );
        assert_eq!(
            live.get("env")
                .and_then(|env| env.get("ANTHROPIC_MODEL"))
                .and_then(|v| v.as_str()),
            Some("claude-refresh"),
            "the combined helper should refresh live config from the provider under takeover"
        );
    }

    #[tokio::test]
    #[serial]
    async fn refresh_takeover_state_from_provider_reapplies_current_common_config_to_live() {
        let _home = TempHome::new();
        crate::settings::reload_settings().expect("reload settings");

        let db = Arc::new(Database::memory().expect("init db"));
        let service = ProxyService::new(db.clone());

        db.set_config_snippet(
            "claude",
            Some(r#"{ "includeCoAuthoredBy": true }"#.to_string()),
        )
        .expect("set common config snippet");

        let mut provider = Provider::with_id(
            "p1".to_string(),
            "P1".to_string(),
            json!({
                "env": {
                    "ANTHROPIC_AUTH_TOKEN": "token",
                    "ANTHROPIC_BASE_URL": "https://claude.example",
                    "ANTHROPIC_MODEL": "claude-refresh"
                }
            }),
            None,
        );
        provider.meta = Some(ProviderMeta {
            common_config_enabled: Some(true),
            ..Default::default()
        });

        write_json_file(
            &get_claude_settings_path(),
            &json!({
                "env": {
                    "ANTHROPIC_BASE_URL": "http://127.0.0.1:14555",
                    "ANTHROPIC_AUTH_TOKEN": PROXY_TOKEN_PLACEHOLDER,
                    "ANTHROPIC_MODEL": "stale-model"
                },
                "permissions": { "allow": ["Bash"] }
            }),
        )
        .expect("seed taken-over live file");

        db.save_live_backup(
            "claude",
            &serde_json::to_string(&json!({
                "env": {
                    "ANTHROPIC_AUTH_TOKEN": "token",
                    "ANTHROPIC_BASE_URL": "https://claude.example",
                    "ANTHROPIC_MODEL": "claude-refresh"
                },
                "permissions": { "allow": ["Bash"] }
            }))
            .expect("serialize backup"),
        )
        .await
        .expect("seed live backup");

        service
            .refresh_takeover_state_from_provider(&AppType::Claude, &provider)
            .await
            .expect("refresh takeover state");

        let live = service.read_claude_live().expect("read live config");
        assert_eq!(
            live.get("includeCoAuthoredBy").and_then(|v| v.as_bool()),
            Some(true),
            "takeover live refresh should reapply current common-config fields"
        );
        assert_eq!(
            live.get("permissions"),
            Some(&json!({ "allow": ["Bash"] })),
            "takeover live refresh should preserve live-only Claude settings"
        );
        assert_eq!(
            live.get("env")
                .and_then(|env| env.get("ANTHROPIC_AUTH_TOKEN"))
                .and_then(|v| v.as_str()),
            Some(PROXY_TOKEN_PLACEHOLDER),
            "takeover live refresh should keep proxy placeholders after reapplying common config"
        );
    }

    #[tokio::test]
    #[serial]
    async fn update_live_backup_from_provider_applies_claude_common_config() {
        let _home = TempHome::new();
        crate::settings::reload_settings().expect("reload settings");

        let db = Arc::new(Database::memory().expect("init db"));
        db.set_config_snippet(
            "claude",
            Some(
                serde_json::json!({
                    "includeCoAuthoredBy": false
                })
                .to_string(),
            ),
        )
        .expect("set common config snippet");

        let service = ProxyService::new(db.clone());

        let mut provider = Provider::with_id(
            "p1".to_string(),
            "P1".to_string(),
            json!({
                "env": {
                    "ANTHROPIC_AUTH_TOKEN": "token",
                    "ANTHROPIC_BASE_URL": "https://claude.example"
                }
            }),
            None,
        );
        provider.meta = Some(ProviderMeta {
            common_config_enabled: Some(true),
            ..Default::default()
        });

        service
            .update_live_backup_from_provider("claude", &provider)
            .await
            .expect("update live backup");

        let backup = db
            .get_live_backup("claude")
            .await
            .expect("get live backup")
            .expect("backup exists");
        let stored: Value =
            serde_json::from_str(&backup.original_config).expect("parse backup json");

        assert_eq!(
            stored.get("includeCoAuthoredBy").and_then(|v| v.as_bool()),
            Some(false),
            "common config should be applied into Claude restore backup"
        );
    }

    #[tokio::test]
    #[serial]
    async fn update_live_backup_from_provider_applies_codex_common_config() {
        let _home = TempHome::new();
        crate::settings::reload_settings().expect("reload settings");

        let db = Arc::new(Database::memory().expect("init db"));
        db.set_config_snippet(
            "codex",
            Some("disable_response_storage = true\n".to_string()),
        )
        .expect("set common config snippet");

        let service = ProxyService::new(db.clone());

        let mut provider = Provider::with_id(
            "p1".to_string(),
            "P1".to_string(),
            json!({
                "auth": {
                    "OPENAI_API_KEY": "token"
                },
                "config": r#"model_provider = "any"
model = "gpt-5"

[model_providers.any]
base_url = "https://codex.example/v1"
"#
            }),
            None,
        );
        provider.meta = Some(ProviderMeta {
            common_config_enabled: Some(true),
            ..Default::default()
        });

        service
            .update_live_backup_from_provider("codex", &provider)
            .await
            .expect("update live backup");

        let backup = db
            .get_live_backup("codex")
            .await
            .expect("get live backup")
            .expect("backup exists");
        let stored: Value =
            serde_json::from_str(&backup.original_config).expect("parse backup json");
        let config = stored
            .get("config")
            .and_then(|v| v.as_str())
            .expect("config string");

        assert!(
            config.contains("disable_response_storage = true"),
            "common config should be applied into Codex restore backup"
        );
    }

    #[tokio::test]
    #[serial]
    async fn update_live_backup_from_provider_preserves_codex_mcp_servers() {
        let _home = TempHome::new();
        crate::settings::reload_settings().expect("reload settings");

        let db = Arc::new(Database::memory().expect("init db"));
        let service = ProxyService::new(db.clone());

        db.save_live_backup(
            "codex",
            &serde_json::to_string(&json!({
                "auth": {
                    "OPENAI_API_KEY": "old-token"
                },
                "config": r#"model_provider = "any"
model = "gpt-4"

[model_providers.any]
base_url = "https://old.example/v1"

[mcp_servers.echo]
command = "npx"
args = ["echo-server"]
"#
            }))
            .expect("serialize seed backup"),
        )
        .await
        .expect("seed live backup");

        let provider = Provider::with_id(
            "p2".to_string(),
            "P2".to_string(),
            json!({
                "auth": {
                    "OPENAI_API_KEY": "new-token"
                },
                "config": r#"model_provider = "any"
model = "gpt-5"

[model_providers.any]
base_url = "https://new.example/v1"
"#
            }),
            None,
        );

        service
            .update_live_backup_from_provider("codex", &provider)
            .await
            .expect("update live backup");

        let backup = db
            .get_live_backup("codex")
            .await
            .expect("get live backup")
            .expect("backup exists");
        let stored: Value =
            serde_json::from_str(&backup.original_config).expect("parse backup json");
        let config = stored
            .get("config")
            .and_then(|v| v.as_str())
            .expect("config string");

        assert!(
            config.contains("[mcp_servers.echo]"),
            "existing Codex MCP section should survive proxy hot-switch backup update"
        );
        assert!(
            config.contains("https://new.example/v1"),
            "provider-specific base_url should still update to the new provider"
        );
    }

    #[tokio::test]
    #[serial]
    async fn update_live_backup_from_provider_keeps_new_codex_mcp_entries_on_conflict() {
        let _home = TempHome::new();
        crate::settings::reload_settings().expect("reload settings");

        let db = Arc::new(Database::memory().expect("init db"));
        let service = ProxyService::new(db.clone());

        db.save_live_backup(
            "codex",
            &serde_json::to_string(&json!({
                "auth": {
                    "OPENAI_API_KEY": "old-token"
                },
                "config": r#"[mcp_servers.shared]
command = "old-command"

[mcp_servers.legacy]
command = "legacy-command"
"#
            }))
            .expect("serialize seed backup"),
        )
        .await
        .expect("seed live backup");

        let provider = Provider::with_id(
            "p2".to_string(),
            "P2".to_string(),
            json!({
                "auth": {
                    "OPENAI_API_KEY": "new-token"
                },
                "config": r#"[mcp_servers.shared]
command = "new-command"

[mcp_servers.latest]
command = "latest-command"
"#
            }),
            None,
        );

        service
            .update_live_backup_from_provider("codex", &provider)
            .await
            .expect("update live backup");

        let backup = db
            .get_live_backup("codex")
            .await
            .expect("get live backup")
            .expect("backup exists");
        let stored: Value =
            serde_json::from_str(&backup.original_config).expect("parse backup json");
        let config = stored
            .get("config")
            .and_then(|v| v.as_str())
            .expect("config string");
        let parsed: toml::Value = toml::from_str(config).expect("parse merged codex config");

        let mcp_servers = parsed
            .get("mcp_servers")
            .expect("mcp_servers should be present");
        assert_eq!(
            mcp_servers
                .get("shared")
                .and_then(|v| v.get("command"))
                .and_then(|v| v.as_str()),
            Some("new-command"),
            "new provider/common-config MCP definition should win on conflict"
        );
        assert_eq!(
            mcp_servers
                .get("legacy")
                .and_then(|v| v.get("command"))
                .and_then(|v| v.as_str()),
            Some("legacy-command"),
            "backup-only MCP entries should still be preserved"
        );
        assert_eq!(
            mcp_servers
                .get("latest")
                .and_then(|v| v.get("command"))
                .and_then(|v| v.as_str()),
            Some("latest-command"),
            "new MCP entries should remain in the restore backup"
        );
    }
}

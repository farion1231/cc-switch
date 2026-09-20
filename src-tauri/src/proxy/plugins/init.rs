//! 插件注册表初始化与运行时重载
//!
//! 负责把内置插件、用户外部插件与 `plugins_config`（settings 表）组装成一个
//! [`PluginRegistry`]：
//! - 全局开关（`PluginsConfig.enabled`）通过注册表的 `set_global_enabled` 生效
//!   （关闭时 `plugins_for_stage` 返回空、`list()` 条目全部展示为禁用）；
//! - 用户插件目录：`<配置目录>/plugins/`，加载失败的条目以 `PluginInfo`
//!   失败条目形式进入注册表（list 时展示，前端提示用）；
//! - overrides 持久化在 `plugins_config` 中，注册表只存内存态。
//!
//! 详见 docs/dev/plugin-system-contract.md。

use std::path::{Path, PathBuf};
use std::sync::Arc;

use super::builtin::{BuiltinCacheInjectorPlugin, BuiltinThinkingOptimizerPlugin};
use super::external::load_user_plugins;
use super::registry::PluginRegistry;
use super::types::{PluginInfo, PluginsConfig};
use crate::database::Database;

/// 用户插件目录：`<应用配置目录>/plugins`
pub fn plugins_dir() -> PathBuf {
    crate::config::get_app_config_dir().join("plugins")
}

/// 从 settings 表读取插件配置；key 缺失或解析失败时使用默认值（全局开启、无覆盖）
fn load_plugins_config(db: &Database) -> PluginsConfig {
    match db.get_setting("plugins_config") {
        Ok(Some(json)) => serde_json::from_str(&json).unwrap_or_else(|e| {
            log::warn!("[PLUGIN] 解析 plugins_config 失败，使用默认配置: {e}");
            PluginsConfig::default()
        }),
        Ok(None) => PluginsConfig::default(),
        Err(e) => {
            log::warn!("[PLUGIN] 读取 plugins_config 失败，使用默认配置: {e}");
            PluginsConfig::default()
        }
    }
}

/// 把 plugins_config 中的 overrides 应用到注册表（覆盖 enabled/priority）
pub fn apply_overrides(registry: &PluginRegistry, config: &PluginsConfig) {
    for (plugin_id, override_item) in &config.overrides {
        registry.set_override(plugin_id, override_item.enabled, override_item.priority);
    }
}

/// 把用户插件目录中的插件加载进注册表：
/// 成功条目 register，失败条目转成 PluginInfo 失败条目（id = `user:<目录名>`）
fn load_user_plugins_into(registry: &PluginRegistry, dir: &Path) {
    let (plugins, errors) = load_user_plugins(dir);
    for plugin in plugins {
        registry.register(plugin);
    }
    for error in errors {
        registry.add_failed_entry(PluginInfo {
            id: format!("user:{}", error.dir_name),
            display_name: error.dir_name,
            description: String::new(),
            is_builtin: false,
            stages: Vec::new(),
            priority: 500,
            enabled: false,
            version: None,
            source: error.manifest_path,
            has_config: false,
            error: Some(error.message),
        });
    }
}

/// 初始化插件注册表：注册内置插件 → 加载用户插件 → 应用 overrides →
/// 按 plugins_config.enabled 设置全局开关。
pub fn init_registry(db: Arc<Database>) -> Arc<PluginRegistry> {
    let dir = plugins_dir();
    build_registry(db, &dir)
}

/// [`init_registry`] 的可注入目录版本（单元测试用临时目录，不触碰真实配置目录）
fn build_registry(db: Arc<Database>, dir: &Path) -> Arc<PluginRegistry> {
    let config = load_plugins_config(&db);
    let registry = PluginRegistry::new();
    registry.register(Arc::new(BuiltinThinkingOptimizerPlugin::new(db.clone())));
    registry.register(Arc::new(BuiltinCacheInjectorPlugin::new(db.clone())));
    load_user_plugins_into(&registry, dir);
    apply_overrides(&registry, &config);
    // 全局开关：关闭时注册表内容保留，但 plugins_for_stage/list 均表现为禁用
    registry.set_global_enabled(config.enabled);
    Arc::new(registry)
}

/// 运行时重载用户插件（供重载命令调用）：
/// 清除非内置插件与失败条目 → 重新扫描目录 → 重新应用 overrides 与全局开关。
pub fn reload_user_plugins(registry: &PluginRegistry, db: &Database) {
    let dir = plugins_dir();
    reload_user_plugins_from(registry, db, &dir);
}

/// [`reload_user_plugins`] 的可注入目录版本（单元测试用临时目录）
fn reload_user_plugins_from(registry: &PluginRegistry, db: &Database, dir: &Path) {
    registry.clear_user_plugins();
    load_user_plugins_into(registry, dir);
    let config = load_plugins_config(db);
    apply_overrides(registry, &config);
    // 全局开关与 overrides 一样，重载时按最新配置同步
    registry.set_global_enabled(config.enabled);
}

#[cfg(test)]
mod tests {
    use std::fs;

    use serde_json::json;

    use super::super::types::{PluginProviderInfo, PluginRequestContext, PluginStage};
    use super::*;

    /// 构造一个最小可用的 PreRequest 插件目录
    fn write_plugin(dir: &Path, dir_name: &str, plugin_id: &str) {
        let plugin_dir = dir.join(dir_name);
        fs::create_dir_all(&plugin_dir).unwrap();
        fs::write(
            plugin_dir.join(super::super::external::MANIFEST_FILE),
            json!({
                "id": plugin_id,
                "name": "测试插件",
                "stages": ["pre_request"],
                "command": ["node", "index.js"],
                "enabled": true
            })
            .to_string(),
        )
        .unwrap();
    }

    fn pre_request_ctx() -> PluginRequestContext {
        PluginRequestContext {
            app_type: "claude".to_string(),
            session_id: "sess-1".to_string(),
            request_model: "claude-x".to_string(),
            stage: PluginStage::PreRequest,
            provider: Some(PluginProviderInfo {
                id: "p1".to_string(),
                name: "Provider 1".to_string(),
                is_bedrock: false,
            }),
        }
    }

    #[test]
    fn test_init_registry_loads_builtin_and_user_plugins() {
        let tmp = tempfile::tempdir().unwrap();
        write_plugin(tmp.path(), "good", "good-plugin");
        // 无 plugin.json 的失败条目
        fs::create_dir_all(tmp.path().join("broken")).unwrap();

        let db = Arc::new(Database::memory().unwrap());
        let registry = build_registry(db, tmp.path());

        let infos = registry.list();
        let ids: Vec<&str> = infos.iter().map(|i| i.id.as_str()).collect();
        assert!(ids.contains(&"builtin:cache-injector"));
        assert!(ids.contains(&"builtin:thinking-optimizer"));
        assert!(ids.contains(&"user:good-plugin"));

        // 失败条目：id 用 user:<目录名>，stages 空、enabled=false、带错误信息
        let failed = infos
            .iter()
            .find(|i| i.id == "user:broken")
            .expect("失败条目应存在");
        assert!(failed.stages.is_empty());
        assert!(!failed.enabled);
        assert!(failed.error.is_some());
        assert!(!failed.is_builtin);
    }

    #[test]
    fn test_init_registry_applies_overrides_from_settings() {
        let tmp = tempfile::tempdir().unwrap();
        write_plugin(tmp.path(), "good", "good-plugin");

        let db = Arc::new(Database::memory().unwrap());
        db.set_setting(
            "plugins_config",
            r#"{"enabled": true, "overrides": {"user:good-plugin": {"enabled": false}}}"#,
        )
        .unwrap();

        let registry = build_registry(db, tmp.path());

        // override 生效：user:good-plugin 被禁用，不再出现在 PreRequest 管线中
        let pre_request_ids: Vec<String> = registry
            .plugins_for_stage(PluginStage::PreRequest)
            .iter()
            .map(|p| p.id().to_string())
            .collect();
        assert!(!pre_request_ids.contains(&"user:good-plugin".to_string()));
        let info = registry
            .list()
            .into_iter()
            .find(|i| i.id == "user:good-plugin")
            .expect("插件条目仍应存在（只是被禁用）");
        assert!(!info.enabled);
        assert!(info.error.is_none());
    }

    #[test]
    fn test_init_registry_global_switch_disabled_marks_all_disabled() {
        let tmp = tempfile::tempdir().unwrap();
        write_plugin(tmp.path(), "good", "good-plugin");

        let db = Arc::new(Database::memory().unwrap());
        db.set_setting("plugins_config", r#"{"enabled": false}"#)
            .unwrap();

        let registry = build_registry(db, tmp.path());
        // 全局关闭：注册表内容保留，但全局开关关闭 → 管线为空、条目展示为禁用
        assert!(!registry.global_enabled());
        assert!(registry
            .plugins_for_stage(PluginStage::PreRequest)
            .is_empty());
        let infos = registry.list();
        assert!(!infos.is_empty());
        assert!(infos.iter().all(|i| !i.enabled));
        assert!(infos.iter().any(|i| i.id == "user:good-plugin"));
    }

    #[test]
    fn test_init_registry_invalid_config_uses_default() {
        // 配置解析失败时回退默认值：全局开启，插件正常加载
        let tmp = tempfile::tempdir().unwrap();
        write_plugin(tmp.path(), "good", "good-plugin");

        let db = Arc::new(Database::memory().unwrap());
        db.set_setting("plugins_config", "not json").unwrap();

        let registry = build_registry(db, tmp.path());
        let infos = registry.list();
        let ids: Vec<&str> = infos.iter().map(|i| i.id.as_str()).collect();
        assert!(ids.contains(&"user:good-plugin"));
    }

    #[test]
    fn test_reload_user_plugins_replaces_user_entries_and_reapplies_overrides() {
        let tmp = tempfile::tempdir().unwrap();
        write_plugin(tmp.path(), "first", "first-plugin");

        let db = Arc::new(Database::memory().unwrap());
        let registry = build_registry(db.clone(), tmp.path());
        assert!(registry.list().iter().any(|i| i.id == "user:first-plugin"));

        // 目录内容变化 + 配置变化后重载
        fs::remove_dir_all(tmp.path().join("first")).unwrap();
        write_plugin(tmp.path(), "second", "second-plugin");
        db.set_setting(
            "plugins_config",
            r#"{"enabled": true, "overrides": {"user:second-plugin": {"priority": 50}}}"#,
        )
        .unwrap();

        reload_user_plugins_from(&registry, &db, tmp.path());

        let infos = registry.list();
        let ids: Vec<&str> = infos.iter().map(|i| i.id.as_str()).collect();
        assert!(!ids.contains(&"user:first-plugin"), "旧插件应被清除");
        assert!(ids.contains(&"user:second-plugin"));
        assert!(ids.contains(&"builtin:cache-injector"), "内置插件应保留");
        let info = registry
            .list()
            .into_iter()
            .find(|i| i.id == "user:second-plugin")
            .unwrap();
        assert_eq!(info.priority, 50);
    }

    #[test]
    fn test_pipeline_noop_with_empty_plugin_dir() {
        // 目录无用户插件时 PreRequest 管线零改动（核心注册表只含内置 PreSend
        // 插件与外部用户插件，无 PreRequest 内置逻辑）；
        // 外部插件改写 body 的路径由 external.rs 的 mock runner 测试覆盖；
        // forwarder 侧的 PreSend 管线调用测试见 forwarder.rs 测试模块
        let tmp = tempfile::tempdir().unwrap();
        let db = Arc::new(Database::memory().unwrap());
        let registry = build_registry(db, tmp.path());

        let mut body = json!({"model": "claude-x"});
        let changed = super::super::registry::run_request_pipeline(
            &registry,
            PluginStage::PreRequest,
            &pre_request_ctx(),
            &mut body,
            |p, c, b| p.transform_request(c, b),
        );
        assert!(!changed, "无 PreRequest 插件时管线不应有改动");
        assert_eq!(body, json!({"model": "claude-x"}));
    }
}

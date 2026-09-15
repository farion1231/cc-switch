#![allow(non_snake_case)]

//! 插件系统 Tauri 命令（契约 2.8）
//!
//! list/查询只读注册表内存态；set_* 同时更新注册表内存态并持久化到
//! settings 表（`plugins_config`）。DAO 是同步 sqlite，与仓库现状一致
//! 在 async 命令中直接调用（sqlite 本地库操作足够快，无需 spawn_blocking）。

use std::path::{Path, PathBuf};

use serde::Serialize;
use tauri::AppHandle;
use tauri_plugin_opener::OpenerExt;

use crate::proxy::plugins::external::MANIFEST_FILE;
use crate::proxy::plugins::{
    plugins_dir, PluginInfo, PluginManifest, PluginOverride, PluginsConfig,
};
use crate::store::AppState;

/// 重载用户插件的结果（契约 2.8：`plugin_reload` 返回值）
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PluginReloadResult {
    /// 本次成功加载的用户插件数量（不含内置插件与失败条目）
    pub loaded: usize,
    /// 加载失败的条目（`<id>: <错误信息>`），list 中也会以失败条目形式展示
    pub errors: Vec<String>,
}

/// 列出全部插件元信息（合并内置/manifest 信息 + 运行时覆盖）
#[tauri::command]
pub async fn plugin_list(state: tauri::State<'_, AppState>) -> Result<Vec<PluginInfo>, String> {
    Ok(state.plugins.list())
}

/// 设置单个插件启用/禁用（合并已有 override，保留原 priority，然后持久化）
#[tauri::command]
pub async fn plugin_set_enabled(
    state: tauri::State<'_, AppState>,
    id: String,
    enabled: bool,
) -> Result<bool, String> {
    // 从注册表快照取现有覆盖并合并，避免把 priority 覆盖清掉
    let mut merged = state
        .plugins
        .overrides()
        .get(&id)
        .cloned()
        .unwrap_or_default();
    merged.enabled = Some(enabled);

    state
        .plugins
        .set_override(&id, merged.enabled, merged.priority);

    let mut config = state.db.get_plugins_config().map_err(|e| e.to_string())?;
    config.overrides.insert(id, merged);
    state
        .db
        .set_plugins_config(&config)
        .map_err(|e| e.to_string())?;
    Ok(true)
}

/// 设置单个插件优先级（合并已有 override，保留原 enabled，然后持久化）
#[tauri::command]
pub async fn plugin_set_priority(
    state: tauri::State<'_, AppState>,
    id: String,
    priority: i32,
) -> Result<bool, String> {
    let mut merged = state
        .plugins
        .overrides()
        .get(&id)
        .cloned()
        .unwrap_or_default();
    merged.priority = Some(priority);

    state
        .plugins
        .set_override(&id, merged.enabled, merged.priority);

    let mut config = state.db.get_plugins_config().map_err(|e| e.to_string())?;
    config.overrides.insert(id, merged);
    state
        .db
        .set_plugins_config(&config)
        .map_err(|e| e.to_string())?;
    Ok(true)
}

/// 设置插件系统全局开关（写 `plugins_config.enabled` 并同步注册表内存态）
#[tauri::command]
pub async fn plugin_set_all_enabled(
    state: tauri::State<'_, AppState>,
    enabled: bool,
) -> Result<bool, String> {
    state.plugins.set_global_enabled(enabled);

    let mut config: PluginsConfig = state.db.get_plugins_config().map_err(|e| e.to_string())?;
    config.enabled = enabled;
    state
        .db
        .set_plugins_config(&config)
        .map_err(|e| e.to_string())?;
    log::info!("[PLUGIN] 插件系统全局开关已切换: {enabled}");
    Ok(true)
}

/// 重扫用户插件目录并重新应用 overrides / 全局开关
#[tauri::command]
pub async fn plugin_reload(
    state: tauri::State<'_, AppState>,
) -> Result<PluginReloadResult, String> {
    crate::proxy::plugins::reload_user_plugins(&state.plugins, &state.db);

    let infos = state.plugins.list();
    let loaded = infos
        .iter()
        .filter(|i| !i.is_builtin && i.error.is_none())
        .count();
    let errors: Vec<String> = infos
        .iter()
        .filter_map(|i| i.error.as_ref().map(|e| format!("{}: {e}", i.id)))
        .collect();
    log::info!(
        "[PLUGIN] 用户插件重载完成: loaded={loaded}, errors={}",
        errors.len()
    );
    Ok(PluginReloadResult { loaded, errors })
}

/// 打开插件目录（不存在则先创建）
#[tauri::command]
pub async fn plugin_open_dir(app: AppHandle) -> Result<bool, String> {
    let dir = crate::proxy::plugins::plugins_dir();
    if !dir.exists() {
        std::fs::create_dir_all(&dir).map_err(|e| format!("创建插件目录失败: {e}"))?;
    }

    app.opener()
        .open_path(dir.to_string_lossy().to_string(), None::<String>)
        .map_err(|e| format!("打开插件目录失败: {e}"))?;

    Ok(true)
}

/// 插件导入结果
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PluginImportResult {
    /// 导入后的插件 id（`user:<manifest.id>`）
    pub plugin_id: String,
    /// 插件目录名（取自源目录名）
    pub dir_name: String,
}

/// 递归复制目录（不跟随符号链接；插件目录内容整体搬入）
fn copy_dir_recursive(src: &Path, dst: &Path) -> std::io::Result<()> {
    std::fs::create_dir_all(dst)?;
    for entry in std::fs::read_dir(src)? {
        let entry = entry?;
        let file_type = entry.file_type()?;
        let target = dst.join(entry.file_name());
        if file_type.is_dir() {
            copy_dir_recursive(&entry.path(), &target)?;
        } else if file_type.is_file() {
            std::fs::copy(entry.path(), &target)?;
        }
    }
    Ok(())
}

/// 从本地目录导入用户插件：校验 plugin.json → 递归拷贝进插件目录 →
/// 重载注册表并返回新插件 id。目标目录重名时返回错误（用户重命名后重试）。
#[tauri::command]
pub async fn plugin_import(
    state: tauri::State<'_, AppState>,
    source_dir: String,
) -> Result<PluginImportResult, String> {
    let src = PathBuf::from(&source_dir);
    if !src.is_dir() {
        return Err(format!("所选路径不是目录: {source_dir}"));
    }
    let manifest_path = src.join(MANIFEST_FILE);
    if !manifest_path.is_file() {
        return Err("所选目录缺少 plugin.json，不是有效的用户插件".to_string());
    }
    let content = std::fs::read_to_string(&manifest_path)
        .map_err(|e| format!("读取 plugin.json 失败: {e}"))?;
    let manifest: PluginManifest =
        serde_json::from_str(&content).map_err(|e| format!("plugin.json 解析失败: {e}"))?;
    manifest
        .validate()
        .map_err(|e| format!("plugin.json 校验失败: {e}"))?;

    let dir_name = src
        .file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .ok_or_else(|| "源目录名无效".to_string())?;
    let dest = plugins_dir().join(&dir_name);
    if dest.exists() {
        return Err(format!("插件目录下已存在 {dir_name}，请重命名源目录后重试"));
    }

    std::fs::create_dir_all(plugins_dir()).map_err(|e| format!("创建插件目录失败: {e}"))?;
    if let Err(e) = copy_dir_recursive(&src, &dest) {
        // 拷贝中断：清理半成品目录（best effort），避免留下损坏插件
        let _ = std::fs::remove_dir_all(&dest);
        return Err(format!("复制插件目录失败: {e}"));
    }

    crate::proxy::plugins::reload_user_plugins(&state.plugins, &state.db);
    let plugin_id = format!("user:{}", manifest.id);
    log::info!("[PLUGIN] 已导入用户插件 {dir_name} ({plugin_id})");
    Ok(PluginImportResult {
        plugin_id,
        dir_name,
    })
}

/// 合并单个插件覆盖的共用逻辑占位（供未来扩展）；当前保留 PluginOverride 引用避免未用告警
#[allow(dead_code)]
fn _unused(_o: &PluginOverride) {}

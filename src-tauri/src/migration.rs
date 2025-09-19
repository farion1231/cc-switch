use crate::app_config::{AppType, MultiAppConfig};
use crate::config::{
    archive_file, delete_file, get_app_config_dir, get_app_config_path, get_claude_config_dir,
};
use serde_json::Value;
use std::collections::{HashMap, HashSet};
use std::fs;
use std::path::PathBuf;

fn now_ts() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs()
}

fn get_marker_path() -> PathBuf {
    get_app_config_dir().join("migrated.copies.v1")
}

fn sanitized_id(base: &str) -> String {
    crate::config::sanitize_provider_name(base)
}

fn next_unique_id(existing: &HashSet<String>, base: &str) -> String {
    let base = sanitized_id(base);
    if !existing.contains(&base) {
        return base;
    }
    for i in 2..1000 {
        let candidate = format!("{}-{}", base, i);
        if !existing.contains(&candidate) {
            return candidate;
        }
    }
    format!("{}-dup", base)
}

fn extract_claude_api_key(value: &Value) -> Option<String> {
    value
        .get("env")
        .and_then(|env| env.get("ANTHROPIC_AUTH_TOKEN"))
        .and_then(|v| v.as_str())
        .map(|s| s.to_string())
}

fn extract_codex_api_key(value: &Value) -> Option<String> {
    value
        .get("auth")
        .and_then(|auth| {
            auth.get("OPENAI_API_KEY")
                .or_else(|| auth.get("openai_api_key"))
        })
        .and_then(|v| v.as_str())
        .map(|s| s.to_string())
}

fn norm_name(s: &str) -> String {
    s.trim().to_lowercase()
}

// 去重策略：name + 原始 key 直接比较（不做哈希）

fn scan_claude_copies() -> Vec<(String, PathBuf, Value)> {
    let mut items = Vec::new();
    let dir = get_claude_config_dir();
    if !dir.exists() {
        return items;
    }
    if let Ok(rd) = fs::read_dir(&dir) {
        for e in rd.flatten() {
            let p = e.path();
            let fname = match p.file_name().and_then(|s| s.to_str()) {
                Some(s) => s,
                None => continue,
            };
            if fname == "settings.json" || fname == "claude.json" {
                continue;
            }
            if !fname.starts_with("settings-") || !fname.ends_with(".json") {
                continue;
            }
            let name = fname
                .trim_start_matches("settings-")
                .trim_end_matches(".json");
            if let Ok(val) = crate::config::read_json_file::<Value>(&p) {
                items.push((name.to_string(), p, val));
            }
        }
    }
    items
}

fn scan_codex_copies() -> Vec<(String, Option<PathBuf>, Option<PathBuf>, Value)> {
    let mut by_name: HashMap<String, (Option<PathBuf>, Option<PathBuf>)> = HashMap::new();
    let dir = crate::codex_config::get_codex_config_dir();
    if !dir.exists() {
        return Vec::new();
    }
    if let Ok(rd) = fs::read_dir(&dir) {
        for e in rd.flatten() {
            let p = e.path();
            let fname = match p.file_name().and_then(|s| s.to_str()) {
                Some(s) => s,
                None => continue,
            };
            if fname.starts_with("auth-") && fname.ends_with(".json") {
                let name = fname.trim_start_matches("auth-").trim_end_matches(".json");
                let entry = by_name.entry(name.to_string()).or_default();
                entry.0 = Some(p);
            } else if fname.starts_with("config-") && fname.ends_with(".toml") {
                let name = fname
                    .trim_start_matches("config-")
                    .trim_end_matches(".toml");
                let entry = by_name.entry(name.to_string()).or_default();
                entry.1 = Some(p);
            }
        }
    }

    let mut items = Vec::new();
    for (name, (auth_path, config_path)) in by_name {
        if let Some(authp) = auth_path {
            if let Ok(auth) = crate::config::read_json_file::<Value>(&authp) {
                let config_str = if let Some(cfgp) = &config_path {
                    match crate::codex_config::read_and_validate_config_from_path(cfgp) {
                        Ok(s) => s,
                        Err(e) => {
                            log::warn!("跳过无效 Codex config-{}.toml: {}", name, e);
                            String::new()
                        }
                    }
                } else {
                    String::new()
                };
                let settings = serde_json::json!({
                    "auth": auth,
                    "config": config_str,
                });
                items.push((name, Some(authp), config_path, settings));
            }
        }
    }
    items
}

pub fn migrate_copies_into_config(config: &mut MultiAppConfig) -> Result<bool, String> {
    // 如果已迁移过则跳过；若目录不存在则先创建，避免新装用户写入标记时失败
    let marker = get_marker_path();
    if let Some(parent) = marker.parent() {
        std::fs::create_dir_all(parent)
            .map_err(|e| format!("创建迁移标记目录失败: {}", e))?;
    }
    if marker.exists() {
        return Ok(false);
    }

    let claude_items = scan_claude_copies();
    let codex_items = scan_codex_copies();
    if claude_items.is_empty() && codex_items.is_empty() {
        // 即便没有可迁移项，也写入标记避免每次扫描
        fs::write(&marker, b"no-copies").map_err(|e| format!("写入迁移标记失败: {}", e))?;
        return Ok(false);
    }

    // 备份旧的 config.json
    let ts = now_ts();
    let app_cfg_path = get_app_config_path();
    if app_cfg_path.exists() {
        let _ = archive_file(ts, "cc-switch", &app_cfg_path);
    }

    // 读取 live：Claude（settings.json / claude.json）
    let live_claude: Option<(String, Value)> = {
        let settings_path = crate::config::get_claude_settings_path();
        if settings_path.exists() {
            match crate::config::read_json_file::<Value>(&settings_path) {
                Ok(val) => Some(("default".to_string(), val)),
                Err(e) => {
                    log::warn!("读取 Claude live 配置失败: {}", e);
                    None
                }
            }
        } else {
            None
        }
    };

    // 合并：Claude（优先 live，然后副本） - 去重键: name + apiKey（直接比较）
    config.ensure_app(&AppType::Claude);
    let manager = config.get_manager_mut(&AppType::Claude).unwrap();
    let mut ids: HashSet<String> = manager.providers.keys().cloned().collect();
    let mut live_claude_id: Option<String> = None;

    if let Some((name, value)) = &live_claude {
        let cand_key = extract_claude_api_key(value);
        let exist_id = manager.providers.iter().find_map(|(id, p)| {
            let pk = extract_claude_api_key(&p.settings_config);
            if norm_name(&p.name) == norm_name(name) && pk == cand_key {
                Some(id.clone())
            } else {
                None
            }
        });
        if let Some(exist_id) = exist_id {
            if let Some(prov) = manager.providers.get_mut(&exist_id) {
                log::info!("合并到已存在 Claude 供应商 '{}' (by name+key)", name);
                prov.settings_config = value.clone();
                live_claude_id = Some(exist_id);
            }
        } else {
            let id = next_unique_id(&ids, name);
            ids.insert(id.clone());
            let provider =
                crate::provider::Provider::with_id(id.clone(), name.clone(), value.clone(), None);
            manager.providers.insert(provider.id.clone(), provider);
            live_claude_id = Some(id);
        }
    }
    for (name, path, value) in claude_items.iter() {
        let cand_key = extract_claude_api_key(value);
        let exist_id = manager.providers.iter().find_map(|(id, p)| {
            let pk = extract_claude_api_key(&p.settings_config);
            if norm_name(&p.name) == norm_name(name) && pk == cand_key {
                Some(id.clone())
            } else {
                None
            }
        });
        if let Some(exist_id) = exist_id {
            if let Some(prov) = manager.providers.get_mut(&exist_id) {
                log::info!(
                    "覆盖 Claude 供应商 '{}' 来自 {} (by name+key)",
                    name,
                    path.display()
                );
                prov.settings_config = value.clone();
            }
        } else {
            let id = next_unique_id(&ids, name);
            ids.insert(id.clone());
            let provider =
                crate::provider::Provider::with_id(id.clone(), name.clone(), value.clone(), None);
            manager.providers.insert(provider.id.clone(), provider);
        }
    }

    // 读取 live：Codex（auth.json 必需，config.toml 可空）
    let live_codex: Option<(String, Value)> = {
        let auth_path = crate::codex_config::get_codex_auth_path();
        if auth_path.exists() {
            match crate::config::read_json_file::<Value>(&auth_path) {
                Ok(auth) => {
                    let cfg = match crate::codex_config::read_and_validate_codex_config_text() {
                        Ok(s) => s,
                        Err(e) => {
                            log::warn!("读取/校验 Codex live config.toml 失败: {}", e);
                            String::new()
                        }
                    };
                    Some((
                        "default".to_string(),
                        serde_json::json!({"auth": auth, "config": cfg}),
                    ))
                }
                Err(e) => {
                    log::warn!("读取 Codex live auth.json 失败: {}", e);
                    None
                }
            }
        } else {
            None
        }
    };

    // 合并：Codex（优先 live，然后副本） - 去重键: name + OPENAI_API_KEY（直接比较）
    config.ensure_app(&AppType::Codex);
    let manager = config.get_manager_mut(&AppType::Codex).unwrap();
    let mut ids: HashSet<String> = manager.providers.keys().cloned().collect();
    let mut live_codex_id: Option<String> = None;

    if let Some((name, value)) = &live_codex {
        let cand_key = extract_codex_api_key(value);
        let exist_id = manager.providers.iter().find_map(|(id, p)| {
            let pk = extract_codex_api_key(&p.settings_config);
            if norm_name(&p.name) == norm_name(name) && pk == cand_key {
                Some(id.clone())
            } else {
                None
            }
        });
        if let Some(exist_id) = exist_id {
            if let Some(prov) = manager.providers.get_mut(&exist_id) {
                log::info!("合并到已存在 Codex 供应商 '{}' (by name+key)", name);
                prov.settings_config = value.clone();
                live_codex_id = Some(exist_id);
            }
        } else {
            let id = next_unique_id(&ids, name);
            ids.insert(id.clone());
            let provider =
                crate::provider::Provider::with_id(id.clone(), name.clone(), value.clone(), None);
            manager.providers.insert(provider.id.clone(), provider);
            live_codex_id = Some(id);
        }
    }
    for (name, authp, cfgp, value) in codex_items.iter() {
        let cand_key = extract_codex_api_key(value);
        let exist_id = manager.providers.iter().find_map(|(id, p)| {
            let pk = extract_codex_api_key(&p.settings_config);
            if norm_name(&p.name) == norm_name(name) && pk == cand_key {
                Some(id.clone())
            } else {
                None
            }
        });
        if let Some(exist_id) = exist_id {
            if let Some(prov) = manager.providers.get_mut(&exist_id) {
                log::info!(
                    "覆盖 Codex 供应商 '{}' 来自 {:?}/{:?} (by name+key)",
                    name,
                    authp,
                    cfgp
                );
                prov.settings_config = value.clone();
            }
        } else {
            let id = next_unique_id(&ids, name);
            ids.insert(id.clone());
            let provider =
                crate::provider::Provider::with_id(id.clone(), name.clone(), value.clone(), None);
            manager.providers.insert(provider.id.clone(), provider);
        }
    }

    // 若当前为空，将 live 导入项设为当前
    {
        let manager = config.get_manager_mut(&AppType::Claude).unwrap();
        if manager.current.is_empty() {
            if let Some(id) = live_claude_id {
                manager.current = id;
            }
        }
    }
    {
        let manager = config.get_manager_mut(&AppType::Codex).unwrap();
        if manager.current.is_empty() {
            if let Some(id) = live_codex_id {
                manager.current = id;
            }
        }
    }

    // 归档副本文件
    for (_, p, _) in claude_items.into_iter() {
        match archive_file(ts, "claude", &p) {
            Ok(Some(_)) => {
                let _ = delete_file(&p);
            }
            _ => {
                // 归档失败则不要删除原文件，保守处理
            }
        }
    }
    for (_, ap, cp, _) in codex_items.into_iter() {
        if let Some(ap) = ap {
            match archive_file(ts, "codex", &ap) {
                Ok(Some(_)) => {
                    let _ = delete_file(&ap);
                }
                _ => {}
            }
        }
        if let Some(cp) = cp {
            match archive_file(ts, "codex", &cp) {
                Ok(Some(_)) => {
                    let _ = delete_file(&cp);
                }
                _ => {}
            }
        }
    }

    // 标记完成
    // 仅在迁移阶段执行一次全量去重（忽略大小写的名称 + API Key）
    let removed = dedupe_config(config);
    if removed > 0 {
        log::info!("迁移阶段已去重重复供应商 {} 个", removed);
    }

    fs::write(&marker, b"done").map_err(|e| format!("写入迁移标记失败: {}", e))?;
    Ok(true)
}

/// 启动时对现有配置做一次去重：按名称(忽略大小写)+API Key
pub fn dedupe_config(config: &mut MultiAppConfig) -> usize {
    use std::collections::HashMap as Map;

    fn dedupe_one(
        mgr: &mut crate::provider::ProviderManager,
        extract_key: &dyn Fn(&Value) -> Option<String>,
    ) -> usize {
        let mut keep: Map<String, String> = Map::new(); // key -> id 保留
        let mut remove: Vec<String> = Vec::new();
        for (id, p) in mgr.providers.iter() {
            let k = format!(
                "{}|{}",
                norm_name(&p.name),
                extract_key(&p.settings_config).unwrap_or_default()
            );
            if let Some(exist_id) = keep.get(&k) {
                // 若当前是正在使用的，则用当前替换之前的，反之丢弃当前
                if *id == mgr.current {
                    // 替换：把原先的标记为删除，改保留为当前
                    remove.push(exist_id.clone());
                    keep.insert(k, id.clone());
                } else {
                    remove.push(id.clone());
                }
            } else {
                keep.insert(k, id.clone());
            }
        }
        for id in remove.iter() {
            mgr.providers.remove(id);
        }
        remove.len()
    }

    let mut removed = 0;
    if let Some(mgr) = config.get_manager_mut(&crate::app_config::AppType::Claude) {
        removed += dedupe_one(mgr, &extract_claude_api_key);
    }
    if let Some(mgr) = config.get_manager_mut(&crate::app_config::AppType::Codex) {
        removed += dedupe_one(mgr, &extract_codex_api_key);
    }
    removed
}

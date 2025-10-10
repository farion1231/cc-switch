use serde_json::{json, Value};
use std::collections::HashMap;

use crate::app_config::{AppType, McpConfig, MultiAppConfig};

/// 基础校验：允许 stdio/http；或省略 type（视为 stdio）。对应必填字段存在
fn validate_mcp_spec(spec: &Value) -> Result<(), String> {
    if !spec.is_object() {
        return Err("MCP 服务器定义必须为 JSON 对象".into());
    }
    let t_opt = spec.get("type").and_then(|x| x.as_str());
    // 支持两种：stdio/http；若缺省 type 则按 stdio 处理（与社区常见 .mcp.json 一致）
    let is_stdio = t_opt.map(|t| t == "stdio").unwrap_or(true);
    let is_http = t_opt.map(|t| t == "http").unwrap_or(false);
    
    if !(is_stdio || is_http) {
        return Err("MCP 服务器 type 必须是 'stdio' 或 'http'（或省略表示 stdio）".into());
    }

    if is_stdio {
        let cmd = spec.get("command").and_then(|x| x.as_str()).unwrap_or("");
        if cmd.trim().is_empty() {
            return Err("stdio 类型的 MCP 服务器缺少 command 字段".into());
        }
    }
    if is_http {
        let url = spec.get("url").and_then(|x| x.as_str()).unwrap_or("");
        if url.trim().is_empty() {
            return Err("http 类型的 MCP 服务器缺少 url 字段".into());
        }
    }
    Ok(())
}

/// 返回已启用的 MCP 服务器（过滤 enabled==true）
fn collect_enabled_servers(cfg: &McpConfig) -> HashMap<String, Value> {
    let mut out = HashMap::new();
    for (id, spec) in cfg.servers.iter() {
        let enabled = spec
            .get("enabled")
            .and_then(|v| v.as_bool())
            .unwrap_or(false);
        if enabled {
            out.insert(id.clone(), spec.clone());
        }
    }
    out
}

pub fn get_servers_snapshot_for(config: &MultiAppConfig, app: &AppType) -> HashMap<String, Value> {
    config.mcp_for(app).servers.clone()
}

pub fn upsert_in_config_for(
    config: &mut MultiAppConfig,
    app: &AppType,
    id: &str,
    spec: Value,
) -> Result<bool, String> {
    if id.trim().is_empty() {
        return Err("MCP 服务器 ID 不能为空".into());
    }
    validate_mcp_spec(&spec)?;

    // 默认 enabled 不强制设值；若字段不存在则保持不变（或 UI 决定）
    if spec.get("enabled").is_none() {
        // 缺省不设，以便后续 set_enabled 独立控制
    }

    let servers = &mut config.mcp_for_mut(app).servers;
    let before = servers.get(id).cloned();
    servers.insert(id.to_string(), spec);

    Ok(before.is_none())
}

pub fn delete_in_config_for(config: &mut MultiAppConfig, app: &AppType, id: &str) -> Result<bool, String> {
    if id.trim().is_empty() {
        return Err("MCP 服务器 ID 不能为空".into());
    }
    let existed = config.mcp_for_mut(app).servers.remove(id).is_some();
    Ok(existed)
}

/// 设置启用状态并同步到 ~/.claude.json
pub fn set_enabled_and_sync_for(
    config: &mut MultiAppConfig,
    app: &AppType,
    id: &str,
    enabled: bool,
) -> Result<bool, String> {
    if id.trim().is_empty() {
        return Err("MCP 服务器 ID 不能为空".into());
    }
    if let Some(spec) = config.mcp_for_mut(app).servers.get_mut(id) {
        // 写入 enabled 字段
        let mut obj = spec.as_object().cloned().ok_or_else(|| "MCP 服务器定义必须为 JSON 对象".to_string())?;
        obj.insert("enabled".into(), json!(enabled));
        *spec = Value::Object(obj);
    } else {
        // 若不存在则直接返回 false
        return Ok(false);
    }

    // 同步启用项
    match app {
        AppType::Claude => {
            // 将启用项投影到 ~/.claude.json
            sync_enabled_to_claude(config)?;
        }
        AppType::Codex => {
            // 将启用项投影到 ~/.codex/config.toml
            sync_enabled_to_codex(config)?;
        }
    }
    Ok(true)
}

/// 将 config.json 中 enabled==true 的项投影写入 ~/.claude.json
pub fn sync_enabled_to_claude(config: &MultiAppConfig) -> Result<(), String> {
    let enabled = collect_enabled_servers(&config.mcp.claude);
    crate::claude_mcp::set_mcp_servers_map(&enabled)
}

/// 从 ~/.claude.json 导入 mcpServers 到 config.json（设为 enabled=true）。
/// 已存在的项仅强制 enabled=true，不覆盖其他字段。
pub fn import_from_claude(config: &mut MultiAppConfig) -> Result<usize, String> {
    let text_opt = crate::claude_mcp::read_mcp_json()?;
    let Some(text) = text_opt else { return Ok(0) };
    let v: Value = serde_json::from_str(&text).map_err(|e| format!("解析 ~/.claude.json 失败: {}", e))?;
    let Some(map) = v.get("mcpServers").and_then(|x| x.as_object()) else { return Ok(0) };

    let mut changed = 0usize;
    for (id, spec) in map.iter() {
        // 校验目标 spec
        validate_mcp_spec(spec)?;

        // 规范化为对象
        let mut obj = spec.as_object().cloned().ok_or_else(|| "MCP 服务器定义必须为 JSON 对象".to_string())?;
        obj.insert("enabled".into(), json!(true));

        let entry = config.mcp_for_mut(&AppType::Claude).servers.entry(id.clone());
        use std::collections::hash_map::Entry;
        match entry {
            Entry::Vacant(vac) => {
                vac.insert(Value::Object(obj));
                changed += 1;
            }
            Entry::Occupied(mut occ) => {
                // 只确保 enabled=true；不覆盖其他字段
                if let Some(mut existing) = occ.get().as_object().cloned() {
                    let prev = existing.get("enabled").and_then(|b| b.as_bool()).unwrap_or(false);
                    if !prev {
                        existing.insert("enabled".into(), json!(true));
                        occ.insert(Value::Object(existing));
                        changed += 1;
                    }
                }
            }
        }
    }
    Ok(changed)
}

/// 从 ~/.codex/config.toml 导入 MCP 到 config.json（Codex 作用域），并将导入项设为 enabled=true。
/// 支持两种 schema：[mcp.servers.<id>] 与 [mcp_servers.<id>]。
/// 已存在的项仅强制 enabled=true，不覆盖其他字段。
pub fn import_from_codex(config: &mut MultiAppConfig) -> Result<usize, String> {
    let text = crate::codex_config::read_and_validate_codex_config_text()?;
    if text.trim().is_empty() {
        return Ok(0);
    }

    let root: toml::Table = toml::from_str(&text)
        .map_err(|e| format!("解析 ~/.codex/config.toml 失败: {}", e))?;

    let mut changed_total = 0usize;

    // helper：处理一组 servers 表
    let mut import_servers_tbl = |servers_tbl: &toml::value::Table| {
        let mut changed = 0usize;
        for (id, entry_val) in servers_tbl.iter() {
            let Some(entry_tbl) = entry_val.as_table() else { continue };

            // type 缺省为 stdio
            let typ = entry_tbl
                .get("type")
                .and_then(|v| v.as_str())
                .unwrap_or("stdio");

            // 构建 JSON 规范
            let mut spec = serde_json::Map::new();
            spec.insert("type".into(), json!(typ));

            match typ {
                "stdio" => {
                    if let Some(cmd) = entry_tbl.get("command").and_then(|v| v.as_str()) {
                        spec.insert("command".into(), json!(cmd));
                    }
                    if let Some(args) = entry_tbl.get("args").and_then(|v| v.as_array()) {
                        let arr = args
                            .iter()
                            .filter_map(|x| x.as_str())
                            .map(|s| json!(s))
                            .collect::<Vec<_>>();
                        if !arr.is_empty() {
                            spec.insert("args".into(), serde_json::Value::Array(arr));
                        }
                    }
                    if let Some(cwd) = entry_tbl.get("cwd").and_then(|v| v.as_str()) {
                        if !cwd.trim().is_empty() {
                            spec.insert("cwd".into(), json!(cwd));
                        }
                    }
                    if let Some(env_tbl) = entry_tbl.get("env").and_then(|v| v.as_table()) {
                        let mut env_json = serde_json::Map::new();
                        for (k, v) in env_tbl.iter() {
                            if let Some(sv) = v.as_str() {
                                env_json.insert(k.clone(), json!(sv));
                            }
                        }
                        if !env_json.is_empty() {
                            spec.insert("env".into(), serde_json::Value::Object(env_json));
                        }
                    }
                }
                "http" => {
                    if let Some(url) = entry_tbl.get("url").and_then(|v| v.as_str()) {
                        spec.insert("url".into(), json!(url));
                    }
                    if let Some(headers_tbl) = entry_tbl.get("headers").and_then(|v| v.as_table()) {
                        let mut headers_json = serde_json::Map::new();
                        for (k, v) in headers_tbl.iter() {
                            if let Some(sv) = v.as_str() {
                                headers_json.insert(k.clone(), json!(sv));
                            }
                        }
                        if !headers_json.is_empty() {
                            spec.insert("headers".into(), serde_json::Value::Object(headers_json));
                        }
                    }
                }
                _ => {}
            }

            let spec_v = serde_json::Value::Object(spec);

            // 校验
            if let Err(e) = validate_mcp_spec(&spec_v) {
                log::warn!("跳过无效 Codex MCP 项 '{}': {}", id, e);
                continue;
            }

            // 合并：仅强制 enabled=true
            use std::collections::hash_map::Entry;
            let entry = config
                .mcp_for_mut(&AppType::Codex)
                .servers
                .entry(id.clone());
            match entry {
                Entry::Vacant(vac) => {
                    let mut obj = spec_v.as_object().cloned().unwrap_or_default();
                    obj.insert("enabled".into(), json!(true));
                    vac.insert(serde_json::Value::Object(obj));
                    changed += 1;
                }
                Entry::Occupied(mut occ) => {
                    if let Some(mut existing) = occ.get().as_object().cloned() {
                        let prev = existing
                            .get("enabled")
                            .and_then(|b| b.as_bool())
                            .unwrap_or(false);
                        if !prev {
                            existing.insert("enabled".into(), json!(true));
                            occ.insert(serde_json::Value::Object(existing));
                            changed += 1;
                        }
                    }
                }
            }
        }
        changed
    };

    // 1) 处理 mcp.servers
    if let Some(mcp_val) = root.get("mcp") {
        if let Some(mcp_tbl) = mcp_val.as_table() {
            if let Some(servers_val) = mcp_tbl.get("servers") {
                if let Some(servers_tbl) = servers_val.as_table() {
                    changed_total += import_servers_tbl(servers_tbl);
                }
            }
        }
    }

    // 2) 处理 mcp_servers
    if let Some(servers_val) = root.get("mcp_servers") {
        if let Some(servers_tbl) = servers_val.as_table() {
            changed_total += import_servers_tbl(servers_tbl);
        }
    }

    Ok(changed_total)
}

/// 将 config.json 中 Codex 的 enabled==true 项以 TOML 形式写入 ~/.codex/config.toml 的 [mcp.servers]
/// 策略：
/// - 读取现有 config.toml；若语法无效则报错，不尝试覆盖
/// - 重写根下的 `mcp` 节点（整体替换），其他节点保持不变
/// - 仅写入启用项；无启用项时移除 `mcp` 节点
pub fn sync_enabled_to_codex(config: &MultiAppConfig) -> Result<(), String> {
    use toml::{value::Value as TomlValue, Table as TomlTable};

    // 1) 收集启用项（Codex 维度）
    let enabled = collect_enabled_servers(&config.mcp.codex);

    // 2) 读取现有 config.toml 并解析为 Table（允许空文件）
    let base_text = crate::codex_config::read_and_validate_codex_config_text()?;
    let mut root: TomlTable = if base_text.trim().is_empty() {
        TomlTable::new()
    } else {
        toml::from_str::<TomlTable>(&base_text)
            .map_err(|e| format!("解析 config.toml 失败: {}", e))?
    };

    // 3) 写入 servers 表（支持 mcp.servers 与 mcp_servers；优先沿用已有风格，默认 mcp_servers）
    let prefer_mcp_servers = root.contains_key("mcp_servers") || !root.contains_key("mcp");
    if enabled.is_empty() {
        // 无启用项：移除两种节点
        root.remove("mcp");
        root.remove("mcp_servers");
    } else {
        let mut servers_tbl = TomlTable::new();

        for (id, spec) in enabled.iter() {
            let mut s = TomlTable::new();

            // 类型（缺省视为 stdio）
            let typ = spec
                .get("type")
                .and_then(|v| v.as_str())
                .unwrap_or("stdio");
            s.insert("type".into(), TomlValue::String(typ.to_string()));

            match typ {
                "stdio" => {
                    let cmd = spec
                        .get("command")
                        .and_then(|v| v.as_str())
                        .unwrap_or("")
                        .to_string();
                    s.insert("command".into(), TomlValue::String(cmd));

                    if let Some(args) = spec.get("args").and_then(|v| v.as_array()) {
                        let arr = args
                            .iter()
                            .filter_map(|x| x.as_str())
                            .map(|x| TomlValue::String(x.to_string()))
                            .collect::<Vec<_>>();
                        if !arr.is_empty() {
                            s.insert("args".into(), TomlValue::Array(arr));
                        }
                    }

                    if let Some(cwd) = spec.get("cwd").and_then(|v| v.as_str()) {
                        if !cwd.trim().is_empty() {
                            s.insert("cwd".into(), TomlValue::String(cwd.to_string()));
                        }
                    }

                    if let Some(env) = spec.get("env").and_then(|v| v.as_object()) {
                        let mut env_tbl = TomlTable::new();
                        for (k, v) in env.iter() {
                            if let Some(sv) = v.as_str() {
                                env_tbl.insert(k.clone(), TomlValue::String(sv.to_string()));
                            }
                        }
                        if !env_tbl.is_empty() {
                            s.insert("env".into(), TomlValue::Table(env_tbl));
                        }
                    }
                }
                "http" => {
                    let url = spec
                        .get("url")
                        .and_then(|v| v.as_str())
                        .unwrap_or("")
                        .to_string();
                    s.insert("url".into(), TomlValue::String(url));

                    if let Some(headers) = spec.get("headers").and_then(|v| v.as_object()) {
                        let mut h_tbl = TomlTable::new();
                        for (k, v) in headers.iter() {
                            if let Some(sv) = v.as_str() {
                                h_tbl.insert(k.clone(), TomlValue::String(sv.to_string()));
                            }
                        }
                        if !h_tbl.is_empty() {
                            s.insert("headers".into(), TomlValue::Table(h_tbl));
                        }
                    }
                }
                _ => {}
            }

            servers_tbl.insert(id.clone(), TomlValue::Table(s));
        }

        if prefer_mcp_servers {
            root.insert("mcp_servers".into(), TomlValue::Table(servers_tbl));
            root.remove("mcp");
        } else {
            let mut mcp_tbl = TomlTable::new();
            mcp_tbl.insert("servers".into(), TomlValue::Table(servers_tbl));
            root.insert("mcp".into(), TomlValue::Table(mcp_tbl));
            root.remove("mcp_servers");
        }
    }

    // 4) 序列化并写回 config.toml（仅改 TOML，不触碰 auth.json）
    let new_text = toml::to_string(&TomlValue::Table(root))
        .map_err(|e| format!("序列化 config.toml 失败: {}", e))?;
    let path = crate::codex_config::get_codex_config_path();
    crate::config::write_text_file(&path, &new_text)?;

    Ok(())
}

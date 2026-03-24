//! Codex MCP 同步和导入模块
//!
//! 包含 Codex 的 MCP 配置管理：
//! - 从 ~/.codex/config.toml 导入
//! - 同步到 ~/.codex/config.toml
//! - JSON 到 TOML 的转换逻辑

use serde_json::{json, Value};
use std::collections::HashMap;

use crate::app_config::{AppType, McpConfig, MultiAppConfig};
use crate::error::AppError;

use super::validation::{extract_server_spec, normalize_server_spec};
use super::{apply_parsed_import, build_imported_server, invalid_issue, ParsedImport};

fn should_sync_codex_mcp() -> bool {
    // Codex 未安装/未初始化时：~/.codex 目录不存在。
    // 按用户偏好：目录缺失时跳过写入/删除，不创建任何文件或目录。
    crate::codex_config::get_codex_config_dir().exists()
}

/// 返回已启用的 MCP 服务器（过滤 enabled==true）
fn collect_enabled_servers(cfg: &McpConfig) -> HashMap<String, Value> {
    let mut out = HashMap::new();
    for (id, entry) in cfg.servers.iter() {
        let enabled = entry
            .get("enabled")
            .and_then(|v| v.as_bool())
            .unwrap_or(false);
        if !enabled {
            continue;
        }
        match extract_server_spec(entry) {
            Ok(spec) => {
                out.insert(id.clone(), spec);
            }
            Err(err) => {
                log::warn!("跳过无效的 MCP 条目 '{id}': {err}");
            }
        }
    }
    out
}

/// 从 ~/.codex/config.toml 导入 MCP 到统一结构（v3.7.0+）
///
/// 格式支持：
/// - 正确格式：[mcp_servers.*]（Codex 官方标准）
/// - 错误格式：[mcp.servers.*]（容错读取，用于迁移错误写入的配置）
///
/// 已存在的服务器将启用 Codex 应用，不覆盖其他字段和应用状态
pub fn import_from_codex(config: &mut MultiAppConfig) -> Result<usize, AppError> {
    let parsed = parse_import_from_codex()?;
    apply_parsed_import(config, parsed, AppType::Codex)
}

pub(crate) fn parse_import_from_codex() -> Result<ParsedImport, AppError> {
    let text = crate::codex_config::read_and_validate_codex_config_text()?;
    if text.trim().is_empty() {
        return Ok(ParsedImport::default());
    }

    let root: toml::Table = toml::from_str(&text)
        .map_err(|e| AppError::McpValidation(format!("解析 ~/.codex/config.toml 失败: {e}")))?;

    let mut parsed = ParsedImport::default();

    // helper：处理一组 servers 表
    let mut import_servers_tbl = |servers_tbl: &toml::value::Table| {
        for (id, entry_val) in servers_tbl.iter() {
            let Some(entry_tbl) = entry_val.as_table() else {
                continue;
            };

            let typ = entry_tbl.get("type").and_then(|v| v.as_str());
            if let Some(typ) = typ {
                if !matches!(typ, "stdio" | "http" | "sse") {
                    log::warn!("跳过未知类型 '{typ}' 的 Codex MCP 项 '{id}'");
                    parsed.issues.push(invalid_issue(
                        id.clone(),
                        AppType::Codex,
                        format!("未知的 Codex MCP 类型: {typ}"),
                    ));
                    continue;
                }
            }

            let mut spec = serde_json::Map::new();
            if let Some(typ) = typ {
                spec.insert("type".into(), json!(typ));
            }

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
            if let Some(url) = entry_tbl.get("url").and_then(|v| v.as_str()) {
                spec.insert("url".into(), json!(url));
            }
            let headers_tbl = entry_tbl
                .get("http_headers")
                .and_then(|v| v.as_table())
                .or_else(|| entry_tbl.get("headers").and_then(|v| v.as_table()));
            if let Some(headers_tbl) = headers_tbl {
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

            // 2. 处理扩展字段和其他未知字段（通用 TOML → JSON 转换）
            for (key, toml_val) in entry_tbl.iter() {
                // 跳过已处理的核心字段
                if matches!(
                    key.as_str(),
                    "type" | "command" | "args" | "env" | "cwd" | "url" | "http_headers"
                        | "headers"
                ) {
                    continue;
                }

                // 通用 TOML 值到 JSON 值转换
                let json_val = match toml_val {
                    toml::Value::String(s) => Some(json!(s)),
                    toml::Value::Integer(i) => Some(json!(i)),
                    toml::Value::Float(f) => Some(json!(f)),
                    toml::Value::Boolean(b) => Some(json!(b)),
                    toml::Value::Array(arr) => {
                        // 只支持简单类型数组
                        let json_arr: Vec<serde_json::Value> = arr
                            .iter()
                            .filter_map(|item| match item {
                                toml::Value::String(s) => Some(json!(s)),
                                toml::Value::Integer(i) => Some(json!(i)),
                                toml::Value::Float(f) => Some(json!(f)),
                                toml::Value::Boolean(b) => Some(json!(b)),
                                _ => None,
                            })
                            .collect();
                        if !json_arr.is_empty() {
                            Some(serde_json::Value::Array(json_arr))
                        } else {
                            log::debug!("跳过复杂数组字段 '{key}' (TOML → JSON)");
                            None
                        }
                    }
                    toml::Value::Table(tbl) => {
                        // 浅层表转为 JSON 对象（仅支持字符串值）
                        let mut json_obj = serde_json::Map::new();
                        for (k, v) in tbl.iter() {
                            if let Some(s) = v.as_str() {
                                json_obj.insert(k.clone(), json!(s));
                            }
                        }
                        if !json_obj.is_empty() {
                            Some(serde_json::Value::Object(json_obj))
                        } else {
                            log::debug!("跳过复杂对象字段 '{key}' (TOML → JSON)");
                            None
                        }
                    }
                    toml::Value::Datetime(_) => {
                        log::debug!("跳过日期时间字段 '{key}' (TOML → JSON)");
                        None
                    }
                };

                if let Some(val) = json_val {
                    spec.insert(key.clone(), val);
                    log::debug!("导入扩展字段 '{key}' = {toml_val:?}");
                }
            }

            let spec_v = serde_json::Value::Object(spec);

            match normalize_server_spec(&spec_v) {
                Ok(spec) => parsed
                    .servers
                    .push(build_imported_server(id.clone(), AppType::Codex, spec)),
                Err(e) => {
                    log::warn!("跳过无效 Codex MCP 项 '{id}': {e}");
                    parsed
                        .issues
                        .push(invalid_issue(id.clone(), AppType::Codex, e.to_string()));
                }
            }
        }
    };

    // 1) 处理 mcp.servers
    if let Some(mcp_val) = root.get("mcp") {
        if let Some(mcp_tbl) = mcp_val.as_table() {
            if let Some(servers_val) = mcp_tbl.get("servers") {
                if let Some(servers_tbl) = servers_val.as_table() {
                    import_servers_tbl(servers_tbl);
                }
            }
        }
    }

    // 2) 处理 mcp_servers
    if let Some(servers_val) = root.get("mcp_servers") {
        if let Some(servers_tbl) = servers_val.as_table() {
            import_servers_tbl(servers_tbl);
        }
    }

    Ok(parsed)
}

/// 将 config.json 中 Codex 的 enabled==true 项以 TOML 形式写入 ~/.codex/config.toml
///
/// 格式策略：
/// - 唯一正确格式：[mcp_servers] 顶层表（Codex 官方标准）
/// - 自动清理错误格式：[mcp.servers]（如果存在）
/// - 读取现有 config.toml；若语法无效则报错，不尝试覆盖
/// - 仅更新 `mcp_servers` 表，保留其它键
/// - 仅写入启用项；无启用项时清理 mcp_servers 表
pub fn sync_enabled_to_codex(config: &MultiAppConfig) -> Result<(), AppError> {
    if !should_sync_codex_mcp() {
        return Ok(());
    }
    use toml_edit::{Item, Table};

    // 1) 收集启用项（Codex 维度）
    let enabled = collect_enabled_servers(&config.mcp.codex);

    // 2) 读取现有 config.toml 文本；保持无效 TOML 的错误返回（不覆盖文件）
    let base_text = crate::codex_config::read_and_validate_codex_config_text()?;

    // 3) 使用 toml_edit 解析（允许空文件）
    let mut doc = if base_text.trim().is_empty() {
        toml_edit::DocumentMut::default()
    } else {
        base_text
            .parse::<toml_edit::DocumentMut>()
            .map_err(|e| AppError::McpValidation(format!("解析 config.toml 失败: {e}")))?
    };

    // 4) 清理可能存在的错误格式 [mcp.servers]
    if let Some(mcp_item) = doc.get_mut("mcp") {
        if let Some(tbl) = mcp_item.as_table_like_mut() {
            if tbl.contains_key("servers") {
                log::warn!("检测到错误的 MCP 格式 [mcp.servers]，正在清理并迁移到 [mcp_servers]");
                tbl.remove("servers");
            }
        }
    }

    // 5) 构造目标 servers 表（稳定的键顺序）
    if enabled.is_empty() {
        // 无启用项：移除 mcp_servers 表
        doc.as_table_mut().remove("mcp_servers");
    } else {
        // 构建 servers 表
        let mut servers_tbl = Table::new();
        let mut ids: Vec<_> = enabled.keys().cloned().collect();
        ids.sort();
        for id in ids {
            let spec = enabled.get(&id).expect("spec must exist");
            // 复用通用转换函数（已包含扩展字段支持）
            match json_server_to_toml_table(spec) {
                Ok(table) => {
                    servers_tbl[&id[..]] = Item::Table(table);
                }
                Err(err) => {
                    log::error!("跳过无效的 MCP 服务器 '{id}': {err}");
                }
            }
        }
        // 使用唯一正确的格式：[mcp_servers]
        doc["mcp_servers"] = Item::Table(servers_tbl);
    }

    // 6) 写回（仅改 TOML，不触碰 auth.json）；toml_edit 会尽量保留未改区域的注释/空白/顺序
    let new_text = doc.to_string();
    let path = crate::codex_config::get_codex_config_path();
    crate::config::write_text_file(&path, &new_text)?;
    Ok(())
}

/// 将单个 MCP 服务器同步到 Codex live 配置
/// 始终使用 Codex 官方格式 [mcp_servers]，并清理可能存在的错误格式 [mcp.servers]
pub fn sync_single_server_to_codex(
    _config: &MultiAppConfig,
    id: &str,
    server_spec: &Value,
) -> Result<(), AppError> {
    if !should_sync_codex_mcp() {
        return Ok(());
    }
    use toml_edit::Item;

    // 读取现有的 config.toml
    let config_path = crate::codex_config::get_codex_config_path();

    let mut doc = if config_path.exists() {
        let content =
            std::fs::read_to_string(&config_path).map_err(|e| AppError::io(&config_path, e))?;
        // 尝试解析现有配置，如果失败则创建新文档（容错处理）
        match content.parse::<toml_edit::DocumentMut>() {
            Ok(doc) => doc,
            Err(e) => {
                log::warn!("解析 Codex config.toml 失败: {e}，将创建新配置");
                toml_edit::DocumentMut::new()
            }
        }
    } else {
        toml_edit::DocumentMut::new()
    };

    // 清理可能存在的错误格式 [mcp.servers]
    if let Some(mcp_item) = doc.get_mut("mcp") {
        if let Some(tbl) = mcp_item.as_table_like_mut() {
            if tbl.contains_key("servers") {
                log::warn!("检测到错误的 MCP 格式 [mcp.servers]，正在清理并迁移到 [mcp_servers]");
                tbl.remove("servers");
            }
        }
    }

    // 确保 [mcp_servers] 表存在
    if !doc.contains_key("mcp_servers") {
        doc["mcp_servers"] = toml_edit::table();
    }

    // 将 JSON 服务器规范转换为 TOML 表
    let toml_table = json_server_to_toml_table(server_spec)?;

    // 使用唯一正确的格式：[mcp_servers]
    doc["mcp_servers"][id] = Item::Table(toml_table);

    // 写回文件
    let new_text = doc.to_string();
    crate::config::write_text_file(&config_path, &new_text)?;

    Ok(())
}

/// 从 Codex live 配置中移除单个 MCP 服务器
/// 从正确的 [mcp_servers] 表中删除，同时清理可能存在于错误位置 [mcp.servers] 的数据
pub fn remove_server_from_codex(id: &str) -> Result<(), AppError> {
    if !should_sync_codex_mcp() {
        return Ok(());
    }
    let config_path = crate::codex_config::get_codex_config_path();

    if !config_path.exists() {
        return Ok(()); // 文件不存在，无需删除
    }

    let content =
        std::fs::read_to_string(&config_path).map_err(|e| AppError::io(&config_path, e))?;

    // 尝试解析现有配置，如果失败则直接返回（无法删除不存在的内容）
    let mut doc = match content.parse::<toml_edit::DocumentMut>() {
        Ok(doc) => doc,
        Err(e) => {
            log::warn!("解析 Codex config.toml 失败: {e}，跳过删除操作");
            return Ok(());
        }
    };

    // 从正确的位置删除：[mcp_servers]
    if let Some(mcp_servers) = doc.get_mut("mcp_servers").and_then(|s| s.as_table_mut()) {
        mcp_servers.remove(id);
    }

    // 同时清理可能存在于错误位置的数据：[mcp.servers]（如果存在）
    if let Some(mcp_table) = doc.get_mut("mcp").and_then(|t| t.as_table_mut()) {
        if let Some(servers) = mcp_table.get_mut("servers").and_then(|s| s.as_table_mut()) {
            if servers.remove(id).is_some() {
                log::warn!("从错误的 MCP 格式 [mcp.servers] 中清理了服务器 '{id}'");
            }
        }
    }

    // 写回文件
    let new_text = doc.to_string();
    crate::config::write_text_file(&config_path, &new_text)?;

    Ok(())
}

// ============================================================================
// TOML 转换辅助函数
// ============================================================================

/// 通用 JSON 值到 TOML 值转换器（支持简单类型和浅层嵌套）
///
/// 支持的类型转换：
/// - String → TOML String
/// - Number (i64) → TOML Integer
/// - Number (f64) → TOML Float
/// - Boolean → TOML Boolean
/// - Array[简单类型] → TOML Array
/// - Object → TOML Inline Table (仅字符串值)
///
/// 不支持的类型（返回 None）：
/// - null
/// - 深度嵌套对象
/// - 混合类型数组
fn json_value_to_toml_item(value: &Value, field_name: &str) -> Option<toml_edit::Item> {
    use toml_edit::{Array, InlineTable, Item};

    match value {
        Value::String(s) => Some(toml_edit::value(s.as_str())),

        Value::Number(n) => {
            if let Some(i) = n.as_i64() {
                Some(toml_edit::value(i))
            } else if let Some(f) = n.as_f64() {
                Some(toml_edit::value(f))
            } else {
                log::warn!("跳过字段 '{field_name}': 无法转换的数字类型 {n}");
                None
            }
        }

        Value::Bool(b) => Some(toml_edit::value(*b)),

        Value::Array(arr) => {
            // 只支持简单类型的数组（字符串、数字、布尔）
            let mut toml_arr = Array::default();
            let mut all_same_type = true;

            for item in arr {
                match item {
                    Value::String(s) => toml_arr.push(s.as_str()),
                    Value::Number(n) if n.is_i64() => {
                        if let Some(i) = n.as_i64() {
                            toml_arr.push(i);
                        } else {
                            all_same_type = false;
                            break;
                        }
                    }
                    Value::Number(n) if n.is_f64() => {
                        if let Some(f) = n.as_f64() {
                            toml_arr.push(f);
                        } else {
                            all_same_type = false;
                            break;
                        }
                    }
                    Value::Bool(b) => toml_arr.push(*b),
                    _ => {
                        all_same_type = false;
                        break;
                    }
                }
            }

            if all_same_type && !toml_arr.is_empty() {
                Some(Item::Value(toml_edit::Value::Array(toml_arr)))
            } else {
                log::warn!("跳过字段 '{field_name}': 不支持的数组类型（混合类型或嵌套结构）");
                None
            }
        }

        Value::Object(obj) => {
            // 只支持浅层对象（所有值都是字符串）→ TOML Inline Table
            let mut inline_table = InlineTable::new();
            let mut all_strings = true;

            for (k, v) in obj {
                if let Some(s) = v.as_str() {
                    // InlineTable 需要 Value 类型，toml_edit::value() 返回 Item，需要提取内部的 Value
                    inline_table.insert(k, s.into());
                } else {
                    all_strings = false;
                    break;
                }
            }

            if all_strings && !inline_table.is_empty() {
                Some(Item::Value(toml_edit::Value::InlineTable(inline_table)))
            } else {
                log::warn!("跳过字段 '{field_name}': 对象值包含非字符串类型，建议使用子表语法");
                None
            }
        }

        Value::Null => {
            log::debug!("跳过字段 '{field_name}': TOML 不支持 null 值");
            None
        }
    }
}

/// Helper: 将 JSON MCP 服务器规范转换为 toml_edit::Table
///
/// 策略：
/// 1. 核心字段（type, command, args, url, headers, env, cwd）使用强类型处理
/// 2. 扩展字段（timeout、retry 等）通过白名单列表自动转换
/// 3. 其他未知字段使用通用转换器尝试转换
fn json_server_to_toml_table(spec: &Value) -> Result<toml_edit::Table, AppError> {
    use toml_edit::{Array, Item, Table};

    let spec = normalize_server_spec(spec)?;
    let mut t = Table::new();
    let typ = spec
        .get("type")
        .and_then(|v| v.as_str())
        .ok_or_else(|| AppError::McpValidation("MCP 服务器缺少合法的 type 字段".into()))?;
    t["type"] = toml_edit::value(typ);

    // 定义核心字段（已在下方处理，跳过通用转换）
    let core_fields = match typ {
        "stdio" => vec!["type", "command", "args", "env", "cwd"],
        "http" | "sse" => vec!["type", "url", "headers", "http_headers"],
        _ => {
            return Err(AppError::McpValidation(format!(
                "不支持的 MCP 服务器类型: {typ}"
            )));
        }
    };

    // 定义扩展字段白名单（Codex 常见可选字段）
    let extended_fields = [
        // 通用字段
        "timeout",
        "timeout_ms",
        "startup_timeout_ms",
        "startup_timeout_sec",
        "connection_timeout",
        "read_timeout",
        "debug",
        "log_level",
        "disabled",
        // stdio 特有
        "shell",
        "encoding",
        "working_dir",
        "restart_on_exit",
        "max_restart_count",
        // http/sse 特有
        "retry_count",
        "max_retry_attempts",
        "retry_delay",
        "cache_tools_list",
        "verify_ssl",
        "insecure",
        "proxy",
    ];

    // 1. 处理核心字段（强类型）
    match typ {
        "stdio" => {
            let cmd = spec
                .get("command")
                .and_then(|v| v.as_str())
                .ok_or_else(|| AppError::McpValidation("stdio 类型的 MCP 服务器缺少 command 字段".into()))?;
            t["command"] = toml_edit::value(cmd);

            if let Some(args) = spec.get("args").and_then(|v| v.as_array()) {
                let mut arr_v = Array::default();
                for a in args.iter().filter_map(|x| x.as_str()) {
                    arr_v.push(a);
                }
                if !arr_v.is_empty() {
                    t["args"] = Item::Value(toml_edit::Value::Array(arr_v));
                }
            }

            if let Some(cwd) = spec.get("cwd").and_then(|v| v.as_str()) {
                if !cwd.trim().is_empty() {
                    t["cwd"] = toml_edit::value(cwd);
                }
            }

            if let Some(env) = spec.get("env").and_then(|v| v.as_object()) {
                let mut env_tbl = Table::new();
                for (k, v) in env.iter() {
                    if let Some(s) = v.as_str() {
                        env_tbl[&k[..]] = toml_edit::value(s);
                    }
                }
                if !env_tbl.is_empty() {
                    t["env"] = Item::Table(env_tbl);
                }
            }
        }
        "http" | "sse" => {
            let url = spec
                .get("url")
                .and_then(|v| v.as_str())
                .ok_or_else(|| AppError::McpValidation(format!("{typ} 类型的 MCP 服务器缺少 url 字段")))?;
            t["url"] = toml_edit::value(url);

            if let Some(headers) = spec.get("headers").and_then(|v| v.as_object()) {
                let mut h_tbl = Table::new();
                for (k, v) in headers.iter() {
                    if let Some(s) = v.as_str() {
                        h_tbl[&k[..]] = toml_edit::value(s);
                    }
                }
                if !h_tbl.is_empty() {
                    t["http_headers"] = Item::Table(h_tbl);
                }
            }
        }
        _ => unreachable!("type already validated"),
    }

    // 2. 处理扩展字段和其他未知字段
    if let Some(obj) = spec.as_object() {
        for (key, value) in obj {
            // 跳过已处理的核心字段
            if core_fields.contains(&key.as_str()) {
                continue;
            }

            // 尝试使用通用转换器
            if let Some(toml_item) = json_value_to_toml_item(value, key) {
                t[&key[..]] = toml_item;

                // 记录扩展字段的处理
                if extended_fields.contains(&key.as_str()) {
                    log::debug!("已转换扩展字段 '{key}' = {value:?}");
                } else {
                    log::info!("已转换自定义字段 '{key}' = {value:?}");
                }
            }
        }
    }

    Ok(t)
}

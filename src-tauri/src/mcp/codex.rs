//! Codex MCP 同步和导入模块
//!
//! 包含 Codex 的 MCP 配置管理：
//! - 从 ~/.codex/config.toml 导入
//! - 同步到 ~/.codex/config.toml
//! - JSON 到 TOML 的转换逻辑

use serde_json::{json, Value};
use std::collections::HashMap;

use crate::app_config::{McpApps, McpConfig, McpServer, MultiAppConfig};
use crate::error::AppError;

use super::validation::{extract_server_spec, validate_server_spec};

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
    let text = crate::codex_config::read_and_validate_codex_config_text()?;
    if text.trim().is_empty() {
        return Ok(0);
    }

    let root: toml::Table = toml::from_str(&text)
        .map_err(|e| AppError::McpValidation(format!("解析 ~/.codex/config.toml 失败: {e}")))?;

    // 确保新结构存在
    let servers = config.mcp.servers.get_or_insert_with(HashMap::new);

    let mut changed_total = 0usize;

    // helper：处理一组 servers 表
    let mut import_servers_tbl = |servers_tbl: &toml::value::Table| {
        let mut changed = 0usize;
        for (id, entry_val) in servers_tbl.iter() {
            let Some(entry_tbl) = entry_val.as_table() else {
                continue;
            };

            // type 缺省为 stdio
            let typ = entry_tbl
                .get("type")
                .and_then(|v| v.as_str())
                .unwrap_or("stdio");

            // 构建 JSON 规范
            let mut spec = serde_json::Map::new();
            spec.insert("type".into(), json!(typ));

            // 核心字段（需要手动处理的字段）
            let core_fields = match typ {
                "stdio" => vec!["type", "command", "args", "env", "cwd"],
                "http" | "sse" => vec!["type", "url", "http_headers"],
                _ => vec!["type"],
            };

            // 1. 处理核心字段（强类型）
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
                "http" | "sse" => {
                    if let Some(url) = entry_tbl.get("url").and_then(|v| v.as_str()) {
                        spec.insert("url".into(), json!(url));
                    }
                    // Read from http_headers (correct Codex format) or headers (legacy) with priority to http_headers
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
                }
                _ => {
                    log::warn!("跳过未知类型 '{typ}' 的 Codex MCP 项 '{id}'");
                    return changed;
                }
            }

            // 2. 处理扩展字段和其他未知字段（通用 TOML → JSON 转换）
            for (key, toml_val) in entry_tbl.iter() {
                // 跳过已处理的核心字段
                if core_fields.contains(&key.as_str()) {
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

            // 校验：单项失败继续处理
            if let Err(e) = validate_server_spec(&spec_v) {
                log::warn!("跳过无效 Codex MCP 项 '{id}': {e}");
                continue;
            }

            if let Some(existing) = servers.get_mut(id) {
                // 已存在：仅启用 Codex 应用
                if !existing.apps.codex {
                    existing.apps.codex = true;
                    changed += 1;
                    log::info!("MCP 服务器 '{id}' 已启用 Codex 应用");
                }
            } else {
                // 新建服务器：默认仅启用 Codex
                servers.insert(
                    id.clone(),
                    McpServer {
                        id: id.clone(),
                        name: id.clone(),
                        server: spec_v,
                        apps: McpApps {
                            claude: false,
                            codex: true,
                            gemini: false,
                            opencode: false,
                            hermes: false,
                        },
                        description: None,
                        homepage: None,
                        docs: None,
                        tags: Vec::new(),
                    },
                );
                changed += 1;
                log::info!("导入新 MCP 服务器 '{id}'");
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

/// 将 config.json 中 Codex 启用的项以 TOML 形式写入 ~/.codex/config.toml。
///
/// 格式策略：
/// - 唯一正确格式：[mcp_servers] 顶层表（Codex 官方标准）
/// - 自动清理错误格式：[mcp.servers]（如果存在）
/// - 读取现有 config.toml；若语法无效则报错，不尝试覆盖
/// - 重写每个 enabled server 的子表时保留其非 cc-switch 管辖的子表
/// - 无启用项时清理 mcp_servers 表（pre-existing 行为）
pub fn sync_enabled_to_codex(config: &MultiAppConfig) -> Result<(), AppError> {
    if !should_sync_codex_mcp() {
        return Ok(());
    }

    let enabled = collect_enabled_servers(&config.mcp.codex);
    let base_text = crate::codex_config::read_and_validate_codex_config_text()?;

    let mut doc = if base_text.trim().is_empty() {
        toml_edit::DocumentMut::default()
    } else {
        base_text
            .parse::<toml_edit::DocumentMut>()
            .map_err(|e| AppError::McpValidation(format!("解析 config.toml 失败: {e}")))?
    };

    apply_enabled_servers_to_doc(&mut doc, &enabled);

    let new_text = doc.to_string();
    let path = crate::codex_config::get_codex_config_path();
    crate::config::write_text_file(&path, &new_text)?;
    Ok(())
}

/// 将单个 MCP 服务器同步到 Codex live 配置。
/// 始终使用 Codex 官方格式 [mcp_servers]，并清理可能存在的错误格式 [mcp.servers]。
/// 重写 [mcp_servers.<id>] 时保留非 cc-switch 管辖的子表（典型为 Codex CLI 写入的 tools.*）。
pub fn sync_single_server_to_codex(
    _config: &MultiAppConfig,
    id: &str,
    server_spec: &Value,
) -> Result<(), AppError> {
    if !should_sync_codex_mcp() {
        return Ok(());
    }

    let config_path = crate::codex_config::get_codex_config_path();

    let mut doc = if config_path.exists() {
        let content =
            std::fs::read_to_string(&config_path).map_err(|e| AppError::io(&config_path, e))?;
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

    apply_single_server_to_doc(&mut doc, id, server_spec)?;

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

    let mut t = Table::new();
    let typ = spec.get("type").and_then(|v| v.as_str()).unwrap_or("stdio");
    t["type"] = toml_edit::value(typ);

    // 定义核心字段（已在下方处理，跳过通用转换）
    let core_fields = match typ {
        "stdio" => vec!["type", "command", "args", "env", "cwd"],
        "http" | "sse" => vec!["type", "url", "http_headers"],
        _ => vec!["type"],
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
            let cmd = spec.get("command").and_then(|v| v.as_str()).unwrap_or("");
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
            let url = spec.get("url").and_then(|v| v.as_str()).unwrap_or("");
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
        _ => {}
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

/// `[mcp_servers.<id>]` 下 cc-switch 当前 / 曾经会写入为子表（toml_edit::Item::Table）的键名。
/// 不在此列表里、但以子表形式存在的键（典型为 Codex CLI 运行时写入的
/// `tools.<tool_name>` 权限声明）一律视为非 cc-switch 管辖，sync / live 写入时必须保留。
///
/// `"headers"` 是 compat 读取名：write 路径已切换到 `http_headers`，但 import
/// 仍会读取旧文件里残留的 `headers`（见 `import_from_codex`），所以这里把它
/// 视为受管，允许在重写时被清理，避免与新写入的 `http_headers` 共存。
const CC_SWITCH_MANAGED_SUBTABLE_KEYS: &[&str] = &["env", "http_headers", "headers"];

/// 抓取 `[mcp_servers.<server_id>]` 下所有非 cc-switch 管辖的子表，
/// 用于在 sync 重写前快照、重写后再 restore。
fn snapshot_unmanaged_subtables(
    doc: &toml_edit::DocumentMut,
    server_id: &str,
) -> Vec<(String, toml_edit::Table)> {
    let Some(server_tbl) = doc
        .get("mcp_servers")
        .and_then(|item| item.as_table())
        .and_then(|t| t.get(server_id))
        .and_then(|item| item.as_table())
    else {
        return Vec::new();
    };

    server_tbl
        .iter()
        .filter_map(|(k, v)| {
            if CC_SWITCH_MANAGED_SUBTABLE_KEYS.contains(&k) {
                return None;
            }
            v.as_table().map(|tbl| (k.to_string(), tbl.clone()))
        })
        .collect()
}

/// 把 snapshot_unmanaged_subtables 抓到的子表写回 `[mcp_servers.<server_id>]`。
/// 在 sync 函数把 server 子表整体重写之后调用。
fn restore_unmanaged_subtables(
    doc: &mut toml_edit::DocumentMut,
    server_id: &str,
    preserved: Vec<(String, toml_edit::Table)>,
) {
    if preserved.is_empty() {
        return;
    }
    let Some(server_tbl) = doc
        .get_mut("mcp_servers")
        .and_then(|item| item.as_table_mut())
        .and_then(|t| t.get_mut(server_id))
        .and_then(|item| item.as_table_mut())
    else {
        return;
    };
    for (k, tbl) in preserved {
        server_tbl.insert(&k, toml_edit::Item::Table(tbl));
    }
}

/// **Layer 2 纯函数**：把 `old_text` 中所有非 cc-switch 管辖的子表
/// （典型为 Codex CLI 运行时写入的 `[mcp_servers.<id>.tools.<tool>]`）
/// 合并进 `new_text`，返回合并后的 TOML 文本。
///
/// 用于 Codex live 写入边界：provider switch / common-config save 会用新 provider
/// 的 stored config 整张覆盖 `~/.codex/config.toml`，但 stored config 不带 runtime
/// 子表，本函数在写入前把旧 live 中的 runtime 子表抢救出来合并进去。
///
/// 语义：
/// - "受管子表"（`CC_SWITCH_MANAGED_SUBTABLE_KEYS`）跳过——它们由 cc-switch 自己负责。
/// - **逐子键深度合并**：旧 live 与新文本同时存在某子表（如 `tools`）时，按叶子键
///   合并而非整体取舍——旧 `tools.search` 与新 `tools.read` 都保留；同名叶子键冲突
///   时新文本优先（处理用户在 provider config 中显式改写 `tools.*` 的少见情况）。
/// - **不为新文本中不存在的 server 凭空建父表**：新文本没有的 server 说明切换后的
///   provider 配置不含它，且 provider 切换路径之后不会再跑 MCP sync 补全 command/url，
///   建一个只有 `tools.*` 的残缺 server 会让 Codex 无法加载——这类孤儿 runtime 子表直接丢弃。
/// - 解析失败 / 旧文件不可读 / 新旧任一方没有 `[mcp_servers]` 时退回原文本，
///   best-effort 不阻塞底层写入。
pub(crate) fn merge_codex_runtime_subtables(new_text: &str, old_text: &str) -> String {
    use toml_edit::DocumentMut;

    let Ok(old_doc) = old_text.parse::<DocumentMut>() else {
        return new_text.to_string();
    };
    let Some(old_mcp_servers) = old_doc
        .get("mcp_servers")
        .and_then(|item| item.as_table())
    else {
        return new_text.to_string();
    };

    // server_id, subtable_key, table
    let mut preserved: Vec<(String, String, toml_edit::Table)> = Vec::new();
    for (server_id, server_item) in old_mcp_servers.iter() {
        let Some(server_tbl) = server_item.as_table() else {
            continue;
        };
        for (k, v) in server_tbl.iter() {
            if CC_SWITCH_MANAGED_SUBTABLE_KEYS.contains(&k) {
                continue;
            }
            if let Some(tbl) = v.as_table() {
                preserved.push((server_id.to_string(), k.to_string(), tbl.clone()));
            }
        }
    }

    if preserved.is_empty() {
        return new_text.to_string();
    }

    let mut new_doc = if new_text.trim().is_empty() {
        DocumentMut::default()
    } else {
        match new_text.parse::<DocumentMut>() {
            Ok(doc) => doc,
            Err(_) => return new_text.to_string(),
        }
    };

    // P1：只把 runtime 子表回写到新文本中已存在的 server，不凭空创建残缺父表。
    let Some(mcp_servers) = new_doc
        .get_mut("mcp_servers")
        .and_then(|item| item.as_table_mut())
    else {
        return new_text.to_string();
    };

    for (server_id, key, tbl) in preserved {
        let Some(server_tbl) = mcp_servers
            .get_mut(&server_id)
            .and_then(|item| item.as_table_mut())
        else {
            continue;
        };
        // P2：逐子键合并——新文本已有同名子表（如 `tools`）时深度合并（新文本优先），
        //     而非整体跳过，从而保住旧 live 中新文本未声明的 per-tool 授权。
        match server_tbl.get_mut(&key) {
            None => {
                server_tbl.insert(&key, toml_edit::Item::Table(tbl));
            }
            Some(existing) => {
                if let Some(existing_tbl) = existing.as_table_mut() {
                    merge_table_preserving_new(existing_tbl, &tbl);
                }
            }
        }
    }

    new_doc.to_string()
}

/// 深度合并：把 `old` 的键并入 `target`，target（新文本）已有的叶子键优先保留，
/// 仅补齐缺失键；两边同名且皆为子表时递归合并。用于在 `tools` 等子表层面做到
/// 逐工具粒度的保留。
fn merge_table_preserving_new(target: &mut toml_edit::Table, old: &toml_edit::Table) {
    for (k, v) in old.iter() {
        match target.get_mut(k) {
            None => {
                target.insert(k, v.clone());
            }
            Some(existing) => {
                if let (Some(existing_tbl), Some(old_tbl)) =
                    (existing.as_table_mut(), v.as_table())
                {
                    merge_table_preserving_new(existing_tbl, old_tbl);
                }
            }
        }
    }
}

/// 纯逻辑：把单个 server 的 spec 应用到 DocumentMut，保留非 cc-switch 管辖的子表。
/// `sync_single_server_to_codex` 的可单测内核。
fn apply_single_server_to_doc(
    doc: &mut toml_edit::DocumentMut,
    id: &str,
    server_spec: &Value,
) -> Result<(), AppError> {
    use toml_edit::Item;

    let preserved = snapshot_unmanaged_subtables(doc, id);

    // 清理可能存在的错误格式 [mcp.servers]
    if let Some(mcp_item) = doc.get_mut("mcp") {
        if let Some(tbl) = mcp_item.as_table_like_mut() {
            if tbl.contains_key("servers") {
                log::warn!("检测到错误的 MCP 格式 [mcp.servers]，正在清理并迁移到 [mcp_servers]");
                tbl.remove("servers");
            }
        }
    }

    if !doc.contains_key("mcp_servers") {
        doc["mcp_servers"] = toml_edit::table();
    }

    let toml_table = json_server_to_toml_table(server_spec)?;
    doc["mcp_servers"][id] = Item::Table(toml_table);

    restore_unmanaged_subtables(doc, id, preserved);

    Ok(())
}

/// 纯逻辑：把 enabled servers 批量应用到 DocumentMut，保留 enabled server 自己的非托管子表。
/// 未在 enabled 中的 server 沿用原有"整体抹除"行为（pre-existing），本 PR 不调整这一语义。
/// `sync_enabled_to_codex` 的可单测内核。
fn apply_enabled_servers_to_doc(
    doc: &mut toml_edit::DocumentMut,
    enabled: &HashMap<String, Value>,
) {
    use toml_edit::{Item, Table};

    // 仅为 enabled 中的 server 快照非托管子表
    let preserved_per_server: HashMap<String, Vec<(String, toml_edit::Table)>> = enabled
        .keys()
        .filter_map(|id| {
            let preserved = snapshot_unmanaged_subtables(doc, id);
            (!preserved.is_empty()).then(|| (id.clone(), preserved))
        })
        .collect();

    // 清理可能存在的错误格式 [mcp.servers]
    if let Some(mcp_item) = doc.get_mut("mcp") {
        if let Some(tbl) = mcp_item.as_table_like_mut() {
            if tbl.contains_key("servers") {
                log::warn!("检测到错误的 MCP 格式 [mcp.servers]，正在清理并迁移到 [mcp_servers]");
                tbl.remove("servers");
            }
        }
    }

    if enabled.is_empty() {
        // pre-existing behavior：无 enabled 时整体移除 [mcp_servers]。
        // preserved_per_server 在此分支必为空。
        doc.as_table_mut().remove("mcp_servers");
        return;
    }

    let mut servers_tbl = Table::new();
    let mut ids: Vec<_> = enabled.keys().cloned().collect();
    ids.sort();
    for id in ids {
        let spec = enabled.get(&id).expect("spec must exist");
        match json_server_to_toml_table(spec) {
            Ok(table) => {
                servers_tbl[&id[..]] = Item::Table(table);
            }
            Err(err) => {
                log::error!("跳过无效的 MCP 服务器 '{id}': {err}");
            }
        }
    }
    doc["mcp_servers"] = Item::Table(servers_tbl);

    for (server_id, preserved) in preserved_per_server {
        restore_unmanaged_subtables(doc, &server_id, preserved);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    use toml_edit::DocumentMut;

    fn parse(text: &str) -> DocumentMut {
        text.parse::<DocumentMut>().expect("valid toml")
    }

    fn parse_value(doc: &DocumentMut) -> toml::Value {
        toml::from_str(&doc.to_string()).expect("valid toml round-trip")
    }

    #[test]
    fn sync_single_preserves_tools_permission_subtable() {
        let mut doc = parse(
            r#"
[mcp_servers.ace-tool-rs]
type = "stdio"
command = "old-cmd"

[mcp_servers.ace-tool-rs.tools.search_context]
approval_mode = "approve"
"#,
        );

        let new_spec = json!({
            "type": "stdio",
            "command": "new-cmd",
            "args": ["--foo"]
        });

        apply_single_server_to_doc(&mut doc, "ace-tool-rs", &new_spec).unwrap();
        let v = parse_value(&doc);

        // 管辖字段已覆盖
        assert_eq!(
            v["mcp_servers"]["ace-tool-rs"]["command"].as_str(),
            Some("new-cmd")
        );
        let args = v["mcp_servers"]["ace-tool-rs"]["args"]
            .as_array()
            .expect("args is array");
        assert_eq!(args.len(), 1);
        assert_eq!(args[0].as_str(), Some("--foo"));

        // 非托管子表必须保留
        assert_eq!(
            v["mcp_servers"]["ace-tool-rs"]["tools"]["search_context"]["approval_mode"].as_str(),
            Some("approve"),
            "Codex CLI 运行时写入的 tools.* 权限必须在 sync 后保留"
        );
    }

    #[test]
    fn sync_single_overwrites_managed_env_subtable() {
        // env 是 cc-switch 管辖的子表——spec 中没有 env 时，旧 env 必须被清掉
        let mut doc = parse(
            r#"
[mcp_servers.x]
type = "stdio"
command = "old"

[mcp_servers.x.env]
OLD_VAR = "1"
"#,
        );

        let new_spec = json!({
            "type": "stdio",
            "command": "new"
        });

        apply_single_server_to_doc(&mut doc, "x", &new_spec).unwrap();
        let v = parse_value(&doc);

        assert_eq!(v["mcp_servers"]["x"]["command"].as_str(), Some("new"));
        assert!(
            v.get("mcp_servers")
                .and_then(|m| m.get("x"))
                .and_then(|x| x.get("env"))
                .is_none(),
            "env 是 cc-switch 管辖子表，spec 无 env 时必须清除"
        );
    }

    #[test]
    fn sync_single_overwrites_managed_http_headers_subtable() {
        // http_headers 同样是 cc-switch 管辖的子表
        let mut doc = parse(
            r#"
[mcp_servers.h]
type = "http"
url = "https://old.example/"

[mcp_servers.h.http_headers]
X-Old = "1"
"#,
        );

        let new_spec = json!({
            "type": "http",
            "url": "https://new.example/",
            "headers": { "X-New": "2" }
        });

        apply_single_server_to_doc(&mut doc, "h", &new_spec).unwrap();
        let v = parse_value(&doc);

        assert_eq!(
            v["mcp_servers"]["h"]["url"].as_str(),
            Some("https://new.example/")
        );
        assert_eq!(
            v["mcp_servers"]["h"]["http_headers"]["X-New"].as_str(),
            Some("2")
        );
        assert!(
            v["mcp_servers"]["h"]["http_headers"]
                .as_table()
                .map(|t| !t.contains_key("X-Old"))
                .unwrap_or(false),
            "旧 header 应被新 spec 覆盖"
        );
    }

    #[test]
    fn sync_enabled_preserves_tools_for_enabled_server() {
        let mut doc = parse(
            r#"
[mcp_servers.x]
type = "stdio"
command = "old"

[mcp_servers.x.tools.t1]
approval_mode = "deny"
"#,
        );

        let mut enabled = HashMap::new();
        enabled.insert(
            "x".to_string(),
            json!({
                "type": "stdio",
                "command": "new"
            }),
        );

        apply_enabled_servers_to_doc(&mut doc, &enabled);
        let v = parse_value(&doc);

        assert_eq!(v["mcp_servers"]["x"]["command"].as_str(), Some("new"));
        assert_eq!(
            v["mcp_servers"]["x"]["tools"]["t1"]["approval_mode"].as_str(),
            Some("deny"),
            "enabled server 的 tools.* 必须保留"
        );
    }

    #[test]
    fn sync_enabled_drops_mcp_servers_when_empty() {
        let mut doc = parse(
            r#"
[mcp_servers.x]
type = "stdio"
command = "x"

[mcp_servers.x.tools.t1]
approval_mode = "approve"
"#,
        );

        let enabled: HashMap<String, Value> = HashMap::new();
        apply_enabled_servers_to_doc(&mut doc, &enabled);

        let text = doc.to_string();
        assert!(
            !text.contains("mcp_servers"),
            "enabled 为空时整体移除 [mcp_servers]（pre-existing 行为）"
        );
    }

    #[test]
    fn sync_enabled_drops_unmentioned_server_including_its_tools() {
        // 不在 enabled 中的 server 沿用原有"整体移除"行为
        let mut doc = parse(
            r#"
[mcp_servers.x]
type = "stdio"
command = "x"

[mcp_servers.x.tools.t1]
approval_mode = "approve"

[mcp_servers.y]
type = "stdio"
command = "y"
"#,
        );

        let mut enabled = HashMap::new();
        enabled.insert(
            "y".to_string(),
            json!({ "type": "stdio", "command": "y2" }),
        );

        apply_enabled_servers_to_doc(&mut doc, &enabled);
        let v = parse_value(&doc);

        assert!(
            v.get("mcp_servers")
                .and_then(|m| m.get("x"))
                .is_none(),
            "未在 enabled 中的 server 应被整体移除（pre-existing 行为）"
        );
        assert_eq!(v["mcp_servers"]["y"]["command"].as_str(), Some("y2"));
    }

    #[test]
    fn sync_single_handles_empty_doc() {
        let mut doc = DocumentMut::new();
        let spec = json!({ "type": "stdio", "command": "c" });
        apply_single_server_to_doc(&mut doc, "x", &spec).unwrap();
        let v = parse_value(&doc);
        assert_eq!(v["mcp_servers"]["x"]["command"].as_str(), Some("c"));
    }

    // ====== Layer 2: merge_codex_runtime_subtables ======

    #[test]
    fn merge_runtime_preserves_tools_when_new_text_has_server_without_tools() {
        // 主复现路径：tools.* 由 Codex CLI 在 provider 保存之后才追加，所以切换回来的
        // provider stored config 带 [mcp_servers.ace-tool-rs]（含 command）但不含 tools.*。
        let old = r#"
model_provider = "openai"

[mcp_servers.ace-tool-rs]
type = "stdio"
command = "ace"

[mcp_servers.ace-tool-rs.tools.search_context]
approval_mode = "approve"
"#;
        let new = r#"
model_provider = "anthropic"

[mcp_servers.ace-tool-rs]
type = "stdio"
command = "ace"
"#;
        let merged = merge_codex_runtime_subtables(new, old);
        let v: toml::Value = toml::from_str(&merged).expect("merged is valid toml");

        // 新文本的字段保留
        assert_eq!(v["model_provider"].as_str(), Some("anthropic"));
        assert_eq!(v["mcp_servers"]["ace-tool-rs"]["command"].as_str(), Some("ace"));
        // 旧 tools.* 必须保留
        assert_eq!(
            v["mcp_servers"]["ace-tool-rs"]["tools"]["search_context"]["approval_mode"].as_str(),
            Some("approve"),
            "Layer 2 必须把 Codex CLI runtime 写入的 tools.* 从旧 live 抢救到新文本"
        );
    }

    #[test]
    fn merge_runtime_skips_managed_subtables() {
        // env / http_headers / headers 是受管子表，不应被 Layer 2 保留
        let old = r#"
[mcp_servers.x]
command = "old"

[mcp_servers.x.env]
OLD = "1"

[mcp_servers.x.http_headers]
X-Old = "1"

[mcp_servers.x.headers]
X-Compat = "1"

[mcp_servers.x.tools.t1]
approval_mode = "approve"
"#;
        let new = r#"
[mcp_servers.x]
command = "new"
"#;
        let merged = merge_codex_runtime_subtables(new, old);
        let v: toml::Value = toml::from_str(&merged).expect("merged is valid toml");

        assert_eq!(v["mcp_servers"]["x"]["command"].as_str(), Some("new"));
        // 受管子表都不应该被保留进来
        assert!(v["mcp_servers"]["x"].get("env").is_none(), "env 受管，不应被 Layer 2 保留");
        assert!(
            v["mcp_servers"]["x"].get("http_headers").is_none(),
            "http_headers 受管，不应被 Layer 2 保留"
        );
        assert!(
            v["mcp_servers"]["x"].get("headers").is_none(),
            "compat headers 受管，不应被 Layer 2 保留"
        );
        // 未知子表必须保留
        assert_eq!(
            v["mcp_servers"]["x"]["tools"]["t1"]["approval_mode"].as_str(),
            Some("approve"),
        );
    }

    #[test]
    fn merge_runtime_new_wins_when_key_collides() {
        // 用户在 provider config 中显式声明了 tools.*——新文本优先
        let old = r#"
[mcp_servers.x.tools.t1]
approval_mode = "approve"
"#;
        let new = r#"
[mcp_servers.x.tools.t1]
approval_mode = "deny"
"#;
        let merged = merge_codex_runtime_subtables(new, old);
        let v: toml::Value = toml::from_str(&merged).expect("merged is valid toml");

        assert_eq!(
            v["mcp_servers"]["x"]["tools"]["t1"]["approval_mode"].as_str(),
            Some("deny"),
            "新文本显式声明同名子表时应保留新值"
        );
    }

    #[test]
    fn merge_runtime_returns_unchanged_when_old_has_no_mcp_servers() {
        let old = r#"model_provider = "openai""#;
        let new = r#"model_provider = "anthropic""#;
        let merged = merge_codex_runtime_subtables(new, old);
        // best-effort：原样返回新文本（不要求字符串完全相等，只要语义等价）
        let v: toml::Value = toml::from_str(&merged).unwrap();
        assert_eq!(v["model_provider"].as_str(), Some("anthropic"));
        assert!(v.get("mcp_servers").is_none());
    }

    #[test]
    fn merge_runtime_returns_unchanged_when_old_unparseable() {
        let old = "this is :: not toml";
        let new = r#"model_provider = "anthropic""#;
        let merged = merge_codex_runtime_subtables(new, old);
        assert_eq!(merged, new, "旧文本无法解析时退回原新文本，不应阻断写入");
    }

    #[test]
    fn merge_runtime_merges_tools_per_tool_not_per_parent() {
        // P2：旧 live 有 tools.search、新文本（用户在 provider config 里）声明了 tools.read，
        // 两者无键冲突，必须都保留——不能因为新文本已有 `tools` 父表就整体跳过旧表。
        let old = r#"
[mcp_servers.x]
command = "x"

[mcp_servers.x.tools.search]
approval_mode = "approve"
"#;
        let new = r#"
[mcp_servers.x]
command = "x"

[mcp_servers.x.tools.read]
approval_mode = "approve"
"#;
        let merged = merge_codex_runtime_subtables(new, old);
        let v: toml::Value = toml::from_str(&merged).expect("merged is valid toml");

        assert_eq!(
            v["mcp_servers"]["x"]["tools"]["search"]["approval_mode"].as_str(),
            Some("approve"),
            "新文本声明了其它工具时，旧 live 的 per-tool 授权仍须逐工具保留"
        );
        assert_eq!(
            v["mcp_servers"]["x"]["tools"]["read"]["approval_mode"].as_str(),
            Some("approve"),
            "新文本声明的工具同样保留"
        );
    }

    #[test]
    fn merge_runtime_drops_orphan_tools_when_new_lacks_server() {
        // P1：新文本里没有该 server（切换后的 provider 不含它，且之后不会再跑 MCP sync）。
        // 不能为了塞 tools.* 而凭空建一个没有 command/url 的残缺 server——Codex 无法加载它。
        let old = r#"
[mcp_servers.x.tools.t1]
approval_mode = "approve"
"#;
        let new = r#"
model_provider = "anthropic"
"#;
        let merged = merge_codex_runtime_subtables(new, old);
        let v: toml::Value = toml::from_str(&merged).expect("merged is valid toml");
        assert_eq!(v["model_provider"].as_str(), Some("anthropic"));
        assert!(
            v.get("mcp_servers").is_none(),
            "新文本不含该 server 时，不得为孤儿 tools.* 创建残缺父表"
        );
    }
}

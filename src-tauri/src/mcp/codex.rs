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

use super::validation::{extract_server_spec, spec_transport_type, validate_server_spec};

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

/// 推断 Codex `[mcp_servers.*]` 条目的传输类型。
///
/// Codex 没有 `type` 字段：传输方式由 `command`（stdio）或 `url`
/// （streamable HTTP）推断。手写配置以及新版本工具写出的配置都可能只有
/// `command`/`url`，因此显式 `type` 缺失时按 `url` 存在与否兜底，
/// 避免 HTTP server 被误判成 stdio 而丢失 `url`。
fn codex_entry_transport_type(entry_tbl: &toml::value::Table) -> &str {
    if let Some(explicit) = entry_tbl.get("type").and_then(|v| v.as_str()) {
        return explicit;
    }
    if entry_tbl.contains_key("url") {
        "http"
    } else {
        "stdio"
    }
}

/// 单个 Codex `[mcp_servers.<id>]` 表 → 统一 JSON 规范（未知传输类型返回 None）。
///
/// Codex 原生字段原样保留（嵌套表如 `tools.<name>.approval_mode` 也保留），
/// 再写回 Codex 时才不会丢字段。
fn codex_entry_to_spec(id: &str, entry_tbl: &toml::value::Table) -> Option<Value> {
    // 传输类型：显式 `type` 优先；缺失时由 command/url 推断（见 helper）
    let typ = codex_entry_transport_type(entry_tbl);

    let mut spec = serde_json::Map::new();
    spec.insert("type".into(), json!(typ));

    // 核心字段（需要手动处理的字段）
    let core_fields: &[&str] = match typ {
        "stdio" => &["type", "command", "args", "env", "cwd"],
        // DB 中的统一规范使用 headers，Codex TOML 使用 http_headers。
        // 两者都必须视为核心字段，避免鉴权值落入通用日志路径。
        "http" | "sse" => &["type", "url", "headers", "http_headers"],
        _ => {
            log::warn!("跳过未知类型 '{typ}' 的 Codex MCP 项 '{id}'");
            return None;
        }
    };

    // 1. 处理核心字段（强类型）
    if typ == "stdio" {
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
                spec.insert("args".into(), Value::Array(arr));
            }
        }
        if let Some(cwd) = entry_tbl.get("cwd").and_then(|v| v.as_str()) {
            if !cwd.trim().is_empty() {
                spec.insert("cwd".into(), json!(cwd));
            }
        }
        if let Some(env_tbl) = entry_tbl.get("env").and_then(|v| v.as_table()) {
            let env_json: serde_json::Map<String, Value> = env_tbl
                .iter()
                .filter_map(|(k, v)| Some((k.clone(), json!(v.as_str()?))))
                .collect();
            if !env_json.is_empty() {
                spec.insert("env".into(), Value::Object(env_json));
            }
        }
    } else {
        if let Some(url) = entry_tbl.get("url").and_then(|v| v.as_str()) {
            spec.insert("url".into(), json!(url));
        }
        // http_headers（Codex 正确格式）优先，兼容旧的 headers
        let headers_tbl = entry_tbl
            .get("http_headers")
            .and_then(|v| v.as_table())
            .or_else(|| entry_tbl.get("headers").and_then(|v| v.as_table()));
        if let Some(headers_tbl) = headers_tbl {
            let headers_json: serde_json::Map<String, Value> = headers_tbl
                .iter()
                .filter_map(|(k, v)| Some((k.clone(), json!(v.as_str()?))))
                .collect();
            if !headers_json.is_empty() {
                spec.insert("headers".into(), Value::Object(headers_json));
            }
        }
    }

    // 2. 其他字段：通用 TOML → JSON 转换
    for (key, toml_val) in entry_tbl.iter() {
        if core_fields.contains(&key.as_str()) {
            continue;
        }
        match toml_to_json_value(toml_val) {
            // 空数组 / 空表沿用旧行为：不导入
            Some(Value::Array(a)) if a.is_empty() => {}
            Some(Value::Object(o)) if o.is_empty() => {}
            Some(val) => {
                spec.insert(key.clone(), val);
                log::debug!("导入扩展字段 '{key}'（值已省略）");
            }
            None => log::debug!("跳过无法转换的字段 '{key}' (TOML → JSON)"),
        }
    }

    Some(Value::Object(spec))
}

/// 递归的 TOML → JSON 值转换；日期时间没有 JSON 对应物，跳过。
fn toml_to_json_value(value: &toml::Value) -> Option<Value> {
    Some(match value {
        toml::Value::String(s) => json!(s),
        toml::Value::Integer(i) => json!(i),
        toml::Value::Float(f) => json!(f),
        toml::Value::Boolean(b) => json!(b),
        toml::Value::Datetime(_) => return None,
        toml::Value::Array(arr) => {
            Value::Array(arr.iter().filter_map(toml_to_json_value).collect())
        }
        toml::Value::Table(tbl) => Value::Object(
            tbl.iter()
                .filter_map(|(k, v)| Some((k.clone(), toml_to_json_value(v)?)))
                .collect(),
        ),
    })
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

            let Some(spec_v) = codex_entry_to_spec(id, entry_tbl) else {
                continue;
            };

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
                            agy: false,
                            claude: false,
                            codex: true,
                            gemini: false,
                            grokbuild: false,
                            opencode: false,
                            hermes: false,
                            mcode: false,
                            kimi: false,
                            dsh: false,
                            pi: false,
                            omp: false,
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
            match json_server_to_codex_toml_table(spec) {
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
/// 把单个 MCP server 表写入 `[mcp_servers]`，并保证该键是「表」。
///
/// `~/.codex/config.toml` 是用户可手改的：若 `mcp_servers` 存在但不是表
/// （如 `mcp_servers = "x"` / `[]`），仅判 `contains_key` 会跳过重建，随后的
/// `doc["mcp_servers"][id] = …` 会触发 toml_edit 的 `IndexMut` panic
/// （panic 发生在 Tauri command 内、跨 FFI 展开）。这里统一归一化后再插入。
fn upsert_mcp_server_table(
    doc: &mut toml_edit::DocumentMut,
    id: &str,
    table: toml_edit::Table,
) -> Result<(), AppError> {
    if doc
        .get_mut("mcp_servers")
        .and_then(toml_edit::Item::as_table_like_mut)
        .is_none()
    {
        // 键存在但不是表时，归一化会丢掉用户手写的那个值——必须留痕，
        // 否则用户只会看到自己的改动凭空消失。
        if doc.get("mcp_servers").is_some_and(|item| !item.is_none()) {
            log::warn!("config.toml 的 mcp_servers 不是表，已重置为空表");
        }
        doc["mcp_servers"] = toml_edit::table();
    }
    let servers = doc
        .get_mut("mcp_servers")
        .and_then(toml_edit::Item::as_table_like_mut)
        .ok_or_else(|| AppError::McpValidation("config.toml 的 mcp_servers 不是表".to_string()))?;
    servers.insert(id, toml_edit::Item::Table(table));
    Ok(())
}

/// 从 `[mcp_servers]`（以及历史错误格式 `[mcp.servers]`）中删除单个 MCP server。
///
/// 与 `upsert_mcp_server_table` 对称地使用 `as_table_like_mut`：用户若把配置写成
/// inline table（`mcp_servers = { foo = {...} }`，TOML 合法），`as_table_mut` 会返回
/// None 导致删除**静默失效**——界面提示已移除，条目却还在文件里，Codex 下次启动照样
/// 加载。这比 panic 更隐蔽，因为用户往往正是发现某个 MCP 有问题才来关它的。
///
/// 与写入分离成纯 doc 级函数，使守卫可脱离真实 `~/.codex/config.toml` 单测。
fn remove_mcp_server_from_doc(doc: &mut toml_edit::DocumentMut, id: &str) {
    if let Some(item) = doc.get_mut("mcp_servers") {
        // `Item::None` 是 toml_edit 的占位形态，不是用户写下的值——对它告警是噪音。
        // 必须在取可变借用之前算出来。
        let user_authored = !item.is_none();
        match item.as_table_like_mut() {
            Some(mcp_servers) => {
                mcp_servers.remove(id);
            }
            None if user_authored => {
                log::warn!("config.toml 的 mcp_servers 不是表，无法删除服务器 '{id}'");
            }
            None => {}
        }
    }

    // 同时清理可能存在于错误位置的数据：[mcp.servers]（如果存在）
    if let Some(mcp_table) = doc.get_mut("mcp").and_then(|t| t.as_table_like_mut()) {
        if let Some(servers) = mcp_table
            .get_mut("servers")
            .and_then(|s| s.as_table_like_mut())
        {
            if servers.remove(id).is_some() {
                log::warn!("从错误的 MCP 格式 [mcp.servers] 中清理了服务器 '{id}'");
            }
        }
    }
}

pub fn sync_single_server_to_codex(
    _config: &MultiAppConfig,
    id: &str,
    server_spec: &Value,
) -> Result<(), AppError> {
    if !should_sync_codex_mcp() {
        return Ok(());
    }

    // 读取现有的 config.toml
    let config_path = crate::codex_config::get_codex_config_path();

    let mut doc = if config_path.exists() {
        let content =
            std::fs::read_to_string(&config_path).map_err(|e| AppError::io(&config_path, e))?;
        // 解析失败必须报错而不是用空文档顶替：写回空文档会把用户
        // config.toml 里的其它段落（model/model_providers/注释等）整体清空
        content
            .parse::<toml_edit::DocumentMut>()
            .map_err(|e| AppError::McpValidation(format!("解析 config.toml 失败: {e}")))?
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

    // 将 JSON 服务器规范转换为 TOML 表
    let toml_table = json_server_to_codex_toml_table(server_spec)?;
    upsert_mcp_server_table(&mut doc, id, toml_table)?;

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

    remove_mcp_server_from_doc(&mut doc, id);

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

/// Codex `[mcp_servers.*]` 认识的字段（按 codex-cli 0.166 `--strict-config` 逐项实测）。
///
/// 共享规范里常带着其他客户端的字段：`timeout` / `autoApprove` / `disabled` /
/// `alwaysAllow`（Cline、Roo）、`trust` / `includeTools` / `description` / `oauth`
/// （Gemini CLI）、`transport`（Hermes、Kimi）等。Codex 对未知字段报 ignored-settings
/// 警告，`--strict-config` 直接拒绝加载整份配置。黑名单永远列不全，所以按白名单写出；
/// 库里的共享规范不动，其他客户端照常使用。
///
/// `timeout` 不换算成 `tool_timeout_sec`：各客户端单位不一，猜错会把超时放大或缩小
/// 一千倍；需要时直接在规范里写 Codex 原生的 `tool_timeout_sec`，会原样写出。
const CODEX_COMMON_FIELDS: &[&str] = &[
    "enabled",
    "required",
    "startup_timeout_sec",
    "startup_timeout_ms",
    "tool_timeout_sec",
    "enabled_tools",
    "disabled_tools",
    "scopes",
    "tools",
    "default_tools_approval_mode",
    "environment_id",
    "supports_parallel_tool_calls",
    "tool_input_schema_max_bytes",
    "startup_readiness",
    "omit_tools_from",
];
/// 只属于 stdio 条目：写到 url 条目上 Codex 会拒绝加载。
const CODEX_STDIO_FIELDS: &[&str] = &["command", "args", "env", "env_vars", "cwd"];
/// 只属于 url（streamable HTTP）条目：`auth` / `oauth` 写到 stdio 条目上同样会被拒绝
/// （`auth is not supported for stdio`）。
const CODEX_HTTP_FIELDS: &[&str] = &[
    "url",
    "http_headers",
    "env_http_headers",
    "http_headers_helper",
    "bearer_token_env_var",
    "oauth_resource",
    "auth",
    "oauth",
];
/// Codex `[mcp_servers.*.oauth]` 认识的子字段。Gemini CLI 的 `oauth` 同名但字段不同
/// （`clientId` / `enabled` / `scopes` …），原样写出会让 Codex 拒绝加载。
const CODEX_OAUTH_FIELDS: &[&str] = &["client_id", "client_secret", "callback_url"];

/// Codex 专用的 JSON → TOML 转换：在通用转换的基础上只保留 Codex 认识的字段。
/// Grok Build 复用通用转换，不受影响。
fn json_server_to_codex_toml_table(spec: &Value) -> Result<toml_edit::Table, AppError> {
    let mut table = json_server_to_toml_table(spec)?;
    let is_stdio = spec_transport_type(spec) == "stdio";
    let transport_fields = if is_stdio {
        CODEX_STDIO_FIELDS
    } else {
        CODEX_HTTP_FIELDS
    };
    let mut dropped: Vec<String> = table
        .iter()
        .map(|(key, _)| key.to_string())
        .filter(|key| {
            !CODEX_COMMON_FIELDS.contains(&key.as_str())
                && !transport_fields.contains(&key.as_str())
        })
        .collect();
    for key in &dropped {
        table.remove(key);
    }

    // 下面两个字段 Codex 要求特定形状，通用转换给不出来（嵌套表）或会给错（数组），
    // 直接从规范重建；形状不对就不写，否则 Codex 严格模式拒绝加载整份配置。
    table.remove("tools");
    if let Some(tools) = spec.get("tools") {
        match codex_tools_table(tools) {
            Some(tools) => {
                table.insert("tools", toml_edit::Item::Table(tools));
            }
            None => dropped.push("tools".into()),
        }
    }
    table.remove("oauth");
    if let Some(oauth) = spec.get("oauth").filter(|_| !is_stdio) {
        match codex_oauth_table(oauth) {
            Some(oauth) => {
                table.insert("oauth", toml_edit::Item::Table(oauth));
            }
            None => dropped.push("oauth".into()),
        }
    }

    if !dropped.is_empty() {
        // 只记录字段名：值可能含 token。
        log::debug!("写入 Codex 时跳过 Codex 不认识的字段: {dropped:?}");
    }
    Ok(table)
}

/// `tools` 在 Codex 里是「工具名 → 该工具的设置表」（如 `approval_mode`）。
/// 其他客户端常写成工具名数组（`["echo"]`），Codex 不接受，整项不写。
fn codex_tools_table(tools: &Value) -> Option<toml_edit::Table> {
    let mut out = toml_edit::Table::new();
    for (name, settings) in tools.as_object()? {
        let Some(toml_edit::Value::InlineTable(settings)) = json_to_toml_value(settings) else {
            continue;
        };
        out.insert(name, toml_edit::Item::Table(settings.into_table()));
    }
    (!out.is_empty()).then_some(out)
}

/// 只保留 Codex 认识的 oauth 子字段；一个都没有（比如纯 Gemini 的 oauth）就整项不写。
fn codex_oauth_table(oauth: &Value) -> Option<toml_edit::Table> {
    let mut out = toml_edit::Table::new();
    for (key, value) in oauth.as_object()? {
        if let (true, Some(s)) = (CODEX_OAUTH_FIELDS.contains(&key.as_str()), value.as_str()) {
            out.insert(key, toml_edit::value(s));
        }
    }
    (!out.is_empty()).then_some(out)
}

/// 递归的 JSON → TOML 值转换（null 无法表示，跳过；数组里的 null 同样跳过）。
fn json_to_toml_value(value: &Value) -> Option<toml_edit::Value> {
    Some(match value {
        Value::Null => return None,
        Value::Bool(b) => (*b).into(),
        Value::String(s) => s.as_str().into(),
        Value::Number(n) => match n.as_i64() {
            Some(i) => i.into(),
            None => n.as_f64()?.into(),
        },
        Value::Array(items) => {
            toml_edit::Value::Array(items.iter().filter_map(json_to_toml_value).collect())
        }
        Value::Object(obj) => {
            let mut table = toml_edit::InlineTable::new();
            for (k, v) in obj {
                if let Some(v) = json_to_toml_value(v) {
                    table.insert(k, v);
                }
            }
            toml_edit::Value::InlineTable(table)
        }
    })
}

/// Helper: 将 JSON MCP 服务器规范转换为 toml_edit::Table
///
/// 策略：
/// 1. 核心字段（command, args, url, headers, env, cwd）使用强类型处理
/// 2. 扩展字段（timeout、retry 等）通过白名单列表自动转换
/// 3. 其他未知字段使用通用转换器尝试转换
///
/// 注意：**不向 Codex 写出 `type`**。`[mcp_servers.*]` 没有 `type` 字段，
/// Codex 由 `command`（stdio）或 `url`（streamable HTTP）推断传输方式；
/// 写出 `type = "stdio"` 会被 Codex 0.158+ 的配置校验当作未知字段告警
/// （`unknown configuration field mcp_servers.<id>.type`）。DB 中仍保留 `type`
/// 以便在各客户端之间共享同一份 server 规范。
pub(super) fn json_server_to_toml_table(spec: &Value) -> Result<toml_edit::Table, AppError> {
    use toml_edit::{Array, Item, Table};

    let mut t = Table::new();
    let typ = spec_transport_type(spec);

    // 不走通用转换的字段：本传输方式里已在下方强类型处理的（`type` 仅为兼容 DB
    // 规范而跳过），加上另一种传输方式专属的。Codex 先按 `command` / `url` 定下
    // 传输方式，再逐个检查对面的专属字段，命中任何一个就拒绝加载整份配置
    // （`<field> is not supported for stdio|streamable_http`），所以只能写所选
    // 传输的字段。两份名单按 Codex 0.160 实测；`scopes` 两边都接受，不在其中。
    let skipped_fields: &[&str] = match typ {
        "stdio" => &[
            "type",
            "command",
            "args",
            "env",
            "cwd",
            // 只属于 url 条目
            "url",
            "headers",
            "http_headers",
            "env_http_headers",
            "http_headers_helper",
            "bearer_token_env_var",
            "oauth_resource",
        ],
        "http" | "sse" => &[
            "type",
            "url",
            "headers",
            "http_headers",
            // 只属于 stdio 条目
            "command",
            "args",
            "env",
            "env_vars",
            "cwd",
        ],
        _ => &["type"],
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

            // 统一规范用 `headers`；只写了 Codex 原生 `http_headers` 的规范也要带上，
            // 否则请求头（常含鉴权）会在写出时丢掉。
            let headers = spec
                .get("headers")
                .and_then(|v| v.as_object())
                .or_else(|| spec.get("http_headers").and_then(|v| v.as_object()));
            if let Some(headers) = headers {
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
            // 跳过已处理的字段和另一种传输方式专属的字段
            if skipped_fields.contains(&key.as_str()) {
                continue;
            }

            // 尝试使用通用转换器
            if let Some(toml_item) = json_value_to_toml_item(value, key) {
                t[&key[..]] = toml_item;

                // 只记录字段名：未知字段同样可能携带 token / secret。
                if extended_fields.contains(&key.as_str()) {
                    log::debug!("已转换扩展字段 '{key}'（值已省略）");
                } else {
                    log::debug!("已转换自定义字段 '{key}'（值已省略）");
                }
            }
        }
    }

    Ok(t)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn upsert_normalizes_non_table_mcp_servers_without_panicking() {
        // 用户手改过的 config.toml：mcp_servers 是字符串而不是表。
        // 修复前 `doc["mcp_servers"][id] = …` 会 panic。
        for malformed in [
            "mcp_servers = \"x\"\n",
            "mcp_servers = []\n",
            "mcp_servers = 42\n",
        ] {
            let mut doc = malformed
                .parse::<toml_edit::DocumentMut>()
                .expect("fixture parses");
            let table = json_server_to_toml_table(&json!({
                "type": "stdio",
                "command": "npx"
            }))
            .expect("server table");

            upsert_mcp_server_table(&mut doc, "echo", table)
                .unwrap_or_else(|e| panic!("upsert must not fail for {malformed:?}: {e}"));

            let servers = doc
                .get("mcp_servers")
                .and_then(|item| item.as_table_like())
                .unwrap_or_else(|| panic!("mcp_servers must be normalized to a table"));
            assert!(servers.contains_key("echo"));
        }
    }

    #[test]
    fn upsert_preserves_existing_servers_in_a_valid_table() {
        let mut doc = "[mcp_servers.keep]\ncommand = \"keep\"\n"
            .parse::<toml_edit::DocumentMut>()
            .expect("fixture parses");
        let table = json_server_to_toml_table(&json!({
            "type": "stdio",
            "command": "npx"
        }))
        .expect("server table");

        upsert_mcp_server_table(&mut doc, "added", table).expect("upsert");

        let servers = doc
            .get("mcp_servers")
            .and_then(|item| item.as_table_like())
            .expect("table");
        assert!(servers.contains_key("keep"), "existing server must survive");
        assert!(servers.contains_key("added"));
    }

    #[test]
    fn remove_deletes_from_inline_table_form_too() {
        // inline table 是合法 TOML，但 as_table_mut() 对它返回 None——用它做守卫
        // 会让删除静默失效：界面说移除成功，条目却还在，Codex 下次启动照样加载。
        let mut doc = "mcp_servers = { drop = { command = \"x\" }, keep = { command = \"y\" } }\n"
            .parse::<toml_edit::DocumentMut>()
            .expect("fixture parses");

        remove_mcp_server_from_doc(&mut doc, "drop");

        let servers = doc
            .get("mcp_servers")
            .and_then(|item| item.as_table_like())
            .expect("mcp_servers must still be table-like");
        assert!(
            !servers.contains_key("drop"),
            "removal must work on the inline-table form"
        );
        assert!(servers.contains_key("keep"), "siblings must survive");
    }

    #[test]
    fn remove_is_a_noop_on_non_table_mcp_servers() {
        // 既不能 panic，也不能把用户手写的值悄悄抹掉
        let mut doc = "mcp_servers = 42\n"
            .parse::<toml_edit::DocumentMut>()
            .expect("fixture parses");

        remove_mcp_server_from_doc(&mut doc, "whatever");

        assert_eq!(doc.to_string(), "mcp_servers = 42\n");
    }

    #[test]
    fn http_headers_are_only_written_to_codex_http_headers() {
        let table = json_server_to_toml_table(&json!({
            "type": "http",
            "url": "https://mcp.example.com",
            "headers": {
                "Authorization": "Bearer top-secret",
                "X-Api-Key": "also-secret"
            },
            "timeout": 30
        }))
        .unwrap();

        let headers = table
            .get("http_headers")
            .and_then(|item| item.as_table())
            .expect("Codex http_headers table should be written");
        assert_eq!(
            headers.get("Authorization").and_then(|item| item.as_str()),
            Some("Bearer top-secret")
        );
        assert!(
            table.get("headers").is_none(),
            "legacy headers must not be emitted a second time"
        );
        assert_eq!(
            table.get("timeout").and_then(|item| item.as_integer()),
            Some(30)
        );
    }

    #[test]
    fn codex_mcp_table_omits_type_field() {
        // Codex 的 [mcp_servers.*] 没有 `type` 字段：传输方式由 command / url 推断。
        // 写出 `type` 会被 Codex 0.158+ 当作未知配置字段告警。
        for spec in [
            json!({ "type": "stdio", "command": "echo", "args": ["hi"] }),
            json!({ "type": "http", "url": "https://mcp.example.com/mcp" }),
        ] {
            let table = json_server_to_toml_table(&spec).unwrap();
            assert!(
                table.get("type").is_none(),
                "Codex MCP table must not contain a `type` field"
            );
        }
    }

    #[test]
    fn spec_without_type_is_written_by_its_url_or_command() {
        // 面板 JSON 模式粘贴的 `{"url": …}` 不带 type：必须按 url 写，
        // 不能写成 `command = ""` + `url`（Codex 会拒绝加载整份配置）。
        let remote = json_server_to_toml_table(&json!({
            "url": "https://mcp.example.com/mcp",
            "headers": { "Authorization": "Bearer t" }
        }))
        .unwrap();
        assert_eq!(
            remote.get("url").and_then(|item| item.as_str()),
            Some("https://mcp.example.com/mcp")
        );
        assert!(remote.get("command").is_none());
        assert!(remote.get("http_headers").is_some());
        assert!(remote.get("headers").is_none());

        // 显式 stdio 却只有 url：同样按 url 写。
        let mislabeled = json_server_to_toml_table(&json!({
            "type": "stdio",
            "command": "",
            "url": "https://mcp.example.com/mcp"
        }))
        .unwrap();
        assert!(mislabeled.get("command").is_none());
        assert!(mislabeled.get("url").is_some());

        // 没有 type、只有 command：仍是 stdio。
        let local =
            json_server_to_toml_table(&json!({ "command": "npx", "args": ["-y", "x"] })).unwrap();
        assert_eq!(
            local.get("command").and_then(|item| item.as_str()),
            Some("npx")
        );
        assert!(local.get("url").is_none());
    }

    #[test]
    fn codex_mcp_table_only_carries_the_chosen_transports_fields() {
        // Codex 对同时带 command 和 url 的条目、带 args/env 的 url 条目都拒绝加载。
        let stdio = json_server_to_toml_table(&json!({
            "type": "stdio",
            "command": "npx",
            "url": "https://stray.example.com",
            "headers": { "X": "y" }
        }))
        .unwrap();
        assert!(stdio.get("url").is_none());
        assert!(stdio.get("headers").is_none());
        assert!(stdio.get("http_headers").is_none());

        let remote = json_server_to_toml_table(&json!({
            "type": "http",
            "url": "https://mcp.example.com/mcp",
            "command": "stray",
            "args": ["a"],
            "env": { "A": "b" },
            "cwd": "/tmp"
        }))
        .unwrap();
        for key in ["command", "args", "env", "cwd"] {
            assert!(remote.get(key).is_none(), "{key} must not be written");
        }
    }

    #[test]
    fn codex_only_fields_follow_their_transport() {
        // 从 Codex 导入的专属字段原样存在规范里。传输方式被手改之后留下的那些，
        // Codex 同样会拒绝加载整份配置；放在适用的传输方式下则必须照常写出。
        let stdio_only = json!({ "env_vars": ["TOKEN"] });
        let url_only = json!({
            "env_http_headers": { "X-Key": "TOKEN" },
            "http_headers_helper": "helper",
            "bearer_token_env_var": "TOKEN",
            "oauth_resource": "https://mcp.example.com"
        });
        let spec = |base: Value| {
            let mut spec = base;
            for extra in [&stdio_only, &url_only] {
                for (key, value) in extra.as_object().unwrap() {
                    spec[key] = value.clone();
                }
            }
            spec["scopes"] = json!(["read"]);
            spec
        };
        let keys = |fields: &Value| {
            fields
                .as_object()
                .unwrap()
                .keys()
                .cloned()
                .collect::<Vec<_>>()
        };

        let stdio =
            json_server_to_toml_table(&spec(json!({ "type": "stdio", "command": "npx" }))).unwrap();
        for key in keys(&stdio_only) {
            assert!(stdio.get(&key).is_some(), "{key} belongs to stdio entries");
        }
        for key in keys(&url_only) {
            assert!(
                stdio.get(&key).is_none(),
                "{key} must not reach a stdio entry"
            );
        }
        assert!(stdio.get("scopes").is_some());

        let remote = json_server_to_toml_table(&spec(
            json!({ "type": "http", "url": "https://mcp.example.com/mcp" }),
        ))
        .unwrap();
        for key in keys(&url_only) {
            assert!(remote.get(&key).is_some(), "{key} belongs to url entries");
        }
        for key in keys(&stdio_only) {
            assert!(
                remote.get(&key).is_none(),
                "{key} must not reach a url entry"
            );
        }
        assert!(remote.get("scopes").is_some());
    }

    #[test]
    fn codex_table_drops_other_clients_timeout_and_auto_approve() {
        // #8843：`timeout` / `autoApprove` 是其他客户端的字段，Codex 严格加载会拒绝。
        let spec = json!({
            "type": "stdio",
            "command": "echo",
            "timeout": 900000,
            "autoApprove": ["read_document"],
            "tool_timeout_sec": 120,
            "startup_timeout_sec": 20
        });
        let table = json_server_to_codex_toml_table(&spec).unwrap();
        assert!(table.get("timeout").is_none());
        assert!(table.get("autoApprove").is_none());
        // Codex 原生字段照常写出
        assert_eq!(
            table
                .get("tool_timeout_sec")
                .and_then(|item| item.as_integer()),
            Some(120)
        );
        assert_eq!(
            table
                .get("startup_timeout_sec")
                .and_then(|item| item.as_integer()),
            Some(20)
        );
        assert_eq!(
            table.get("command").and_then(|item| item.as_str()),
            Some("echo")
        );

        // 通用转换（Grok Build 复用）不变
        let shared = json_server_to_toml_table(&spec).unwrap();
        assert!(shared.get("timeout").is_some());
        assert!(shared.get("autoApprove").is_some());
    }

    #[test]
    fn codex_table_keeps_only_fields_codex_accepts() {
        // 其他客户端写在共享规范里的字段，Codex --strict-config 全部拒绝加载。
        let foreign = json!({
            "disabled": false,
            "alwaysAllow": ["a"],
            "trust": true,
            "includeTools": ["a"],
            "excludeTools": ["b"],
            "description": "d",
            "transport": "sse",
            "debug": true,
        });
        let native_common = json!({
            "enabled": true,
            "required": false,
            "startup_timeout_ms": 900,
            "enabled_tools": ["a"],
            "disabled_tools": ["b"],
            "default_tools_approval_mode": "approve",
            "environment_id": "env",
            "supports_parallel_tool_calls": true,
            "tool_input_schema_max_bytes": 4096,
            "startup_readiness": "connection",
            "omit_tools_from": ["deferred"],
            "tools": { "echo": { "approval_mode": "approve" } },
        });
        let mut stdio = json!({
            "type": "stdio",
            "command": "node",
            "args": ["server.js"],
            "env_vars": ["TOKEN"],
            "cwd": "/srv",
        });
        let mut remote = json!({
            "type": "http",
            "url": "https://example.com/mcp",
            "headers": { "X-Key": "k" },
            "bearer_token_env_var": "TOKEN",
            "env_http_headers": { "X-Env": "ENV" },
            "oauth_resource": "https://example.com",
            "auth": "oauth",
        });
        for spec in [&mut stdio, &mut remote] {
            for source in [&foreign, &native_common] {
                for (key, value) in source.as_object().unwrap() {
                    spec[key] = value.clone();
                }
            }
        }

        for (spec, transport_keys) in [
            (&stdio, &["command", "args", "env_vars", "cwd"][..]),
            (
                &remote,
                &[
                    "url",
                    "http_headers",
                    "bearer_token_env_var",
                    "env_http_headers",
                    "oauth_resource",
                    "auth",
                ][..],
            ),
        ] {
            let table = json_server_to_codex_toml_table(spec).unwrap();
            for key in foreign.as_object().unwrap().keys() {
                assert!(table.get(key).is_none(), "foreign field {key} leaked");
            }
            for key in native_common
                .as_object()
                .unwrap()
                .keys()
                .map(String::as_str)
                .chain(transport_keys.iter().copied())
            {
                assert!(table.get(key).is_some(), "Codex field {key} was dropped");
            }
            assert!(table.get("type").is_none());
            assert!(table.get("headers").is_none());
        }
    }

    #[test]
    fn codex_oauth_keeps_only_codex_subfields_and_only_on_url_entries() {
        // Gemini CLI 的 oauth 与 Codex 同名不同字段：只留 Codex 认识的子字段，不整表删。
        let oauth = json!({
            "client_id": "cid",
            "client_secret": "secret",
            "callback_url": "http://localhost:1455/cb",
            "clientId": "gemini-id",
            "enabled": true,
            "scopes": ["read"],
        });
        let remote = json_server_to_codex_toml_table(&json!({
            "type": "http",
            "url": "https://example.com/mcp",
            "oauth": oauth,
        }))
        .unwrap();
        let kept = remote
            .get("oauth")
            .and_then(|item| item.as_table_like())
            .expect("Codex oauth subfields must survive");
        let mut keys: Vec<_> = kept.iter().map(|(k, _)| k.to_string()).collect();
        keys.sort();
        assert_eq!(keys, ["callback_url", "client_id", "client_secret"]);

        // 纯 Gemini 的 oauth：一个 Codex 子字段都没有，不写出空表
        let gemini_only = json_server_to_codex_toml_table(&json!({
            "type": "http",
            "url": "https://example.com/mcp",
            "oauth": { "enabled": true, "clientId": "x" },
        }))
        .unwrap();
        assert!(gemini_only.get("oauth").is_none());

        // stdio 条目：Codex 报 `oauth is not supported for stdio`
        let stdio = json_server_to_codex_toml_table(&json!({
            "type": "stdio",
            "command": "node",
            "oauth": { "client_id": "cid" },
            "auth": "oauth",
        }))
        .unwrap();
        assert!(stdio.get("oauth").is_none());
        assert!(stdio.get("auth").is_none());
    }

    #[test]
    fn codex_tools_must_be_a_table_of_tables() {
        // 其他客户端写的工具名数组：Codex 报 `invalid type: sequence, expected a map`
        let array = json_server_to_codex_toml_table(&json!({
            "type": "stdio",
            "command": "node",
            "tools": ["echo"],
        }))
        .unwrap();
        assert!(array.get("tools").is_none());

        let table = json_server_to_codex_toml_table(&json!({
            "type": "stdio",
            "command": "node",
            "tools": { "echo": { "approval_mode": "approve" }, "bad": true },
        }))
        .unwrap();
        let tools = table
            .get("tools")
            .and_then(|item| item.as_table_like())
            .expect("tools table");
        assert!(
            tools.get("bad").is_none(),
            "non-table tool settings dropped"
        );
        assert_eq!(
            tools
                .get("echo")
                .and_then(|item| item.as_table_like())
                .and_then(|t| t.get("approval_mode"))
                .and_then(|item| item.as_str()),
            Some("approve")
        );
    }

    #[test]
    fn codex_native_http_headers_are_written_without_shared_headers() {
        let table = json_server_to_codex_toml_table(&json!({
            "url": "https://example.com/mcp",
            "http_headers": { "Authorization": "Bearer t" },
        }))
        .unwrap();
        assert_eq!(
            table
                .get("http_headers")
                .and_then(|item| item.as_table_like())
                .and_then(|t| t.get("Authorization"))
                .and_then(|item| item.as_str()),
            Some("Bearer t")
        );
    }

    #[test]
    fn codex_entries_survive_import_and_sync_back() {
        // 从 Codex 导入的条目再同步回 Codex，Codex 自己的字段一个都不能丢。
        let doc: toml::Table = toml::from_str(
            r#"
[mcp_servers.local]
command = "node"
args = ["server.js"]
env = { TOKEN = "t" }
env_vars = ["HOME"]
cwd = "/srv"
enabled = true
required = false
startup_timeout_sec = 20
tool_timeout_sec = 120
enabled_tools = ["a"]
disabled_tools = ["b"]
default_tools_approval_mode = "approve"
environment_id = "env"
supports_parallel_tool_calls = true
tool_input_schema_max_bytes = 4096
startup_readiness = "connection"
omit_tools_from = ["deferred"]

[mcp_servers.local.tools.echo]
approval_mode = "approve"

[mcp_servers.remote]
url = "https://example.com/mcp"
http_headers = { "X-Key" = "k" }
env_http_headers = { "X-Env" = "ENV" }
bearer_token_env_var = "TOKEN"
oauth_resource = "https://example.com"
scopes = ["read"]
auth = "oauth"
oauth = { client_id = "cid", client_secret = "secret", callback_url = "http://localhost:1455/cb" }
"#,
        )
        .expect("fixture parses");
        let servers = doc["mcp_servers"].as_table().unwrap();

        for (id, original) in servers {
            let original = original.as_table().unwrap();
            let spec = codex_entry_to_spec(id, original).expect("known transport");
            validate_server_spec(&spec).expect("imported spec is valid");
            let written = json_server_to_codex_toml_table(&spec).unwrap();
            // 经 toml_edit 输出再解析，按值比较（不受 inline/子表写法影响）
            let mut out = toml_edit::DocumentMut::new();
            out.insert(id, toml_edit::Item::Table(written));
            let reparsed: toml::Table = toml::from_str(&out.to_string()).unwrap();
            assert_eq!(
                reparsed[id.as_str()].as_table().unwrap(),
                original,
                "{id} changed after an import/sync round trip"
            );
        }
    }

    #[test]
    fn codex_entry_transport_type_infers_from_url() {
        // 显式 type 优先
        let explicit: toml::value::Table =
            toml::from_str("type = \"sse\"\nurl = \"https://x/sse\"\n").expect("fixture parses");
        assert_eq!(codex_entry_transport_type(&explicit), "sse");

        // 无 type：有 url → http（否则 HTTP server 会被误判成 stdio）
        let http: toml::value::Table =
            toml::from_str("url = \"https://x/mcp\"\n").expect("fixture parses");
        assert_eq!(codex_entry_transport_type(&http), "http");

        // 无 type：只有 command → stdio
        let stdio: toml::value::Table =
            toml::from_str("command = \"npx\"\nargs = [\"-y\", \"server\"]\n")
                .expect("fixture parses");
        assert_eq!(codex_entry_transport_type(&stdio), "stdio");
    }
}

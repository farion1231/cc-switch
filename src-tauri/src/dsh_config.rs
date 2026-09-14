//! DeepSeek Harness (DSH) 配置文件读写模块
//!
//! DeepSeek Harness (DSH) configuration read/write module.
//!
//! 处理 `~/.dsh/settings.yaml` 的读写操作（YAML 格式）。DSH 使用累加式
//! 供应商管理：所有 provider profiles 共存于 `llm-pi-ai.providers.<id>`
//! 之下，当前激活路由存于顶层 `agent-default-model` 小节。
//!
//! ## 配置结构示例
//!
//! ```yaml
//! llm-pi-ai:
//!   providers:
//!     a6api-under-0.1-cny:
//!       displayName: A6API ≤¥0.1
//!       apiKeyEnv: A6API_UNDER_0_1_CNY_API_KEY
//!       api: openai-completions
//!       baseURL: https://api.a6api.com/v1
//!       models:
//!         - id: glm-5.3
//!           reasoningEfforts:
//!             off: null
//!             low: low
//!
//! agent-default-model:
//!   provider: a6api-under-0.1-cny
//!   model: glm-5.3
//!   reasoningEffort: low
//! ```
//!
//! ## 写入策略
//!
//! 与 Hermes 模块一致，采用「区块级替换」而非整文件 serde 往返：
//! - `agent-default-model` 是顶层小节，复用顶层小节定位/替换算法；
//! - `llm-pi-ai.providers.<id>` 是嵌套块，使用本模块的嵌套块扫描器，
//!   只替换目标 provider 的块，保留文件其余部分的注释与格式；
//! - 每次写入前先备份到 `~/.cc-switch/backups/dsh/`，再原子写回。
//!
//! DSH 的 settings.yaml 由 dsh-settings-file 以跨进程写锁 + 原子重命名
//! 维护，本模块无法参与其锁协议，因此仅在单次读-改-写内保持一致性，
//! 并通过备份支持人工回滚。

use std::fs;
use std::path::{Path, PathBuf};
use std::sync::{Mutex, OnceLock};

use chrono::Local;
use serde::{Deserialize, Serialize};
use serde_json::Value as JsonValue;
use serde_yaml::Value as YamlValue;

use crate::config::{atomic_write, get_app_config_dir};
use crate::error::AppError;
use crate::settings::effective_backup_retain_count;

// ============================================================================
// Path Functions
// ============================================================================

/// 获取 DSH 配置目录
///
/// 解析顺序：
///   1. CCS 设置 `dsh_config_dir`（显式覆盖）
///   2. `DSH_HOME` 环境变量（trim 后非空）
///   3. 平台默认 `~/.dsh`
pub fn get_dsh_dir() -> PathBuf {
    if let Some(override_dir) = crate::settings::get_dsh_override_dir() {
        return override_dir;
    }

    if let Some(raw) = std::env::var_os("DSH_HOME") {
        let value = raw.to_string_lossy();
        let trimmed = value.trim();
        if !trimmed.is_empty() {
            return PathBuf::from(trimmed);
        }
    }

    crate::config::get_home_dir().join(".dsh")
}

/// 获取 DSH 设置文件路径（`~/.dsh/settings.yaml`）
pub fn get_dsh_settings_path() -> PathBuf {
    get_dsh_dir().join("settings.yaml")
}

fn dsh_write_lock() -> &'static Mutex<()> {
    static LOCK: OnceLock<Mutex<()>> = OnceLock::new();
    LOCK.get_or_init(|| Mutex::new(()))
}

// ============================================================================
// Types
// ============================================================================

/// DSH 写入结果
#[derive(Debug, Clone, Default, Serialize)]
pub struct DshWriteOutcome {
    pub backup_path: Option<String>,
}

/// `agent-default-model` 顶层小节的形状
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct DshDefaultModel {
    pub provider: String,
    pub model: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub reasoning_effort: Option<String>,
}

// ============================================================================
// Reading
// ============================================================================

/// 读取并解析 DSH settings.yaml
fn read_dsh_settings_at(path: &Path) -> Result<YamlValue, AppError> {
    if !path.exists() {
        return Ok(YamlValue::Mapping(serde_yaml::Mapping::new()));
    }
    let raw = fs::read_to_string(path).map_err(|e| AppError::io(path, e))?;
    serde_yaml::from_str(&raw).map_err(|e| {
        AppError::Config(format!(
            "Failed to parse DSH settings at {}: {e}",
            path.display()
        ))
    })
}

/// 读取 `llm-pi-ai.providers` 下的全部 provider（id → JSON config）。
///
/// 返回的 config 形状即为 DSH YAML 中 provider 块的原样 JSON：
/// displayName / api / baseURL / apiKeyEnv / models（数组）等字段原样透传。
pub fn get_providers_at(path: &Path) -> Result<serde_json::Map<String, JsonValue>, AppError> {
    let config = read_dsh_settings_at(path)?;
    let mut map = serde_json::Map::new();

    let providers = config
        .get("llm-pi-ai")
        .and_then(|v| v.get("providers"))
        .and_then(|v| v.as_mapping());

    if let Some(providers) = providers {
        for (key, value) in providers {
            let Some(name) = key.as_str() else {
                continue;
            };
            if name.trim().is_empty() {
                continue;
            }
            match crate::hermes_config::yaml_to_json(value) {
                Ok(json_val) => {
                    map.insert(name.to_string(), json_val);
                }
                Err(e) => {
                    log::warn!("Failed to convert DSH provider '{name}' to JSON: {e}");
                }
            }
        }
    }

    Ok(map)
}

/// 便捷封装：使用默认路径读取 providers。
pub fn get_providers() -> Result<serde_json::Map<String, JsonValue>, AppError> {
    get_providers_at(&get_dsh_settings_path())
}

/// 读取单个 provider。
pub fn get_provider_at(path: &Path, id: &str) -> Result<Option<JsonValue>, AppError> {
    Ok(get_providers_at(path)?.get(id).cloned())
}

/// 读取 `agent-default-model` 顶层小节。
pub fn get_default_model_at(path: &Path) -> Result<Option<DshDefaultModel>, AppError> {
    let config = read_dsh_settings_at(path)?;
    match config.get("agent-default-model") {
        Some(value) => {
            let json = crate::hermes_config::yaml_to_json(value)?;
            // 容忍未知字段：只提取 provider/model/reasoningEffort。
            let provider = json
                .get("provider")
                .and_then(|v| v.as_str())
                .unwrap_or_default()
                .to_string();
            let model = json
                .get("model")
                .and_then(|v| v.as_str())
                .unwrap_or_default()
                .to_string();
            if provider.is_empty() || model.is_empty() {
                return Ok(None);
            }
            Ok(Some(DshDefaultModel {
                provider,
                model,
                reasoning_effort: json
                    .get("reasoningEffort")
                    .and_then(|v| v.as_str())
                    .map(|s| s.to_string()),
            }))
        }
        None => Ok(None),
    }
}

/// 便捷封装：使用默认路径读取 agent-default-model。
pub fn get_default_model() -> Result<Option<DshDefaultModel>, AppError> {
    get_default_model_at(&get_dsh_settings_path())
}

// ============================================================================
// Top-level section helpers（与 hermes_config 相同算法的私有副本）
// ============================================================================

fn is_top_level_key_line(line: &str) -> bool {
    !line.starts_with(' ')
        && !line.starts_with('\t')
        && !line.starts_with('#')
        && !line.starts_with("---")
        && !line.starts_with("...")
        && line.contains(':')
}

/// 查找顶层小节的字节范围（含小节头行，直到下一个顶层 key 或 EOF）。
fn find_yaml_section_range(raw: &str, section_key: &str) -> Option<(usize, usize)> {
    let target = format!("{section_key}:");
    let mut section_start = None;
    let mut offset = 0;

    for line in raw.split('\n') {
        if section_start.is_none() && is_top_level_key_line(line) && line.starts_with(&target) {
            let after_target = &line[target.len()..];
            if after_target.is_empty()
                || after_target.starts_with(' ')
                || after_target.starts_with('\t')
                || after_target.starts_with('\r')
                || after_target.starts_with('#')
            {
                section_start = Some(offset);
            }
        } else if section_start.is_some() && is_top_level_key_line(line) {
            return Some((section_start.unwrap(), offset));
        }
        offset += line.len() + 1;
    }

    section_start.map(|start| (start, raw.len()))
}

fn serialize_yaml_section(key: &str, value: &YamlValue) -> Result<String, AppError> {
    let mut section = serde_yaml::Mapping::new();
    section.insert(YamlValue::String(key.to_string()), value.clone());
    serde_yaml::to_string(&YamlValue::Mapping(section))
        .map_err(|e| AppError::Config(format!("Failed to serialize YAML section '{key}': {e}")))
}

fn remove_all_sections(raw: &str, section_key: &str) -> String {
    let mut result = String::with_capacity(raw.len());
    let mut rest = raw;
    while let Some((start, end)) = find_yaml_section_range(rest, section_key) {
        result.push_str(&rest[..start]);
        rest = &rest[end..];
    }
    result.push_str(rest);
    result
}

fn replace_yaml_section(
    raw: &str,
    section_key: &str,
    value: &YamlValue,
) -> Result<String, AppError> {
    let serialized = serialize_yaml_section(section_key, value)?;

    if let Some((start, end)) = find_yaml_section_range(raw, section_key) {
        let mut result = String::with_capacity(raw.len());
        result.push_str(&raw[..start]);
        result.push_str(&serialized);
        let remainder = remove_all_sections(&raw[end..], section_key);
        if !serialized.ends_with('\n') && !remainder.is_empty() && !remainder.starts_with('\n') {
            result.push('\n');
        }
        result.push_str(&remainder);
        Ok(result)
    } else {
        let mut result = raw.to_string();
        if !result.is_empty() && !result.ends_with('\n') {
            result.push('\n');
        }
        result.push_str(&serialized);
        if !result.ends_with('\n') {
            result.push('\n');
        }
        Ok(result)
    }
}

// ============================================================================
// Nested provider block scanner（DSH 专属：llm-pi-ai.providers.<id>）
// ============================================================================

/// 一行的缩进宽度（tab 计 1；DSH settings 由机器生成，只用空格）。
fn line_indent(line: &str) -> usize {
    let trimmed = line.trim_end_matches('\r');
    trimmed.len() - trimmed.trim_start_matches(' ').len()
}

/// 行是否为空行或注释行（不参与块边界判定）。
fn is_blank_or_comment(line: &str) -> bool {
    let t = line.trim();
    t.is_empty() || t.starts_with('#')
}

/// 在 `lines` 的 `[from, to)` 范围内查找缩进为 `indent` 且 key 精确匹配
/// `key` 的行，返回行号。
///
/// 支持 `key:`、`"key":`、`'key':` 三种 YAML key 写法。
fn find_key_line(
    lines: &[&str],
    from: usize,
    to: usize,
    indent: usize,
    key: &str,
) -> Option<usize> {
    for (idx, line) in lines.iter().enumerate().skip(from).take(to - from) {
        if is_blank_or_comment(line) {
            continue;
        }
        let line_indent = line_indent(line);
        if line_indent < indent {
            // 已经离开目标层：继续外层调用方负责的边界。
            return None;
        }
        if line_indent != indent {
            continue;
        }
        let body = line.trim_start_matches(' ');
        let body = body.trim_end_matches('\r');
        let matches = body == format!("{key}:")
            || body == format!("\"{key}\":")
            || body == format!("'{key}':")
            || body == format!("{key}: ")
            || body.starts_with(&format!("{key}: "))
            || body.starts_with(&format!("\"{key}\": "))
            || body.starts_with(&format!("'{key}': "));
        if matches {
            return Some(idx);
        }
    }
    None
}

/// 从 key 行开始，找到该映射块的结束行（exclusive）：
/// 下一行非空、非注释且缩进 <= key 缩进。
fn block_end(lines: &[&str], key_line: usize, key_indent: usize) -> usize {
    let mut end = lines.len();
    for (idx, line) in lines.iter().enumerate().skip(key_line + 1) {
        if is_blank_or_comment(line) {
            continue;
        }
        if line_indent(line) <= key_indent {
            end = idx;
            break;
        }
    }
    end
}

/// 在 `llm-pi-ai:` 小节内查找 `  providers:` 行。
///
/// 返回 (providers 行号, providers 块结束行)。
fn find_providers_section(lines: &[&str]) -> Option<(usize, usize)> {
    let llm_start = find_key_line(lines, 0, lines.len(), 0, "llm-pi-ai")?;
    let llm_end = block_end(lines, llm_start, 0);
    let providers_line = find_key_line(lines, llm_start + 1, llm_end, 2, "providers")?;
    let providers_end = block_end(lines, providers_line, 2);
    Some((providers_line, providers_end))
}

/// 把 provider config（JSON 对象）序列化为缩进 4 空格的 YAML 块。
///
/// serde_yaml 输出的属性行从 column 0 开始；本函数统一加上 6 空格，
/// 使属性落在 provider key（indent 4）的下一层。key 行本身给 4 空格。
fn serialize_provider_block(id: &str, config: &JsonValue) -> Result<String, AppError> {
    let obj = config.as_object().ok_or_else(|| {
        AppError::Config(format!("DSH provider '{id}' config must be a JSON object"))
    })?;
    if obj.get("baseURL").and_then(|v| v.as_str()).is_none() {
        return Err(AppError::Config(format!(
            "DSH provider '{id}' config is missing required string field 'baseURL'"
        )));
    }

    let yaml_value = crate::hermes_config::json_to_yaml(config)?;
    let serialized = serde_yaml::to_string(&yaml_value)
        .map_err(|e| AppError::Config(format!("Failed to serialize DSH provider '{id}': {e}")))?;

    let mut block = String::new();
    block.push_str("    ");
    block.push_str(&yaml_key_literal(id));
    block.push_str(":\n");
    for line in serialized.lines() {
        if line.trim().is_empty() {
            block.push('\n');
        } else {
            block.push_str("      ");
            block.push_str(line);
            block.push('\n');
        }
    }
    Ok(block)
}

/// 生成 YAML key 字面量（仅在需要时加引号）。
fn yaml_key_literal(id: &str) -> String {
    let needs_quotes = id.is_empty()
        || id.contains(':')
        || id.contains('#')
        || id.starts_with(' ')
        || id.ends_with(' ')
        || id.starts_with('\'')
        || id.starts_with('"')
        || id.starts_with('&')
        || id.starts_with('*')
        || id.starts_with('!')
        || id.starts_with('%')
        || id.starts_with('@')
        || id.starts_with('`')
        || id.starts_with('{')
        || id.starts_with('[');
    if needs_quotes {
        format!("\"{}\"", id.replace('\\', "\\\\").replace('"', "\\\""))
    } else {
        id.to_string()
    }
}

/// 对 raw 文本执行「upsert provider 块」的纯函数核心。
///
/// 策略（按结构复杂度递增）：
/// 1. `llm-pi-ai:` + `  providers:` + `    <id>:` 均为块格式 → 块级替换/插入；
/// 2. `  providers: {}` 内联空 → 展开为块格式；
/// 3. `llm-pi-ai:` 行带内联值 → 退化为整节重写（解析-修改-序列化），
///    该路径会丢失 llm-pi-ai 节内注释，但保留文件其余部分；
/// 4. 无 `llm-pi-ai:` 小节 → 文件末尾追加新节。
fn upsert_provider_raw(raw: &str, id: &str, config: &JsonValue) -> Result<String, AppError> {
    let block = serialize_provider_block(id, config)?;
    let lines: Vec<&str> = raw.split('\n').collect();

    // 情形 4：无 llm-pi-ai 节 → 追加。
    let Some(llm_line) = find_key_line(&lines, 0, lines.len(), 0, "llm-pi-ai") else {
        let mut result = raw.to_string();
        if !result.is_empty() && !result.trim().is_empty() && !result.ends_with('\n') {
            result.push('\n');
        }
        if !result.trim().is_empty() {
            result.push('\n');
        }
        result.push_str("llm-pi-ai:\n  providers:\n");
        result.push_str(&block);
        return Ok(result);
    };

    // 情形 3：llm-pi-ai 行带内联值（非空、非注释）→ 整节重写。
    let llm_body = lines[llm_line]
        .trim_start_matches(' ')
        .trim_end_matches('\r')
        .trim_start_matches("llm-pi-ai:")
        .trim();
    if !llm_body.is_empty() && !llm_body.starts_with('#') {
        return rewrite_llm_section_via_parse(raw, |providers| {
            providers.insert(
                serde_yaml::Value::String(id.to_string()),
                crate::hermes_config::json_to_yaml(config)?,
            );
            Ok(())
        });
    }

    let llm_end = block_end(&lines, llm_line, 0);

    // 查找 providers 键。
    match find_key_line(&lines, llm_line + 1, llm_end, 2, "providers") {
        Some(providers_line) => {
            let p_body = lines[providers_line]
                .trim_start_matches(' ')
                .trim_end_matches('\r')
                .trim_start_matches("providers:")
                .trim();
            if p_body == "{}" {
                // 情形 2：内联空 providers → 展开为块格式并插入。
                let mut result_lines: Vec<String> = lines[..providers_line]
                    .iter()
                    .map(|s| s.to_string())
                    .collect();
                result_lines.push("  providers:".to_string());
                for line in block.lines() {
                    result_lines.push(line.to_string());
                }
                result_lines.extend(lines[providers_line + 1..].iter().map(|s| s.to_string()));
                return Ok(join_lines(&result_lines));
            }
            if !p_body.is_empty() && !p_body.starts_with('#') {
                // providers 行带其他内联值 → 整节重写。
                return rewrite_llm_section_via_parse(raw, |providers| {
                    providers.insert(
                        serde_yaml::Value::String(id.to_string()),
                        crate::hermes_config::json_to_yaml(config)?,
                    );
                    Ok(())
                });
            }

            let providers_end = block_end(&lines, providers_line, 2);

            match find_key_line(&lines, providers_line + 1, providers_end, 4, id) {
                Some(existing_line) => {
                    // 情形 1a：块级替换。
                    let end = block_end(&lines, existing_line, 4);
                    let mut result_lines: Vec<String> = lines[..existing_line]
                        .iter()
                        .map(|s| s.to_string())
                        .collect();
                    for line in block.lines() {
                        result_lines.push(line.to_string());
                    }
                    result_lines.extend(lines[end..].iter().map(|s| s.to_string()));
                    Ok(join_lines(&result_lines))
                }
                None => {
                    // 情形 1b：providers 节内插入（在其块尾之前）。
                    let mut insert_at = providers_end;
                    // 回退到块内最后一个非空行之后，避免插在尾部注释后。
                    while insert_at > providers_line + 1
                        && is_blank_or_comment(lines[insert_at - 1])
                    {
                        insert_at -= 1;
                    }
                    let mut result_lines: Vec<String> =
                        lines[..insert_at].iter().map(|s| s.to_string()).collect();
                    for line in block.lines() {
                        result_lines.push(line.to_string());
                    }
                    result_lines.extend(lines[insert_at..].iter().map(|s| s.to_string()));
                    Ok(join_lines(&result_lines))
                }
            }
        }
        None => {
            // llm-pi-ai 存在但无 providers 键 → 在节尾插入 providers + 块。
            let mut insert_at = llm_end;
            while insert_at > llm_line + 1 && is_blank_or_comment(lines[insert_at - 1]) {
                insert_at -= 1;
            }
            let mut result_lines: Vec<String> =
                lines[..insert_at].iter().map(|s| s.to_string()).collect();
            result_lines.push("  providers:".to_string());
            for line in block.lines() {
                result_lines.push(line.to_string());
            }
            result_lines.extend(lines[insert_at..].iter().map(|s| s.to_string()));
            Ok(join_lines(&result_lines))
        }
    }
}

/// 对 raw 文本执行「删除 provider 块」的纯函数核心。
fn remove_provider_raw(raw: &str, id: &str) -> Result<String, AppError> {
    let lines: Vec<&str> = raw.split('\n').collect();
    let Some((providers_line, _providers_end)) = find_providers_section(&lines) else {
        return Ok(raw.to_string());
    };
    let providers_end = block_end(&lines, providers_line, 2);

    let Some(existing_line) = find_key_line(&lines, providers_line + 1, providers_end, 4, id)
    else {
        return Ok(raw.to_string());
    };

    let end = block_end(&lines, existing_line, 4);
    let mut result_lines: Vec<String> = lines[..existing_line]
        .iter()
        .map(|s| s.to_string())
        .collect();
    result_lines.extend(lines[end..].iter().map(|s| s.to_string()));

    // 若删除后 providers 块内已无 indent-4 key，则折叠为 `  providers: {}`。
    let has_remaining = result_lines[providers_line + 1..]
        .iter()
        .take_while(|l| {
            // 只看 providers 块范围内的行；用缩进判断近似即可。
            let t = l.trim();
            !t.is_empty() && !t.starts_with('#') && line_indent(l) >= 2
        })
        .any(|l| line_indent(l) == 4 && !is_blank_or_comment(l));
    if !has_remaining {
        // 找到 providers 块的实际结束（在 result_lines 坐标系里，
        // providers 行号不变，因为删除发生在其之后）。
        let mut p_end = result_lines.len();
        for (idx, line) in result_lines.iter().enumerate().skip(providers_line + 1) {
            if is_blank_or_comment(line) {
                continue;
            }
            if line_indent(line) <= 2 {
                p_end = idx;
                break;
            }
        }
        result_lines.splice(providers_line + 1..p_end, std::iter::once(String::new()));
        // 将 `  providers:` 行改写为内联空映射。
        result_lines[providers_line] = "  providers: {}".to_string();
    }

    Ok(join_lines(&result_lines))
}

/// 整节重写路径：解析 llm-pi-ai 节 → 修改 providers → 重新序列化整节。
fn rewrite_llm_section_via_parse<F>(raw: &str, mutator: F) -> Result<String, AppError>
where
    F: FnOnce(&mut serde_yaml::Mapping) -> Result<(), AppError>,
{
    let parsed: YamlValue = serde_yaml::from_str(raw)
        .map_err(|e| AppError::Config(format!("Failed to parse DSH settings: {e}")))?;

    let mut llm = parsed
        .get("llm-pi-ai")
        .cloned()
        .unwrap_or(YamlValue::Mapping(serde_yaml::Mapping::new()));
    if !llm.is_mapping() {
        llm = YamlValue::Mapping(serde_yaml::Mapping::new());
    }
    let llm_map = llm.as_mapping_mut().expect("checked is_mapping");

    let mut providers = llm_map
        .get(YamlValue::String("providers".to_string()))
        .cloned()
        .unwrap_or(YamlValue::Mapping(serde_yaml::Mapping::new()));
    if !providers.is_mapping() {
        providers = YamlValue::Mapping(serde_yaml::Mapping::new());
    }
    let providers_map = providers.as_mapping_mut().expect("checked is_mapping");

    mutator(providers_map)?;

    llm_map.insert(
        YamlValue::String("providers".to_string()),
        YamlValue::Mapping(providers_map.clone()),
    );

    replace_yaml_section(raw, "llm-pi-ai", &YamlValue::Mapping(llm_map.clone()))
}

fn join_lines(lines: &[String]) -> String {
    let mut out = lines.join("\n");
    if !out.ends_with('\n') {
        out.push('\n');
    }
    out
}

// ============================================================================
// Backup & Cleanup
// ============================================================================

fn create_dsh_backup(source: &str) -> Result<PathBuf, AppError> {
    let backup_dir = get_app_config_dir().join("backups").join("dsh");
    fs::create_dir_all(&backup_dir).map_err(|e| AppError::io(&backup_dir, e))?;

    let base_id = format!("dsh_{}", Local::now().format("%Y%m%d_%H%M%S"));
    let mut filename = format!("{base_id}.yaml");
    let mut backup_path = backup_dir.join(&filename);
    let mut counter = 1;

    while backup_path.exists() {
        filename = format!("{base_id}_{counter}.yaml");
        backup_path = backup_dir.join(&filename);
        counter += 1;
    }

    atomic_write(&backup_path, source.as_bytes())?;
    cleanup_dsh_backups(&backup_dir)?;
    Ok(backup_path)
}

fn cleanup_dsh_backups(dir: &Path) -> Result<(), AppError> {
    let retain = effective_backup_retain_count();
    let mut entries = fs::read_dir(dir)
        .map_err(|e| AppError::io(dir, e))?
        .filter_map(|entry| entry.ok())
        .filter(|entry| {
            entry
                .path()
                .extension()
                .map(|ext| ext == "yaml" || ext == "yml")
                .unwrap_or(false)
        })
        .collect::<Vec<_>>();

    if entries.len() <= retain {
        return Ok(());
    }

    entries.sort_by_key(|entry| entry.metadata().and_then(|m| m.modified()).ok());
    let remove_count = entries.len().saturating_sub(retain);
    for entry in entries.into_iter().take(remove_count) {
        if let Err(err) = fs::remove_file(entry.path()) {
            log::warn!(
                "Failed to remove old DSH settings backup {}: {err}",
                entry.path().display()
            );
        }
    }

    Ok(())
}

// ============================================================================
// High-level Write Operations
// ============================================================================

/// 写入（upsert）一个 DSH provider 到指定 settings 文件。
///
/// 区块级替换 + 备份 + 原子写；不触碰 `agent-default-model`。
pub fn set_provider_at(
    path: &Path,
    id: &str,
    config: &JsonValue,
) -> Result<DshWriteOutcome, AppError> {
    let _guard = dsh_write_lock()
        .lock()
        .map_err(|e| AppError::Config(format!("DSH write lock poisoned: {e}")))?;

    let raw = if path.exists() {
        fs::read_to_string(path).map_err(|e| AppError::io(path, e))?
    } else {
        String::new()
    };

    let new_raw = upsert_provider_raw(&raw, id, config)?;

    if new_raw == raw {
        return Ok(DshWriteOutcome::default());
    }

    // 写前校验：新内容必须能被解析，且 provider 可读回。
    let reparsed: YamlValue = serde_yaml::from_str(&new_raw).map_err(|e| {
        AppError::Config(format!(
            "Refusing to write invalid DSH settings for provider '{id}': {e}"
        ))
    })?;
    let readback = reparsed
        .get("llm-pi-ai")
        .and_then(|v| v.get("providers"))
        .and_then(|v| v.get(id));
    if readback.is_none() {
        return Err(AppError::Config(format!(
            "DSH provider '{id}' missing after upsert — refusing to write"
        )));
    }

    let backup_path = if !raw.is_empty() {
        Some(create_dsh_backup(&raw)?)
    } else {
        None
    };

    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).map_err(|e| AppError::io(parent, e))?;
    }

    atomic_write(path, new_raw.as_bytes())?;

    log::debug!("DSH provider '{id}' written to {path:?}");
    Ok(DshWriteOutcome {
        backup_path: backup_path.map(|p| p.display().to_string()),
    })
}

/// 便捷封装：默认路径 upsert provider。
pub fn set_provider(id: &str, config: JsonValue) -> Result<DshWriteOutcome, AppError> {
    set_provider_at(&get_dsh_settings_path(), id, &config)
}

/// 从指定 settings 文件删除一个 DSH provider。
pub fn remove_provider_at(path: &Path, id: &str) -> Result<DshWriteOutcome, AppError> {
    let _guard = dsh_write_lock()
        .lock()
        .map_err(|e| AppError::Config(format!("DSH write lock poisoned: {e}")))?;

    if !path.exists() {
        return Ok(DshWriteOutcome::default());
    }
    let raw = fs::read_to_string(path).map_err(|e| AppError::io(path, e))?;
    let new_raw = remove_provider_raw(&raw, id)?;

    if new_raw == raw {
        return Ok(DshWriteOutcome::default());
    }

    // 写前校验：删除后的内容必须仍可解析。
    serde_yaml::from_str::<YamlValue>(&new_raw).map_err(|e| {
        AppError::Config(format!(
            "Refusing to write invalid DSH settings after removing '{id}': {e}"
        ))
    })?;

    let backup_path = if !raw.is_empty() {
        Some(create_dsh_backup(&raw)?)
    } else {
        None
    };

    atomic_write(path, new_raw.as_bytes())?;

    log::debug!("DSH provider '{id}' removed from {path:?}");
    Ok(DshWriteOutcome {
        backup_path: backup_path.map(|p| p.display().to_string()),
    })
}

/// 便捷封装：默认路径删除 provider。
pub fn remove_provider(id: &str) -> Result<DshWriteOutcome, AppError> {
    remove_provider_at(&get_dsh_settings_path(), id)
}

/// 覆写顶层 `agent-default-model` 小节（切换当前路由）。
///
/// 只重写该顶层小节，保留其余内容。
pub fn set_default_model_at(
    path: &Path,
    default_model: &DshDefaultModel,
) -> Result<DshWriteOutcome, AppError> {
    let _guard = dsh_write_lock()
        .lock()
        .map_err(|e| AppError::Config(format!("DSH write lock poisoned: {e}")))?;

    let raw = if path.exists() {
        fs::read_to_string(path).map_err(|e| AppError::io(path, e))?
    } else {
        String::new()
    };

    if default_model.provider.trim().is_empty() || default_model.model.trim().is_empty() {
        return Err(AppError::Config(
            "DSH default model requires non-empty provider and model".to_string(),
        ));
    }

    let mut section = serde_yaml::Mapping::new();
    section.insert(
        YamlValue::String("provider".to_string()),
        YamlValue::String(default_model.provider.clone()),
    );
    section.insert(
        YamlValue::String("model".to_string()),
        YamlValue::String(default_model.model.clone()),
    );
    if let Some(effort) = &default_model.reasoning_effort {
        section.insert(
            YamlValue::String("reasoningEffort".to_string()),
            YamlValue::String(effort.clone()),
        );
    }

    let new_raw = replace_yaml_section(&raw, "agent-default-model", &YamlValue::Mapping(section))?;

    if new_raw == raw {
        return Ok(DshWriteOutcome::default());
    }

    let backup_path = if !raw.is_empty() {
        Some(create_dsh_backup(&raw)?)
    } else {
        None
    };

    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).map_err(|e| AppError::io(parent, e))?;
    }

    atomic_write(path, new_raw.as_bytes())?;

    log::debug!(
        "DSH agent-default-model set to {}/{} at {:?}",
        default_model.provider,
        default_model.model,
        path
    );
    Ok(DshWriteOutcome {
        backup_path: backup_path.map(|p| p.display().to_string()),
    })
}

/// 便捷封装：默认路径设置 agent-default-model。
pub fn set_default_model(default_model: &DshDefaultModel) -> Result<DshWriteOutcome, AppError> {
    set_default_model_at(&get_dsh_settings_path(), default_model)
}

/// 检查 provider 是否存在于 DSH settings。
pub fn provider_exists(id: &str) -> Result<bool, AppError> {
    Ok(get_providers()?.contains_key(id))
}

// ============================================================================
// Tests
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    const SAMPLE: &str = r#"# DSH settings
ui-onboarding:
  welcomeNoticeVersion: 2026-08-13.1
agent-presets:
  default: standard-noweb
llm-pi-ai:
  providers:
    a6api-under-0.1-cny:
      displayName: A6API <= 0.1
      apiKeyEnv: A6API_UNDER_0_1_CNY_API_KEY
      api: openai-completions
      baseURL: https://api.a6api.com/v1
      models:
        - id: glm-5.3
          reasoningEfforts:
            off: null
            low: low
        - id: glm-5.3-flash
agent-default-model:
  provider: a6api-under-0.5-cny
  model: gpt-6-astra
  reasoningEffort: xhigh
"#;

    fn provider_config() -> JsonValue {
        json!({
            "displayName": "Test Relay",
            "apiKeyEnv": "TEST_RELAY_API_KEY",
            "api": "openai-completions",
            "baseURL": "https://relay.example.com/v1",
            "models": [
                {"id": "test-model", "reasoningEfforts": {"off": null, "low": "low"}}
            ]
        })
    }

    #[test]
    fn upsert_into_existing_providers_preserves_other_sections() {
        let raw = upsert_provider_raw(SAMPLE, "test-relay", &provider_config()).unwrap();
        assert!(raw.contains("test-relay:"));
        assert!(raw.contains("https://relay.example.com/v1"));
        // 其他顶层小节原样保留
        assert!(raw.contains("agent-presets:"));
        assert!(raw.contains("standard-noweb"));
        assert!(raw.contains("# DSH settings"));
        // 既有 provider 不受影响
        assert!(raw.contains("a6api-under-0.1-cny:"));
        assert!(raw.contains("glm-5.3-flash"));
        // agent-default-model 未被触碰
        assert!(raw.contains("gpt-6-astra"));
        // 结果可解析且新 provider 可读回
        let parsed: YamlValue = serde_yaml::from_str(&raw).unwrap();
        assert!(parsed
            .get("llm-pi-ai")
            .and_then(|v| v.get("providers"))
            .and_then(|v| v.get("test-relay"))
            .is_some());
    }

    #[test]
    fn upsert_replaces_existing_provider_block() {
        let once = upsert_provider_raw(SAMPLE, "test-relay", &provider_config()).unwrap();
        let updated_config = json!({
            "displayName": "Test Relay v2",
            "apiKeyEnv": "TEST_RELAY_API_KEY",
            "api": "openai-completions",
            "baseURL": "https://relay2.example.com/v1",
            "models": [
                {"id": "test-model-v2"}
            ]
        });
        let twice = upsert_provider_raw(&once, "test-relay", &updated_config).unwrap();
        assert!(twice.contains("Test Relay v2"));
        assert!(twice.contains("relay2.example.com"));
        assert!(!twice.contains("relay.example.com"));
        // 只有一份 test-relay 块
        assert_eq!(twice.matches("test-relay:").count(), 1);
    }

    #[test]
    fn upsert_into_file_without_llm_section_appends() {
        let raw = "locale:\n  preference: en\n";
        let out = upsert_provider_raw(raw, "test-relay", &provider_config()).unwrap();
        assert!(out.contains("locale:"));
        assert!(out.contains("llm-pi-ai:"));
        assert!(out.contains("  providers:"));
        let parsed: YamlValue = serde_yaml::from_str(&out).unwrap();
        assert!(parsed
            .get("llm-pi-ai")
            .and_then(|v| v.get("providers"))
            .and_then(|v| v.get("test-relay"))
            .is_some());
    }

    #[test]
    fn upsert_expands_inline_empty_providers() {
        let raw = "llm-pi-ai:\n  providers: {}\nagent-default-model:\n  provider: x\n";
        let out = upsert_provider_raw(raw, "test-relay", &provider_config()).unwrap();
        assert!(out.contains("test-relay:"));
        let parsed: YamlValue = serde_yaml::from_str(&out).unwrap();
        assert!(parsed
            .get("llm-pi-ai")
            .and_then(|v| v.get("providers"))
            .and_then(|v| v.get("test-relay"))
            .is_some());
    }

    #[test]
    fn upsert_rejects_config_without_base_url() {
        let bad = json!({"displayName": "No URL"});
        assert!(upsert_provider_raw(SAMPLE, "bad-relay", &bad).is_err());
    }

    #[test]
    fn remove_existing_provider_keeps_siblings() {
        let raw = upsert_provider_raw(SAMPLE, "test-relay", &provider_config()).unwrap();
        let out = remove_provider_raw(&raw, "test-relay").unwrap();
        assert!(!out.contains("test-relay:"));
        assert!(!out.contains("relay.example.com"));
        assert!(out.contains("a6api-under-0.1-cny:"));
        assert!(out.contains("gpt-6-astra"));
        let parsed: YamlValue = serde_yaml::from_str(&out).unwrap();
        assert!(parsed
            .get("llm-pi-ai")
            .and_then(|v| v.get("providers"))
            .and_then(|v| v.get("test-relay"))
            .is_none());
        assert!(parsed
            .get("llm-pi-ai")
            .and_then(|v| v.get("providers"))
            .and_then(|v| v.get("a6api-under-0.1-cny"))
            .is_some());
    }

    #[test]
    fn remove_last_provider_collapses_to_empty_map() {
        let raw = "llm-pi-ai:\n  providers:\n    only-one:\n      baseURL: https://x.example\n";
        let out = remove_provider_raw(raw, "only-one").unwrap();
        assert!(out.contains("providers: {}"));
        let parsed: YamlValue = serde_yaml::from_str(&out).unwrap();
        let providers = parsed
            .get("llm-pi-ai")
            .and_then(|v| v.get("providers"))
            .unwrap();
        assert!(providers
            .as_mapping()
            .map(|m| m.is_empty())
            .unwrap_or(false));
    }

    #[test]
    fn remove_missing_provider_is_noop() {
        let out = remove_provider_raw(SAMPLE, "ghost-relay").unwrap();
        assert_eq!(out, SAMPLE);
    }

    #[test]
    fn set_default_model_replaces_only_that_section() {
        let raw = SAMPLE.to_string();
        let out = replace_yaml_section(
            &raw,
            "agent-default-model",
            &serde_yaml::from_str::<YamlValue>(
                "provider: a6api-under-0.1-cny\nmodel: glm-5.3\nreasoningEffort: low",
            )
            .unwrap(),
        )
        .unwrap();
        assert!(out.contains("glm-5.3"));
        assert!(!out.contains("gpt-6-astra"));
        assert!(out.contains("standard-noweb"));
        assert!(out.contains("# DSH settings"));
    }

    #[test]
    fn end_to_end_file_roundtrip() {
        let dir = std::env::temp_dir().join(format!(
            "ccswitch-dsh-test-{}",
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_millis()
        ));
        fs::create_dir_all(&dir).unwrap();
        let path = dir.join("settings.yaml");
        fs::write(&path, SAMPLE).unwrap();

        // upsert
        let outcome = set_provider_at(&path, "test-relay", &provider_config()).unwrap();
        assert!(outcome.backup_path.is_some());
        let providers = get_providers_at(&path).unwrap();
        assert!(providers.contains_key("test-relay"));
        assert!(providers.contains_key("a6api-under-0.1-cny"));
        assert_eq!(
            providers.get("test-relay").unwrap().get("baseURL"),
            Some(&json!("https://relay.example.com/v1"))
        );

        // default model
        set_default_model_at(
            &path,
            &DshDefaultModel {
                provider: "test-relay".to_string(),
                model: "test-model".to_string(),
                reasoning_effort: Some("low".to_string()),
            },
        )
        .unwrap();
        let dm = get_default_model_at(&path).unwrap().unwrap();
        assert_eq!(dm.provider, "test-relay");
        assert_eq!(dm.model, "test-model");
        assert_eq!(dm.reasoning_effort.as_deref(), Some("low"));

        // remove
        remove_provider_at(&path, "test-relay").unwrap();
        let providers = get_providers_at(&path).unwrap();
        assert!(!providers.contains_key("test-relay"));

        fs::remove_dir_all(&dir).ok();
    }
}

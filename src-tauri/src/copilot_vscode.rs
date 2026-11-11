//! One-way projection of the native Copilot CLI registry into selected VS Code profiles.
mod targets;
pub use targets::{discover_vscode_targets, VsCodeProfileTarget};

use crate::config::{stage_write, StagedWrite};
use crate::error::AppError;
use crate::jsonc_document::JsoncDocument;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::collections::{BTreeMap, HashSet};
use std::fs;
use std::io::Write as _;
use std::path::{Component, Path, PathBuf};

const OWNER: &str = "ccSwitchCopilotCli";
const MAX_CONFIG_BYTES: u64 = 8 * 1024 * 1024;

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SyncSettings {
    #[serde(default)]
    pub enabled: bool,
    #[serde(default)]
    pub targets: Vec<String>,
}

#[derive(Debug, Clone, Default, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SyncStatus {
    pub pending: bool,
    pub error: Option<String>,
}

pub(crate) fn inspect(options: &SyncSettings, document: &Value) -> Result<SyncStatus, AppError> {
    Ok(SyncStatus {
        pending: !plan(options, options, document)?.is_empty(),
        error: None,
    })
}

fn invalid(zh: impl Into<String>, en: impl Into<String>) -> AppError {
    AppError::localized("copilot.vscode.sync", zh, en)
}

fn unsupported(name: &str, field: &str) -> AppError {
    invalid(
        format!("「{name}」的 {field} 暂不能同步到 VS Code Copilot，请先调整 CLI 配置或关闭联动"),
        format!("{field} on \"{name}\" cannot be synced to VS Code Copilot yet. Adjust the CLI configuration or turn off sync"),
    )
}

fn positive(model: &Value, key: &str, limit: &str) -> Option<u64> {
    model
        .get(key)
        .or_else(|| model.pointer(&format!("/capabilities/limits/{limit}")))
        .and_then(Value::as_u64)
        .filter(|n| *n > 0)
}

/// Pass explicit endpoints so VS Code does not add /v1 to custom CLI API prefixes.
fn endpoint(provider: &Value, api: &str) -> Result<String, AppError> {
    let base = provider["baseUrl"].as_str().unwrap_or_default();
    let mut url = url::Url::parse(base)
        .ok()
        .filter(|u| matches!(u.scheme(), "http" | "https") && u.host_str().is_some())
        .ok_or_else(|| {
            invalid(
                "CLI 接口地址必须是 HTTP(S) 地址",
                "The CLI endpoint must be an HTTP(S) URL",
            )
        })?;
    let path = url.path().trim_end_matches('/');
    let suffix = match api {
        "responses" => "/responses",
        "messages" => "/messages",
        _ => "/chat/completions",
    };
    let prefix = if provider["type"] == "azure" && !path.ends_with("/openai/v1") {
        format!("{path}/openai/v1")
    } else if api == "messages" && !path.ends_with("/v1") && !path.ends_with("/messages") {
        format!("{path}/v1")
    } else {
        path.to_owned()
    };
    if !path.ends_with(suffix) {
        url.set_path(&format!("{prefix}{suffix}"));
    }
    Ok(url.to_string())
}

fn project(document: &Value) -> Result<Vec<Value>, AppError> {
    crate::copilot_config::validate_document(document, None)?;
    let empty = Vec::new();
    let models = document["models"].as_array().unwrap_or(&empty);
    let mut groups = Vec::new();
    for provider in document["providers"].as_array().unwrap_or(&empty) {
        let name = provider["name"].as_str().unwrap_or_default();
        let selected: Vec<_> = models.iter().filter(|m| m["provider"] == name).collect();
        if selected.is_empty() {
            continue;
        }
        if provider["transport"] == "websockets" {
            return Err(unsupported(name, "transport=websockets"));
        }
        if provider["hasBearerTokenProvider"] == true {
            return Err(unsupported(name, "hasBearerTokenProvider"));
        }
        if provider
            .pointer("/azure/apiVersion")
            .and_then(Value::as_str)
            .is_some_and(|s| !s.is_empty())
        {
            return Err(unsupported(name, "azure.apiVersion"));
        }
        let api = if provider["type"] == "anthropic" {
            "messages"
        } else if provider["wireApi"] == "responses" {
            "responses"
        } else {
            "chat-completions"
        };
        let url = endpoint(provider, api)?;
        let bearer = provider["bearerToken"].as_str().filter(|s| !s.is_empty());
        let api_key = provider["apiKey"].as_str().filter(|s| !s.is_empty());
        let mut auth_headers = Vec::new();
        if api == "messages" {
            if let Some(key) = api_key {
                auth_headers.push(("x-api-key", key.to_owned()));
            }
            if let Some(token) = bearer {
                auth_headers.push(("Authorization", format!("Bearer {token}")));
            }
        } else if let Some(token) = bearer {
            auth_headers.push(("Authorization", format!("Bearer {token}")));
        } else if let Some(key) = api_key {
            if provider["type"] == "azure" {
                auth_headers.push(("api-key", key.to_owned()));
            } else {
                auth_headers.push(("Authorization", format!("Bearer {key}")));
            }
        }
        if auth_headers.iter().any(|(_, value)| {
            value.contains(concat!("$", "{input:")) || value.contains("${apiKey}")
        }) {
            return Err(unsupported(
                name,
                "credentials with a VS Code secret reference",
            ));
        }
        let mut headers = provider["headers"].as_object().cloned().unwrap_or_default();
        if headers
            .values()
            .any(|v| v.as_str().is_some_and(|s| s.contains("${apiKey}")))
        {
            return Err(unsupported(
                name,
                "headers containing the literal API-key placeholder",
            ));
        }
        for (header, value) in auth_headers {
            if !headers
                .keys()
                .any(|custom| custom.eq_ignore_ascii_case(header))
            {
                headers.insert(header.into(), json!(value));
            }
        }
        let mut wire_ids = HashSet::new();
        let mut projected = Vec::new();
        for model in selected {
            // modelId only selects CLI-specific defaults; wireModel selects the request model.
            let id = model["wireModel"]
                .as_str()
                .filter(|s| !s.is_empty())
                .or_else(|| model["id"].as_str())
                .unwrap_or_default();
            if !wire_ids.insert(id) {
                return Err(unsupported(name, "duplicate wireModel"));
            }
            if model.get("systemMessage").is_some() {
                return Err(unsupported(name, "systemMessage"));
            }
            let supports = &model["capabilities"]["supports"];
            let mut item = json!({
                "id": id, "name": model["name"].as_str().unwrap_or(model["id"].as_str().unwrap_or(id)),
                "url": url, "apiType": api,
                "toolCalling": supports["tool_calls"].as_bool().or_else(|| supports["toolCalls"].as_bool()).unwrap_or(true),
                "vision": supports["vision"].as_bool().unwrap_or(false),
                "requestHeaders": headers,
            });
            for (key, limit, output) in [
                ("maxPromptTokens", "max_prompt_tokens", "maxInputTokens"),
                (
                    "maxContextWindowTokens",
                    "max_context_window_tokens",
                    "contextWindow",
                ),
                ("maxOutputTokens", "max_output_tokens", "maxOutputTokens"),
            ] {
                if let Some(value) = positive(model, key, limit) {
                    item[output] = json!(value);
                }
            }
            // VS Code requires output tokens and either an input limit or a context window.
            if item.get("maxOutputTokens").is_none() {
                let output = item["contextWindow"]
                    .as_u64()
                    .map(|context| 8192.min((context / 4).max(1)))
                    .unwrap_or(8192);
                item["maxOutputTokens"] = json!(output);
            }
            if item.get("maxInputTokens").is_none() && item.get("contextWindow").is_none() {
                item["maxInputTokens"] = json!(128_000);
            }
            if let (Some(context), Some(output)) = (
                item["contextWindow"].as_u64(),
                item["maxOutputTokens"].as_u64(),
            ) {
                if output >= context {
                    return Err(unsupported(name, "maxOutputTokens >= contextWindow"));
                }
            }
            for field in ["thinking", "streaming"] {
                if let Some(value) = supports[field].as_bool() {
                    item[field] = json!(value);
                }
            }
            if let Some(efforts) = supports["reasoning_effort"].as_array() {
                item["supportsReasoningEffort"] = json!(efforts);
            }
            if let Some(value) = supports.get("adaptive_thinking") {
                item["adaptiveThinking"] = json!(
                    value == true
                        || matches!(
                            value.as_str(),
                            Some("optional" | "required" | "adaptive_only")
                        )
                );
            }
            projected.push(item);
        }
        groups.push(json!({
            "name": format!("Copilot CLI · {name}"), "vendor": "customendpoint",
            "models": projected, (OWNER): name,
        }));
    }
    Ok(groups)
}

fn read_raw(path: &Path) -> Result<Option<Vec<u8>>, AppError> {
    match fs::metadata(path) {
        Ok(meta) if meta.len() > MAX_CONFIG_BYTES || !meta.is_file() => Err(invalid(
            format!("{} 不是可同步的配置文件（上限 8 MiB）", path.display()),
            format!(
                "{} is not a supported config file (8 MiB limit)",
                path.display()
            ),
        )),
        Ok(_) => fs::read(path).map(Some).map_err(|e| AppError::io(path, e)),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(None),
        Err(e) => Err(AppError::io(path, e)),
    }
}

fn target_path(raw: &str) -> Result<PathBuf, AppError> {
    let path = PathBuf::from(raw);
    if !path.is_absolute()
        || path.file_name().and_then(|s| s.to_str()) != Some("chatLanguageModels.json")
        || path.components().any(|c| matches!(c, Component::ParentDir))
    {
        return Err(invalid(
            "同步目标必须是 chatLanguageModels.json 的完整路径",
            "The sync target must be an absolute path to chatLanguageModels.json",
        ));
    }
    Ok(fs::canonicalize(&path).unwrap_or_else(|_| {
        path.parent()
            .and_then(|p| fs::canonicalize(p).ok())
            .map(|p| p.join("chatLanguageModels.json"))
            .unwrap_or(path)
    }))
}

fn owned(group: &Value) -> bool {
    group["vendor"] == "customendpoint" && group.get(OWNER).and_then(Value::as_str).is_some()
}

fn merged_source(before: Option<&[u8]>, groups: &[Value]) -> Result<Option<Vec<u8>>, AppError> {
    let source = match before {
        Some(bytes) => std::str::from_utf8(bytes)
            .map_err(|_| {
                invalid(
                    "VS Code 配置不是 UTF-8",
                    "VS Code configuration is not UTF-8",
                )
            })?
            .trim_start_matches('\u{feff}'),
        None => "[]",
    };
    let mut doc = JsoncDocument::parse_array(source).map_err(|_| {
        invalid(
            "VS Code 模型配置必须是有效的 JSONC 数组，请修复该文件后重试",
            "The VS Code model configuration must be a valid JSONC array. Fix the file and retry",
        )
    })?;
    let mut desired = Vec::new();
    for group in doc.value().as_array().expect("validated array") {
        if !owned(group) {
            if groups.iter().any(|next| next["name"] == group["name"]) {
                return Err(invalid(
                    "VS Code 中已有同名的非联动分组，请先重命名",
                    "VS Code already has an unlinked group with this name. Rename it first",
                ));
            }
            desired.push(group.clone());
            continue;
        }
    }
    desired.extend_from_slice(groups);
    let desired = json!(desired);
    if doc.value() == &desired {
        return Ok(None);
    }
    doc.retain_array(|group| !owned(group))?;
    doc.apply(&desired)?;
    let mut output = doc.validated_source()?;
    if before.is_some_and(|bytes| bytes.starts_with(&[0xef, 0xbb, 0xbf])) {
        output.insert(0, '\u{feff}');
    }
    Ok(Some(output.into_bytes()))
}

#[cfg(test)]
mod tests;

struct Write {
    path: PathBuf,
    before: Option<Vec<u8>>,
    after: Vec<u8>,
    stage: Option<StagedWrite>,
    committed: bool,
}
impl Drop for Write {
    fn drop(&mut self) {
        if let Some(stage) = self.stage.take() {
            stage.discard();
        }
    }
}

fn changed(path: &Path) -> AppError {
    invalid(
        format!("{} 已被其他程序修改，请重试同步", path.display()),
        format!(
            "{} was changed by another program. Retry synchronization",
            path.display()
        ),
    )
}

fn backup_original(path: &Path, bytes: &[u8]) -> Result<(), AppError> {
    let backup = path.with_extension("json.cc-switch-copilot-cli.bak");
    if backup.exists() {
        return Ok(());
    }
    let mut temp = tempfile::NamedTempFile::new_in(path.parent().expect("validated target"))
        .map_err(|e| AppError::io(&backup, e))?;
    temp.write_all(bytes)
        .map_err(|e| AppError::io(&backup, e))?;
    match temp.persist_noclobber(&backup) {
        Ok(_) => Ok(()),
        Err(e) if e.error.kind() == std::io::ErrorKind::AlreadyExists => Ok(()),
        Err(e) => Err(AppError::io(&backup, e.error)),
    }
}

fn plan(
    previous: &SyncSettings,
    next: &SyncSettings,
    document: &Value,
) -> Result<Vec<Write>, AppError> {
    if next.targets.len() > 64 {
        return Err(invalid(
            "最多选择 64 个同步目标",
            "Select at most 64 sync targets",
        ));
    }
    if next.enabled && next.targets.is_empty() {
        return Err(invalid(
            "请先选择 VS Code 同步目标",
            "Select a VS Code sync target first",
        ));
    }
    let groups = if next.enabled {
        project(document)?
    } else {
        Vec::new()
    };
    let mut paths = BTreeMap::new();
    for (options, enable) in [(previous, false), (next, true)] {
        for raw in &options.targets {
            let path = target_path(raw)?;
            if !options.enabled {
                continue;
            }
            let key = path.to_string_lossy().to_string();
            #[cfg(windows)]
            let key = key.to_lowercase();
            paths.insert(key, (path, enable));
        }
    }
    let mut writes = Vec::new();
    for (_, (path, enable)) in paths {
        if enable && path.parent().is_none_or(|p| !p.is_dir()) {
            return Err(invalid(
                format!("同步目录不存在：{}", path.display()),
                format!("The sync directory does not exist: {}", path.display()),
            ));
        }
        let before = read_raw(&path)?;
        let Some(after) = merged_source(before.as_deref(), if enable { &groups } else { &[] })
            .map_err(|e| {
                invalid(
                    format!("无法同步 {}：{e}", path.display()),
                    format!("Cannot synchronize {}: {e}", path.display()),
                )
            })?
        else {
            continue;
        };
        if after.len() as u64 > MAX_CONFIG_BYTES {
            return Err(invalid(
                format!(
                    "{} 的生成配置超过 8 MiB，请减少同步的模型数据",
                    path.display()
                ),
                format!(
                    "The generated configuration for {} exceeds 8 MiB. Reduce the model data",
                    path.display()
                ),
            ));
        }
        writes.push(Write {
            path,
            before,
            after,
            stage: None,
            committed: false,
        });
    }
    Ok(writes)
}

/// Called under copilot_config's write lock. Prepare every destination first and
/// roll back our writes if a destination or the caller's settings commit fails.
pub(crate) fn apply<T>(
    previous: &SyncSettings,
    next: &SyncSettings,
    document: &Value,
    commit: impl FnOnce() -> Result<T, AppError>,
) -> Result<T, AppError> {
    let mut writes = plan(previous, next, document)?;
    for write in &mut writes {
        write.stage = Some(stage_write(&write.path, &write.after, Some(0o600), false)?);
    }
    let result = (|| {
        for write in &mut writes {
            if read_raw(&write.path)? != write.before {
                return Err(changed(&write.path));
            }
            if let Some(bytes) = &write.before {
                backup_original(&write.path, bytes)?;
            }
            write.stage.take().expect("staged write").commit()?;
            write.committed = true;
            if read_raw(&write.path)?.as_deref() != Some(write.after.as_slice()) {
                return Err(changed(&write.path));
            }
        }
        commit()
    })();
    if let Err(error) = result {
        let mut rollback_errors = Vec::new();
        for write in writes.iter().rev().filter(|w| w.committed) {
            let restore = (|| {
                if read_raw(&write.path)?.as_deref() != Some(write.after.as_slice()) {
                    return Err(changed(&write.path));
                }
                match &write.before {
                    Some(bytes) => crate::config::atomic_write_private(&write.path, bytes),
                    None => fs::remove_file(&write.path).map_err(|e| AppError::io(&write.path, e)),
                }
            })();
            if let Err(e) = restore {
                rollback_errors.push(e.to_string());
            }
        }
        if !rollback_errors.is_empty() {
            return Err(invalid(
                format!(
                    "同步失败：{error}；部分文件无法回滚：{}",
                    rollback_errors.join("; ")
                ),
                format!(
                    "Sync failed: {error}; some files could not be restored: {}",
                    rollback_errors.join("; ")
                ),
            ));
        }
        return Err(error);
    }
    result
}

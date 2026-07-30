use std::collections::BTreeMap;
use std::fs::{self, OpenOptions};
use std::io::{self, Write};
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

use serde::{Deserialize, Serialize};
use serde_json::{json, Map, Value};
use toml_edit::DocumentMut;

use crate::CoreError;

use super::ProviderRecord;

/// live writer 的目标平台由建连状态固定，不能用桌面宿主机平台替代远端平台。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TargetPlatform {
    Linux,
    Windows,
    Macos,
}

impl TargetPlatform {
    /// 普通本机状态使用当前编译目标；跨平台测试和远程握手可以显式覆盖。
    pub const fn current() -> Self {
        if cfg!(target_os = "windows") {
            Self::Windows
        } else if cfg!(target_os = "macos") {
            Self::Macos
        } else {
            Self::Linux
        }
    }
}

/// live 投影只接受显式 HOME 与目标平台，禁止在领域代码中再次读取全局 HOME。
pub struct LiveContext<'a> {
    pub home: &'a Path,
    pub platform: TargetPlatform,
}

/// Provider 切换结果预留非致命警告，保持桌面与 Agent 的 camelCase 协议一致。
#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct SwitchResult {
    pub warnings: Vec<String>,
}

/// 在数据库事务前检查平台条件能力；当前仅 Claude Desktop 不支持 Linux live 投影。
pub fn ensure_projection_supported(context: &LiveContext<'_>, app: &str) -> Result<(), CoreError> {
    if app == "claude-desktop" && context.platform == TargetPlatform::Linux {
        return Err(CoreError::CapabilityUnavailable(
            "Linux 目标不支持 Claude Desktop live 配置".to_string(),
        ));
    }
    match app {
        "claude" | "claude-desktop" | "codex" | "gemini" | "grokbuild" | "opencode"
        | "openclaw" | "hermes" => Ok(()),
        _ => Err(CoreError::UnsupportedApp(app.to_string())),
    }
}

/// 将完整 Provider 投影到目标 HOME；内部错误统一收敛为不含凭据和完整路径的稳定错误。
pub fn project_provider(
    context: &LiveContext<'_>,
    app: &str,
    provider: &ProviderRecord,
) -> Result<SwitchResult, CoreError> {
    ensure_projection_supported(context, app)?;
    let result = match app {
        "claude" => project_claude(context, provider),
        "claude-desktop" => project_claude_desktop(context, provider),
        "codex" => project_codex(context, provider),
        "gemini" => project_gemini(context, provider),
        "grokbuild" => project_grokbuild(context, provider),
        "opencode" => project_opencode(context, provider),
        "openclaw" => project_openclaw(context, provider),
        "hermes" => project_hermes(context, provider),
        _ => return Err(CoreError::UnsupportedApp(app.to_string())),
    };
    result.map_err(|error| CoreError::LiveWriteFailed(format!("{app}: {error}")))?;
    Ok(SwitchResult::default())
}

fn project_claude(context: &LiveContext<'_>, provider: &ProviderRecord) -> Result<(), LiveError> {
    let path = configured_live_path(
        provider,
        "claudeSettings",
        context.home.join(".claude").join("settings.json"),
    );
    if provider_uses_full_snapshot(provider) {
        return write_json_atomic(&path, &provider.settings_config);
    }
    let mut live = read_json5_object(&path, Value::Object(Map::new()))?;
    shallow_merge_json_object(&mut live, provider.settings_config.clone())?;
    write_json_atomic(&path, &live)
}

fn shallow_merge_json_object(target: &mut Value, incoming: Value) -> Result<(), LiveError> {
    // Claude 的 env 等顶层块由当前 Provider 完整拥有；只保留未被 payload 涉及的顶层公共项，
    // 不能递归保留上一 Provider 的 token/base URL 等凭据。
    let target = target
        .as_object_mut()
        .ok_or_else(|| LiveError::new("现有 JSON 根节点必须是对象"))?;
    let incoming = incoming
        .as_object()
        .ok_or_else(|| LiveError::new("Provider JSON 根节点必须是对象"))?;
    for (key, value) in incoming {
        target.insert(key.clone(), value.clone());
    }
    Ok(())
}

fn project_claude_desktop(
    _context: &LiveContext<'_>,
    _provider: &ProviderRecord,
) -> Result<(), LiveError> {
    // Windows/macOS 的 Claude Desktop profile 仍需要桌面集成上下文；当前 Core 仅明确拒绝 Linux。
    Err(LiveError::new(
        "Claude Desktop profile writer 尚未接入无界面 Core",
    ))
}

fn project_codex(context: &LiveContext<'_>, provider: &ProviderRecord) -> Result<(), LiveError> {
    let settings = provider_object(provider, "Codex")?;
    let auth = settings
        .get("auth")
        .filter(|value| value.is_object())
        .cloned()
        .ok_or_else(|| LiveError::new("Codex 配置缺少 auth 对象"))?;
    let auth_path = configured_live_path(
        provider,
        "codexAuth",
        context.home.join(".codex").join("auth.json"),
    );
    let preserve_official_auth = provider
        .meta
        .as_ref()
        .and_then(|meta| meta.get("_ccLive"))
        .and_then(|live| live.get("preserveCodexOfficialAuth"))
        .and_then(Value::as_bool)
        .unwrap_or(false);
    let is_official = provider.category.as_deref() == Some("official");
    let should_write_auth = (is_official && codex_auth_has_login_material(&auth))
        || (!is_official && !preserve_official_auth);
    let mut config = settings
        .get("config")
        .and_then(Value::as_str)
        .map(str::to_string);
    if should_write_auth {
        // auth.json 是 Provider 拥有的完整快照，不能合并残留上一账号的 OAuth/token 字段。
        write_json_atomic(&auth_path, &auth)?;
    } else if let Some(api_key) = auth.get("OPENAI_API_KEY").and_then(Value::as_str) {
        config = Some(set_codex_bearer_token(
            config.as_deref().unwrap_or_default(),
            api_key,
        )?);
    }

    if let Some(config) = config.as_deref() {
        let config_path = configured_live_path(
            provider,
            "codexConfig",
            context.home.join(".codex").join("config.toml"),
        );
        if provider_uses_full_snapshot(provider) {
            toml::from_str::<toml::Table>(config)
                .map_err(|error| LiveError::new(format!("Provider TOML 无效: {error}")))?;
            atomic_write(&config_path, config.as_bytes())?;
        } else {
            merge_toml_file(&config_path, config)?;
        }
    }
    Ok(())
}

fn provider_uses_full_snapshot(provider: &ProviderRecord) -> bool {
    provider
        .meta
        .as_ref()
        .and_then(|meta| meta.get("_ccLive"))
        .and_then(|live| live.get("fullSnapshot"))
        .and_then(Value::as_bool)
        .unwrap_or(false)
}

fn codex_auth_has_login_material(auth: &Value) -> bool {
    // 空官方快照只描述内置路由，不拥有 auth.json；已有 ChatGPT 登录必须继续保留。
    auth.get("OPENAI_API_KEY")
        .and_then(Value::as_str)
        .is_some_and(|value| !value.trim().is_empty())
        || auth
            .pointer("/tokens/access_token")
            .and_then(Value::as_str)
            .is_some_and(|value| !value.trim().is_empty())
}

fn set_codex_bearer_token(config: &str, token: &str) -> Result<String, LiveError> {
    if config.trim().is_empty() {
        return Err(LiveError::new(
            "Codex 第三方 Provider 缺少 config.toml，无法保留官方 auth",
        ));
    }
    let mut document = config
        .parse::<DocumentMut>()
        .map_err(|error| LiveError::new(format!("Codex TOML 无效: {error}")))?;
    let provider_id = document
        .get("model_provider")
        .and_then(|item| item.as_str())
        .map(str::to_string);
    if let Some(provider_id) = provider_id {
        if let Some(provider_table) = document
            .get_mut("model_providers")
            .and_then(|item| item.as_table_mut())
            .and_then(|providers| providers.get_mut(&provider_id))
            .and_then(|item| item.as_table_mut())
        {
            provider_table["experimental_bearer_token"] = toml_edit::value(token);
            return Ok(document.to_string());
        }
    }
    // 非标准或缺失 provider 表时使用 Codex 支持的顶层兼容字段，避免重建用户表结构。
    document["experimental_bearer_token"] = toml_edit::value(token);
    Ok(document.to_string())
}

fn project_gemini(context: &LiveContext<'_>, provider: &ProviderRecord) -> Result<(), LiveError> {
    let settings = provider_object(provider, "Gemini")?;
    let env = settings
        .get("env")
        .and_then(Value::as_object)
        .ok_or_else(|| LiveError::new("Gemini 配置缺少 env 对象"))?;
    let env_path = configured_live_path(
        provider,
        "geminiEnv",
        context.home.join(".gemini").join(".env"),
    );
    merge_env_file(&env_path, env)?;

    let mut projected_config = settings
        .get("config")
        .filter(|config| !config.is_null())
        .cloned()
        .unwrap_or_else(|| json!({}));
    if !projected_config.is_object() {
        return Err(LiveError::new("Gemini config 必须是对象或 null"));
    }
    deep_merge_json(
        &mut projected_config,
        json!({
            "security": {
                "auth": { "selectedType": gemini_selected_type(provider) }
            }
        }),
    );
    {
        // 即使 Provider 没有 config，也要投影鉴权类型；Gemini CLI 依赖该字段选择 OAuth/API key。
        let config = &projected_config;
        if !config.is_null() {
            let config_path = configured_live_path(
                provider,
                "geminiSettings",
                context.home.join(".gemini").join("settings.json"),
            );
            let mut live = read_json5_object(&config_path, Value::Object(Map::new()))?;
            deep_merge_json(&mut live, config.clone());
            write_json_atomic(&config_path, &live)?;
        }
    }
    Ok(())
}

fn gemini_selected_type(provider: &ProviderRecord) -> &'static str {
    let partner_key = provider
        .meta
        .as_ref()
        .and_then(|meta| meta.get("partnerPromotionKey"))
        .and_then(Value::as_str);
    let google_official = partner_key
        .is_some_and(|key| key.eq_ignore_ascii_case("google-official"))
        || provider.name.eq_ignore_ascii_case("google")
        || provider.name.to_ascii_lowercase().starts_with("google ");
    if google_official {
        "oauth-personal"
    } else {
        "gemini-api-key"
    }
}

fn project_grokbuild(
    context: &LiveContext<'_>,
    provider: &ProviderRecord,
) -> Result<(), LiveError> {
    let config = provider_object(provider, "Grok Build")?
        .get("config")
        .and_then(Value::as_str)
        .ok_or_else(|| LiveError::new("Grok Build 配置缺少 config 字符串"))?;
    let path = configured_live_path(
        provider,
        "grokConfig",
        context.home.join(".grok").join("config.toml"),
    );
    merge_toml_file(&path, config)
}

fn project_opencode(context: &LiveContext<'_>, provider: &ProviderRecord) -> Result<(), LiveError> {
    let path = configured_live_path(
        provider,
        "opencodeConfig",
        context
            .home
            .join(".config")
            .join("opencode")
            .join("opencode.json"),
    );
    let mut live = read_json5_object(
        &path,
        json!({ "$schema": "https://opencode.ai/config.json" }),
    )?;
    let root = live
        .as_object_mut()
        .ok_or_else(|| LiveError::new("OpenCode 根配置必须是对象"))?;
    let fragment = provider
        .settings_config
        .get("provider")
        .and_then(|providers| providers.get(&provider.id))
        .cloned()
        .unwrap_or_else(|| provider.settings_config.clone());
    let providers = ensure_object_entry(root, "provider");
    providers.insert(provider.id.clone(), fragment);
    write_json_atomic(&path, &live)
}

fn project_openclaw(context: &LiveContext<'_>, provider: &ProviderRecord) -> Result<(), LiveError> {
    let path = configured_live_path(
        provider,
        "openclawConfig",
        context.home.join(".openclaw").join("openclaw.json"),
    );
    let mut live = read_json5_object(
        &path,
        json!({ "models": { "mode": "merge", "providers": {} } }),
    )?;
    let root = live
        .as_object_mut()
        .ok_or_else(|| LiveError::new("OpenClaw 根配置必须是对象"))?;
    let models = ensure_object_entry(root, "models");
    models
        .entry("mode".to_string())
        .or_insert_with(|| Value::String("merge".to_string()));
    let providers = ensure_object_entry(models, "providers");
    providers.insert(provider.id.clone(), provider.settings_config.clone());
    write_json_atomic(&path, &live)
}

fn project_hermes(context: &LiveContext<'_>, provider: &ProviderRecord) -> Result<(), LiveError> {
    let path = configured_live_path(provider, "hermesConfig", hermes_path(context));
    let mut root = read_yaml_mapping(&path)?;
    let key = serde_yaml::Value::String("custom_providers".to_string());
    let mut providers = root
        .remove(&key)
        .and_then(|value| value.as_sequence().cloned())
        .unwrap_or_default();
    let normalized = normalize_hermes_provider(provider)?;
    if let Some(existing) = providers
        .iter_mut()
        .find(|item| item.get("name").and_then(|name| name.as_str()) == Some(&provider.id))
    {
        merge_yaml_mapping(existing, normalized);
    } else {
        providers.push(normalized);
    }
    root.insert(key, serde_yaml::Value::Sequence(providers));
    let text = serde_yaml::to_string(&serde_yaml::Value::Mapping(root))
        .map_err(|error| LiveError::new(format!("Hermes YAML 序列化失败: {error}")))?;
    atomic_write(&path, text.as_bytes())
}

fn hermes_path(context: &LiveContext<'_>) -> PathBuf {
    if context.platform == TargetPlatform::Windows {
        context
            .home
            .join("AppData")
            .join("Local")
            .join("hermes")
            .join("config.yaml")
    } else {
        context.home.join(".hermes").join("config.yaml")
    }
}

fn configured_live_path(provider: &ProviderRecord, key: &str, default: PathBuf) -> PathBuf {
    // 桌面适配层可传入已解析的 per-app override；Agent 不携带该控制块时只使用显式 HOME。
    provider
        .meta
        .as_ref()
        .and_then(|meta| meta.get("_ccLive"))
        .and_then(|live| live.get("paths"))
        .and_then(|paths| paths.get(key))
        .and_then(Value::as_str)
        .filter(|path| !path.trim().is_empty())
        .map(PathBuf::from)
        .unwrap_or(default)
}

fn normalize_hermes_provider(provider: &ProviderRecord) -> Result<serde_yaml::Value, LiveError> {
    let mut value = provider.settings_config.clone();
    let object = value
        .as_object_mut()
        .ok_or_else(|| LiveError::new("Hermes Provider 配置必须是对象"))?;
    for (camel, snake) in [
        ("baseUrl", "base_url"),
        ("apiKey", "api_key"),
        ("apiMode", "api_mode"),
        ("maxTokens", "max_tokens"),
        ("contextLength", "context_length"),
    ] {
        if let Some(field) = object.remove(camel) {
            object.entry(snake.to_string()).or_insert(field);
        }
    }
    for field in ["api", "_cc_source", "provider_key"] {
        object.remove(field);
    }
    if let Some(Value::Array(models)) = object.remove("models") {
        let mut normalized = Map::new();
        for model in models {
            let Value::Object(mut model) = model else {
                continue;
            };
            let Some(id) = model
                .remove("id")
                .and_then(|value| value.as_str().map(str::trim).map(str::to_string))
                .filter(|id| !id.is_empty())
            else {
                continue;
            };
            normalized.insert(id, Value::Object(model));
        }
        object.insert("models".to_string(), Value::Object(normalized));
    }
    object.insert("name".to_string(), Value::String(provider.id.clone()));
    if let Some(first_model) = object
        .get("models")
        .and_then(Value::as_object)
        .and_then(|models| models.keys().next())
        .cloned()
    {
        object.insert("model".to_string(), Value::String(first_model));
    }
    serde_yaml::to_value(value)
        .map_err(|error| LiveError::new(format!("Hermes Provider 转换失败: {error}")))
}

fn merge_yaml_mapping(target: &mut serde_yaml::Value, incoming: serde_yaml::Value) {
    // Hermes 对选中条目执行前向兼容合并：新 payload 覆盖同名字段，未知旧字段继续保留。
    let (Some(target), Some(incoming)) = (target.as_mapping_mut(), incoming.as_mapping()) else {
        *target = incoming;
        return;
    };
    for (key, value) in incoming {
        target.insert(key.clone(), value.clone());
    }
}

fn provider_object<'a>(
    provider: &'a ProviderRecord,
    app_name: &str,
) -> Result<&'a Map<String, Value>, LiveError> {
    provider
        .settings_config
        .as_object()
        .ok_or_else(|| LiveError::new(format!("{app_name} Provider 配置必须是对象")))
}

fn ensure_object_entry<'a>(
    root: &'a mut Map<String, Value>,
    key: &str,
) -> &'a mut Map<String, Value> {
    let value = root
        .entry(key.to_string())
        .or_insert_with(|| Value::Object(Map::new()));
    if !value.is_object() {
        *value = Value::Object(Map::new());
    }
    value.as_object_mut().expect("刚刚归一化为 JSON 对象")
}

fn deep_merge_json(target: &mut Value, incoming: Value) {
    // 对象递归合并以保留公共片段和未知字段；标量/数组由 Provider 明确覆盖。
    match (target, incoming) {
        (Value::Object(target), Value::Object(incoming)) => {
            for (key, value) in incoming {
                if let Some(existing) = target.get_mut(&key) {
                    deep_merge_json(existing, value);
                } else {
                    target.insert(key, value);
                }
            }
        }
        (target, incoming) => *target = incoming,
    }
}

fn merge_env_file(path: &Path, incoming: &Map<String, Value>) -> Result<(), LiveError> {
    let mut env = BTreeMap::new();
    if path.exists() {
        let content = fs::read_to_string(path).map_err(|error| LiveError::io("读取 env", error))?;
        for line in content.lines() {
            let trimmed = line.trim();
            if trimmed.is_empty() || trimmed.starts_with('#') {
                continue;
            }
            if let Some((key, value)) = trimmed.split_once('=') {
                env.insert(key.trim().to_string(), value.trim().to_string());
            }
        }
    }
    for (key, value) in incoming {
        let value = value
            .as_str()
            .ok_or_else(|| LiveError::new(format!("Gemini env 字段 {key} 必须是字符串")))?;
        env.insert(key.clone(), value.to_string());
    }
    let text = env
        .into_iter()
        .map(|(key, value)| format!("{key}={value}"))
        .collect::<Vec<_>>()
        .join("\n");
    atomic_write(path, text.as_bytes())
}

fn merge_toml_file(path: &Path, incoming: &str) -> Result<(), LiveError> {
    // toml 做语法验证，toml_edit 负责保留现有未被 Provider 拥有的顶层项和注释。
    toml::from_str::<toml::Table>(incoming)
        .map_err(|error| LiveError::new(format!("Provider TOML 无效: {error}")))?;
    let existing = if path.exists() {
        fs::read_to_string(path).map_err(|error| LiveError::io("读取 TOML", error))?
    } else {
        String::new()
    };
    let mut target = existing
        .parse::<DocumentMut>()
        .map_err(|error| LiveError::new(format!("现有 TOML 无效: {error}")))?;
    let incoming = incoming
        .parse::<DocumentMut>()
        .map_err(|error| LiveError::new(format!("Provider TOML 无效: {error}")))?;
    // Provider 携带的顶层项归它所有，直接替换；未出现的公共项和用户项原样保留。
    for (key, item) in incoming.as_table().iter() {
        target.as_table_mut().insert(key, item.clone());
    }
    atomic_write(path, target.to_string().as_bytes())
}

fn read_json5_object(path: &Path, default: Value) -> Result<Value, LiveError> {
    if !path.exists() {
        return Ok(default);
    }
    let text = fs::read_to_string(path).map_err(|error| LiveError::io("读取 JSON", error))?;
    let value: Value = json5::from_str(&text)
        .map_err(|error| LiveError::new(format!("现有 JSON/JSON5 无效: {error}")))?;
    if !value.is_object() {
        return Err(LiveError::new("现有 JSON/JSON5 根节点必须是对象"));
    }
    Ok(value)
}

fn read_yaml_mapping(path: &Path) -> Result<serde_yaml::Mapping, LiveError> {
    if !path.exists() {
        return Ok(serde_yaml::Mapping::new());
    }
    let text = fs::read_to_string(path).map_err(|error| LiveError::io("读取 YAML", error))?;
    if text.trim().is_empty() {
        return Ok(serde_yaml::Mapping::new());
    }
    serde_yaml::from_str::<serde_yaml::Value>(&text)
        .map_err(|error| LiveError::new(format!("现有 Hermes YAML 无效: {error}")))?
        .as_mapping()
        .cloned()
        .ok_or_else(|| LiveError::new("Hermes YAML 根节点必须是映射"))
}

fn write_json_atomic(path: &Path, value: &Value) -> Result<(), LiveError> {
    let bytes = serde_json::to_vec_pretty(value)
        .map_err(|error| LiveError::new(format!("JSON 序列化失败: {error}")))?;
    atomic_write(path, &bytes)
}

fn atomic_write(path: &Path, bytes: &[u8]) -> Result<(), LiveError> {
    let parent = path
        .parent()
        .ok_or_else(|| LiveError::new("live 文件缺少父目录"))?;
    fs::create_dir_all(parent).map_err(|error| LiveError::io("创建 live 目录", error))?;
    let file_name = path
        .file_name()
        .and_then(|value| value.to_str())
        .ok_or_else(|| LiveError::new("live 文件名无效"))?;
    let nonce = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_nanos();
    let temporary = parent.join(format!(
        ".{file_name}.cc-switch-{}-{nonce}.tmp",
        std::process::id()
    ));
    let result = (|| -> Result<(), LiveError> {
        let mut file = OpenOptions::new()
            .create_new(true)
            .write(true)
            .open(&temporary)
            .map_err(|error| LiveError::io("创建 live 临时文件", error))?;
        file.write_all(bytes)
            .map_err(|error| LiveError::io("写入 live 临时文件", error))?;
        file.flush()
            .map_err(|error| LiveError::io("刷新 live 临时文件", error))?;
        file.sync_all()
            .map_err(|error| LiveError::io("同步 live 临时文件", error))?;
        if let Ok(metadata) = fs::metadata(path) {
            fs::set_permissions(&temporary, metadata.permissions())
                .map_err(|error| LiveError::io("继承 live 文件权限", error))?;
        }
        replace_file(&temporary, path)
    })();
    if result.is_err() {
        let _ = fs::remove_file(&temporary);
    }
    result
}

#[cfg(windows)]
fn replace_file(temporary: &Path, path: &Path) -> Result<(), LiveError> {
    // Rust 标准库在 Windows 上不能覆盖已存在目标；保持桌面现有兼容策略。
    if path.exists() {
        fs::remove_file(path).map_err(|error| LiveError::io("替换旧 live 文件", error))?;
    }
    fs::rename(temporary, path).map_err(|error| LiveError::io("提交 live 文件", error))
}

#[cfg(not(windows))]
fn replace_file(temporary: &Path, path: &Path) -> Result<(), LiveError> {
    fs::rename(temporary, path).map_err(|error| LiveError::io("提交 live 文件", error))
}

#[derive(Debug)]
struct LiveError(String);

impl LiveError {
    fn new(message: impl Into<String>) -> Self {
        Self(message.into())
    }

    fn io(action: &str, error: io::Error) -> Self {
        // 不拼接完整路径，避免 RPC 错误暴露远端目录；保留 OS 错误类别便于诊断权限问题。
        Self(format!("{action}失败: {error}"))
    }
}

impl std::fmt::Display for LiveError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str(&self.0)
    }
}

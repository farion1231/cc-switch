//! AtomCode TUI 的 live 配置（`~/.atomcode/config.toml`）。
//!
//! AtomCode 以共存模式管理供应商：一个账号对应一个
//! `[provider_accounts.<id>]` 节点，其模型以 `account = "<id>"` 挂在
//! `[models."<key>"]` 下；切换供应商即把 `default_provider` /
//! `default_model` 指到该账号的某个模型键。
use crate::config::get_home_dir;
use crate::error::AppError;
use indexmap::IndexMap;
use serde_json::{json, Value};
use std::{
    fs,
    path::{Path, PathBuf},
    sync::Mutex,
};

static WRITE_LOCK: Mutex<()> = Mutex::new(());

pub(crate) fn data_dir() -> PathBuf {
    // 设置里的目录覆盖（与其他应用的 override-dir 体系一致）优先于 ATOMCODE_HOME。
    if let Some(dir) = crate::settings::get_atomcode_override_dir() {
        return dir;
    }
    std::env::var_os("ATOMCODE_HOME")
        .filter(|value| !value.is_empty())
        .map(PathBuf::from)
        .unwrap_or_else(|| get_home_dir().join(".atomcode"))
}

pub(crate) fn config_path() -> PathBuf {
    data_dir().join("config.toml")
}

pub(crate) fn read(path: &Path) -> Result<Value, AppError> {
    let text = match fs::read_to_string(path) {
        Ok(text) => text,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(json!({})),
        Err(error) => return Err(AppError::io(path, error)),
    };
    let value: Value = toml::from_str(&text)
        .map_err(|_| AppError::Config("Invalid AtomCode TOML configuration".into()))?;
    if !value.is_object()
        || ["provider_accounts", "models"]
            .iter()
            .any(|key| value.get(key).is_some_and(|v| !v.is_object()))
    {
        return Err(AppError::Config(
            "AtomCode provider_accounts and models must be objects".into(),
        ));
    }
    Ok(value)
}

fn models_of(document: &Value, account: &str) -> IndexMap<String, Value> {
    document
        .get("models")
        .and_then(Value::as_object)
        .into_iter()
        .flatten()
        .filter(|(_, model)| model.get("account").and_then(Value::as_str) == Some(account))
        .map(|(key, model)| (key.clone(), model.clone()))
        .collect()
}

pub(crate) fn get_providers() -> Result<IndexMap<String, Value>, AppError> {
    let document = read(&config_path())?;
    let mut providers: IndexMap<String, Value> = document
        .get("provider_accounts")
        .and_then(Value::as_object)
        .into_iter()
        .flatten()
        .map(|(id, account)| {
            let models = models_of(&document, id);
            (id.clone(), json!({"account": account, "models": models}))
        })
        .collect();
    // 兼容读取：官方配置参考的 legacy `[providers.*]` 格式（AtomCode 启动时会在
    // 原位迁移，但迁移前 CC Switch 导入会拿到 0 个账号）。只读投影，不写盘；
    // 新 schema 里已存在的账号 id 优先，legacy 同名条目跳过（不改现有行为）。
    if let Some(legacy) = document.get("providers").and_then(Value::as_object) {
        for (name, entry) in legacy {
            if providers.contains_key(name) {
                continue;
            }
            if let Some(projected) = project_legacy_provider(name, entry) {
                providers.insert(name.clone(), projected);
            }
        }
    }
    Ok(providers)
}

/// legacy `[providers.<name>]` → 新 schema 形状的只读投影。
/// 官方文档：`type` 仅 openai / claude / ollama，`model` 必填；
/// 模型键沿用 provider 名，使 legacy `default_provider` 能按同一规则反查。
fn project_legacy_provider(name: &str, entry: &Value) -> Option<Value> {
    let legacy_type = entry.get("type").and_then(Value::as_str)?;
    let model = entry.get("model").and_then(Value::as_str)?;
    if model.trim().is_empty() {
        return None;
    }
    let provider = match legacy_type {
        "claude" => "anthropic",
        "ollama" => "ollama",
        // 带 base_url 的 openai 协议走通用 chat 协议；不带则用 openai 厂商预设
        "openai" => {
            if entry
                .get("base_url")
                .and_then(Value::as_str)
                .is_some_and(|url| !url.trim().is_empty())
            {
                "openai-compatible"
            } else {
                "openai"
            }
        }
        _ => return None,
    };
    let mut account = json!({"provider": provider});
    for field in ["base_url", "api_key"] {
        if let Some(value) = entry.get(field).and_then(Value::as_str) {
            if !value.trim().is_empty() {
                account[field] = json!(value);
            }
        }
    }
    let mut model_entry = json!({"account": name, "model": model});
    if let Some(size) = entry.get("context_window").and_then(Value::as_i64) {
        model_entry["context_window"] = json!(size);
    }
    Some(json!({"account": account, "models": {name: model_entry}}))
}

/// 获取 AtomCode 当前选中的账号 ID（如果有的话）。
/// 返回空字符串表示未设置、配置文件不存在或模型键解析不到账号。
///
/// 解析顺序与 AtomCode 自身一致（`effective_model_selection()`，
/// `crates/atomcode-config/src/config/mod.rs:1448-1457`）：`default_model`
/// （canonical，`mod.rs:661-665`）优先，回退 legacy 的 `default_provider`
/// （`mod.rs:633-637`）。两者在 AtomCode CLI 程序化切换时写成同一个
/// selection id（`crates/atomcode-cli/src/main.rs:3215-3219`），本分支的
/// select_provider_model 双写行为与之对齐。
///
/// 模型键（selection id）需通过 models[模型键].account 反查账号 ID；与
/// AtomCode 推荐的 selection id 格式 `<account>/<model>`（`mod.rs:657-660`）
/// 相容，legacy `[providers.*]` 投影出来的配置同样能解析。
pub(crate) fn current_provider() -> Result<String, AppError> {
    let document = read(&config_path())?;
    let default_model_key = ["default_model", "default_provider"]
        .iter()
        .find_map(|field| {
            document
                .get(*field)
                .and_then(Value::as_str)
                .and_then(|key| if key.is_empty() { None } else { Some(key) })
        });
    let default_model_key = match default_model_key {
        Some(key) => key,
        None => return Ok(String::new()),
    };

    // 通过模型键找到对应的账号 ID
    let account_id = document
        .get("models")
        .and_then(|models| models.get(default_model_key))
        .and_then(|model| model.get("account"))
        .and_then(Value::as_str)
        // legacy `[providers.*]` 配置没有 models 表，`default_provider` 直接
        // 命名 provider 名；与 get_providers 的投影 id（provider 名）一致。
        .or_else(|| {
            document
                .get("providers")
                .and_then(Value::as_object)
                .filter(|providers| providers.contains_key(default_model_key))
                .map(|_| default_model_key)
        })
        .unwrap_or_default();

    Ok(account_id.to_string())
}

/// `provider_accounts.<id>.provider` 的合法取值 = AtomCode 内置预设 id
/// （atomcode 仓库 `crates/atomcode-config/src/config/provider_preset.rs` 的
/// `PRESETS`：5 个通用协议 + 厂商预设）。AtomCode 对未知 id **静默回退**
/// `openai-compatible`，写错值不会报错、只会换协议跑，所以这里必须白名单拦下。
const PROVIDER_IDS: &[&str] = &[
    // 通用协议（AtomCode TUI 的 PROTOCOLS）
    "openai-compatible",
    "anthropic-compatible",
    "openai-responses",
    "deepseek-messages",
    "ollama",
    // 厂商预设
    "openai",
    "anthropic",
    "deepseek",
    "zhipu",
    "zhipu-coding",
    "aliyun",
    "siliconflow",
    "moonshot",
    "minimax",
    "minimax-token-plan",
    "xiaomi-mimo",
    "longcat",
    "volcengine",
    "volcengine-coding",
    "openrouter",
    "taotoken",
    "opencode",
];

/// 早期 cc-switch 写入的 wire 名 → AtomCode 预设 id。`claude` 若不归一化会被
/// AtomCode 当成未知 id 回退到 openai-compatible，Anthropic 账号直接不可用；
/// `openai_responses` 同理会退成 chat 协议。
fn canonical_provider_id(provider: &str) -> &str {
    match provider {
        "claude" => "anthropic",
        "openai_responses" => "openai-responses",
        "gemini" => "openai-compatible",
        other => other,
    }
}

pub(crate) fn validate_provider(id: &str, config: &Value) -> Result<(), AppError> {
    if id.trim().is_empty() || id.chars().any(char::is_control) {
        return Err(AppError::InvalidInput(
            "Enter an AtomCode account key".into(),
        ));
    }
    if !id
        .chars()
        .all(|c| c.is_ascii_alphanumeric() || matches!(c, '-' | '_' | '.'))
    {
        return Err(AppError::InvalidInput(
            "AtomCode account key may only contain letters, digits, '-', '_' or '.'".into(),
        ));
    }
    let account = &config["account"];
    let provider = account
        .get("provider")
        .and_then(Value::as_str)
        .unwrap_or_default();
    // 历史 wire 名（claude / openai_responses / gemini）照样接住，写盘时归一化
    if !PROVIDER_IDS.contains(&canonical_provider_id(provider)) {
        return Err(AppError::InvalidInput(
            "Select a supported AtomCode provider".into(),
        ));
    }
    // base_url is optional: native presets carry a default endpoint, and accounts imported
    // from an existing config.toml may not carry one. When present it must be a valid http(s) URL.
    let base_url = account
        .get("base_url")
        .and_then(Value::as_str)
        .unwrap_or_default()
        .trim();
    if !base_url.is_empty()
        && !url::Url::parse(base_url)
            .is_ok_and(|url| matches!(url.scheme(), "http" | "https") && url.host_str().is_some())
    {
        return Err(AppError::InvalidInput(
            "Enter a valid AtomCode endpoint URL".into(),
        ));
    }
    // API key is optional: an account may rely on AtomCode's native OAuth (`auth.toml`).
    let models = config
        .get("models")
        .and_then(Value::as_object)
        .filter(|models| !models.is_empty())
        .ok_or_else(|| AppError::InvalidInput("Add at least one AtomCode model".into()))?;
    for (key, model) in models {
        if key.trim().is_empty()
            || model
                .get("account")
                .is_some_and(|value| value.as_str() != Some(id))
            || model
                .get("model")
                .and_then(Value::as_str)
                .is_none_or(|name| name.trim().is_empty())
        {
            return Err(AppError::InvalidInput(
                "AtomCode models need a key, model ID and matching account".into(),
            ));
        }
        if let Some(size) = model.get("context_window").and_then(Value::as_u64) {
            if size == 0 {
                return Err(AppError::InvalidInput(
                    "AtomCode model context_window must be positive".into(),
                ));
            }
        }
    }
    Ok(())
}

/// Preserve TOML comments and all tables outside the edited account's nodes.
/// `providers` 是 legacy `[providers.*]` 兼容表：升格/删除会移除其中的同名
/// 节点（`apply_provider`），这里必须一并同步，否则清理只发生在内存里、
/// 文件上的 legacy 节点依旧残留并复活。
fn write(path: &Path, before: &Value, after: &Value) -> Result<(), AppError> {
    let text = {
        let original = fs::read_to_string(path).unwrap_or_default();
        let mut document = original
            .parse::<toml_edit::DocumentMut>()
            .map_err(|_| AppError::Config("Invalid AtomCode TOML configuration".into()))?;
        let serialized = toml::to_string(after).map_err(|e| AppError::Config(e.to_string()))?;
        let next = serialized
            .parse::<toml_edit::DocumentMut>()
            .map_err(|e| AppError::Config(e.to_string()))?;
        for section in ["provider_accounts", "models", "providers"] {
            let old = before.get(section).and_then(Value::as_object);
            let new = after.get(section).and_then(Value::as_object);
            if old == new {
                continue;
            }
            if document.get(section).is_none() {
                document[section] = toml_edit::Item::Table(toml_edit::Table::new());
            }
            let table = document[section]
                .as_table_like_mut()
                .ok_or_else(|| AppError::Config("Invalid AtomCode table".into()))?;
            for (key, _) in old.into_iter().flatten() {
                if new.is_none_or(|map| !map.contains_key(key)) {
                    table.remove(key);
                }
            }
            for (key, value) in new.into_iter().flatten() {
                if old.and_then(|map| map.get(key)) != Some(value) {
                    table.insert(key, next[section][key].clone());
                }
            }
        }
        for field in ["default_provider", "default_model"] {
            if before.get(field) != after.get(field) {
                document[field] = next[field].clone();
            }
        }
        document.to_string()
    };
    crate::config::atomic_write_private(path, text.as_bytes())
}

fn apply_provider(
    before: &Value,
    id: &str,
    config: Option<Value>,
    create: bool,
) -> Result<Value, AppError> {
    if let Some(config) = &config {
        validate_provider(id, config)?;
    }
    let mut document = before.clone();
    let exists = document
        .get("provider_accounts")
        .and_then(|accounts| accounts.get(id))
        .is_some();
    if config.is_none() && !exists {
        // legacy `[providers.*]` 投影出来的卡不在 provider_accounts 里，删除只动
        // 新 schema 节点 → 静默无效、下次启动又回来。明确拒绝并指路：先在
        // AtomCode 里跑一次让它自己迁移成新格式，之后正常删。
        if document
            .get("providers")
            .and_then(Value::as_object)
            .is_some_and(|legacy| legacy.contains_key(id))
        {
            return Err(AppError::InvalidInput(
                "This provider comes from a legacy [providers.*] configuration; run AtomCode once to migrate it, then remove it".into(),
            ));
        }
        return Ok(before.clone());
    }
    if create && exists {
        return Err(AppError::InvalidInput(format!(
            "AtomCode account key '{id}' already exists"
        )));
    }
    // Removing a provider must not strand the active selection on its models.
    if config.is_none() && selected_model_belongs_to(&document, id) {
        return Err(AppError::InvalidInput(
            "Select another provider in AtomCode before removing this one".into(),
        ));
    }
    for key in ["provider_accounts", "models"] {
        if document.get(key).is_none() {
            document[key] = json!({});
        }
    }
    // New model keys must not collide with models owned by another account.
    if let Some(config) = &config {
        for key in config["models"].as_object().unwrap().keys() {
            if document["models"]
                .as_object()
                .unwrap()
                .get(key)
                .and_then(|model| model.get("account"))
                .and_then(Value::as_str)
                .is_some_and(|account| account != id)
            {
                return Err(AppError::InvalidInput(format!(
                    "AtomCode model key '{key}' belongs to another provider"
                )));
            }
        }
        // Editing the current account must not delete or rename the default model.
        // 与 current_provider 同一 dual-field 口径：default_model 优先，回退 legacy
        // default_provider——只写了 legacy 字段的配置同样受保护，不留悬空选择。
        if let Some(default_key) = effective_default_model_key(&document) {
            if document["models"]
                .as_object()
                .unwrap()
                .get(default_key)
                .and_then(|model| model.get("account"))
                .and_then(Value::as_str)
                == Some(id)
                && !config["models"]
                    .as_object()
                    .unwrap()
                    .contains_key(default_key)
            {
                return Err(AppError::InvalidInput(format!(
                    "Cannot delete or rename the default model '{}' while it is selected",
                    default_key
                )));
            }
        }
    }
    document["models"]
        .as_object_mut()
        .unwrap()
        .retain(|_, model| model.get("account").and_then(Value::as_str) != Some(id));
    // 升格（switch/add 写入新节点）或删除后，同 id 的 legacy `[providers.<id>]`
    // 节点必须一并清除：删除守卫只挡纯投影卡（`!exists` 且 legacy 存在），
    // 一旦 `exists == true` 就走正常路径只删新 schema 节点，legacy 节点残留
    // 会在下次启动/导入重新投影出同名卡（账号复活）。投影覆盖 legacy 条目的
    // 全部官方字段（type/model/base_url/api_key/context_window），且模型键
    // 沿用 provider 名，legacy `default_provider` 指向它时仍能按 dual-field
    // 口径解析到新 models 表。
    let promoted = config.is_some() && !exists;
    let removing = config.is_none();
    if let Some(config) = config {
        // An empty base_url means "use the provider's built-in endpoint"; writing
        // `base_url = ""` into config.toml would override that default with an invalid URL.
        let mut account = config["account"].clone();
        if let Some(map) = account.as_object_mut() {
            // 写盘只留 AtomCode 认识的预设 id（claude → anthropic 等），
            // 否则 AtomCode 会把未知 id 静默回退到 openai-compatible
            if let Some(provider) = map
                .get("provider")
                .and_then(Value::as_str)
                .map(str::to_string)
            {
                let canonical = canonical_provider_id(&provider);
                if canonical != provider {
                    map.insert("provider".into(), json!(canonical));
                }
            }
            if map.get("base_url").and_then(Value::as_str) == Some("") {
                map.remove("base_url");
            }
        }
        document["provider_accounts"][id] = account;
        for (key, model) in config["models"].as_object().unwrap() {
            let mut model = model.clone();
            model["account"] = json!(id);
            document["models"][key] = model;
        }
    } else {
        document["provider_accounts"]
            .as_object_mut()
            .unwrap()
            .remove(id);
    }
    // removing 时必然 exists（纯投影卡在上面已早退），即删除路径。
    if promoted || removing {
        if let Some(legacy) = document.get_mut("providers").and_then(Value::as_object_mut) {
            legacy.remove(id);
        }
    }
    Ok(document)
}

/// 按 AtomCode `effective_model_selection()` 的口径解析当前选中的模型键：
/// `default_model` 优先，回退 legacy `default_provider`。守卫与选中判定
/// 必须用同一口径，否则只写 legacy 字段的配置会漏拦、留下悬空选择。
fn effective_default_model_key(document: &Value) -> Option<&str> {
    ["default_model", "default_provider"]
        .iter()
        .find_map(|field| {
            document
                .get(*field)
                .and_then(Value::as_str)
                .filter(|key| !key.is_empty())
        })
}

fn selected_model_belongs_to(document: &Value, id: &str) -> bool {
    effective_default_model_key(document)
        .and_then(|key| document.get("models").and_then(|models| models.get(key)))
        .and_then(|model| model.get("account"))
        .and_then(Value::as_str)
        == Some(id)
}

pub(crate) fn save_and_commit<T>(
    id: &str,
    config: Option<Value>,
    create: bool,
    commit: impl FnOnce() -> Result<T, AppError>,
) -> Result<T, AppError> {
    let path = config_path();
    save_at_and_commit(&path, id, config, create, commit)
}

fn save_at_and_commit<T>(
    path: &Path,
    id: &str,
    config: Option<Value>,
    create: bool,
    commit: impl FnOnce() -> Result<T, AppError>,
) -> Result<T, AppError> {
    let _guard = WRITE_LOCK
        .lock()
        .map_err(|e| AppError::Message(e.to_string()))?;
    crate::mcode_config::write_and_commit(
        path,
        || {
            crate::mode::operation::failpoint::hit("atomcode:write")?;
            let before = read(path)?;
            let after = apply_provider(&before, id, config, create)?;
            if before == after {
                return Ok(());
            }
            write(path, &before, &after)
        },
        commit,
    )
}

pub(crate) fn set_provider(id: &str, config: Value) -> Result<(), AppError> {
    save_and_commit(id, Some(config), false, || Ok(()))
}

pub(crate) fn remove_provider(id: &str) -> Result<(), AppError> {
    save_and_commit(id, None, false, || Ok(()))
}

fn select_provider_model(document: &mut Value, id: &str) -> Result<(), AppError> {
    let models = document
        .get("models")
        .and_then(Value::as_object)
        .ok_or_else(|| AppError::InvalidInput("The AtomCode provider has no models".into()))?;
    let belongs_to_account = |key: &str| {
        models
            .get(key)
            .and_then(|model| model.get("account"))
            .and_then(Value::as_str)
            == Some(id)
    };
    let selected = document
        .get("default_model")
        .and_then(Value::as_str)
        .filter(|key| belongs_to_account(key))
        .or_else(|| {
            models
                .keys()
                .find(|key| belongs_to_account(key))
                .map(String::as_str)
        })
        .ok_or_else(|| AppError::InvalidInput("The AtomCode provider has no models".into()))?
        .to_string();
    document["default_model"] = json!(selected);
    document["default_provider"] = json!(selected);
    Ok(())
}

pub(crate) fn switch_and_commit<T>(
    id: &str,
    archived_config: Value,
    commit: impl FnOnce(Value) -> Result<T, AppError>,
) -> Result<T, AppError> {
    let path = config_path();
    switch_at_and_commit(&path, id, archived_config, commit)
}

fn switch_at_and_commit<T>(
    path: &Path,
    id: &str,
    archived_config: Value,
    commit: impl FnOnce(Value) -> Result<T, AppError>,
) -> Result<T, AppError> {
    let _guard = WRITE_LOCK
        .lock()
        .map_err(|e| AppError::Message(e.to_string()))?;
    let before = read(path)?;
    // A node already in config.toml belongs to AtomCode; a card click must not
    // overwrite changes made outside CC Switch since the last refresh.
    let mut after = if before
        .get("provider_accounts")
        .and_then(|accounts| accounts.get(id))
        .is_some()
    {
        before.clone()
    } else {
        apply_provider(&before, id, Some(archived_config), false)?
    };
    select_provider_model(&mut after, id)?;
    let models = models_of(&after, id);
    let config = json!({
        "account": after["provider_accounts"][id],
        "models": models,
    });
    crate::mcode_config::write_and_commit(path, || write(path, &before, &after), || commit(config))
}

/// "Add" writes a saved account into config.toml without changing
/// `default_model` ("switch" does that).
pub(crate) fn add_and_commit<T>(
    id: &str,
    archived_config: Value,
    commit: impl FnOnce() -> Result<T, AppError>,
) -> Result<T, AppError> {
    let path = config_path();
    add_at_and_commit(&path, id, archived_config, commit)
}

fn add_at_and_commit<T>(
    path: &Path,
    id: &str,
    archived_config: Value,
    commit: impl FnOnce() -> Result<T, AppError>,
) -> Result<T, AppError> {
    let _guard = WRITE_LOCK
        .lock()
        .map_err(|e| AppError::Message(e.to_string()))?;
    let before = read(path)?;
    let after = if before
        .get("provider_accounts")
        .and_then(|accounts| accounts.get(id))
        .is_some()
    {
        before.clone()
    } else {
        apply_provider(&before, id, Some(archived_config), false)?
    };
    crate::mcode_config::write_and_commit(
        path,
        || {
            if before == after {
                Ok(())
            } else {
                write(path, &before, &after)
            }
        },
        commit,
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use serial_test::serial;

    fn toml_text() -> String {
        r#"
# AtomCode keeps user comments here.
default_provider = "acct-key-a"
default_model = "acct-key-a"

[lsp]
enabled = false

[provider_accounts.acct]
provider = "openai"
base_url = "https://api.example.com/v1"
api_key = "sk-test"

[models."acct-key-a"]
account = "acct"
model = "key-a"
context_window = 262144
"#
        .to_string()
    }

    #[test]
    fn switch_preserves_comments_and_unrelated_tables() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("config.toml");
        fs::write(&path, toml_text()).unwrap();
        let config = json!({
            "account": {"provider": "deepseek", "base_url": "https://api.deepseek.example/v1", "api_key": "sk-2"},
            "models": {"fresh-ds": {"model": "deepseek-chat", "context_window": 65536}}
        });
        switch_at_and_commit(&path, "fresh", config.clone(), |written| {
            assert_eq!(written["account"]["provider"], json!("deepseek"));
            Ok(())
        })
        .unwrap();
        let text = fs::read_to_string(&path).unwrap();
        assert!(text.contains("AtomCode keeps user comments here."));
        assert!(text.contains("[lsp]"));
        let document = read(&path).unwrap();
        assert_eq!(document["default_model"], json!("fresh-ds"));
        assert_eq!(document["default_provider"], json!("fresh-ds"));
        // 未被切换的既有账号原样保留。
        assert_eq!(
            document["provider_accounts"]["acct"]["provider"],
            json!("openai")
        );
        assert_eq!(document["models"]["acct-key-a"]["model"], json!("key-a"));
    }

    #[test]
    fn add_writes_account_without_changing_default_model() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("config.toml");
        fs::write(&path, toml_text()).unwrap();
        let config = json!({
            "account": {"provider": "openai", "base_url": "https://b.example/v1"},
            "models": {"b-m": {"model": "m"}}
        });
        add_at_and_commit(&path, "b", config, || Ok(())).unwrap();
        let document = read(&path).unwrap();
        assert_eq!(document["default_model"], json!("acct-key-a"));
        assert_eq!(
            document["provider_accounts"]["b"]["base_url"],
            json!("https://b.example/v1")
        );
    }

    #[test]
    fn remove_rejects_current_account_and_preserves_others() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("config.toml");
        fs::write(&path, toml_text()).unwrap();
        assert!(save_at_and_commit(&path, "acct", None, false, || Ok(())).is_err());
        let config = json!({
            "account": {"provider": "openai", "base_url": "https://b.example/v1"},
            "models": {"b-m": {"model": "m"}}
        });
        switch_at_and_commit(&path, "b", config, |_| Ok(())).unwrap();
        save_at_and_commit(&path, "acct", None, false, || Ok(())).unwrap();
        let document = read(&path).unwrap();
        assert!(document["provider_accounts"].get("acct").is_none());
        assert_eq!(document["default_model"], json!("b-m"));
    }

    /// 升格 legacy 投影卡后，同 id 的 `[providers.*]` 节点必须随批清除；
    /// 否则 exists == true 后删除守卫不再触发，卡会复活。
    #[test]
    fn switch_promotes_legacy_card_and_delete_sticks() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("config.toml");
        fs::write(
            &path,
            r#"
default_provider = "deepseek"

[providers.deepseek]
type = "openai"
api_key = "sk-legacy"
model = "deepseek-chat"
base_url = "https://api.deepseek.example/v1"
"#,
        )
        .unwrap();
        let archived = json!({
            "account": {
                "provider": "openai-compatible",
                "base_url": "https://api.deepseek.example/v1",
                "api_key": "sk-legacy"
            },
            "models": {"deepseek": {"model": "deepseek-chat"}}
        });
        switch_at_and_commit(&path, "deepseek", archived, |_| Ok(())).unwrap();
        let document = read(&path).unwrap();
        assert_eq!(
            document["provider_accounts"]["deepseek"]["api_key"],
            json!("sk-legacy")
        );
        assert!(document["providers"].get("deepseek").is_none());
        assert_eq!(document["default_model"], json!("deepseek"));

        // 切走后删除：走正常删除路径，legacy 节点不复活
        let other = json!({
            "account": {"provider": "openai", "base_url": "https://b.example/v1"},
            "models": {"b-m": {"model": "m"}}
        });
        switch_at_and_commit(&path, "other", other, |_| Ok(())).unwrap();
        save_at_and_commit(&path, "deepseek", None, false, || Ok(())).unwrap();
        let document = read(&path).unwrap();
        assert!(document["provider_accounts"].get("deepseek").is_none());
        assert!(document["providers"].get("deepseek").is_none());
        assert!(document["models"].get("deepseek").is_none());
        assert_eq!(document["default_model"], json!("b-m"));
    }

    /// add 路径不改选择，但升格清掉 legacy 节点后 `default_provider` 仍指向
    /// 同名模型键：模型键沿用 provider 名，dual-field 解析不受影响。
    #[test]
    fn add_promotes_legacy_card_and_selection_still_resolves() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("config.toml");
        fs::write(
            &path,
            r#"
default_provider = "deepseek"

[providers.deepseek]
type = "openai"
api_key = "sk-legacy"
model = "deepseek-chat"
"#,
        )
        .unwrap();
        let archived = json!({
            "account": {"provider": "openai-compatible", "api_key": "sk-legacy"},
            "models": {"deepseek": {"model": "deepseek-chat"}}
        });
        add_at_and_commit(&path, "deepseek", archived, || Ok(())).unwrap();
        let document = read(&path).unwrap();
        assert!(document["providers"].get("deepseek").is_none());
        assert!(document["provider_accounts"].get("deepseek").is_some());
        assert_eq!(document["default_provider"], json!("deepseek"));
        assert_eq!(document["models"]["deepseek"]["account"], json!("deepseek"));
    }

    /// 新 schema 与 legacy 同名并存（legacy 被投影跳过）时，删除必须连
    /// shadow 的 legacy 节点一起清掉，否则下次导入复活。
    #[test]
    fn delete_clears_shadowed_legacy_entry() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("config.toml");
        fs::write(
            &path,
            r#"
default_model = "acct-key-a"
default_provider = "deepseek"

[providers.deepseek]
type = "openai"
api_key = "sk-legacy"
model = "deepseek-chat"

[provider_accounts.acct]
provider = "openai"
base_url = "https://api.example.com/v1"

[models."acct-key-a"]
account = "acct"
model = "key-a"

[provider_accounts.deepseek]
provider = "openai-compatible"
api_key = "sk-new"
"#,
        )
        .unwrap();
        let other = json!({
            "account": {"provider": "openai", "base_url": "https://b.example/v1"},
            "models": {"b-m": {"model": "m"}}
        });
        switch_at_and_commit(&path, "other", other, |_| Ok(())).unwrap();
        save_at_and_commit(&path, "deepseek", None, false, || Ok(())).unwrap();
        let document = read(&path).unwrap();
        assert!(document["provider_accounts"].get("deepseek").is_none());
        assert!(document["providers"].get("deepseek").is_none());
        assert_eq!(document["default_model"], json!("b-m"));
    }

    #[test]
    fn set_provider_rejects_collisions_and_bad_models() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("config.toml");
        fs::write(&path, toml_text()).unwrap();
        let config = json!({
            "account": {"provider": "openai", "base_url": "https://x.example/v1"},
            "models": {"acct-key-a": {"model": "m"}}
        });
        assert!(save_at_and_commit(&path, "x", Some(config.clone()), false, || Ok(())).is_err());
        let bad = json!({
            "account": {"provider": "openai", "base_url": "not a url"},
            "models": {"m": {"model": "m"}}
        });
        assert!(save_at_and_commit(&path, "x", Some(bad), false, || Ok(())).is_err());
        let document = read(&path).unwrap();
        assert!(document["provider_accounts"].get("x").is_none());
    }

    /// 测试用的 `ATOMCODE_HOME` 守卫：构造时保存原值再设新值，Drop 时按原值恢复
    /// （有则 set、无则 remove；断言 panic 也会恢复），避免污染同 crate 的后续测试。
    /// 保存的原值在 Drop 中读取，天然满足 clippy 的 dead_code 检查。
    struct AtomcodeHomeGuard(Option<std::ffi::OsString>);
    impl AtomcodeHomeGuard {
        fn set(dir: &std::path::Path) -> Self {
            let previous = std::env::var_os("ATOMCODE_HOME");
            std::env::set_var("ATOMCODE_HOME", dir);
            Self(previous)
        }
    }
    impl Drop for AtomcodeHomeGuard {
        fn drop(&mut self) {
            // take() 保证 panic 回溯期间重复 Drop 不会二次恢复。
            if let Some(previous) = self.0.take() {
                std::env::set_var("ATOMCODE_HOME", previous);
            } else {
                std::env::remove_var("ATOMCODE_HOME");
            }
        }
    }

    #[test]
    #[serial]
    fn legacy_providers_are_projected_readonly_and_new_schema_wins() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("config.toml");
        let _home = AtomcodeHomeGuard::set(dir.path());
        // 官方配置参考的 legacy 格式：[providers.*] + default_provider
        fs::write(
            &path,
            r#"
default_provider = "deepseek"

[providers.deepseek]
type = "openai"
api_key = "sk-xxx"
model = "deepseek-chat"
base_url = "https://api.deepseek.com/v1"
context_window = 64000

[providers.claude]
type = "claude"
api_key = "sk-ant-..."
model = "claude-sonnet-4-6"

[providers.local]
type = "ollama"
model = "llama3.2"
base_url = "http://localhost:11434"
context_window = 8000

[providers.acct]
type = "openai"
model = "gpt-4o"

[provider_accounts.acct]
provider = "openai"
api_key = "sk-new"

[models."acct/new"]
account = "acct"
model = "gpt-5"
"#,
        )
        .unwrap();
        let providers = get_providers().unwrap();
        // 三种协议都投影出来
        let ds = &providers["deepseek"];
        assert_eq!(ds["account"]["provider"], json!("openai-compatible"));
        assert_eq!(
            ds["account"]["base_url"],
            json!("https://api.deepseek.com/v1")
        );
        assert_eq!(ds["account"]["api_key"], json!("sk-xxx"));
        assert_eq!(ds["models"]["deepseek"]["model"], json!("deepseek-chat"));
        assert_eq!(ds["models"]["deepseek"]["account"], json!("deepseek"));
        assert_eq!(ds["models"]["deepseek"]["context_window"], json!(64000));
        assert_eq!(
            providers["claude"]["account"]["provider"],
            json!("anthropic")
        );
        assert_eq!(providers["local"]["account"]["provider"], json!("ollama"));
        // 无 base_url 的 openai 用 openai 厂商预设
        assert_eq!(providers["claude"]["account"].get("base_url"), None);
        // 新 schema 已存在的账号 id 优先，legacy 同名条目不投影、原账号不被覆盖
        assert_eq!(providers["acct"]["account"]["api_key"], json!("sk-new"));
        assert_eq!(
            providers["acct"]["models"]["acct/new"]["model"],
            json!("gpt-5")
        );
        // legacy 选择也能解析到账号（投影 id = provider 名）
        assert_eq!(current_provider().unwrap(), "deepseek");

        // 新 schema 的选择仍优先：default_model 命中新账号
        fs::write(
            &path,
            r#"
default_model = "acct/new"
default_provider = "deepseek"
[providers.deepseek]
type = "openai"
model = "deepseek-chat"
[provider_accounts.acct]
provider = "openai"
[models."acct/new"]
account = "acct"
model = "gpt-5"
"#,
        )
        .unwrap();
        assert_eq!(current_provider().unwrap(), "acct");
        // 未投影的 legacy 条目不该出现在导入结果里（无法识别的 type）
        fs::write(
            &path,
            r#"
[providers.odd]
type = "gemini"
model = "m"
"#,
        )
        .unwrap();
        assert!(get_providers().unwrap().is_empty());
    }

    #[test]
    #[serial]
    fn current_provider_returns_account_id_from_default_provider() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("config.toml");
        fs::write(&path, toml_text()).unwrap();

        // Mock the config path to use our test directory
        let _home = AtomcodeHomeGuard::set(dir.path());

        // toml_text() has:
        // default_provider = "acct-key-a"
        // [models."acct-key-a"]
        // account = "acct"
        // So current_provider() should return "acct", not "acct-key-a"
        let current = current_provider().unwrap();
        assert_eq!(current, "acct", "Should return account ID, not model key");
    }

    #[test]
    #[serial]
    fn current_provider_prefers_default_model_and_degrades_to_empty() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("config.toml");
        let _home = AtomcodeHomeGuard::set(dir.path());

        // `default_model` 是 canonical 选择：同时存在时优先于 legacy 的 `default_provider`
        //（与 AtomCode `effective_model_selection()` 的解析顺序一致）。
        fs::write(
            &path,
            r#"
default_model = "acct-b/m2"
default_provider = "acct-a/m1"

[provider_accounts.acct-a]
provider = "anthropic"
[provider_accounts.acct-b]
provider = "openai"

[models."acct-a/m1"]
account = "acct-a"
model = "m1"
[models."acct-b/m2"]
account = "acct-b"
model = "m2"
"#,
        )
        .unwrap();
        assert_eq!(current_provider().unwrap(), "acct-b");

        // 只写了 legacy `default_provider` 时回退解析。
        fs::write(
            &path,
            r#"
default_provider = "acct-a/m1"

[provider_accounts.acct-a]
provider = "anthropic"
[models."acct-a/m1"]
account = "acct-a"
model = "m1"
"#,
        )
        .unwrap();
        assert_eq!(current_provider().unwrap(), "acct-a");

        // 两个选择字段都缺失 → 空串。
        fs::write(
            &path,
            "[provider_accounts.acct-a]\nprovider = \"anthropic\"\n",
        )
        .unwrap();
        assert_eq!(current_provider().unwrap(), "");

        // 模型键解析不到 `account` → 空串（不 panic）。
        fs::write(
            &path,
            "default_model = \"gone\"\n[models.gone]\nmodel = \"m\"\n",
        )
        .unwrap();
        assert_eq!(current_provider().unwrap(), "");
    }

    #[test]
    fn accepts_preset_ids_and_normalizes_legacy_wire_names() {
        // `account.provider` 是 AtomCode 预设 id（协议 + 厂商），原生预设可不写 base_url。
        for (provider, model) in [
            ("anthropic-compatible", "claude-sonnet-4-6"),
            ("openai-responses", "gpt-6-sol"),
            ("openrouter", "openai/gpt-6.1-sol"),
            ("taotoken", "GLM-5.3-Flash"),
            ("ollama", "llama3.2"),
        ] {
            let config = json!({
                "account": {"provider": provider},
                "models": {"m": {"model": model}}
            });
            validate_provider("acct", &config)
                .unwrap_or_else(|e| panic!("provider {provider} should validate: {e}"));
        }
        // 早期 cc-switch 写过的 wire 名仍可保存（写盘时归一化）
        for legacy in ["claude", "openai_responses", "gemini"] {
            let config = json!({
                "account": {"provider": legacy, "base_url": "https://api.anthropic.com"},
                "models": {"m": {"model": "claude-sonnet-4-6"}}
            });
            validate_provider("acct", &config)
                .unwrap_or_else(|e| panic!("legacy provider {legacy} should validate: {e}"));
        }
        // AtomCode 不认识的 id 直接拒绝：它会被静默回退成 openai-compatible
        let unknown = json!({
            "account": {"provider": "minimax_cn"},
            "models": {"m": {"model": "MiniMax-M3.1-Flash"}}
        });
        assert!(validate_provider("acct", &unknown).is_err());
        // A non-empty base_url must still be a valid http(s) URL.
        let bad = json!({
            "account": {"provider": "ollama", "base_url": "not a url"},
            "models": {"m": {"model": "llama3.2"}}
        });
        assert!(validate_provider("acct", &bad).is_err());
        // Saving an empty base_url must not write `base_url = ""` (it would override the
        // provider's built-in endpoint with an invalid URL), and a legacy provider name
        // must be written back as the preset id AtomCode resolves.
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("config.toml");
        fs::write(&path, toml_text()).unwrap();
        let config = json!({
            "account": {"provider": "ollama", "base_url": ""},
            "models": {"llama3.2": {"model": "llama3.2", "context_window": 8000}}
        });
        add_at_and_commit(&path, "ollama", config, || Ok(())).unwrap();
        let document = read(&path).unwrap();
        assert!(document["provider_accounts"]["ollama"]
            .get("base_url")
            .is_none());

        let legacy_config = json!({
            "account": {"provider": "claude", "base_url": "https://api.anthropic.com"},
            "models": {"sonnet": {"model": "claude-sonnet-4-6"}}
        });
        add_at_and_commit(&path, "legacy-claude", legacy_config, || Ok(())).unwrap();
        let document = read(&path).unwrap();
        assert_eq!(
            document["provider_accounts"]["legacy-claude"]["provider"],
            json!("anthropic")
        );
    }

    #[test]
    fn cannot_delete_or_rename_default_model() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("config.toml");
        fs::write(&path, toml_text()).unwrap();
        // Editing the current account (acct) and trying to delete its default model (acct-key-a)
        let config = json!({
            "account": {"provider": "openai", "base_url": "https://api.example.com/v1", "api_key": "sk-test"},
            "models": {"acct-key-b": {"model": "key-b"}}  // acct-key-a is gone
        });
        let result = save_at_and_commit(&path, "acct", Some(config.clone()), false, || Ok(()));
        assert!(result.is_err());
        assert!(result.unwrap_err().to_string().contains("default model"));
        // Renaming also fails
        let renamed = json!({
            "account": {"provider": "openai", "base_url": "https://api.example.com/v1", "api_key": "sk-test"},
            "models": {"acct-key-renamed": {"model": "key-a"}}
        });
        let result = save_at_and_commit(&path, "acct", Some(renamed), false, || Ok(()));
        assert!(result.is_err());
        assert!(result.unwrap_err().to_string().contains("default model"));
        // But editing a non-default account is fine
        add_at_and_commit(
            &path,
            "other",
            json!({
                "account": {"provider": "deepseek", "base_url": "https://api.deepseek.com"},
                "models": {"other-m": {"model": "deepseek-chat"}}
            }),
            || Ok(()),
        )
        .unwrap();
        let edit_other = json!({
            "account": {"provider": "deepseek", "base_url": "https://api.deepseek.com"},
            "models": {"other-new": {"model": "deepseek-reasoner"}}  // renamed model
        });
        save_at_and_commit(&path, "other", Some(edit_other), false, || Ok(())).unwrap();
    }

    /// dual-field 口径：只写 legacy `default_provider`（无 `default_model`）时，
    /// 删除/改名选中账号的默认模型同样要被拦，不留悬空的 legacy 选择。
    #[test]
    fn legacy_default_provider_only_config_is_guarded_too() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("config.toml");
        fs::write(
            &path,
            r#"
default_provider = "acct-key-a"

[provider_accounts.acct]
provider = "openai"

[models."acct-key-a"]
account = "acct"
model = "key-a"
"#,
        )
        .unwrap();
        // 改名默认模型 → 拒绝
        let renamed = json!({
            "account": {"provider": "openai"},
            "models": {"acct-key-renamed": {"model": "key-a"}}
        });
        let result = save_at_and_commit(&path, "acct", Some(renamed), false, || Ok(()));
        assert!(result.is_err());
        assert!(result.unwrap_err().to_string().contains("default model"));
        // 删除选中的默认模型 → 拒绝
        let deleted = json!({
            "account": {"provider": "openai"},
            "models": {}
        });
        let result = save_at_and_commit(&path, "acct", Some(deleted), false, || Ok(()));
        assert!(result.is_err());
        // 删除被选中的账号 → 拒绝（selected_model_belongs_to 走同一解析）
        let result = save_at_and_commit(&path, "acct", None, false, || Ok(()));
        assert!(result.is_err());
        assert!(result.unwrap_err().to_string().contains("another"));
    }

    /// legacy `[providers.*]` 投影卡：删除必须明确拒绝（只删库会静默无效、
    /// 下次启动复活），错误信息指路「先在 AtomCode 里跑一次完成迁移」。
    #[test]
    #[serial]
    fn legacy_projection_cannot_be_removed() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("config.toml");
        let _home = AtomcodeHomeGuard::set(dir.path());
        fs::write(
            &path,
            r#"
default_provider = "deepseek"

[providers.deepseek]
type = "openai"
api_key = "sk-xxx"
model = "deepseek-chat"
base_url = "https://api.deepseek.com/v1"
"#,
        )
        .unwrap();
        // 投影卡在导入列表里可见
        assert!(get_providers().unwrap().contains_key("deepseek"));
        // 删除被明确拒绝，错误指路迁移
        let result = save_at_and_commit(&path, "deepseek", None, false, || Ok(()));
        let error = result.unwrap_err().to_string();
        assert!(error.contains("legacy [providers.*]"));
        assert!(error.contains("run AtomCode once to migrate it"));
        // 配置文件原样保留（只读投影未被写盘改动）
        assert!(fs::read_to_string(&path)
            .unwrap()
            .contains("[providers.deepseek]"));
        // 迁移成新格式后即可正常删除（先不在 AtomCode 里选中该账号，
        // 否则触发的会是 removeSelected 而不是迁移指路）
        fs::write(
            &path,
            r#"
default_model = ""

[provider_accounts.deepseek]
provider = "openai"

[models."acct/new"]
account = "deepseek"
model = "deepseek-chat"
"#,
        )
        .unwrap();
        save_at_and_commit(&path, "deepseek", None, false, || Ok(())).unwrap();
        assert!(read(&path).unwrap()["provider_accounts"]
            .get("deepseek")
            .is_none());
    }
}

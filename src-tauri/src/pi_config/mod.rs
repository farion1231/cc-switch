//! Thin adapter for Pi's native files.
//!
//! Pi owns account login and the active provider/model in `settings.json`.
//! CC Switch only manages explicit provider entries in `models.json`, plus
//! read/write access to the `skills` array in `settings.json` (shared skill
//! roots & exclusion rules) — every other settings field stays untouched.

use crate::config::{atomic_write_private, get_home_dir};
use crate::error::AppError;
use indexmap::IndexMap;
use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};
use sha2::{Digest, Sha256};
use std::fs;
use std::io::Read;
use std::path::{Path, PathBuf};
use std::sync::{LazyLock, Mutex, MutexGuard};

const MAX_PI_FILE_BYTES: u64 = 1024 * 1024;
const MISSING_MODELS_REVISION: &str = "missing";
static MODELS_FILE_LOCK: LazyLock<Mutex<()>> = LazyLock::new(|| Mutex::new(()));
static PI_SETTINGS_FILE_LOCK: LazyLock<Mutex<()>> = LazyLock::new(|| Mutex::new(()));
#[cfg(test)]
static TEST_AGENT_DIR: LazyLock<Mutex<Option<PathBuf>>> = LazyLock::new(|| Mutex::new(None));

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub(crate) struct PiNativeDefaults {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub default_provider: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub default_model: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub session_dir: Option<String>,
}

pub(crate) fn get_pi_agent_dir() -> Result<PathBuf, AppError> {
    #[cfg(test)]
    if let Some(path) = TEST_AGENT_DIR
        .lock()
        .expect("lock Pi test directory")
        .clone()
    {
        return resolve_pi_agent_dir(Some(path), None, get_home_dir().join(".pi").join("agent"));
    }

    resolve_pi_agent_dir(
        crate::settings::get_pi_override_dir(),
        std::env::var_os("PI_CODING_AGENT_DIR"),
        get_home_dir().join(".pi").join("agent"),
    )
}

fn resolve_pi_agent_dir(
    settings_override: Option<PathBuf>,
    env_override: Option<std::ffi::OsString>,
    default_path: PathBuf,
) -> Result<PathBuf, AppError> {
    let (path, source) = match settings_override {
        Some(path) => (path, "Pi settings override"),
        None => match env_override {
            Some(value) if !value.is_empty() => (
                crate::settings::resolve_override_path(value.to_string_lossy().as_ref()),
                "PI_CODING_AGENT_DIR",
            ),
            _ => (default_path, "Pi default"),
        },
    };
    if !path.is_absolute() {
        return Err(AppError::InvalidInput(format!(
            "{source} must resolve to an absolute directory: {}",
            path.display()
        )));
    }
    Ok(path)
}

pub(crate) fn get_pi_models_path() -> Result<PathBuf, AppError> {
    Ok(get_pi_agent_dir()?.join("models.json"))
}

pub(crate) fn get_pi_settings_path() -> Result<PathBuf, AppError> {
    Ok(get_pi_agent_dir()?.join("settings.json"))
}

pub(crate) fn read_pi_native_defaults() -> Result<PiNativeDefaults, AppError> {
    let path = get_pi_settings_path()?;
    if !path.exists() {
        return Ok(PiNativeDefaults::default());
    }
    let value = read_json5_value(&path, "Pi settings")?;
    let object = value.as_object().ok_or_else(|| {
        AppError::Config(format!(
            "Pi settings root must be an object: {}",
            path.display()
        ))
    })?;
    Ok(PiNativeDefaults {
        default_provider: optional_string(object, "defaultProvider", &path)?,
        default_model: optional_string(object, "defaultModel", &path)?,
        session_dir: optional_string(object, "sessionDir", &path)?,
    })
}

/// 读取 pi settings.json 的 `skills` 数组（共享技能扫描路径与排除/强制规则）。
/// - `Ok(None)`：文件不存在或无 `skills` 键；
/// - `Ok(Some)`：数组内容（跳过非字符串条目）；
/// - `Err`：文件存在但解析失败，或 `skills` 存在但不是数组。
pub(crate) fn read_pi_settings_skills_array() -> Result<Option<Vec<String>>, AppError> {
    let path = get_pi_settings_path()?;
    if !path.exists() {
        return Ok(None);
    }
    let value = read_json5_value(&path, "Pi settings")?;
    let object = value.as_object().ok_or_else(|| {
        AppError::Config(format!(
            "Pi settings root must be an object: {}",
            path.display()
        ))
    })?;
    let Some(skills) = object.get("skills") else {
        return Ok(None);
    };
    let array = skills.as_array().ok_or_else(|| {
        AppError::Config(format!(
            "Pi settings \"skills\" must be an array: {}",
            path.display()
        ))
    })?;
    Ok(Some(
        array
            .iter()
            .filter_map(|entry| entry.as_str().map(str::to_string))
            .collect(),
    ))
}

pub(crate) fn read_pi_native_providers() -> Result<IndexMap<String, Value>, AppError> {
    let _guard = lock_models_file()?;
    read_pi_native_providers_locked(&get_pi_models_path()?)
}

pub(crate) fn read_pi_native_provider(provider_key: &str) -> Result<Option<Value>, AppError> {
    let _guard = lock_models_file()?;
    let path = get_pi_models_path()?;
    let document = read_models_document(&path)?;
    Ok(providers(&document, &path)?.get(provider_key).cloned())
}

pub(crate) fn pi_provider_exists(provider_key: &str) -> Result<bool, AppError> {
    let _guard = lock_models_file()?;
    let path = get_pi_models_path()?;
    let document = read_models_document(&path)?;
    Ok(providers(&document, &path)?.contains_key(provider_key))
}

pub(crate) fn insert_pi_provider(provider_key: &str, config: &Value) -> Result<bool, AppError> {
    validate_provider_node(provider_key, config)?;
    let _guard = lock_models_file()?;
    let path = get_pi_models_path()?;
    let (mut document, expected_revision) = read_models_document_with_revision(&path)?;
    let providers = providers_mut(&mut document, &path)?;

    match providers.get(provider_key) {
        Some(current) if current == config => return Ok(false),
        Some(_) => {
            return Err(AppError::InvalidInput(format!(
                "Pi provider key '{provider_key}' already exists in models.json"
            )))
        }
        None => {}
    }

    providers.insert(provider_key.to_string(), config.clone());
    write_models_document(&path, &document, &expected_revision)?;
    Ok(true)
}

pub(crate) fn replace_pi_provider(
    provider_key: &str,
    expected: &Value,
    replacement: &Value,
) -> Result<(), AppError> {
    validate_provider_node(provider_key, replacement)?;
    let _guard = lock_models_file()?;
    let path = get_pi_models_path()?;
    let (mut document, expected_revision) = read_models_document_with_revision(&path)?;
    let providers = providers_mut(&mut document, &path)?;
    let current = providers.get(provider_key).ok_or_else(|| {
        AppError::Conflict(format!(
            "Pi provider '{provider_key}' is no longer present in models.json"
        ))
    })?;
    if current != expected {
        return Err(AppError::Conflict(format!(
            "Pi provider '{provider_key}' changed outside CC Switch"
        )));
    }
    if current == replacement {
        return Ok(());
    }
    providers.insert(provider_key.to_string(), replacement.clone());
    write_models_document(&path, &document, &expected_revision)
}

pub(crate) fn replace_pi_provider_if_present(
    provider_key: &str,
    replacement: &Value,
) -> Result<Option<Value>, AppError> {
    validate_provider_node(provider_key, replacement)?;
    let _guard = lock_models_file()?;
    let path = get_pi_models_path()?;
    let (mut document, expected_revision) = read_models_document_with_revision(&path)?;
    let providers = providers_mut(&mut document, &path)?;
    let Some(current) = providers.get(provider_key).cloned() else {
        return Ok(None);
    };
    if current == *replacement {
        return Ok(Some(current));
    }
    providers.insert(provider_key.to_string(), replacement.clone());
    write_models_document(&path, &document, &expected_revision)?;
    Ok(Some(current))
}

pub(crate) fn remove_pi_provider(provider_key: &str) -> Result<Option<Value>, AppError> {
    remove_pi_provider_inner(provider_key, None)
}

pub(crate) fn remove_pi_provider_if_matches(
    provider_key: &str,
    expected: &Value,
) -> Result<bool, AppError> {
    remove_pi_provider_inner(provider_key, Some(expected)).map(|removed| removed.is_some())
}

fn remove_pi_provider_inner(
    provider_key: &str,
    expected: Option<&Value>,
) -> Result<Option<Value>, AppError> {
    let _guard = lock_models_file()?;
    let path = get_pi_models_path()?;
    let (mut document, expected_revision) = read_models_document_with_revision(&path)?;
    let providers = providers_mut(&mut document, &path)?;
    let Some(current) = providers.get(provider_key).cloned() else {
        return Ok(None);
    };
    if expected.is_some_and(|expected| current != *expected) {
        return Err(AppError::Conflict(format!(
            "Pi provider '{provider_key}' changed outside CC Switch"
        )));
    }
    providers.remove(provider_key);
    write_models_document(&path, &document, &expected_revision)?;
    Ok(Some(current))
}

pub(crate) fn restore_pi_provider_if_missing(
    provider_key: &str,
    config: &Value,
) -> Result<(), AppError> {
    let _guard = lock_models_file()?;
    let path = get_pi_models_path()?;
    let (mut document, expected_revision) = read_models_document_with_revision(&path)?;
    let providers = providers_mut(&mut document, &path)?;
    match providers.get(provider_key) {
        Some(current) if current == config => Ok(()),
        Some(_) => Err(AppError::Conflict(format!(
            "cannot restore Pi provider '{provider_key}' because another value now owns the key"
        ))),
        None => {
            providers.insert(provider_key.to_string(), config.clone());
            write_models_document(&path, &document, &expected_revision)
        }
    }
}

/// Validate the shape CC Switch can persist as one
/// `models.json.providers.<provider_key>` node.
///
/// Provider ownership is intentionally source-based: every explicit object in
/// `models.json.providers` is manageable, including keys also built into Pi.
/// Pi's `/login` credentials live in `auth.json` and are never read here.
pub(crate) fn validate_provider_node(provider_key: &str, config: &Value) -> Result<(), AppError> {
    if provider_key.trim().is_empty() {
        return Err(AppError::InvalidInput(
            "Pi provider key cannot be empty".to_string(),
        ));
    }
    config.as_object().ok_or_else(|| {
        AppError::InvalidInput("Pi provider configuration must be an object".to_string())
    })?;
    Ok(())
}

pub(crate) fn provider_base_url(config: &Value) -> Result<String, AppError> {
    let provider = config.as_object().ok_or_else(|| {
        AppError::InvalidInput("Pi provider configuration must be an object".to_string())
    })?;
    nonempty_string(provider.get("baseUrl"))
        .or_else(|| {
            provider
                .get("models")
                .and_then(Value::as_array)
                .and_then(|models| {
                    models
                        .iter()
                        .find_map(|model| nonempty_string(model.get("baseUrl")))
                })
        })
        .map(str::to_string)
        .ok_or_else(|| AppError::InvalidInput("Pi provider has no request URL".to_string()))
}

fn lock_models_file() -> Result<MutexGuard<'static, ()>, AppError> {
    MODELS_FILE_LOCK
        .lock()
        .map_err(|error| AppError::Config(format!("Pi models file lock is poisoned: {error}")))
}

fn read_pi_native_providers_locked(path: &Path) -> Result<IndexMap<String, Value>, AppError> {
    let document = read_models_document(path)?;
    let providers = providers(&document, path)?;
    Ok(providers
        .iter()
        .map(|(provider_key, config)| (provider_key.clone(), config.clone()))
        .collect())
}

fn read_models_document(path: &Path) -> Result<Value, AppError> {
    read_models_document_with_revision(path).map(|(document, _)| document)
}

fn read_models_document_with_revision(path: &Path) -> Result<(Value, String), AppError> {
    if !path.exists() {
        return Ok((
            Value::Object(Map::new()),
            MISSING_MODELS_REVISION.to_string(),
        ));
    }
    let bytes = read_file_limited(path, "Pi models")?;
    let revision = revision(&bytes);
    let document = parse_json5_value(path, "Pi models", bytes)?;
    Ok((document, revision))
}

fn read_json5_value(path: &Path, label: &str) -> Result<Value, AppError> {
    parse_json5_value(path, label, read_file_limited(path, label)?)
}

fn read_file_limited(path: &Path, label: &str) -> Result<Vec<u8>, AppError> {
    let file = fs::File::open(path).map_err(|error| AppError::io(path, error))?;
    let metadata = file.metadata().map_err(|error| AppError::io(path, error))?;
    if metadata.len() > MAX_PI_FILE_BYTES {
        return Err(AppError::InvalidInput(format!(
            "{label} file exceeds the 1 MiB limit: {}",
            path.display()
        )));
    }
    let mut bytes = Vec::with_capacity(metadata.len() as usize);
    file.take(MAX_PI_FILE_BYTES + 1)
        .read_to_end(&mut bytes)
        .map_err(|error| AppError::io(path, error))?;
    if bytes.len() as u64 > MAX_PI_FILE_BYTES {
        return Err(AppError::InvalidInput(format!(
            "{label} file exceeds the 1 MiB limit: {}",
            path.display()
        )));
    }
    Ok(bytes)
}

fn parse_json5_value(path: &Path, label: &str, bytes: Vec<u8>) -> Result<Value, AppError> {
    let source = String::from_utf8(bytes).map_err(|error| {
        AppError::Config(format!(
            "{label} file must be UTF-8 ({}): {error}",
            path.display()
        ))
    })?;
    json5::from_str(&source).map_err(|error| {
        AppError::Config(format!(
            "{label} file is not valid JSON/JSONC ({}): {error}",
            path.display()
        ))
    })
}

fn providers<'a>(document: &'a Value, path: &Path) -> Result<&'a Map<String, Value>, AppError> {
    let root = document.as_object().ok_or_else(|| {
        AppError::Config(format!(
            "Pi models root must be an object: {}",
            path.display()
        ))
    })?;
    match root.get("providers") {
        None => Ok(empty_json_object()),
        Some(Value::Object(providers)) => Ok(providers),
        Some(_) => Err(AppError::Config(format!(
            "Pi models 'providers' must be an object: {}",
            path.display()
        ))),
    }
}

fn providers_mut<'a>(
    document: &'a mut Value,
    path: &Path,
) -> Result<&'a mut Map<String, Value>, AppError> {
    let root = document.as_object_mut().ok_or_else(|| {
        AppError::Config(format!(
            "Pi models root must be an object: {}",
            path.display()
        ))
    })?;
    let value = root
        .entry("providers".to_string())
        .or_insert_with(|| Value::Object(Map::new()));
    value.as_object_mut().ok_or_else(|| {
        AppError::Config(format!(
            "Pi models 'providers' must be an object: {}",
            path.display()
        ))
    })
}

fn empty_json_object() -> &'static Map<String, Value> {
    static EMPTY: LazyLock<Map<String, Value>> = LazyLock::new(Map::new);
    &EMPTY
}

fn write_models_document(
    path: &Path,
    document: &Value,
    expected_revision: &str,
) -> Result<(), AppError> {
    let mut bytes =
        serde_json::to_vec_pretty(document).map_err(|source| AppError::JsonSerialize { source })?;
    bytes.push(b'\n');
    ensure_private_models_parent(path)?;
    ensure_models_revision(path, expected_revision)?;
    atomic_write_private(path, &bytes)
}

fn ensure_models_revision(path: &Path, expected_revision: &str) -> Result<(), AppError> {
    let actual_revision = match fs::File::open(path) {
        Ok(_) => revision(&read_file_limited(path, "Pi models")?),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            MISSING_MODELS_REVISION.to_string()
        }
        Err(error) => return Err(AppError::io(path, error)),
    };
    if actual_revision == expected_revision {
        Ok(())
    } else {
        Err(AppError::Conflict(format!(
            "Pi models.json changed outside CC Switch: {}",
            path.display()
        )))
    }
}

fn revision(bytes: &[u8]) -> String {
    format!("{:x}", Sha256::digest(bytes))
}

fn ensure_private_models_parent(path: &Path) -> Result<(), AppError> {
    let parent = path.parent().ok_or_else(|| {
        AppError::Config(format!(
            "Pi models path has no parent directory: {}",
            path.display()
        ))
    })?;
    let created = !parent.exists();
    fs::create_dir_all(parent).map_err(|source| AppError::io(parent, source))?;

    #[cfg(not(unix))]
    let _ = created;

    #[cfg(unix)]
    if created {
        use std::os::unix::fs::PermissionsExt;
        fs::set_permissions(parent, fs::Permissions::from_mode(0o700))
            .map_err(|source| AppError::io(parent, source))?;
    }

    Ok(())
}

fn optional_string(
    object: &Map<String, Value>,
    key: &str,
    path: &Path,
) -> Result<Option<String>, AppError> {
    match object.get(key) {
        None | Some(Value::Null) => Ok(None),
        Some(Value::String(value)) => Ok(Some(value.clone())),
        Some(_) => Err(AppError::Config(format!(
            "Pi settings '{key}' must be a string: {}",
            path.display()
        ))),
    }
}

fn nonempty_string(value: Option<&Value>) -> Option<&str> {
    value
        .and_then(Value::as_str)
        .filter(|value| !value.is_empty())
}

// ─── skills 数组编辑（Pi 共享配置管理）────────────────────────────────────

/// 编辑器装填：严格读取（数组含非字符串元素 → parse_error，不静默过滤——
/// 编辑场景下过滤即销毁）+ 分类/警告（pi_shared 轻量版）+ revision（编辑
/// 会话乐观锁）+ 复选候选源（各应用技能目录实际路径，不含 Pi 自身与
/// ~/.agents/skills——后者 pi 无条件自动发现，加入冗余）。
pub(crate) fn get_pi_skills_config() -> Result<crate::app_config::PiSkillsConfig, AppError> {
    use crate::app_config::{PiSkillsConfig, PiSkillsEntryInfo, PiSkillsSourceInfo};

    let path = get_pi_settings_path()?;
    let mut config = PiSkillsConfig {
        path: path.display().to_string(),
        entries: None,
        parse_error: None,
        revision: String::new(),
        sources: Vec::new(),
    };

    if !path.exists() {
        return Ok(config); // 文件不存在：空态，保存时创建骨架
    }
    let bytes = read_file_limited(&path, "Pi settings")?;
    config.revision = revision(&bytes);
    let source = std::str::from_utf8(&bytes)
        .map_err(|e| AppError::Config(format!("Pi settings 不是 UTF-8: {e}")))?;

    let parsed: serde_json::Value = match json5::from_str(source) {
        Ok(v) => v,
        Err(e) => {
            config.parse_error = Some(format!("settings.json 解析失败：{e}"));
            return Ok(config);
        }
    };
    let Some(root) = parsed.as_object() else {
        config.parse_error = Some("settings.json 根节点必须是对象".to_string());
        return Ok(config);
    };
    let Some(skills) = root.get("skills") else {
        return Ok(config); // 无 skills 键：空态
    };
    let Some(array) = skills.as_array() else {
        config.parse_error = Some("settings.json 的 skills 必须是字符串数组".to_string());
        return Ok(config);
    };
    // 严格读取：非字符串元素 → parse_error（编辑禁用 + 手动修复出口），
    // 防止 get 时过滤、set 时全量替换把非字符串元素静默销毁
    let mut entries = Vec::with_capacity(array.len());
    for item in array {
        match item.as_str() {
            Some(raw) => entries.push(PiSkillsEntryInfo {
                raw: raw.to_string(),
                kind: crate::services::skill::pi_shared::classify_entry_kind(raw)
                    .kind_str()
                    .to_string(),
                warnings: crate::services::skill::pi_shared::entry_warnings(raw),
            }),
            None => {
                config.parse_error = Some(format!(
                    "skills 数组含非字符串元素（{item}），请手动修复后再编辑"
                ));
                config.entries = None;
                return Ok(config);
            }
        }
    }
    config.entries = Some(entries);

    // 复选候选源：各应用技能目录（排除 Pi 自身），仅列存在的
    for app in crate::app_config::AppType::all() {
        if matches!(app, crate::app_config::AppType::Pi) {
            continue;
        }
        let Ok(dir) = crate::services::skill::SkillService::get_app_skills_dir(&app) else {
            continue;
        };
        if !dir.is_dir() {
            continue;
        }
        let listed = config
            .entries
            .as_deref()
            .unwrap_or_default()
            .iter()
            .any(|e| entry_resolves_to(&e.raw, &dir));
        config.sources.push(PiSkillsSourceInfo {
            app: app.as_str().to_string(),
            path: dir.display().to_string(),
            already_listed: listed,
        });
    }
    Ok(config)
}

/// 写入 skills 数组（全量替换；文件中 skills 内容 = 编辑器所见）。
/// 安全链：双层 revision → 空白条目拒绝 → JsoncDocument（数组外逐字保留）
/// → round-trip 三重校验 → 无变化零写入 → 首次改写前快照 → 原子写。
pub(crate) fn set_pi_skills_entries(
    entries: &[String],
    session_revision: &str,
) -> Result<(), AppError> {
    let _guard = PI_SETTINGS_FILE_LOCK
        .lock()
        .map_err(|_| AppError::Config("Pi settings 锁已毒化".into()))?;

    // 空白条目拒绝："" 会被 pi 解析为 agentDir 本身成扫描根（危险静默行为）
    if let Some(bad) = entries.iter().find(|e| e.trim().is_empty()) {
        return Err(AppError::InvalidInput(format!(
            "skills 条目不能为空或纯空白：{bad:?}"
        )));
    }

    let path = get_pi_settings_path()?;
    let (previous_source, previous_bytes) = if path.exists() {
        let bytes = read_file_limited(&path, "Pi settings")?;
        let source = std::str::from_utf8(&bytes)
            .map_err(|e| AppError::Config(format!("Pi settings 不是 UTF-8: {e}")))?
            .to_string();
        (source, Some(bytes))
    } else {
        (String::from("{\n  \"skills\": []\n}\n"), None)
    };

    // 会话层 revision：get 之后文件被外部改过 → 拒绝（防编辑会话内的外部修改被静默吞掉）
    let current_revision = previous_bytes
        .as_ref()
        .map(|b| revision(b))
        .unwrap_or_default();
    let file_existed = previous_bytes.is_some();
    if file_existed && current_revision != session_revision {
        return Err(AppError::Conflict(
            "pi settings.json 已在外部修改，请刷新后重试".into(),
        ));
    }
    if !file_existed && !session_revision.is_empty() {
        return Err(AppError::Conflict(
            "pi settings.json 已在外部创建，请刷新后重试".into(),
        ));
    }

    let mut document = crate::jsonc_document::JsoncDocument::parse(&previous_source)
        .map_err(|e| AppError::Config(format!("Pi settings 解析失败：{e}")))?;
    let mut desired = document.value().clone();
    // skills 键存在但非数组 → 拒绝（不猜用户意图）
    if let Some(existing) = desired.get("skills") {
        if !existing.is_array() {
            return Err(AppError::InvalidInput(
                "settings.json 的 skills 必须是字符串数组".into(),
            ));
        }
    }
    desired["skills"] = serde_json::Value::Array(
        entries
            .iter()
            .map(|e| serde_json::Value::String(e.clone()))
            .collect(),
    );
    document
        .apply(&desired)
        .map_err(|e| AppError::Config(format!("Pi settings 合并失败：{e}")))?;
    let new_source = document
        .validated_source()
        .map_err(|e| AppError::Config(format!("Pi settings 校验失败：{e}")))?;

    // 无变化零写入
    if new_source == previous_source {
        return Ok(());
    }

    // 操作层乐观锁：load 之后、写之前磁盘被外部改过 → 拒绝
    if path.exists() {
        let now = read_file_limited(&path, "Pi settings")?;
        if Some(&now) != previous_bytes.as_ref() {
            return Err(AppError::Conflict(
                "Pi settings.json 已在外部修改，请重试".into(),
            ));
        }
    }

    // 首次改写前快照（保留 20 份，对齐 SKILL_BACKUP_RETAIN_COUNT 惯例）
    if file_existed {
        snapshot_pi_settings(&path, &previous_source)?;
    }

    atomic_write_private(&path, new_source.as_bytes())
        .map_err(|e| AppError::Config(format!("写入 Pi settings 失败：{e}")))?;
    Ok(())
}

/// 条目（raw）解析后是否指向目标目录（canonicalize 去重）
fn entry_resolves_to(raw: &str, target: &std::path::Path) -> bool {
    let resolved = if let Some(rest) = raw.strip_prefix("~/") {
        get_home_dir().join(rest)
    } else if raw == "~" {
        get_home_dir()
    } else if raw.starts_with('/') {
        std::path::PathBuf::from(raw)
    } else {
        return false; // 相对条目解析依赖 agent 目录，候选源均为绝对/~ 形态
    };
    resolved.canonicalize().ok() == target.canonicalize().ok()
}

/// 首次改写前快照到 ~/.cc-switch/backups/pi-settings/（保留最近 20 份）
fn snapshot_pi_settings(path: &std::path::Path, source: &str) -> Result<(), AppError> {
    const RETAIN: usize = 20;
    let dir = crate::config::get_app_config_dir()
        .join("backups")
        .join("pi-settings");
    std::fs::create_dir_all(&dir).map_err(|e| AppError::io(&dir, e))?;
    let ts = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis())
        .unwrap_or(0);
    let backup = dir.join(format!("{ts}.json"));
    std::fs::write(&backup, source).map_err(|e| AppError::io(&backup, e))?;
    // 轮换：按文件名（时间戳）降序保留 RETAIN 份
    let mut entries: Vec<_> = std::fs::read_dir(&dir)
        .map_err(|e| AppError::io(&dir, e))?
        .filter_map(|e| e.ok())
        .map(|e| e.path())
        .filter(|p| p.extension().is_some_and(|x| x == "json"))
        .collect();
    entries.sort();
    if entries.len() > RETAIN {
        for old in &entries[..entries.len() - RETAIN] {
            let _ = std::fs::remove_file(old);
        }
    }
    let _ = path; // path 仅用于日志语义，快照内容为 source
    Ok(())
}

#[cfg(test)]
pub(crate) mod test_support {
    use std::path::{Path, PathBuf};

    pub(crate) struct TestAgentDir {
        _dir: Option<tempfile::TempDir>,
        previous: Option<PathBuf>,
    }

    impl TestAgentDir {
        /// 写入测试用 settings.json（JSON5 内容原样落盘），返回文件路径。
        pub(crate) fn write_settings(&self, json5: &str) -> PathBuf {
            let path = super::get_pi_settings_path().expect("resolve Pi settings path");
            std::fs::create_dir_all(path.parent().expect("agent directory"))
                .expect("create Pi agent directory");
            std::fs::write(&path, json5).expect("write Pi settings");
            path
        }
    }

    impl TestAgentDir {
        pub(crate) fn new() -> Self {
            let dir = tempfile::tempdir().expect("create Pi test directory");
            let agent_dir = dir.path().join("agent");
            Self::set(agent_dir, Some(dir))
        }

        pub(crate) fn at(agent_dir: &Path) -> Self {
            Self::set(agent_dir.to_path_buf(), None)
        }

        fn set(agent_dir: PathBuf, dir: Option<tempfile::TempDir>) -> Self {
            let previous = super::TEST_AGENT_DIR
                .lock()
                .expect("lock Pi test directory")
                .replace(agent_dir);
            Self {
                _dir: dir,
                previous,
            }
        }
    }

    impl Drop for TestAgentDir {
        fn drop(&mut self) {
            *super::TEST_AGENT_DIR
                .lock()
                .expect("lock Pi test directory") = self.previous.take();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    use serial_test::serial;

    fn provider() -> Value {
        json!({
            "name": "Example",
            "baseUrl": "https://api.example.com/v1",
            "api": "openai-completions",
            "apiKey": "secret",
            "models": [{"id": "example-model"}]
        })
    }

    #[test]
    fn provider_node_accepts_unknown_native_fields() {
        let mut value = provider();
        value["sdkOption"] = json!({"timeout": 30});
        value["models"][0]["compat"] = json!({"supportsDeveloperRole": true});
        validate_provider_node("cc-switch-example", &value).expect("valid provider");
    }

    #[test]
    fn provider_node_ownership_depends_on_models_json_membership() {
        let mut oauth = provider();
        oauth["oauth"] = json!("anthropic");
        validate_provider_node("cc-switch-example", &oauth)
            .expect("an explicit models.json node stays manageable");
        validate_provider_node("anthropic", &json!({}))
            .expect("a built-in provider key may be explicitly configured");
        assert!(validate_provider_node("", &json!({})).is_err());
        assert!(validate_provider_node("anthropic", &json!("invalid")).is_err());
    }

    #[test]
    fn relative_agent_directory_is_rejected() {
        let error = resolve_pi_agent_dir(
            None,
            Some("relative/pi-agent".into()),
            PathBuf::from("default"),
        )
        .expect_err("relative Pi directory must be rejected");
        assert!(error.to_string().contains("absolute directory"));
    }

    #[test]
    fn settings_directory_precedes_the_environment() {
        let temp = tempfile::tempdir().expect("tempdir");
        let settings_dir = temp.path().join("settings-agent");
        let env_dir = temp.path().join("env-agent");

        assert_eq!(
            resolve_pi_agent_dir(
                Some(settings_dir.clone()),
                Some(env_dir.into_os_string()),
                temp.path().join("default-agent"),
            )
            .expect("resolve Pi directory"),
            settings_dir
        );
    }

    // ===== Pi skills 配置编辑（get/set + revision + 快照）=====

    use super::{get_pi_skills_config, set_pi_skills_entries};
    use crate::pi_config::test_support::TestAgentDir;
    use std::fs;

    #[test]
    #[serial]
    fn get_returns_entries_classification_and_sources() {
        let temp = tempfile::tempdir().expect("tempdir");
        let _home = crate::services::skill::tests::TestHomeGuard::set(temp.path());
        let _agent = TestAgentDir::new();
        let settings = crate::pi_config::get_pi_settings_path().unwrap();
        fs::create_dir_all(settings.parent().unwrap()).unwrap();
        fs::write(
            &settings,
            "{\n  // 注释\n  \"defaultProvider\": \"anthropic\",\n  \"skills\": [\"~/.claude/skills\", \"!pdf\", \"npm:x\"]\n}\n",
        )
        .unwrap();
        // 候选源目录（Claude 技能目录存在）
        fs::create_dir_all(temp.path().join(".claude").join("skills")).unwrap();

        let config = get_pi_skills_config().unwrap();
        assert!(config.parse_error.is_none());
        let entries = config.entries.expect("entries");
        assert_eq!(entries.len(), 3);
        assert_eq!(entries[0].kind, "scanPath");
        assert_eq!(entries[1].kind, "exclude");
        assert_eq!(entries[2].kind, "packageScheme");
        // 候选源：Claude 存在且已列入
        assert!(config
            .sources
            .iter()
            .any(|s| s.app == "claude" && s.already_listed));
        assert!(!config.revision.is_empty());
    }

    #[test]
    #[serial]
    fn get_parse_error_states() {
        let temp = tempfile::tempdir().expect("tempdir");
        let _home = crate::services::skill::tests::TestHomeGuard::set(temp.path());
        let _agent = TestAgentDir::new();
        let settings = crate::pi_config::get_pi_settings_path().unwrap();
        fs::create_dir_all(settings.parent().unwrap()).unwrap();

        // 非法 JSON5
        fs::write(&settings, "{ broken").unwrap();
        assert!(get_pi_skills_config().unwrap().parse_error.is_some());

        // 数组含非字符串元素（严格保护，不静默过滤）
        fs::write(&settings, r#"{ "skills": ["a", 42] }"#).unwrap();
        let config = get_pi_skills_config().unwrap();
        assert!(config.parse_error.is_some());
        assert!(config.entries.is_none());

        // skills 非数组
        fs::write(&settings, r#"{ "skills": "x" }"#).unwrap();
        assert!(get_pi_skills_config().unwrap().parse_error.is_some());

        // 文件不存在 → 空态
        fs::remove_file(&settings).unwrap();
        let config = get_pi_skills_config().unwrap();
        assert!(config.entries.is_none());
        assert!(config.parse_error.is_none());
        assert!(config.revision.is_empty());
    }

    #[test]
    #[serial]
    fn set_preserves_other_fields_and_rejects_bad_input() {
        let temp = tempfile::tempdir().expect("tempdir");
        let _home = crate::services::skill::tests::TestHomeGuard::set(temp.path());
        let _agent = TestAgentDir::new();
        let settings = crate::pi_config::get_pi_settings_path().unwrap();
        fs::create_dir_all(settings.parent().unwrap()).unwrap();
        let original = "{\n  // 顶层注释\n  \"defaultProvider\": \"anthropic\",  // 行内注释\n  \"skills\": [\"!old\"]\n}\n";
        fs::write(&settings, original).unwrap();
        let revision = get_pi_skills_config().unwrap().revision;

        // 空白条目拒绝
        assert!(set_pi_skills_entries(&["".to_string()], &revision).is_err());
        assert!(set_pi_skills_entries(&["  ".to_string()], &revision).is_err());

        // 正常写入：数组外逐字保留
        set_pi_skills_entries(&["~/.claude/skills".into(), "!pdf".into()], &revision).unwrap();
        let after = fs::read_to_string(&settings).unwrap();
        assert!(after.contains("// 顶层注释"));
        assert!(after.contains("\"defaultProvider\": \"anthropic\",  // 行内注释"));
        assert!(after.contains("\"~/.claude/skills\""));
        assert!(after.contains("\"!pdf\""));
        assert!(!after.contains("\"!old\""));

        // 无变化零写入（内容不回退）
        let rev2 = get_pi_skills_config().unwrap().revision;
        let before_meta = fs::metadata(&settings).unwrap().modified().unwrap();
        set_pi_skills_entries(&["~/.claude/skills".into(), "!pdf".into()], &rev2).unwrap();
        assert_eq!(fs::read_to_string(&settings).unwrap(), after);
        let _ = before_meta;
    }

    #[test]
    #[serial]
    fn set_revision_conflict_and_snapshot() {
        let temp = tempfile::tempdir().expect("tempdir");
        let _home = crate::services::skill::tests::TestHomeGuard::set(temp.path());
        let _agent = TestAgentDir::new();
        let settings = crate::pi_config::get_pi_settings_path().unwrap();
        fs::create_dir_all(settings.parent().unwrap()).unwrap();
        fs::write(&settings, r#"{ "skills": ["!old"] }"#).unwrap();
        let revision = get_pi_skills_config().unwrap().revision;

        // 会话层冲突：get 之后外部改盘
        fs::write(&settings, r#"{ "skills": ["!external"] }"#).unwrap();
        let err = set_pi_skills_entries(&["!new".into()], &revision).unwrap_err();
        assert!(err.to_string().contains("外部修改"));
        // 未写入外部内容
        assert!(fs::read_to_string(&settings).unwrap().contains("!external"));

        // 修正 revision 后写入成功 + 快照生成
        let revision2 = get_pi_skills_config().unwrap().revision;
        set_pi_skills_entries(&["!new".into()], &revision2).unwrap();
        assert!(fs::read_to_string(&settings).unwrap().contains("!new"));
        let backup_dir = crate::config::get_app_config_dir()
            .join("backups")
            .join("pi-settings");
        assert!(
            fs::read_dir(&backup_dir)
                .expect("快照目录")
                .filter_map(|e| e.ok())
                .count()
                >= 1
        );
    }

    #[test]
    #[serial]
    fn set_creates_file_when_missing() {
        let temp = tempfile::tempdir().expect("tempdir");
        let _home = crate::services::skill::tests::TestHomeGuard::set(temp.path());
        let _agent = TestAgentDir::new();
        let settings = crate::pi_config::get_pi_settings_path().unwrap();
        assert!(!settings.exists());

        set_pi_skills_entries(&["~/.claude/skills".into()], "").unwrap();
        let after = fs::read_to_string(&settings).unwrap();
        assert!(after.contains("\"skills\""));
        assert!(after.contains("~/.claude/skills"));
        // 无外部创建冲突时空 revision 可写；外部已创建 → 拒绝
        fs::remove_file(&settings).unwrap();
        fs::write(&settings, r#"{ "skills": [] }"#).unwrap();
        let err = set_pi_skills_entries(&["!x".into()], "").unwrap_err();
        assert!(err.to_string().contains("外部"));
    }

    #[test]
    #[serial]
    fn skills_array_is_none_without_file_or_key() {
        let _agent = test_support::TestAgentDir::new();
        assert_eq!(read_pi_settings_skills_array().expect("missing file"), None);

        _agent.write_settings(r#"{ "defaultProvider": "anthropic" }"#);
        assert_eq!(read_pi_settings_skills_array().expect("missing key"), None);
    }

    #[test]
    #[serial]
    fn skills_array_reads_entries_tolerantly() {
        let _agent = test_support::TestAgentDir::new();
        _agent.write_settings(
            r#"{
              // 共享扫描目录
              "skills": ["~/.claude/skills", "!pdf", 42, "+~/.agents/skills",]
            }"#,
        );

        let skills = read_pi_settings_skills_array()
            .expect("parse settings")
            .expect("skills array");
        assert_eq!(
            skills,
            vec![
                "~/.claude/skills".to_string(),
                "!pdf".to_string(),
                "+~/.agents/skills".to_string(),
            ]
        );
    }

    #[test]
    #[serial]
    fn skills_array_rejects_broken_settings() {
        let _agent = test_support::TestAgentDir::new();

        _agent.write_settings(r#"{ "skills": "~/.claude/skills" }"#);
        assert!(read_pi_settings_skills_array().is_err());

        _agent.write_settings("{ broken");
        assert!(read_pi_settings_skills_array().is_err());
    }

    #[test]
    #[serial]
    fn duplicate_provider_key_is_validation_not_a_write_conflict() {
        let _agent = test_support::TestAgentDir::new();
        insert_pi_provider("duplicate", &provider()).expect("insert provider");
        let mut replacement = provider();
        replacement["name"] = json!("Other");

        let error = insert_pi_provider("duplicate", &replacement)
            .expect_err("duplicate provider key must be rejected");
        assert!(matches!(error, AppError::InvalidInput(_)));
    }

    #[cfg(unix)]
    #[test]
    #[serial]
    fn newly_created_models_file_and_agent_directory_are_private() {
        use std::os::unix::fs::PermissionsExt;

        let _agent = test_support::TestAgentDir::new();
        insert_pi_provider("cc-switch-private", &provider()).expect("write private models file");

        let path = get_pi_models_path().expect("models path");
        let file_mode = fs::metadata(&path)
            .expect("models metadata")
            .permissions()
            .mode()
            & 0o777;
        let directory_mode = fs::metadata(path.parent().expect("agent directory"))
            .expect("agent directory metadata")
            .permissions()
            .mode()
            & 0o777;

        assert_eq!(file_mode, 0o600);
        assert_eq!(directory_mode, 0o700);
    }

    #[test]
    #[serial]
    fn stale_models_revision_does_not_overwrite_an_external_edit() {
        let _agent = test_support::TestAgentDir::new();
        let path = get_pi_models_path().expect("models path");
        ensure_private_models_parent(&path).expect("create agent directory");
        fs::write(&path, r#"{"providers":{"external":{"models":[]}}}"#)
            .expect("write initial models");
        let (_, stale_revision) =
            read_models_document_with_revision(&path).expect("read models revision");

        let external = r#"{"providers":{"external":{"models":[]},"pi-added":{"models":[]}}}"#;
        fs::write(&path, external).expect("edit models externally");

        let replacement = json!({"providers": {"cc-switch": provider()}});
        let error = write_models_document(&path, &replacement, &stale_revision)
            .expect_err("stale write must fail");
        assert!(matches!(error, AppError::Conflict(_)));
        assert_eq!(
            fs::read_to_string(path).expect("read external models"),
            external
        );
    }
}

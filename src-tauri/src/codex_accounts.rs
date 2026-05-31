use base64::{engine::general_purpose::URL_SAFE_NO_PAD, Engine as _};
use chrono::Utc;
use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};
use sha2::{Digest, Sha256};
use std::fs;
#[cfg(unix)]
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};

use crate::codex_config::{get_codex_auth_path, get_codex_config_dir};
use crate::config::{atomic_write, get_app_config_dir, read_json_file, write_json_file};
use crate::error::AppError;
use crate::services::subscription::{query_codex_quota, SubscriptionQuota};

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CodexAccountSummary {
    pub account_key: String,
    pub profile_name: String,
    pub email_masked: String,
    pub plan: String,
    pub auth_mode: String,
    pub is_active: bool,
    pub last_used_at: Option<i64>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CodexAccountSwitchResult {
    pub previous_account_key: Option<String>,
    pub active_account_key: String,
    pub backup_path: String,
    pub restart_recommended: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CodexAppRestartResult {
    pub was_running: bool,
    pub quit_requested: bool,
    pub quit_graceful: bool,
    pub force_quit_used: bool,
    pub opened: bool,
    pub running_after: bool,
    pub launch_method: String,
    pub message: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct RegistryItem {
    account_key: String,
    snapshot_path: String,
    #[serde(default)]
    email: String,
    #[serde(default)]
    alias: String,
    #[serde(default)]
    account_name: String,
    #[serde(default)]
    workspace_name: String,
    #[serde(default)]
    profile_name: String,
    #[serde(default)]
    plan: String,
    #[serde(default)]
    auth_mode: String,
    #[serde(default)]
    last_used_at: Option<i64>,
    #[serde(flatten)]
    extra: Map<String, Value>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct Registry {
    schema_version: u32,
    updated_at: i64,
    active_account_key: Option<String>,
    #[serde(default)]
    items: Vec<RegistryItem>,
    #[serde(flatten)]
    extra: Map<String, Value>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct AuthSnapshot {
    #[serde(default)]
    auth_mode: Option<String>,
    #[serde(default)]
    tokens: Option<Value>,
    #[serde(default, rename = "OPENAI_API_KEY")]
    openai_api_key: Option<Value>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct JwtAuthPayload {
    #[serde(default)]
    email: Option<String>,
    #[serde(default)]
    name: Option<String>,
    #[serde(default, rename = "https://api.openai.com/auth")]
    auth: Option<JwtAuthNamespace>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct JwtAuthNamespace {
    #[serde(default)]
    chatgpt_user_id: Option<String>,
    #[serde(default)]
    chatgpt_plan_type: Option<String>,
    #[serde(default)]
    organizations: Vec<JwtOrganization>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct JwtOrganization {
    #[serde(default)]
    id: Option<String>,
    #[serde(default)]
    is_default: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct LatestSwitch {
    previous_account_key: Option<String>,
    active_account_key: String,
    backup_path: String,
    created_at: String,
}

#[derive(Debug, Clone)]
struct AccountMetadata {
    email: String,
    name: String,
    plan: String,
}

#[derive(Debug, Clone)]
struct AccountPaths {
    registry_path: PathBuf,
    snapshots_dir: PathBuf,
    backups_dir: PathBuf,
    latest_switch_path: PathBuf,
}

pub fn list_accounts() -> Result<Vec<CodexAccountSummary>, AppError> {
    let registry = read_registry_with_snapshot_scan()?;
    Ok(registry
        .items
        .iter()
        .map(|item| to_summary(item, registry.active_account_key.as_deref()))
        .collect())
}

/// 获取所有 Codex 账号的 snapshot 路径（用于多账号用量查询）
pub fn list_account_snapshot_paths() -> Result<Vec<(String, String)>, AppError> {
    let registry = read_registry_with_snapshot_scan()?;
    Ok(registry
        .items
        .iter()
        .map(|item| (item.account_key.clone(), item.snapshot_path.clone()))
        .collect())
}

pub fn capture_current(label: Option<String>) -> Result<CodexAccountSummary, AppError> {
    let auth_path = get_codex_auth_path();
    validate_auth_file(&auth_path)?;

    let paths = account_paths();
    let mut registry = read_registry_or_create()?;
    let current_auth: AuthSnapshot = read_json_file(&auth_path)?;
    let metadata = metadata_from_auth(&current_auth);
    let account_key = account_key_from_auth(&current_auth);
    let existing = registry
        .items
        .iter()
        .find(|item| item.account_key == account_key)
        .cloned();
    let snapshot_path = existing
        .as_ref()
        .map(|item| PathBuf::from(&item.snapshot_path))
        .unwrap_or_else(|| {
            paths
                .snapshots_dir
                .join(format!("{}.json", safe_file_name(&account_key)))
        });

    fs::create_dir_all(&paths.snapshots_dir).map_err(|e| AppError::io(&paths.snapshots_dir, e))?;
    copy_file_atomic(&auth_path, &snapshot_path)?;

    let now = now_seconds();
    let label = clean_label(label.as_deref());
    let next_item = RegistryItem {
        account_key: account_key.clone(),
        snapshot_path: snapshot_path.to_string_lossy().to_string(),
        email: existing
            .as_ref()
            .map(|item| item.email.clone())
            .filter(|value| !value.is_empty())
            .unwrap_or(metadata.email),
        alias: existing
            .as_ref()
            .map(|item| item.alias.clone())
            .unwrap_or_default(),
        account_name: existing
            .as_ref()
            .map(|item| item.account_name.clone())
            .filter(|value| !value.is_empty())
            .unwrap_or_else(|| "Personal".to_string()),
        workspace_name: existing
            .as_ref()
            .map(|item| item.workspace_name.clone())
            .filter(|value| !value.is_empty())
            .unwrap_or_else(|| "Personal".to_string()),
        profile_name: if !label.is_empty() {
            label
        } else {
            existing
                .as_ref()
                .map(|item| item.profile_name.clone())
                .filter(|value| !value.is_empty())
                .unwrap_or_else(|| {
                    if metadata.name.is_empty() {
                        "Current Codex Account".to_string()
                    } else {
                        metadata.name
                    }
                })
        },
        plan: existing
            .as_ref()
            .map(|item| item.plan.clone())
            .filter(|value| !value.is_empty())
            .unwrap_or(metadata.plan),
        auth_mode: existing
            .as_ref()
            .map(|item| item.auth_mode.clone())
            .filter(|value| !value.is_empty())
            .unwrap_or_else(|| current_auth.auth_mode.clone().unwrap_or_default()),
        last_used_at: Some(now),
        extra: existing
            .as_ref()
            .map(|item| item.extra.clone())
            .unwrap_or_default(),
    };

    registry.active_account_key = Some(account_key.clone());
    registry.updated_at = now;
    registry
        .items
        .retain(|item| item.account_key != account_key);
    registry.items.insert(0, next_item.clone());
    write_registry(&registry)?;

    Ok(to_summary(&next_item, Some(&account_key)))
}

pub fn switch_account(account_key: String) -> Result<CodexAccountSwitchResult, AppError> {
    if account_key.trim().is_empty() {
        return Err(AppError::InvalidInput("Missing accountKey.".to_string()));
    }

    let mut registry = read_registry_with_snapshot_scan()?;
    let target = registry
        .items
        .iter()
        .find(|item| item.account_key == account_key)
        .cloned()
        .ok_or_else(|| AppError::Config(format!("Codex account not found: {account_key}")))?;

    validate_auth_file(Path::new(&target.snapshot_path))?;
    ensure_snapshot_matches_account(&target)?;

    let auth_path = get_codex_auth_path();
    if !auth_path.exists() {
        return Err(AppError::Config(format!(
            "Current Codex auth file does not exist: {}",
            auth_path.display()
        )));
    }

    if registry.active_account_key.as_deref() == Some(target.account_key.as_str()) {
        return Ok(CodexAccountSwitchResult {
            previous_account_key: registry.active_account_key,
            active_account_key: target.account_key,
            backup_path: String::new(),
            restart_recommended: false,
        });
    }

    let previous_account_key = registry.active_account_key.clone();
    let backup_path = backup_current_auth(previous_account_key.as_deref())?;
    persist_current_auth_to_active_snapshot(&registry)?;
    copy_file_atomic(Path::new(&target.snapshot_path), &auth_path)?;

    let now = now_seconds();
    registry.active_account_key = Some(target.account_key.clone());
    registry.updated_at = now;
    for item in &mut registry.items {
        if item.account_key == target.account_key {
            item.last_used_at = Some(now);
        }
    }
    write_registry(&registry)?;

    let paths = account_paths();
    if let Some(parent) = paths.latest_switch_path.parent() {
        fs::create_dir_all(parent).map_err(|e| AppError::io(parent, e))?;
    }
    let latest = LatestSwitch {
        previous_account_key: previous_account_key.clone(),
        active_account_key: target.account_key.clone(),
        backup_path: backup_path.to_string_lossy().to_string(),
        created_at: Utc::now().to_rfc3339(),
    };
    write_json_file(&paths.latest_switch_path, &latest)?;

    Ok(CodexAccountSwitchResult {
        previous_account_key,
        active_account_key: target.account_key,
        backup_path: backup_path.to_string_lossy().to_string(),
        restart_recommended: true,
    })
}

pub fn rollback_last_switch() -> Result<CodexAccountSwitchResult, AppError> {
    let paths = account_paths();
    let latest: LatestSwitch = read_json_file(&paths.latest_switch_path)?;
    validate_auth_file(Path::new(&latest.backup_path))?;

    let mut registry = read_registry_with_snapshot_scan()?;
    let backup_path = backup_current_auth(registry.active_account_key.as_deref())?;
    let auth_path = get_codex_auth_path();
    copy_file_atomic(Path::new(&latest.backup_path), &auth_path)?;

    registry.active_account_key = latest.previous_account_key.clone();
    registry.updated_at = now_seconds();
    write_registry(&registry)?;

    Ok(CodexAccountSwitchResult {
        previous_account_key: Some(latest.active_account_key),
        active_account_key: latest.previous_account_key.unwrap_or_default(),
        backup_path: backup_path.to_string_lossy().to_string(),
        restart_recommended: true,
    })
}

pub fn restart_codex_app() -> Result<CodexAppRestartResult, AppError> {
    restart_codex_app_impl()
}

fn read_registry() -> Result<Registry, AppError> {
    let path = account_paths().registry_path;
    let registry: Registry = read_json_file(&path)?;
    Ok(registry)
}

fn read_registry_or_create() -> Result<Registry, AppError> {
    let path = account_paths().registry_path;
    if path.exists() {
        return read_registry();
    }
    Ok(Registry {
        schema_version: 2,
        updated_at: now_seconds(),
        active_account_key: None,
        items: Vec::new(),
        extra: Map::new(),
    })
}

fn read_registry_with_snapshot_scan() -> Result<Registry, AppError> {
    let mut registry = read_registry_or_create()?;
    let paths = account_paths();
    if !paths.snapshots_dir.exists() {
        return Ok(registry);
    }

    let mut discovered = Vec::new();
    for entry in
        fs::read_dir(&paths.snapshots_dir).map_err(|e| AppError::io(&paths.snapshots_dir, e))?
    {
        let entry = entry.map_err(|e| AppError::io(&paths.snapshots_dir, e))?;
        let path = entry.path();
        if path.extension().and_then(|ext| ext.to_str()) != Some("json") {
            continue;
        }
        if validate_auth_file(&path).is_err() {
            continue;
        }
        let auth: AuthSnapshot = match read_json_file(&path) {
            Ok(auth) => auth,
            Err(_) => continue,
        };
        let metadata = metadata_from_auth(&auth);
        let account_key = account_key_from_auth(&auth);
        if registry
            .items
            .iter()
            .any(|item| item.account_key == account_key)
        {
            continue;
        }

        let modified = entry
            .metadata()
            .and_then(|metadata| metadata.modified())
            .ok()
            .and_then(|modified| {
                modified
                    .duration_since(std::time::SystemTime::UNIX_EPOCH)
                    .ok()
            })
            .map(|duration| duration.as_secs() as i64);
        discovered.push(RegistryItem {
            account_key,
            snapshot_path: path.to_string_lossy().to_string(),
            email: metadata.email,
            alias: String::new(),
            account_name: "Personal".to_string(),
            workspace_name: "Personal".to_string(),
            profile_name: if metadata.name.is_empty() {
                "Codex Account Snapshot".to_string()
            } else {
                metadata.name
            },
            plan: metadata.plan,
            auth_mode: auth.auth_mode.unwrap_or_default(),
            last_used_at: modified,
            extra: Map::new(),
        });
    }

    registry.items.extend(discovered);
    Ok(registry)
}

fn write_registry(registry: &Registry) -> Result<(), AppError> {
    write_json_file(&account_paths().registry_path, registry)
}

fn validate_auth_file(path: &Path) -> Result<(), AppError> {
    let auth: AuthSnapshot = read_json_file(path)?;
    let auth_mode = auth.auth_mode.as_deref().unwrap_or_default();
    match auth_mode {
        "chatgpt" => {
            if auth.tokens.is_none() {
                return Err(AppError::Config(format!(
                    "ChatGPT snapshot is missing tokens: {}",
                    path.display()
                )));
            }
        }
        "apikey" => {
            if auth
                .openai_api_key
                .as_ref()
                .and_then(Value::as_str)
                .is_none()
            {
                return Err(AppError::Config(format!(
                    "API key snapshot is missing OPENAI_API_KEY: {}",
                    path.display()
                )));
            }
        }
        _ => {
            return Err(AppError::Config(format!(
                "Unsupported auth_mode in snapshot: {}",
                path.display()
            )));
        }
    }
    Ok(())
}

fn ensure_snapshot_matches_account(item: &RegistryItem) -> Result<(), AppError> {
    match validate_snapshot_matches_account(Path::new(&item.snapshot_path), &item.account_key) {
        Ok(()) => Ok(()),
        Err(err) if err.to_string().starts_with("Snapshot account mismatch:") => {
            let recovery_path = find_latest_backup_for_account(&item.account_key)?;
            let Some(recovery_path) = recovery_path else {
                return Err(AppError::Config(format!(
                    "Snapshot account mismatch: expected {}, and no matching backup was found to repair it",
                    item.account_key
                )));
            };
            let paths = account_paths();
            fs::create_dir_all(&paths.backups_dir)
                .map_err(|e| AppError::io(&paths.backups_dir, e))?;
            let mismatched_backup_path = paths.backups_dir.join(format!(
                "{}__mismatched-snapshot__{}.json",
                timestamp_slug(),
                safe_file_name(&item.account_key)
            ));
            copy_file_atomic(Path::new(&item.snapshot_path), &mismatched_backup_path)?;
            copy_file_atomic(&recovery_path, Path::new(&item.snapshot_path))?;
            validate_snapshot_matches_account(Path::new(&item.snapshot_path), &item.account_key)
        }
        Err(err) => Err(err),
    }
}

fn validate_snapshot_matches_account(
    path: &Path,
    expected_account_key: &str,
) -> Result<(), AppError> {
    let auth: AuthSnapshot = read_json_file(path)?;
    if auth.auth_mode.as_deref() == Some("apikey") {
        return Ok(());
    }
    let actual = account_key_from_auth(&auth);
    if actual != expected_account_key {
        return Err(AppError::Config(format!(
            "Snapshot account mismatch: expected {expected_account_key}, got {actual}"
        )));
    }
    Ok(())
}

fn find_latest_backup_for_account(account_key: &str) -> Result<Option<PathBuf>, AppError> {
    let backups_dir = account_paths().backups_dir;
    if !backups_dir.exists() {
        return Ok(None);
    }

    let mut candidates: Vec<(PathBuf, std::time::SystemTime)> = Vec::new();
    for entry in fs::read_dir(&backups_dir).map_err(|e| AppError::io(&backups_dir, e))? {
        let entry = entry.map_err(|e| AppError::io(&backups_dir, e))?;
        let path = entry.path();
        if path.extension().and_then(|ext| ext.to_str()) != Some("json") {
            continue;
        }
        if validate_auth_file(&path).is_err() {
            continue;
        }
        let auth: AuthSnapshot = match read_json_file(&path) {
            Ok(auth) => auth,
            Err(_) => continue,
        };
        if auth.auth_mode.as_deref() == Some("apikey")
            || account_key_from_auth(&auth) != account_key
        {
            continue;
        }
        let modified = entry
            .metadata()
            .and_then(|metadata| metadata.modified())
            .unwrap_or(std::time::SystemTime::UNIX_EPOCH);
        candidates.push((path, modified));
    }

    candidates.sort_by(|(_, left), (_, right)| right.cmp(left));
    Ok(candidates.into_iter().next().map(|(path, _)| path))
}

fn persist_current_auth_to_active_snapshot(registry: &Registry) -> Result<(), AppError> {
    let Some(active_key) = registry.active_account_key.as_deref() else {
        return Ok(());
    };
    let Some(active_item) = registry
        .items
        .iter()
        .find(|item| item.account_key == active_key)
    else {
        return Ok(());
    };

    let auth_path = get_codex_auth_path();
    let current_auth: AuthSnapshot = read_json_file(&auth_path)?;
    if current_auth.auth_mode.as_deref() == Some("chatgpt")
        && account_key_from_auth(&current_auth) != active_item.account_key
    {
        return Ok(());
    }
    validate_auth_file(&auth_path)?;
    copy_file_atomic(&auth_path, Path::new(&active_item.snapshot_path))
}

fn backup_current_auth(previous_account_key: Option<&str>) -> Result<PathBuf, AppError> {
    let paths = account_paths();
    fs::create_dir_all(&paths.backups_dir).map_err(|e| AppError::io(&paths.backups_dir, e))?;
    let account_part = previous_account_key
        .map(safe_file_name)
        .filter(|value| !value.is_empty())
        .unwrap_or_else(|| "unknown".to_string());
    let backup_path =
        paths
            .backups_dir
            .join(format!("{}__{}.json", timestamp_slug(), account_part));
    copy_file_atomic(&get_codex_auth_path(), &backup_path)?;
    Ok(backup_path)
}

fn copy_file_atomic(source: &Path, destination: &Path) -> Result<(), AppError> {
    let bytes = fs::read(source).map_err(|e| AppError::io(source, e))?;
    atomic_write(destination, &bytes)?;
    #[cfg(unix)]
    {
        let _ = fs::set_permissions(destination, fs::Permissions::from_mode(0o600));
    }
    Ok(())
}

fn account_paths() -> AccountPaths {
    let canonical_base = get_codex_config_dir().join("accounts");
    let legacy_base = get_app_config_dir().join("codex-accounts");
    let base = if canonical_base.exists() || !legacy_base.exists() {
        canonical_base
    } else {
        legacy_base
    };
    AccountPaths {
        registry_path: base.join("registry.json"),
        snapshots_dir: base.join("snapshots"),
        backups_dir: base.join("backups"),
        latest_switch_path: base.join("latest-switch.json"),
    }
}

fn account_key_from_auth(auth: &AuthSnapshot) -> String {
    if auth.auth_mode.as_deref() == Some("apikey") {
        let key = auth
            .openai_api_key
            .as_ref()
            .and_then(Value::as_str)
            .unwrap_or_default();
        return format!("apikey::{}", short_sha256(key));
    }

    let tokens = auth.tokens.as_ref();
    let payload = tokens
        .and_then(|tokens| tokens.get("id_token"))
        .and_then(Value::as_str)
        .and_then(decode_id_token);
    let auth_namespace = payload.as_ref().and_then(|payload| payload.auth.as_ref());
    let default_org = auth_namespace.and_then(|namespace| {
        namespace
            .organizations
            .iter()
            .find(|organization| organization.is_default)
            .or_else(|| namespace.organizations.first())
    });
    let account_id = auth_namespace
        .and_then(|namespace| namespace.chatgpt_user_id.as_deref())
        .or_else(|| {
            tokens
                .and_then(|tokens| tokens.get("account_id"))
                .and_then(Value::as_str)
        })
        .unwrap_or("current");
    let workspace_id = tokens
        .and_then(|tokens| tokens.get("workspace_id"))
        .and_then(Value::as_str)
        .or_else(|| {
            tokens
                .and_then(|tokens| tokens.get("account_id"))
                .and_then(Value::as_str)
        })
        .or_else(|| default_org.and_then(|organization| organization.id.as_deref()))
        .unwrap_or("workspace");

    format!("{account_id}::{workspace_id}")
}

fn metadata_from_auth(auth: &AuthSnapshot) -> AccountMetadata {
    let payload = auth
        .tokens
        .as_ref()
        .and_then(|tokens| tokens.get("id_token"))
        .and_then(Value::as_str)
        .and_then(decode_id_token);
    let plan = payload
        .as_ref()
        .and_then(|payload| payload.auth.as_ref())
        .and_then(|auth| auth.chatgpt_plan_type.clone())
        .unwrap_or_default();

    AccountMetadata {
        email: payload
            .as_ref()
            .and_then(|payload| payload.email.clone())
            .unwrap_or_default(),
        name: payload
            .as_ref()
            .and_then(|payload| payload.name.clone())
            .unwrap_or_default(),
        plan,
    }
}

fn decode_id_token(id_token: &str) -> Option<JwtAuthPayload> {
    let payload = id_token.split('.').nth(1)?;
    let bytes = URL_SAFE_NO_PAD.decode(payload).ok()?;
    serde_json::from_slice(&bytes).ok()
}

fn to_summary(item: &RegistryItem, active_account_key: Option<&str>) -> CodexAccountSummary {
    CodexAccountSummary {
        account_key: item.account_key.clone(),
        profile_name: first_non_empty(&[
            item.alias.as_str(),
            item.profile_name.as_str(),
            item.account_name.as_str(),
            "Unnamed account",
        ]),
        email_masked: mask_email(&item.email),
        plan: item.plan.clone(),
        auth_mode: item.auth_mode.clone(),
        is_active: active_account_key == Some(item.account_key.as_str()),
        last_used_at: item.last_used_at,
    }
}

fn first_non_empty(values: &[&str]) -> String {
    values
        .iter()
        .map(|value| value.trim())
        .find(|value| !value.is_empty())
        .unwrap_or("")
        .to_string()
}

fn mask_email(email: &str) -> String {
    let Some((name, domain)) = email.split_once('@') else {
        return if email.is_empty() {
            String::new()
        } else {
            "hidden".to_string()
        };
    };
    let visible_len = if name.chars().count() <= 2 { 1 } else { 2 };
    let visible: String = name.chars().take(visible_len).collect();
    let mask_len = name.chars().count().saturating_sub(visible_len).max(3);
    format!("{visible}{}@{domain}", "*".repeat(mask_len))
}

fn clean_label(label: Option<&str>) -> String {
    label.unwrap_or_default().trim().chars().take(80).collect()
}

fn safe_file_name(input: &str) -> String {
    input
        .chars()
        .map(|ch| {
            if ch.is_ascii_alphanumeric() || matches!(ch, '.' | '_' | '-') {
                ch
            } else {
                '_'
            }
        })
        .take(180)
        .collect()
}

fn short_sha256(input: &str) -> String {
    let mut hasher = Sha256::new();
    hasher.update(input.as_bytes());
    let digest = hasher.finalize();
    digest[..8]
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect()
}

fn timestamp_slug() -> String {
    Utc::now().format("%Y%m%d-%H%M%S%.3f").to_string()
}

fn now_seconds() -> i64 {
    Utc::now().timestamp()
}

// ── 用量查询 ──────────────────────────────────────────────

/// 读取指定账号快照中的 Codex OAuth 凭据并查询官方用量
///
/// 直接读取快照文件（不写入 ~/.codex/auth.json），避免干扰当前正在使用的账号。
pub async fn get_account_quota(account_key: &str) -> Result<SubscriptionQuota, AppError> {
    let registry = read_registry_with_snapshot_scan()?;
    let item = registry
        .items
        .iter()
        .find(|i| i.account_key == account_key)
        .ok_or_else(|| AppError::Config(format!("Codex account not found: {account_key}")))?;

    let auth: AuthSnapshot = read_json_file(Path::new(&item.snapshot_path))?;

    // 仅 OAuth 模式支持用量查询
    if auth.auth_mode.as_deref() != Some("chatgpt") {
        return Ok(SubscriptionQuota {
            tool: "codex".to_string(),
            credential_status: crate::services::subscription::CredentialStatus::NotFound,
            credential_message: Some("API key mode does not support usage query".to_string()),
            success: false,
            tiers: vec![],
            extra_usage: None,
            error: Some("API key mode does not support usage query".to_string()),
            queried_at: Some(crate::services::subscription::now_millis()),
        });
    }

    let tokens = auth.tokens.ok_or_else(|| {
        AppError::Config(format!(
            "Missing tokens in snapshot for account {account_key}"
        ))
    })?;

    let access_token = tokens
        .get("access_token")
        .and_then(|v| v.as_str())
        .ok_or_else(|| {
            AppError::Config(format!(
                "Missing access_token in snapshot for account {account_key}"
            ))
        })?;

    let account_id = tokens.get("account_id").and_then(|v| v.as_str());

    let quota = query_codex_quota(
        access_token,
        account_id,
        "codex",
        "Authentication failed. Please re-login with Codex CLI.",
    )
    .await;

    Ok(quota)
}

/// 查询所有 Codex 账号的用量
///
/// 返回 account_key -> SubscriptionQuota 的映射。
/// 每个账号独立并发查询。
pub async fn get_all_account_quotas(
) -> Result<std::collections::HashMap<String, SubscriptionQuota>, AppError> {
    let accounts = list_accounts()?;
    let mut results = std::collections::HashMap::new();

    for account in accounts {
        // API key 模式跳过查询
        if account.auth_mode == "apikey" {
            results.insert(
                account.account_key.clone(),
                SubscriptionQuota {
                    tool: "codex".to_string(),
                    credential_status: crate::services::subscription::CredentialStatus::NotFound,
                    credential_message: Some("API key mode".to_string()),
                    success: false,
                    tiers: vec![],
                    extra_usage: None,
                    error: Some("API key mode does not support usage query".to_string()),
                    queried_at: None,
                },
            );
            continue;
        }

        match get_account_quota(&account.account_key).await {
            Ok(quota) => {
                results.insert(account.account_key.clone(), quota);
            }
            Err(e) => {
                results.insert(
                    account.account_key.clone(),
                    SubscriptionQuota {
                        tool: "codex".to_string(),
                        credential_status:
                            crate::services::subscription::CredentialStatus::ParseError,
                        credential_message: Some(e.to_string()),
                        success: false,
                        tiers: vec![],
                        extra_usage: None,
                        error: Some(e.to_string()),
                        queried_at: Some(crate::services::subscription::now_millis()),
                    },
                );
            }
        }
    }

    Ok(results)
}

#[cfg(target_os = "macos")]
fn restart_codex_app_impl() -> Result<CodexAppRestartResult, AppError> {
    use std::process::Command;
    use std::thread;
    use std::time::{Duration, Instant};

    const CODEX_APP_NAME: &str = "Codex";
    const CODEX_BUNDLE_ID: &str = "com.openai.codex";
    const CODEX_MAIN_PROCESS_PATTERN: &str = "/Codex.app/Contents/MacOS/Codex";

    fn osascript(script: &str) -> Result<String, AppError> {
        let output = Command::new("osascript")
            .args(["-e", script])
            .output()
            .map_err(|e| AppError::Message(format!("Failed to run AppleScript: {e}")))?;

        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr).trim().to_string();
            return Err(AppError::Message(if stderr.is_empty() {
                "AppleScript command failed without details.".to_string()
            } else {
                stderr
            }));
        }

        Ok(String::from_utf8_lossy(&output.stdout).trim().to_string())
    }

    fn is_running_by_script(script: &str) -> Option<bool> {
        osascript(script)
            .ok()
            .and_then(|output| match output.as_str() {
                "true" => Some(true),
                "false" => Some(false),
                _ => None,
            })
    }

    fn is_codex_running() -> bool {
        is_running_by_script(&format!("application id \"{CODEX_BUNDLE_ID}\" is running"))
            .or_else(|| {
                is_running_by_script(&format!("application \"{CODEX_APP_NAME}\" is running"))
            })
            .unwrap_or(false)
    }

    fn wait_until_running(expected: bool, timeout: Duration) -> bool {
        let deadline = Instant::now() + timeout;
        loop {
            if is_codex_running() == expected {
                return true;
            }
            if Instant::now() >= deadline {
                return false;
            }
            thread::sleep(Duration::from_millis(250));
        }
    }

    fn request_quit() -> Result<(), AppError> {
        osascript(&format!(
            "tell application id \"{CODEX_BUNDLE_ID}\" to quit"
        ))
        .or_else(|_| osascript(&format!("tell application \"{CODEX_APP_NAME}\" to quit")))
        .map(|_| ())
        .map_err(|e| AppError::Message(format!("Failed to quit Codex: {e}")))
    }

    fn terminate_main_process() -> Result<bool, AppError> {
        let status = Command::new("pkill")
            .args(["-TERM", "-f", CODEX_MAIN_PROCESS_PATTERN])
            .status()
            .map_err(|e| AppError::Message(format!("Failed to terminate Codex: {e}")))?;

        // pkill returns 1 when no process matched. Treat that as "nothing to do".
        Ok(status.success() || status.code() == Some(1))
    }

    fn open_codex() -> Result<String, AppError> {
        let bundle_status = Command::new("open")
            .args(["-b", CODEX_BUNDLE_ID])
            .status()
            .map_err(|e| AppError::Message(format!("Failed to open Codex by bundle id: {e}")))?;

        if bundle_status.success() {
            return Ok("bundleId".to_string());
        }

        let app_status = Command::new("open")
            .args(["-a", CODEX_APP_NAME])
            .status()
            .map_err(|e| AppError::Message(format!("Failed to open Codex by app name: {e}")))?;

        if app_status.success() {
            Ok("appName".to_string())
        } else {
            Err(AppError::Message(
                "Codex did not accept the open request.".to_string(),
            ))
        }
    }

    let was_running = is_codex_running();
    let mut quit_requested = false;
    let mut quit_graceful = !was_running;
    let mut force_quit_used = false;

    if was_running {
        quit_requested = true;
        request_quit()?;
        quit_graceful = wait_until_running(false, Duration::from_secs(8));

        if !quit_graceful {
            force_quit_used = terminate_main_process()?;
            if !wait_until_running(false, Duration::from_secs(4)) {
                return Err(AppError::Message(
                    "Codex did not exit completely. Please close it manually and try again."
                        .to_string(),
                ));
            }
        }
    }

    let launch_method = open_codex()?;
    let running_after = wait_until_running(true, Duration::from_secs(10));
    if !running_after {
        return Err(AppError::Message(
            "Codex open command succeeded, but the app was not detected as running.".to_string(),
        ));
    }

    let message = if !was_running {
        "Codex App 已启动".to_string()
    } else if force_quit_used {
        "Codex App 已重启，旧进程退出较慢，已做一次温和终止".to_string()
    } else {
        "Codex App 已重启".to_string()
    };

    Ok(CodexAppRestartResult {
        was_running,
        quit_requested,
        quit_graceful,
        force_quit_used,
        opened: true,
        running_after,
        launch_method,
        message,
    })
}

#[cfg(not(target_os = "macos"))]
fn restart_codex_app_impl() -> Result<CodexAppRestartResult, AppError> {
    Err(AppError::Message(
        "Restarting Codex App is currently supported on macOS only.".to_string(),
    ))
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn chatgpt_account_key_uses_user_and_workspace() {
        let payload = URL_SAFE_NO_PAD.encode(
            serde_json::to_vec(&json!({
                "email": "person@example.com",
                "name": "Person",
                "https://api.openai.com/auth": {
                    "chatgpt_user_id": "user-1",
                    "chatgpt_plan_type": "plus",
                    "organizations": [{ "id": "org-1", "is_default": true }]
                }
            }))
            .unwrap(),
        );
        let auth = AuthSnapshot {
            auth_mode: Some("chatgpt".to_string()),
            tokens: Some(json!({
                "id_token": format!("header.{payload}.sig"),
                "workspace_id": "workspace-1"
            })),
            openai_api_key: None,
        };

        assert_eq!(account_key_from_auth(&auth), "user-1::workspace-1");
        let metadata = metadata_from_auth(&auth);
        assert_eq!(metadata.email, "person@example.com");
        assert_eq!(metadata.name, "Person");
        assert_eq!(metadata.plan, "plus");
    }

    #[test]
    fn email_mask_keeps_domain_without_revealing_full_name() {
        assert_eq!(mask_email("abcdef@example.com"), "ab****@example.com");
        assert_eq!(mask_email("a@example.com"), "a***@example.com");
    }
}

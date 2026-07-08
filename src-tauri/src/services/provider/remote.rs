use std::collections::{HashMap, HashSet};
use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

#[cfg(unix)]
use std::os::unix::fs::PermissionsExt;
#[cfg(windows)]
use std::os::windows::process::CommandExt;

use rust_decimal::Decimal;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};

use crate::app_config::AppType;
use crate::database::{lock_conn, Database};
use crate::error::AppError;
use crate::provider::Provider;
use crate::proxy::usage::calculator::CostCalculator;
use crate::proxy::usage::parser::TokenUsage;
use crate::services::session_usage::{
    retag_legacy_remote_session_log, update_sync_state, SessionSyncResult,
    SessionUsageImportOptions,
};
use crate::services::usage_stats::{
    find_model_pricing, should_skip_session_insert_with_proxy_dedup, DedupKey,
};
use crate::store::AppState;

use super::gemini_auth::{detect_gemini_auth_type, GeminiAuthType};
use super::live::{build_effective_settings_with_common_config, sanitize_claude_settings_for_live};

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct SshHostEntry {
    pub alias: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub host_name: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub user: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub port: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub source: Option<String>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SshConnectionTarget {
    #[serde(default, rename = "type")]
    pub target_type: Option<String>,
    #[serde(default)]
    pub alias: Option<String>,
    #[serde(default)]
    pub host: Option<String>,
    #[serde(default)]
    pub user: Option<String>,
    #[serde(default)]
    pub port: Option<u16>,
    #[serde(default)]
    pub password: Option<String>,
}

#[derive(Debug, Clone)]
struct ResolvedSshTarget {
    label: String,
    connect_target: String,
    port: Option<u16>,
    password: Option<String>,
}

impl ResolvedSshTarget {
    fn label(&self) -> &str {
        &self.label
    }
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RemoteApplyResult {
    pub host_alias: String,
    pub app: String,
    pub provider_id: String,
    pub written_files: Vec<String>,
    pub overwrote_existing_config: bool,
    #[serde(default)]
    pub warnings: Vec<String>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RemoteConfigFile {
    pub path: String,
    pub exists: bool,
    pub bytes: usize,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RemoteProviderState {
    pub host_alias: String,
    pub app: String,
    pub provider: Option<Provider>,
    pub matched_provider_id: Option<String>,
    pub files: Vec<RemoteConfigFile>,
    pub has_existing_config: bool,
    pub has_unmanaged_config: bool,
    pub overwrite_warning: Option<String>,
    #[serde(default)]
    pub warnings: Vec<String>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RemoteImportResult {
    pub host_alias: String,
    pub app: String,
    pub provider: Provider,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RemoteUsageSyncResult {
    pub host_alias: String,
    pub app: String,
    pub imported: u32,
    pub skipped: u32,
    pub unchanged_files: u32,
    pub files_scanned: u32,
    pub errors: Vec<String>,
    #[serde(default)]
    pub warnings: Vec<String>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RemoteUsageSyncProgress {
    pub sync_id: String,
    pub host_alias: String,
    pub app: String,
    pub stage: String,
    pub current_step: u32,
    pub total_steps: u32,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub file_count: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub payload_bytes: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub files_scanned: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub imported: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub skipped: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub unchanged_files: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
}

impl RemoteUsageSyncProgress {
    pub fn failed(sync_id: &str, host_alias: &str, app_type: &AppType, error: String) -> Self {
        Self {
            sync_id: sync_id.to_string(),
            host_alias: host_alias.to_string(),
            app: app_type.as_str().to_string(),
            stage: "failed".to_string(),
            current_step: 0,
            total_steps: REMOTE_USAGE_SYNC_TOTAL_STEPS,
            file_count: None,
            payload_bytes: None,
            files_scanned: None,
            imported: None,
            skipped: None,
            unchanged_files: None,
            error: Some(error),
        }
    }
}

pub type RemoteUsageProgressCallback<'a> = &'a dyn Fn(RemoteUsageSyncProgress);

const REMOTE_USAGE_SYNC_TOTAL_STEPS: u32 = 5;

struct RemoteUsageProgressEmitter<'a> {
    callback: Option<RemoteUsageProgressCallback<'a>>,
    sync_id: Option<&'a str>,
    host_alias: &'a str,
    app_type: &'a AppType,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct RemoteUsageRecord {
    request_id: String,
    #[serde(default)]
    session_id: Option<String>,
    model: String,
    input_tokens: u32,
    output_tokens: u32,
    cache_read_tokens: u32,
    cache_creation_tokens: u32,
    created_at: i64,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct RemoteUsageFileState {
    path: String,
    last_modified: i64,
    last_offset: i64,
}

struct RemoteUsageImportOutcome {
    result: SessionSyncResult,
    unchanged_files: u32,
}

impl RemoteUsageProgressEmitter<'_> {
    fn emit(
        &self,
        stage: &str,
        current_step: u32,
        file_count: Option<u32>,
        payload_bytes: Option<u64>,
        result: Option<&SessionSyncResult>,
        unchanged_files: Option<u32>,
    ) {
        let Some(callback) = self.callback else {
            return;
        };
        let Some(sync_id) = self
            .sync_id
            .map(str::trim)
            .filter(|value| !value.is_empty())
        else {
            return;
        };

        callback(RemoteUsageSyncProgress {
            sync_id: sync_id.to_string(),
            host_alias: self.host_alias.to_string(),
            app: self.app_type.as_str().to_string(),
            stage: stage.to_string(),
            current_step,
            total_steps: REMOTE_USAGE_SYNC_TOTAL_STEPS,
            file_count,
            payload_bytes,
            files_scanned: result.map(|value| value.files_scanned),
            imported: result.map(|value| value.imported),
            skipped: result.map(|value| value.skipped),
            unchanged_files,
            error: None,
        });
    }
}

struct RemoteWrite {
    path: &'static str,
    backup_name: &'static str,
    content: String,
}

struct RemoteSettingsRead {
    settings_config: Option<Value>,
    files: Vec<RemoteConfigFile>,
    warnings: Vec<String>,
}

struct RemoteExistingConfigStatus {
    has_existing_config: bool,
    matched_provider_id: Option<String>,
}

impl RemoteExistingConfigStatus {
    fn requires_confirmation(&self) -> bool {
        self.has_existing_config && self.matched_provider_id.is_none()
    }
}

pub struct RemoteProviderService;

impl RemoteProviderService {
    pub fn list_ssh_hosts() -> Result<Vec<SshHostEntry>, AppError> {
        let config_path = crate::config::get_home_dir().join(".ssh").join("config");
        let mut hosts = Vec::new();
        let mut index = HashMap::new();
        let mut visited = HashSet::new();
        parse_ssh_config_file(&config_path, &mut hosts, &mut index, &mut visited)?;
        Ok(hosts)
    }

    pub fn apply_provider_to_remote(
        state: &AppState,
        app_type: AppType,
        provider_id: &str,
        target: &SshConnectionTarget,
        force_overwrite: bool,
    ) -> Result<RemoteApplyResult, AppError> {
        ensure_remote_supported(&app_type)?;
        let target = resolve_ssh_target(target)?;
        let host_alias = target.label();

        let providers = state.db.get_all_providers(app_type.as_str())?;
        let provider = providers
            .get(provider_id)
            .ok_or_else(|| AppError::Message(format!("供应商 {provider_id} 不存在")))?;

        let existing = remote_existing_config_status(state, &app_type, &target)?;
        if existing.requires_confirmation() && !force_overwrite {
            return Err(AppError::Message(remote_overwrite_block_message(
                &app_type, host_alias,
            )));
        }

        let writes = build_remote_writes(state, &app_type, provider, &target)?;
        let stamp = remote_backup_stamp();
        let mut written_files = Vec::new();

        for write in writes {
            let written = write_remote_file(
                &target,
                write.path,
                write.backup_name,
                write.content.as_bytes(),
                &stamp,
            )?;
            written_files.push(written);
        }

        Ok(RemoteApplyResult {
            host_alias: host_alias.to_string(),
            app: app_type.as_str().to_string(),
            provider_id: provider_id.to_string(),
            written_files,
            overwrote_existing_config: existing.has_existing_config,
            warnings: Vec::new(),
        })
    }

    pub fn inspect_remote_provider(
        state: &AppState,
        app_type: AppType,
        target: &SshConnectionTarget,
    ) -> Result<RemoteProviderState, AppError> {
        ensure_remote_supported(&app_type)?;
        let target = resolve_ssh_target(target)?;
        let host_alias = target.label();

        let read = read_remote_settings_config(&app_type, &target)?;
        let has_existing_config = read.files.iter().any(|file| file.exists);
        let provider = read.settings_config.map(|settings_config| {
            let mut provider = Provider::with_id(
                "remote-current".to_string(),
                format!("远端当前配置 ({host_alias})"),
                settings_config,
                None,
            );
            provider.category = Some("custom".to_string());
            provider.notes = Some(format!(
                "Imported preview from SSH host {host_alias} for {}",
                app_type.as_str()
            ));
            provider
        });
        let matched_provider_id = match provider.as_ref() {
            Some(remote_provider) => {
                find_matching_local_provider(state, &app_type, &remote_provider.settings_config)?
            }
            None => None,
        };
        let has_unmanaged_config = has_existing_config && matched_provider_id.is_none();

        Ok(RemoteProviderState {
            host_alias: host_alias.to_string(),
            app: app_type.as_str().to_string(),
            provider,
            matched_provider_id,
            files: read.files,
            has_existing_config,
            has_unmanaged_config,
            overwrite_warning: has_unmanaged_config
                .then(|| remote_overwrite_warning(&app_type, host_alias)),
            warnings: read.warnings,
        })
    }

    pub fn import_remote_provider(
        state: &AppState,
        app_type: AppType,
        target: &SshConnectionTarget,
    ) -> Result<RemoteImportResult, AppError> {
        ensure_remote_supported(&app_type)?;
        let target = resolve_ssh_target(target)?;
        let host_alias = target.label();

        let read = read_remote_settings_config(&app_type, &target)?;
        let settings_config = read.settings_config.ok_or_else(|| {
            AppError::Message(format!(
                "远端 {host_alias} 没有可导入的 {} 配置",
                app_type.as_str()
            ))
        })?;

        if let Some(existing_id) = find_matching_local_provider(state, &app_type, &settings_config)?
        {
            let providers = state.db.get_all_providers(app_type.as_str())?;
            let provider = providers
                .get(&existing_id)
                .cloned()
                .ok_or_else(|| AppError::Message(format!("本地供应商 {existing_id} 不存在")))?;
            return Ok(RemoteImportResult {
                host_alias: host_alias.to_string(),
                app: app_type.as_str().to_string(),
                provider,
            });
        }

        let provider_id = generate_remote_provider_id(state, &app_type, host_alias)?;
        let mut provider = Provider::with_id(
            provider_id,
            format!("远端 {host_alias}"),
            settings_config,
            None,
        );
        provider.category = Some("custom".to_string());
        provider.created_at = Some(chrono::Utc::now().timestamp_millis());
        provider.notes = Some(format!(
            "Downloaded from SSH host {host_alias} for {}",
            app_type.as_str()
        ));

        state.db.save_provider(app_type.as_str(), &provider)?;

        Ok(RemoteImportResult {
            host_alias: host_alias.to_string(),
            app: app_type.as_str().to_string(),
            provider,
        })
    }

    pub fn sync_remote_session_usage_with_progress(
        state: &AppState,
        app_type: AppType,
        target: &SshConnectionTarget,
        sync_id: Option<&str>,
        progress: Option<RemoteUsageProgressCallback<'_>>,
    ) -> Result<RemoteUsageSyncResult, AppError> {
        ensure_remote_supported(&app_type)?;
        let target = resolve_ssh_target(target)?;
        let host_alias = target.label();

        let progress_emitter = RemoteUsageProgressEmitter {
            callback: progress,
            sync_id,
            host_alias,
            app_type: &app_type,
        };
        let sync_options = crate::services::session_usage::SessionUsageImportOptions::remote(
            host_alias,
            app_type.as_str(),
            format!("remote_{}_session", app_type.as_str()),
        );

        let (sync_result, file_count, payload_bytes, unchanged_files) =
            sync_remote_usage_via_remote_analyzer(
                state.db.as_ref(),
                &target,
                &app_type,
                &sync_options,
                &progress_emitter,
            )?;
        progress_emitter.emit(
            "completed",
            REMOTE_USAGE_SYNC_TOTAL_STEPS,
            Some(file_count),
            Some(payload_bytes),
            Some(&sync_result),
            Some(unchanged_files),
        );
        Ok(remote_usage_result(
            host_alias,
            &app_type,
            sync_result,
            unchanged_files,
        ))
    }
}

fn remote_usage_result(
    host_alias: &str,
    app_type: &AppType,
    result: SessionSyncResult,
    unchanged_files: u32,
) -> RemoteUsageSyncResult {
    RemoteUsageSyncResult {
        host_alias: host_alias.to_string(),
        app: app_type.as_str().to_string(),
        imported: result.imported,
        skipped: result.skipped,
        unchanged_files,
        files_scanned: result.files_scanned,
        errors: result.errors,
        warnings: Vec::new(),
    }
}

fn sync_remote_usage_via_remote_analyzer(
    db: &Database,
    target: &ResolvedSshTarget,
    app_type: &AppType,
    sync_options: &SessionUsageImportOptions,
    progress: &RemoteUsageProgressEmitter<'_>,
) -> Result<(SessionSyncResult, u32, u64, u32), AppError> {
    progress.emit("connecting", 0, None, None, None, None);
    let file_count = count_remote_session_log_files(target, app_type)?;
    progress.emit("listed", 1, Some(file_count), None, None, None);

    let sync_state = remote_usage_sync_state_json(db, sync_options)?;
    progress.emit("analyzing", 2, Some(file_count), None, None, None);
    let command = remote_usage_analyzer_command(app_type);
    let output = run_ssh_command(target, &command, Some(sync_state.as_bytes()))?;
    let payload_bytes = output.len() as u64;
    progress.emit(
        "analyzed",
        3,
        Some(file_count),
        Some(payload_bytes),
        None,
        None,
    );

    progress.emit(
        "importing",
        4,
        Some(file_count),
        Some(payload_bytes),
        None,
        None,
    );
    let mut outcome = import_remote_usage_records(db, app_type, sync_options, output.as_str())?;
    let sync_result = &mut outcome.result;
    if sync_result.files_scanned == 0 {
        sync_result.files_scanned = file_count;
    }

    Ok((
        outcome.result,
        file_count,
        payload_bytes,
        outcome.unchanged_files,
    ))
}

fn remote_usage_sync_state_json(
    db: &Database,
    sync_options: &SessionUsageImportOptions,
) -> Result<String, AppError> {
    let Some(prefix) = sync_options.sync_key_prefix.as_deref() else {
        return Ok("{}".to_string());
    };
    let prefix_with_slash = format!("{prefix}/");
    let like_pattern = format!("{prefix_with_slash}%");
    let conn = lock_conn!(db.conn);
    let mut stmt = conn.prepare(
        "SELECT file_path, last_modified, last_line_offset
         FROM session_log_sync
         WHERE file_path LIKE ?1",
    )?;
    let rows = stmt.query_map([like_pattern], |row| {
        Ok((
            row.get::<_, String>(0)?,
            row.get::<_, i64>(1)?,
            row.get::<_, i64>(2)?,
        ))
    })?;

    let mut state = serde_json::Map::new();
    for row in rows {
        let (file_path, last_modified, last_offset) =
            row.map_err(|e| AppError::Database(e.to_string()))?;
        let Some(relative_path) = file_path.strip_prefix(&prefix_with_slash) else {
            continue;
        };
        state.insert(
            relative_path.to_string(),
            json!({
                "lastModified": last_modified,
                "lastOffset": last_offset,
            }),
        );
    }

    serde_json::to_string(&state).map_err(|e| AppError::JsonSerialize { source: e })
}

fn import_remote_usage_records(
    db: &Database,
    app_type: &AppType,
    sync_options: &SessionUsageImportOptions,
    output: &str,
) -> Result<RemoteUsageImportOutcome, AppError> {
    let mut result = SessionSyncResult {
        imported: 0,
        skipped: 0,
        files_scanned: 0,
        errors: Vec::new(),
    };
    let mut unchanged_files = 0;

    for line in output.lines().filter(|line| !line.trim().is_empty()) {
        let value = match serde_json::from_str::<Value>(line) {
            Ok(value) => value,
            Err(error) => {
                result.errors.push(format!("远端用量输出解析失败: {error}"));
                continue;
            }
        };
        let kind = value
            .get("kind")
            .and_then(Value::as_str)
            .unwrap_or_default();
        match kind {
            "meta" => {
                if let Some(files_scanned) = value.get("filesScanned").and_then(Value::as_u64) {
                    result.files_scanned = files_scanned as u32;
                }
            }
            "record" => {
                let record = match serde_json::from_value::<RemoteUsageRecord>(value) {
                    Ok(record) => record,
                    Err(error) => {
                        result.errors.push(format!("远端用量记录解析失败: {error}"));
                        result.skipped += 1;
                        continue;
                    }
                };
                match insert_remote_usage_record(db, app_type, sync_options, &record) {
                    Ok(true) => result.imported += 1,
                    Ok(false) => result.skipped += 1,
                    Err(error) => {
                        result.errors.push(format!("写入远端用量记录失败: {error}"));
                        result.skipped += 1;
                    }
                }
            }
            "state" => {
                let state = match serde_json::from_value::<RemoteUsageFileState>(value) {
                    Ok(state) => state,
                    Err(error) => {
                        result.errors.push(format!("远端同步状态解析失败: {error}"));
                        continue;
                    }
                };
                update_remote_usage_sync_state(db, sync_options, &state)?;
            }
            "unchangedFile" => {
                unchanged_files += 1;
            }
            "error" => {
                let path = value
                    .get("path")
                    .and_then(Value::as_str)
                    .unwrap_or("unknown");
                let error = value
                    .get("error")
                    .and_then(Value::as_str)
                    .unwrap_or("unknown error");
                result.errors.push(format!("{path}: {error}"));
            }
            _ => {
                result.errors.push("远端用量输出包含未知事件".to_string());
            }
        }
    }

    Ok(RemoteUsageImportOutcome {
        result,
        unchanged_files,
    })
}

fn update_remote_usage_sync_state(
    db: &Database,
    sync_options: &SessionUsageImportOptions,
    state: &RemoteUsageFileState,
) -> Result<(), AppError> {
    let Some(prefix) = sync_options.sync_key_prefix.as_deref() else {
        return Ok(());
    };
    let sync_key = format!("{prefix}/{}", state.path);
    update_sync_state(db, &sync_key, state.last_modified, state.last_offset)
}

fn insert_remote_usage_record(
    db: &Database,
    app_type: &AppType,
    sync_options: &SessionUsageImportOptions,
    record: &RemoteUsageRecord,
) -> Result<bool, AppError> {
    let app = app_type.as_str();
    let conn = lock_conn!(db.conn);
    let request_id = sync_options.request_id(&record.request_id);
    let model = if matches!(app_type, AppType::Codex) {
        crate::services::session_usage_codex::normalize_codex_model(&record.model)
    } else if record.model.trim().is_empty() {
        "unknown".to_string()
    } else {
        record.model.clone()
    };

    let dedup_key = DedupKey {
        app_type: app,
        model: &model,
        input_tokens: record.input_tokens,
        output_tokens: record.output_tokens,
        cache_read_tokens: record.cache_read_tokens,
        cache_creation_tokens: record.cache_creation_tokens,
        created_at: record.created_at,
    };
    if let Some(result) =
        retag_legacy_remote_session_log(&conn, &request_id, app, &model, sync_options)?
    {
        return Ok(result);
    }
    if should_skip_session_insert_with_proxy_dedup(
        &conn,
        &request_id,
        &dedup_key,
        sync_options.dedup_with_proxy,
    )? {
        return Ok(false);
    }

    let usage = TokenUsage {
        input_tokens: record.input_tokens,
        output_tokens: record.output_tokens,
        cache_read_tokens: record.cache_read_tokens,
        cache_creation_tokens: record.cache_creation_tokens,
        model: Some(model.clone()),
        message_id: None,
    };
    let multiplier = Decimal::from(1);
    let pricing = find_model_pricing(&conn, &model);
    let costs = CostCalculator::try_calculate_for_app(app, &usage, pricing.as_ref(), multiplier);
    let (input_cost, output_cost, cache_read_cost, cache_creation_cost, total_cost) = match costs {
        Some(cost) => (
            cost.input_cost.to_string(),
            cost.output_cost.to_string(),
            cost.cache_read_cost.to_string(),
            cost.cache_creation_cost.to_string(),
            cost.total_cost.to_string(),
        ),
        None => (
            "0".to_string(),
            "0".to_string(),
            "0".to_string(),
            "0".to_string(),
            "0".to_string(),
        ),
    };

    conn.execute(
        "INSERT INTO proxy_request_logs (
            request_id, provider_id, app_type, model, request_model,
            input_tokens, output_tokens, cache_read_tokens, cache_creation_tokens,
            input_cost_usd, output_cost_usd, cache_read_cost_usd, cache_creation_cost_usd, total_cost_usd,
            latency_ms, first_token_ms, status_code, error_message, session_id,
            provider_type, is_streaming, cost_multiplier, created_at, data_source
        ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14, ?15, ?16, ?17, ?18, ?19, ?20, ?21, ?22, ?23, ?24)
        ON CONFLICT(request_id) DO UPDATE SET
            provider_id = excluded.provider_id,
            provider_type = excluded.provider_type,
            data_source = excluded.data_source,
            model = excluded.model,
            request_model = excluded.request_model,
            input_tokens = excluded.input_tokens,
            output_tokens = excluded.output_tokens,
            cache_read_tokens = excluded.cache_read_tokens,
            cache_creation_tokens = excluded.cache_creation_tokens,
            input_cost_usd = excluded.input_cost_usd,
            output_cost_usd = excluded.output_cost_usd,
            cache_read_cost_usd = excluded.cache_read_cost_usd,
            cache_creation_cost_usd = excluded.cache_creation_cost_usd,
            total_cost_usd = excluded.total_cost_usd,
            session_id = excluded.session_id,
            created_at = excluded.created_at
        WHERE provider_id != excluded.provider_id
           OR provider_type != excluded.provider_type
           OR COALESCE(data_source, 'proxy') != excluded.data_source
           OR model != excluded.model
           OR input_tokens != excluded.input_tokens
           OR output_tokens != excluded.output_tokens
           OR cache_read_tokens != excluded.cache_read_tokens
           OR cache_creation_tokens != excluded.cache_creation_tokens
           OR total_cost_usd != excluded.total_cost_usd
           OR COALESCE(session_id, '') != COALESCE(excluded.session_id, '')
           OR created_at != excluded.created_at",
        rusqlite::params![
            request_id,
            sync_options.provider_id.as_str(),
            app,
            model.as_str(),
            model.as_str(),
            record.input_tokens,
            record.output_tokens,
            record.cache_read_tokens,
            record.cache_creation_tokens,
            input_cost,
            output_cost,
            cache_read_cost,
            cache_creation_cost,
            total_cost,
            0i64,
            Option::<i64>::None,
            200i64,
            Option::<String>::None,
            record.session_id.clone(),
            Some(sync_options.provider_type.clone()),
            1i64,
            "1.0",
            record.created_at,
            sync_options.data_source.as_str(),
        ],
    )
    .map_err(|e| AppError::Database(format!("写入远端用量记录失败: {e}")))?;

    Ok(conn.changes() > 0)
}

fn remote_existing_config_status(
    state: &AppState,
    app_type: &AppType,
    target: &ResolvedSshTarget,
) -> Result<RemoteExistingConfigStatus, AppError> {
    let read = read_remote_settings_config(app_type, target)?;
    let has_existing_config = read.files.iter().any(|file| file.exists);
    let matched_provider_id = match read.settings_config.as_ref() {
        Some(settings_config) => find_matching_local_provider(state, app_type, settings_config)?,
        None => None,
    };

    Ok(RemoteExistingConfigStatus {
        has_existing_config,
        matched_provider_id,
    })
}

fn remote_overwrite_warning(app_type: &AppType, host_alias: &str) -> String {
    format!(
        "远端 {host_alias} 已有 {} 配置。切换会覆盖这些文件，建议先同步到本地；覆盖前会自动备份到远端 ~/.cc-switch/remote-backups。",
        app_type.as_str()
    )
}

fn remote_overwrite_block_message(app_type: &AppType, host_alias: &str) -> String {
    format!(
        "{} 如确认要覆盖，请重新点击确认。",
        remote_overwrite_warning(app_type, host_alias)
    )
}

fn ensure_remote_supported(app_type: &AppType) -> Result<(), AppError> {
    if matches!(app_type, AppType::Claude | AppType::Codex | AppType::Gemini) {
        return Ok(());
    }

    Err(AppError::Message(format!(
        "远端配置暂时只支持 Claude、Codex 和 Gemini，当前应用为 {}",
        app_type.as_str()
    )))
}

fn resolve_ssh_target(target: &SshConnectionTarget) -> Result<ResolvedSshTarget, AppError> {
    let is_manual = target
        .target_type
        .as_deref()
        .map(|value| value.eq_ignore_ascii_case("manual"))
        .unwrap_or(false)
        || target
            .host
            .as_deref()
            .is_some_and(|value| !value.trim().is_empty());

    if is_manual {
        return resolve_manual_ssh_target(target);
    }

    let alias = target
        .alias
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .ok_or_else(|| AppError::Message("请选择 SSH Host".to_string()))?;
    ensure_known_ssh_host(alias)?;
    Ok(ResolvedSshTarget {
        label: alias.to_string(),
        connect_target: alias.to_string(),
        port: None,
        password: None,
    })
}

fn resolve_manual_ssh_target(target: &SshConnectionTarget) -> Result<ResolvedSshTarget, AppError> {
    let host = target
        .host
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .ok_or_else(|| AppError::Message("请输入 SSH 服务器 IP 或域名".to_string()))?;
    if !is_safe_manual_ssh_host(host) {
        return Err(AppError::Message(
            "SSH 服务器地址包含不受支持的字符".to_string(),
        ));
    }

    let user = target
        .user
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty());
    if let Some(user) = user {
        if !is_safe_ssh_user(user) {
            return Err(AppError::Message(
                "SSH 用户名包含不受支持的字符".to_string(),
            ));
        }
    }

    if matches!(target.port, Some(0)) {
        return Err(AppError::Message("SSH 端口必须在 1-65535 之间".to_string()));
    }

    let connect_target = match user {
        Some(user) => format!("{user}@{host}"),
        None => host.to_string(),
    };
    if connect_target.starts_with('-') {
        return Err(AppError::Message("SSH 连接目标不合法".to_string()));
    }

    let mut label = connect_target.clone();
    if let Some(port) = target.port {
        label = format!("{label}:{port}");
    }

    Ok(ResolvedSshTarget {
        label,
        connect_target,
        port: target.port,
        password: target
            .password
            .as_ref()
            .filter(|value| !value.is_empty())
            .cloned(),
    })
}

fn ensure_known_ssh_host(host_alias: &str) -> Result<(), AppError> {
    if !is_safe_ssh_alias(host_alias) {
        return Err(AppError::Message(format!(
            "SSH Host '{host_alias}' 包含不受支持的字符"
        )));
    }

    let hosts = RemoteProviderService::list_ssh_hosts()?;
    if hosts.iter().any(|host| host.alias == host_alias) {
        return Ok(());
    }

    Err(AppError::Message(format!(
        "SSH Host '{host_alias}' 不在 ~/.ssh/config 中"
    )))
}

fn find_matching_local_provider(
    state: &AppState,
    app_type: &AppType,
    settings_config: &Value,
) -> Result<Option<String>, AppError> {
    let providers = state.db.get_all_providers(app_type.as_str())?;
    Ok(providers.iter().find_map(|(id, provider)| {
        (provider.settings_config == *settings_config).then(|| id.clone())
    }))
}

fn generate_remote_provider_id(
    state: &AppState,
    app_type: &AppType,
    host_alias: &str,
) -> Result<String, AppError> {
    let existing_ids = state.db.get_provider_ids(app_type.as_str())?;
    let base = format!(
        "remote-{}-{}",
        app_type.as_str(),
        slugify_id_fragment(host_alias)
    );
    if !existing_ids.contains(&base) {
        return Ok(base);
    }

    for suffix in 2..1000 {
        let candidate = format!("{base}-{suffix}");
        if !existing_ids.contains(&candidate) {
            return Ok(candidate);
        }
    }

    Err(AppError::Message(format!(
        "无法为远端 {host_alias} 生成唯一供应商 ID"
    )))
}

fn slugify_id_fragment(value: &str) -> String {
    let mut slug = String::new();
    let mut last_dash = false;

    for ch in value.chars() {
        let next = if ch.is_ascii_alphanumeric() {
            Some(ch.to_ascii_lowercase())
        } else if matches!(ch, '-' | '_' | '.' | '@' | ':') {
            Some('-')
        } else {
            None
        };

        let Some(next) = next else {
            continue;
        };
        if next == '-' {
            if last_dash {
                continue;
            }
            last_dash = true;
            slug.push(next);
        } else {
            last_dash = false;
            slug.push(next);
        }
    }

    let trimmed = slug.trim_matches('-');
    if trimmed.is_empty() {
        "host".to_string()
    } else {
        trimmed.to_string()
    }
}

fn build_remote_writes(
    state: &AppState,
    app_type: &AppType,
    provider: &Provider,
    target: &ResolvedSshTarget,
) -> Result<Vec<RemoteWrite>, AppError> {
    let effective_settings =
        build_effective_settings_with_common_config(state.db.as_ref(), app_type, provider)?;

    match app_type {
        AppType::Claude => {
            let settings = sanitize_claude_settings_for_live(&effective_settings);
            Ok(vec![RemoteWrite {
                path: "$HOME/.claude/settings.json",
                backup_name: "claude-settings.json",
                content: json_pretty(&settings)?,
            }])
        }
        AppType::Codex => {
            let obj = effective_settings
                .as_object()
                .ok_or_else(|| AppError::Config("Codex 供应商配置必须是 JSON 对象".to_string()))?;
            let auth = obj
                .get("auth")
                .ok_or_else(|| AppError::Config("Codex 供应商配置缺少 'auth' 字段".to_string()))?;
            let config_text = obj.get("config").and_then(Value::as_str).ok_or_else(|| {
                AppError::Config("Codex 供应商配置缺少 'config' 字段或不是字符串".to_string())
            })?;

            let remote_config =
                read_remote_file(target, "$HOME/.codex/config.toml")?.unwrap_or_default();
            let normalized_config = crate::codex_config::normalize_codex_config_text_with_anchor(
                config_text,
                (!remote_config.trim().is_empty()).then_some(remote_config.as_str()),
            )?;

            Ok(vec![
                RemoteWrite {
                    path: "$HOME/.codex/auth.json",
                    backup_name: "codex-auth.json",
                    content: json_pretty(auth)?,
                },
                RemoteWrite {
                    path: "$HOME/.codex/config.toml",
                    backup_name: "codex-config.toml",
                    content: normalized_config,
                },
            ])
        }
        AppType::Gemini => {
            use crate::gemini_config::{
                json_to_env, serialize_env_file, validate_gemini_settings_strict,
            };

            let auth_type = detect_gemini_auth_type(provider);
            if matches!(
                auth_type,
                GeminiAuthType::Packycode | GeminiAuthType::Generic
            ) {
                validate_gemini_settings_strict(&effective_settings)?;
            }

            let env_map = json_to_env(&effective_settings)?;
            let env_text = serialize_env_file(&env_map);

            let remote_settings =
                read_remote_file(target, "$HOME/.gemini/settings.json")?.unwrap_or_default();
            let mut settings_json = parse_json_object_or_empty(&remote_settings);

            if let Some(config_value) = effective_settings.get("config") {
                if let Some(config_obj) = config_value.as_object() {
                    if let Some(target_obj) = settings_json.as_object_mut() {
                        for (key, value) in config_obj {
                            target_obj.insert(key.clone(), value.clone());
                        }
                    }
                } else if !config_value.is_null() {
                    return Err(AppError::localized(
                        "gemini.validation.invalid_config",
                        "Gemini 配置格式错误: config 必须是对象或 null",
                        "Gemini config invalid: config must be an object or null",
                    ));
                }
            }

            let selected_type = match auth_type {
                GeminiAuthType::GoogleOfficial => "oauth-personal",
                GeminiAuthType::Packycode | GeminiAuthType::Generic => "gemini-api-key",
            };
            set_gemini_selected_type(&mut settings_json, selected_type);

            Ok(vec![
                RemoteWrite {
                    path: "$HOME/.gemini/.env",
                    backup_name: "gemini-env",
                    content: env_text,
                },
                RemoteWrite {
                    path: "$HOME/.gemini/settings.json",
                    backup_name: "gemini-settings.json",
                    content: json_pretty(&settings_json)?,
                },
            ])
        }
        _ => unreachable!("unsupported app type checked by caller"),
    }
}

fn read_remote_settings_config(
    app_type: &AppType,
    target: &ResolvedSshTarget,
) -> Result<RemoteSettingsRead, AppError> {
    match app_type {
        AppType::Claude => {
            const SETTINGS_PATH: &str = "$HOME/.claude/settings.json";
            let (exists, content) = read_remote_file_with_status(target, SETTINGS_PATH)?;
            let files = vec![remote_file_status(SETTINGS_PATH, exists, &content)];
            if !exists {
                return Ok(RemoteSettingsRead {
                    settings_config: None,
                    files,
                    warnings: vec!["远端未找到 Claude Code settings.json".to_string()],
                });
            }

            let settings = parse_remote_json(SETTINGS_PATH, &content)?;
            Ok(RemoteSettingsRead {
                settings_config: Some(settings),
                files,
                warnings: Vec::new(),
            })
        }
        AppType::Codex => {
            const AUTH_PATH: &str = "$HOME/.codex/auth.json";
            const CONFIG_PATH: &str = "$HOME/.codex/config.toml";
            let (auth_exists, auth_content) = read_remote_file_with_status(target, AUTH_PATH)?;
            let (config_exists, config_content) =
                read_remote_file_with_status(target, CONFIG_PATH)?;
            let files = vec![
                remote_file_status(AUTH_PATH, auth_exists, &auth_content),
                remote_file_status(CONFIG_PATH, config_exists, &config_content),
            ];

            let mut warnings = Vec::new();
            if !auth_exists {
                warnings.push("远端未找到 Codex auth.json".to_string());
            }
            if !config_exists {
                warnings.push("远端未找到 Codex config.toml".to_string());
            }
            if !auth_exists && !config_exists {
                return Ok(RemoteSettingsRead {
                    settings_config: None,
                    files,
                    warnings,
                });
            }

            let auth = if auth_exists {
                parse_remote_json(AUTH_PATH, &auth_content)?
            } else {
                json!({})
            };
            Ok(RemoteSettingsRead {
                settings_config: Some(json!({ "auth": auth, "config": config_content })),
                files,
                warnings,
            })
        }
        AppType::Gemini => {
            const ENV_PATH: &str = "$HOME/.gemini/.env";
            const SETTINGS_PATH: &str = "$HOME/.gemini/settings.json";
            let (env_exists, env_content) = read_remote_file_with_status(target, ENV_PATH)?;
            let (settings_exists, settings_content) =
                read_remote_file_with_status(target, SETTINGS_PATH)?;
            let files = vec![
                remote_file_status(ENV_PATH, env_exists, &env_content),
                remote_file_status(SETTINGS_PATH, settings_exists, &settings_content),
            ];

            if !env_exists && !settings_exists {
                return Ok(RemoteSettingsRead {
                    settings_config: None,
                    files,
                    warnings: vec!["远端未找到 Gemini .env 或 settings.json".to_string()],
                });
            }

            let mut warnings = Vec::new();
            if !env_exists {
                warnings.push("远端未找到 Gemini .env".to_string());
            }
            if !settings_exists {
                warnings.push("远端未找到 Gemini settings.json".to_string());
            }

            let env_map = if env_exists {
                crate::gemini_config::parse_env_file(&env_content)
            } else {
                HashMap::new()
            };
            let env_json = crate::gemini_config::env_to_json(&env_map);
            let env_obj = env_json.get("env").cloned().unwrap_or_else(|| json!({}));
            let settings = if settings_exists {
                let value = parse_remote_json(SETTINGS_PATH, &settings_content)?;
                if !value.is_object() {
                    return Err(AppError::Message(
                        "远端 Gemini settings.json 必须是 JSON 对象".to_string(),
                    ));
                }
                value
            } else {
                json!({})
            };

            Ok(RemoteSettingsRead {
                settings_config: Some(json!({ "env": env_obj, "config": settings })),
                files,
                warnings,
            })
        }
        _ => unreachable!("unsupported app type checked by caller"),
    }
}

fn remote_file_status(path: &str, exists: bool, content: &str) -> RemoteConfigFile {
    RemoteConfigFile {
        path: path.to_string(),
        exists,
        bytes: if exists { content.len() } else { 0 },
    }
}

fn parse_remote_json(path: &str, content: &str) -> Result<Value, AppError> {
    serde_json::from_str::<Value>(content)
        .map_err(|e| AppError::Message(format!("解析远端 {path} 失败: {e}")))
}

fn parse_json_object_or_empty(content: &str) -> Value {
    serde_json::from_str::<Value>(content)
        .ok()
        .filter(Value::is_object)
        .unwrap_or_else(|| json!({}))
}

fn set_gemini_selected_type(settings: &mut Value, selected_type: &str) {
    let Some(root) = settings.as_object_mut() else {
        *settings = json!({});
        return set_gemini_selected_type(settings, selected_type);
    };

    let security = root.entry("security").or_insert_with(|| json!({}));
    if !security.is_object() {
        *security = json!({});
    }

    let Some(security_obj) = security.as_object_mut() else {
        return;
    };
    let auth = security_obj.entry("auth").or_insert_with(|| json!({}));
    if !auth.is_object() {
        *auth = json!({});
    }

    if let Some(auth_obj) = auth.as_object_mut() {
        auth_obj.insert(
            "selectedType".to_string(),
            Value::String(selected_type.to_string()),
        );
    }
}

fn json_pretty(value: &Value) -> Result<String, AppError> {
    serde_json::to_string_pretty(value).map_err(|e| AppError::JsonSerialize { source: e })
}

const REMOTE_USAGE_ANALYZER_JS: &str = r#"
var fs = require("fs");
var path = require("path");

var APP = process.argv[1] || "";
var SYNC_STATE = {};

try {
  SYNC_STATE = JSON.parse(fs.readFileSync(0, "utf8") || "{}");
} catch (_) {
  SYNC_STATE = {};
}

function emit(value) {
  process.stdout.write(JSON.stringify(value) + "\n");
}

function toInt(value) {
  var number = Number(value || 0);
  if (!isFinite(number)) {
    return 0;
  }
  return number < 0 ? Math.ceil(number) : Math.floor(number);
}

function epochFromTimestamp(value) {
  if (!value) {
    return Math.floor(Date.now() / 1000);
  }
  var milliseconds = Date.parse(String(value));
  if (!isFinite(milliseconds)) {
    return Math.floor(Date.now() / 1000);
  }
  return Math.floor(milliseconds / 1000);
}

function mtimeNs(filePath) {
  try {
    var statWithBigInt = fs.statSync(filePath, { bigint: true });
    if (statWithBigInt.mtimeNs !== undefined) {
      return Number(statWithBigInt.mtimeNs);
    }
  } catch (_) {}

  var stat = fs.statSync(filePath);
  var milliseconds = stat.mtimeMs !== undefined ? stat.mtimeMs : stat.mtime.getTime();
  return Math.floor(Number(milliseconds) * 1000000);
}

function syncStateFor(relativePath) {
  var state = SYNC_STATE[relativePath] || {};
  return {
    lastModified: toInt(state.lastModified),
    lastOffset: toInt(state.lastOffset),
  };
}

function shouldSkip(relativePath, modified) {
  return modified <= syncStateFor(relativePath).lastModified;
}

function normalizeCodexModel(raw) {
  var name = String(raw || "unknown").toLowerCase();
  if (name.indexOf("/") !== -1) {
    name = name.slice(name.lastIndexOf("/") + 1);
  }

  if (name.length > 11) {
    var suffix = name.slice(-11);
    if (
      suffix[0] === "-" &&
      /^\d{4}$/.test(suffix.slice(1, 5)) &&
      suffix[5] === "-" &&
      /^\d{2}$/.test(suffix.slice(6, 8)) &&
      suffix[8] === "-" &&
      /^\d{2}$/.test(suffix.slice(9, 11))
    ) {
      name = name.slice(0, -11);
    }
  }

  if (name.length > 9 && name.indexOf("-") !== -1) {
    var dashIndex = name.lastIndexOf("-");
    var base = name.slice(0, dashIndex);
    var trailing = name.slice(dashIndex + 1);
    if (/^\d{8}$/.test(trailing)) {
      name = base;
    }
  }

  return name || "unknown";
}

function isDirectory(filePath) {
  try {
    return fs.statSync(filePath).isDirectory();
  } catch (_) {
    return false;
  }
}

function normalizeRelativePath(base, filePath) {
  return path.relative(base, filePath).split(path.sep).join("/");
}

function walkFiles(dir, relativeBase, predicate, files) {
  var names;
  try {
    names = fs.readdirSync(dir);
  } catch (_) {
    return;
  }

  for (var i = 0; i < names.length; i += 1) {
    var name = names[i];
    var fullPath = path.join(dir, name);
    var stat;
    try {
      stat = fs.statSync(fullPath);
    } catch (_) {
      continue;
    }

    if (stat.isDirectory()) {
      walkFiles(fullPath, relativeBase, predicate, files);
    } else if (predicate(fullPath, name, dir)) {
      files.push([fullPath, normalizeRelativePath(relativeBase, fullPath)]);
    }
  }
}

function collectFiles(app) {
  var home = process.env.HOME || process.env.USERPROFILE || "";
  var files = [];

  if (app === "claude") {
    var claudeRoot = path.join(home, ".claude", "projects");
    if (isDirectory(claudeRoot)) {
      walkFiles(claudeRoot, claudeRoot, function (_, name) {
        return name.slice(-6) === ".jsonl";
      }, files);
    }
  } else if (app === "codex") {
    var codexRoot = path.join(home, ".codex");
    ["sessions", "archived_sessions"].forEach(function (relativeRoot) {
      var base = path.join(codexRoot, relativeRoot);
      if (isDirectory(base)) {
        walkFiles(base, codexRoot, function (_, name) {
          return name.slice(-6) === ".jsonl";
        }, files);
      }
    });
  } else if (app === "gemini") {
    var geminiRoot = path.join(home, ".gemini");
    var geminiTmp = path.join(geminiRoot, "tmp");
    if (isDirectory(geminiTmp)) {
      walkFiles(geminiTmp, geminiRoot, function (_, name, dir) {
        return path.basename(dir) === "chats" &&
          name.indexOf("session-") === 0 &&
          name.slice(-5) === ".json";
      }, files);
    }
  }

  files.sort(function (left, right) {
    return left[1] < right[1] ? -1 : left[1] > right[1] ? 1 : 0;
  });
  return files;
}

function emitState(relativePath, modified, offset) {
  emit({
    kind: "state",
    path: relativePath,
    lastModified: toInt(modified),
    lastOffset: toInt(offset),
  });
}

function emitRecord(value) {
  value.kind = "record";
  emit(value);
}

function readLines(filePath) {
  var content = fs.readFileSync(filePath, "utf8");
  if (!content) {
    return [];
  }
  var lines = content.split(/\r?\n/);
  if (lines.length && lines[lines.length - 1] === "") {
    lines.pop();
  }
  return lines;
}

function parseClaudeFile(filePath, relativePath) {
  var modified = mtimeNs(filePath);
  if (shouldSkip(relativePath, modified)) {
    emit({ kind: "unchangedFile", path: relativePath });
    return;
  }

  var state = syncStateFor(relativePath);
  var lineOffset = 0;
  var currentSessionId = null;
  var messages = {};
  var lines = readLines(filePath);

  for (var i = 0; i < lines.length; i += 1) {
    lineOffset += 1;
    if (lineOffset <= state.lastOffset) {
      continue;
    }

    var line = lines[i].trim();
    if (!line) {
      continue;
    }

    var value;
    try {
      value = JSON.parse(line);
    } catch (_) {
      continue;
    }

    if (currentSessionId === null) {
      currentSessionId = value.sessionId;
    }
    if (value.type !== "assistant") {
      continue;
    }

    var message = value.message || {};
    var messageId = message.id;
    var usage = message.usage || {};
    if (!messageId) {
      continue;
    }

    var parsed = {
      message_id: messageId,
      model: message.model || "unknown",
      input_tokens: toInt(usage.input_tokens),
      output_tokens: toInt(usage.output_tokens),
      cache_read_tokens: toInt(usage.cache_read_input_tokens),
      cache_creation_tokens: toInt(usage.cache_creation_input_tokens),
      stop_reason: message.stop_reason,
      created_at: epochFromTimestamp(value.timestamp),
      session_id: currentSessionId,
    };
    var existing = messages[messageId];
    if (
      !existing ||
      (parsed.stop_reason && !existing.stop_reason) ||
      (!!parsed.stop_reason === !!existing.stop_reason &&
        parsed.output_tokens > existing.output_tokens)
    ) {
      messages[messageId] = parsed;
    }
  }

  Object.keys(messages).forEach(function (messageId) {
    var item = messages[messageId];
    if (!item.stop_reason || item.output_tokens === 0) {
      return;
    }
    emitRecord({
      requestId: "session:" + item.message_id,
      sessionId: item.session_id,
      model: item.model,
      inputTokens: item.input_tokens,
      outputTokens: item.output_tokens,
      cacheReadTokens: item.cache_read_tokens,
      cacheCreationTokens: item.cache_creation_tokens,
      createdAt: item.created_at,
    });
  });

  emitState(relativePath, modified, lineOffset);
}

function parseTokenUsage(value) {
  if (!value || typeof value !== "object" || Array.isArray(value)) {
    return null;
  }
  return {
    input: toInt(value.input_tokens),
    cached: toInt(value.cached_input_tokens || value.cache_read_input_tokens),
    output: toInt(value.output_tokens),
  };
}

function tokenDelta(previous, current) {
  if (!previous) {
    return {
      input: current.input,
      cached: current.cached,
      output: current.output,
    };
  }
  return {
    input: Math.max(0, current.input - previous.input),
    cached: Math.max(0, current.cached - previous.cached),
    output: Math.max(0, current.output - previous.output),
  };
}

function parseCodexFile(filePath, relativePath) {
  var modified = mtimeNs(filePath);
  if (shouldSkip(relativePath, modified)) {
    emit({ kind: "unchangedFile", path: relativePath });
    return;
  }

  var state = syncStateFor(relativePath);
  var lineOffset = 0;
  var sessionId = null;
  var currentModel = "unknown";
  var previousTotal = null;
  var eventIndex = 0;
  var lines = readLines(filePath);

  for (var i = 0; i < lines.length; i += 1) {
    lineOffset += 1;
    var line = lines[i].trim();
    if (!line) {
      continue;
    }
    if (
      line.indexOf("event_msg") === -1 &&
      line.indexOf("turn_context") === -1 &&
      line.indexOf("session_meta") === -1
    ) {
      continue;
    }
    if (line.indexOf("event_msg") !== -1 && line.indexOf("token_count") === -1) {
      continue;
    }

    var value;
    try {
      value = JSON.parse(line);
    } catch (_) {
      continue;
    }

    var eventType = value.type;
    var payload = value.payload || {};

    if (eventType === "session_meta" && sessionId === null) {
      sessionId = payload.session_id || payload.sessionId || payload.id;
      continue;
    }

    if (eventType === "turn_context") {
      var contextModel = payload.model || (payload.info || {}).model;
      if (contextModel) {
        currentModel = normalizeCodexModel(contextModel);
      }
      continue;
    }

    if (eventType !== "event_msg" || payload.type !== "token_count") {
      continue;
    }

    var info = payload.info;
    if (!info || typeof info !== "object" || Array.isArray(info)) {
      continue;
    }
    var model = info.model || info.model_name || payload.model;
    if (model) {
      currentModel = normalizeCodexModel(model);
    }

    var usage = null;
    var delta = null;
    if (Object.prototype.hasOwnProperty.call(info, "total_token_usage")) {
      usage = parseTokenUsage(info.total_token_usage);
      if (!usage) {
        continue;
      }
      delta = tokenDelta(previousTotal, usage);
      previousTotal = usage;
    } else if (Object.prototype.hasOwnProperty.call(info, "last_token_usage")) {
      usage = parseTokenUsage(info.last_token_usage);
      if (!usage) {
        continue;
      }
      delta = usage;
    } else {
      continue;
    }

    delta.cached = Math.min(delta.cached, delta.input);
    if (delta.input === 0 && delta.cached === 0 && delta.output === 0) {
      continue;
    }

    eventIndex += 1;
    if (lineOffset <= state.lastOffset) {
      continue;
    }

    var stableSessionId = sessionId || "unknown";
    emitRecord({
      requestId: "codex_session:" + stableSessionId + ":" + eventIndex,
      sessionId: sessionId,
      model: currentModel,
      inputTokens: delta.input,
      outputTokens: delta.output,
      cacheReadTokens: delta.cached,
      cacheCreationTokens: 0,
      createdAt: epochFromTimestamp(value.timestamp),
    });
  }

  emitState(relativePath, modified, lineOffset);
}

function parseGeminiFile(filePath, relativePath) {
  var modified = mtimeNs(filePath);
  if (shouldSkip(relativePath, modified)) {
    emit({ kind: "unchangedFile", path: relativePath });
    return;
  }

  var state = syncStateFor(relativePath);
  var value = JSON.parse(fs.readFileSync(filePath, "utf8"));
  var sessionId = value.sessionId;
  var messages = Array.isArray(value.messages) ? value.messages : [];
  var count = 0;

  for (var i = 0; i < messages.length; i += 1) {
    var message = messages[i] || {};
    if (message.type !== "gemini") {
      continue;
    }
    var tokens = message.tokens || {};
    if (typeof tokens !== "object" || Array.isArray(tokens)) {
      continue;
    }

    var inputTokens = toInt(tokens.input);
    var outputTokens = toInt(tokens.output);
    var thoughts = toInt(tokens.thoughts);
    var cached = toInt(tokens.cached);
    if (inputTokens === 0 && outputTokens === 0 && thoughts === 0 && cached === 0) {
      continue;
    }

    count += 1;
    if (count <= state.lastOffset) {
      continue;
    }

    var messageId = message.id || "unknown";
    var stableSessionId = sessionId || "unknown";
    emitRecord({
      requestId: "gemini_session:" + stableSessionId + ":" + messageId,
      sessionId: sessionId,
      model: message.model || "unknown",
      inputTokens: inputTokens,
      outputTokens: outputTokens + thoughts,
      cacheReadTokens: cached,
      cacheCreationTokens: 0,
      createdAt: epochFromTimestamp(message.timestamp),
    });
  }

  emitState(relativePath, modified, count);
}

var files = collectFiles(APP);
emit({ kind: "meta", filesScanned: files.length });

for (var index = 0; index < files.length; index += 1) {
  var filePath = files[index][0];
  var relativePath = files[index][1];
  try {
    if (APP === "claude") {
      parseClaudeFile(filePath, relativePath);
    } else if (APP === "codex") {
      parseCodexFile(filePath, relativePath);
    } else if (APP === "gemini") {
      parseGeminiFile(filePath, relativePath);
    } else {
      throw new Error("unsupported app: " + APP);
    }
  } catch (error) {
    emit({
      kind: "error",
      path: relativePath,
      error: error && error.message ? error.message : String(error),
    });
  }
}
"#;

const REMOTE_NODE_LOOKUP_BASH: &str = r#"PATH="$PATH:/usr/local/bin:/usr/bin:/bin:/usr/local/sbin:/opt/homebrew/bin:$HOME/.local/bin:$HOME/bin"; command -v node || { for file in "$HOME/.nvm/nvm.sh" "$HOME/.bashrc" "$HOME/.bash_profile" "$HOME/.profile"; do [ -r "$file" ] && . "$file" >/dev/null 2>&1 || true; done; command -v node; }"#;

const REMOTE_NODE_LOOKUP_ZSH: &str = r#"PATH="$PATH:/usr/local/bin:/usr/bin:/bin:/usr/local/sbin:/opt/homebrew/bin:$HOME/.local/bin:$HOME/bin"; command -v node || { for file in "$HOME/.nvm/nvm.sh" "$HOME/.zshrc" "$HOME/.zprofile" "$HOME/.profile"; do [ -r "$file" ] && . "$file" >/dev/null 2>&1 || true; done; command -v node; }"#;

fn remote_usage_analyzer_command(app_type: &AppType) -> String {
    format!(
        "node_bin=\"$(PATH=\"$PATH:/usr/local/bin:/usr/bin:/bin:/usr/local/sbin:/opt/homebrew/bin:$HOME/.local/bin:$HOME/bin\"; command -v node 2>/dev/null || bash -lc {} 2>/dev/null || zsh -lc {} 2>/dev/null || true)\"; node_bin=\"$(printf '%s\\n' \"$node_bin\" | awk 'NF {{ print; exit }}')\"; if [ -z \"$node_bin\" ]; then printf '%s\\n' 'cc-switch 远端用量同步需要远端安装 node，或让 node 在非交互式 SSH 命令中可见' >&2; exit 127; fi; \"$node_bin\" -e {} {}",
        shell_quote_single(REMOTE_NODE_LOOKUP_BASH),
        shell_quote_single(REMOTE_NODE_LOOKUP_ZSH),
        shell_quote_single(REMOTE_USAGE_ANALYZER_JS),
        shell_quote_single(app_type.as_str())
    )
}

fn shell_quote_single(value: &str) -> String {
    format!("'{}'", value.replace('\'', "'\\''"))
}

fn read_remote_file(
    target: &ResolvedSshTarget,
    remote_path: &'static str,
) -> Result<Option<String>, AppError> {
    let (exists, content) = read_remote_file_with_status(target, remote_path)?;
    Ok(exists.then_some(content))
}

fn read_remote_file_with_status(
    target: &ResolvedSshTarget,
    remote_path: &'static str,
) -> Result<(bool, String), AppError> {
    const MARKER: &str = "__CC_SWITCH_REMOTE_FILE_EXISTS__\n";
    let command = format!(
        "file=\"{remote_path}\"; if [ -f \"$file\" ]; then printf '%s\\n' '__CC_SWITCH_REMOTE_FILE_EXISTS__'; cat \"$file\"; fi"
    );
    let output = run_ssh_command(target, &command, None)?;
    if let Some(content) = output.strip_prefix(MARKER) {
        Ok((true, content.to_string()))
    } else {
        Ok((false, String::new()))
    }
}

fn write_remote_file(
    target: &ResolvedSshTarget,
    remote_path: &'static str,
    backup_name: &'static str,
    content: &[u8],
    stamp: &str,
) -> Result<String, AppError> {
    let command = format!(
        "set -e; umask 077; file=\"{remote_path}\"; dir=\"${{file%/*}}\"; backup_root=\"$HOME/.cc-switch/remote-backups/{stamp}\"; mkdir -p \"$dir\"; if [ -f \"$file\" ]; then mkdir -p \"$backup_root\"; cp \"$file\" \"$backup_root/{backup_name}\"; fi; tmp=\"$file.tmp.$$\"; cat > \"$tmp\"; mv \"$tmp\" \"$file\"; chmod 600 \"$file\" 2>/dev/null || true; printf '%s\\n' \"$file\""
    );
    let output = run_ssh_command(target, &command, Some(content))?;
    Ok(output
        .lines()
        .last()
        .map(str::trim)
        .filter(|line| !line.is_empty())
        .unwrap_or(remote_path)
        .to_string())
}

fn count_remote_session_log_files(
    target: &ResolvedSshTarget,
    app_type: &AppType,
) -> Result<u32, AppError> {
    let command = match app_type {
        AppType::Claude => {
            "set -e; cd \"$HOME\"; { if [ -d .claude/projects ]; then find .claude/projects -type f -name '*.jsonl'; fi; } | wc -l"
        }
        AppType::Codex => {
            "set -e; cd \"$HOME\"; { if [ -d .codex/sessions ]; then find .codex/sessions -type f -name '*.jsonl'; fi; if [ -d .codex/archived_sessions ]; then find .codex/archived_sessions -maxdepth 1 -type f -name '*.jsonl'; fi; } | wc -l"
        }
        AppType::Gemini => {
            "set -e; cd \"$HOME\"; { if [ -d .gemini/tmp ]; then find .gemini/tmp -type f -path '*/chats/session-*.json'; fi; } | wc -l"
        }
        _ => "printf '0\\n'",
    };
    let output = run_ssh_command(target, command, None)?;
    output
        .trim()
        .parse::<u32>()
        .map_err(|e| AppError::Message(format!("解析远端日志数量失败: {e}")))
}

fn run_ssh_command(
    target: &ResolvedSshTarget,
    remote_command: &str,
    stdin: Option<&[u8]>,
) -> Result<String, AppError> {
    let output = run_ssh_output(target, remote_command, stdin)?;
    Ok(String::from_utf8_lossy(&output.stdout).to_string())
}

fn run_ssh_output(
    target: &ResolvedSshTarget,
    remote_command: &str,
    stdin: Option<&[u8]>,
) -> Result<std::process::Output, AppError> {
    let mut command = Command::new("ssh");
    hide_ssh_console_window(&mut command);
    configure_ssh_command(&mut command, target, remote_command);
    let _askpass = configure_ssh_password(&mut command, target)?;
    command.stdout(Stdio::piped()).stderr(Stdio::piped());
    command.stdin(if stdin.is_some() {
        Stdio::piped()
    } else {
        Stdio::null()
    });

    let mut child = command
        .spawn()
        .map_err(|e| AppError::Message(format!("启动 ssh 失败: {e}")))?;

    if let Some(input) = stdin {
        let Some(mut child_stdin) = child.stdin.take() else {
            return Err(AppError::Message("无法写入 ssh stdin".to_string()));
        };
        child_stdin
            .write_all(input)
            .map_err(|e| AppError::Message(format!("写入 ssh stdin 失败: {e}")))?;
    }

    let output = child
        .wait_with_output()
        .map_err(|e| AppError::Message(format!("等待 ssh 结束失败: {e}")))?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr).trim().to_string();
        return Err(AppError::Message(if stderr.is_empty() {
            format!("ssh 命令失败，退出码: {}", output.status)
        } else {
            format!("ssh 命令失败: {stderr}")
        }));
    }

    Ok(output)
}

#[cfg(windows)]
fn hide_ssh_console_window(command: &mut Command) {
    const CREATE_NO_WINDOW: u32 = 0x08000000;
    command.creation_flags(CREATE_NO_WINDOW);
}

#[cfg(not(windows))]
fn hide_ssh_console_window(_command: &mut Command) {}

fn configure_ssh_command(command: &mut Command, target: &ResolvedSshTarget, remote_command: &str) {
    command.arg("-C");
    command.args(["-o", "ConnectTimeout=10", "-o", "ClearAllForwardings=yes"]);
    if let Some(port) = target.port {
        command.arg("-p").arg(port.to_string());
    }

    if target.password.is_some() {
        command.args([
            "-o",
            "BatchMode=no",
            "-o",
            "NumberOfPasswordPrompts=1",
            "-o",
            "StrictHostKeyChecking=accept-new",
        ]);
    } else {
        command.args(["-o", "BatchMode=yes", "-o", "NumberOfPasswordPrompts=0"]);
    }

    command
        .arg("--")
        .arg(&target.connect_target)
        .arg(remote_command);
}

fn configure_ssh_password(
    command: &mut Command,
    target: &ResolvedSshTarget,
) -> Result<Option<tempfile::NamedTempFile>, AppError> {
    let Some(password) = target.password.as_deref() else {
        return Ok(None);
    };

    let mut file = tempfile::Builder::new()
        .prefix("cc-switch-ssh-askpass-")
        .suffix(askpass_script_suffix())
        .tempfile()
        .map_err(|e| AppError::Message(format!("创建 SSH 密码辅助脚本失败: {e}")))?;
    file.write_all(askpass_script_content().as_bytes())
        .map_err(|e| AppError::Message(format!("写入 SSH 密码辅助脚本失败: {e}")))?;

    #[cfg(unix)]
    {
        let mut permissions = file
            .as_file()
            .metadata()
            .map_err(|e| AppError::Message(format!("读取 SSH 密码辅助脚本权限失败: {e}")))?
            .permissions();
        permissions.set_mode(0o700);
        file.as_file()
            .set_permissions(permissions)
            .map_err(|e| AppError::Message(format!("设置 SSH 密码辅助脚本权限失败: {e}")))?;
    }

    command.env("SSH_ASKPASS", file.path());
    command.env("SSH_ASKPASS_REQUIRE", "force");
    command.env("CC_SWITCH_SSH_PASSWORD", password);
    if std::env::var_os("DISPLAY").is_none() {
        command.env("DISPLAY", "cc-switch");
    }

    Ok(Some(file))
}

#[cfg(unix)]
fn askpass_script_suffix() -> &'static str {
    ".sh"
}

#[cfg(not(unix))]
fn askpass_script_suffix() -> &'static str {
    ".cmd"
}

#[cfg(unix)]
fn askpass_script_content() -> &'static str {
    "#!/bin/sh\nprintf '%s\\n' \"$CC_SWITCH_SSH_PASSWORD\"\n"
}

#[cfg(not(unix))]
fn askpass_script_content() -> &'static str {
    "@echo off\r\necho %CC_SWITCH_SSH_PASSWORD%\r\n"
}

fn remote_backup_stamp() -> String {
    chrono::Local::now().format("%Y%m%d%H%M%S").to_string()
}

fn is_safe_ssh_alias(alias: &str) -> bool {
    !alias.trim().is_empty()
        && !alias.starts_with('-')
        && alias
            .chars()
            .all(|ch| ch.is_ascii_alphanumeric() || matches!(ch, '-' | '_' | '.' | '@' | ':'))
}

fn is_safe_manual_ssh_host(host: &str) -> bool {
    !host.trim().is_empty()
        && !host.starts_with('-')
        && !host.contains('@')
        && host
            .chars()
            .all(|ch| ch.is_ascii_alphanumeric() || matches!(ch, '-' | '_' | '.' | ':' | '[' | ']'))
}

fn is_safe_ssh_user(user: &str) -> bool {
    !user.trim().is_empty()
        && !user.starts_with('-')
        && user
            .chars()
            .all(|ch| ch.is_ascii_alphanumeric() || matches!(ch, '-' | '_' | '.'))
}

fn parse_ssh_config_file(
    path: &Path,
    hosts: &mut Vec<SshHostEntry>,
    index: &mut HashMap<String, usize>,
    visited: &mut HashSet<PathBuf>,
) -> Result<(), AppError> {
    if !path.exists() {
        return Ok(());
    }

    let canonical = path.canonicalize().unwrap_or_else(|_| path.to_path_buf());
    if !visited.insert(canonical.clone()) {
        return Ok(());
    }

    let content = fs::read_to_string(path).map_err(|e| AppError::io(path, e))?;
    parse_ssh_config_content(&content, path, hosts, index, visited)
}

fn parse_ssh_config_content(
    content: &str,
    source_path: &Path,
    hosts: &mut Vec<SshHostEntry>,
    index: &mut HashMap<String, usize>,
    visited: &mut HashSet<PathBuf>,
) -> Result<(), AppError> {
    let mut current_aliases: Vec<String> = Vec::new();
    let source = source_path.to_string_lossy().to_string();
    let base_dir = source_path.parent().unwrap_or_else(|| Path::new("."));

    for raw_line in content.lines() {
        let line = raw_line
            .split_once('#')
            .map(|(before, _)| before)
            .unwrap_or(raw_line)
            .trim();
        if line.is_empty() {
            continue;
        }

        let mut parts = line.split_whitespace();
        let Some(keyword) = parts.next() else {
            continue;
        };
        let values: Vec<&str> = parts.collect();
        if values.is_empty() {
            continue;
        }

        match keyword.to_ascii_lowercase().as_str() {
            "include" => {
                for pattern in values {
                    for include_path in expand_include_pattern(pattern, base_dir)? {
                        parse_ssh_config_file(&include_path, hosts, index, visited)?;
                    }
                }
            }
            "host" => {
                current_aliases = values
                    .into_iter()
                    .filter_map(normalize_host_alias)
                    .collect::<Vec<_>>();
                for alias in &current_aliases {
                    if index.contains_key(alias) {
                        continue;
                    }
                    let position = hosts.len();
                    index.insert(alias.clone(), position);
                    hosts.push(SshHostEntry {
                        alias: alias.clone(),
                        host_name: None,
                        user: None,
                        port: None,
                        source: Some(source.clone()),
                    });
                }
            }
            "hostname" | "user" | "port" => {
                let value = values.join(" ");
                for alias in &current_aliases {
                    let Some(position) = index.get(alias).copied() else {
                        continue;
                    };
                    let host = &mut hosts[position];
                    match keyword.to_ascii_lowercase().as_str() {
                        "hostname" if host.host_name.is_none() => {
                            host.host_name = Some(value.clone())
                        }
                        "user" if host.user.is_none() => host.user = Some(value.clone()),
                        "port" if host.port.is_none() => host.port = Some(value.clone()),
                        _ => {}
                    }
                }
            }
            _ => {}
        }
    }

    Ok(())
}

fn normalize_host_alias(raw: &str) -> Option<String> {
    let alias = raw.trim().trim_matches('"').trim_matches('\'');
    if alias.is_empty()
        || alias.contains('*')
        || alias.contains('?')
        || alias.starts_with('!')
        || !is_safe_ssh_alias(alias)
    {
        return None;
    }
    Some(alias.to_string())
}

fn expand_include_pattern(pattern: &str, base_dir: &Path) -> Result<Vec<PathBuf>, AppError> {
    let path = expand_ssh_path(pattern, base_dir);
    let file_name = path
        .file_name()
        .and_then(|name| name.to_str())
        .unwrap_or_default()
        .to_string();
    if !file_name.contains('*') && !file_name.contains('?') {
        return Ok(vec![path]);
    }

    let dir = path.parent().unwrap_or_else(|| Path::new("."));
    let entries = match fs::read_dir(dir) {
        Ok(entries) => entries,
        Err(_) => return Ok(Vec::new()),
    };

    let mut paths = Vec::new();
    for entry in entries.flatten() {
        let name = entry.file_name().to_string_lossy().to_string();
        if wildcard_match(&file_name, &name) {
            paths.push(entry.path());
        }
    }
    paths.sort();
    Ok(paths)
}

fn expand_ssh_path(raw: &str, base_dir: &Path) -> PathBuf {
    let expanded = if raw == "~" {
        crate::config::get_home_dir()
    } else if let Some(rest) = raw.strip_prefix("~/") {
        crate::config::get_home_dir().join(rest)
    } else {
        PathBuf::from(raw)
    };

    if expanded.is_relative() {
        base_dir.join(expanded)
    } else {
        expanded
    }
}

fn wildcard_match(pattern: &str, value: &str) -> bool {
    fn inner(pattern: &[u8], value: &[u8]) -> bool {
        match (pattern.first(), value.first()) {
            (None, None) => true,
            (None, Some(_)) => false,
            (Some(b'*'), _) => {
                inner(&pattern[1..], value) || (!value.is_empty() && inner(pattern, &value[1..]))
            }
            (Some(b'?'), Some(_)) => inner(&pattern[1..], &value[1..]),
            (Some(a), Some(b)) if a == b => inner(&pattern[1..], &value[1..]),
            _ => false,
        }
    }
    inner(pattern.as_bytes(), value.as_bytes())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::{HashMap, HashSet};

    #[test]
    fn parse_ssh_config_collects_plain_hosts_and_metadata() {
        let content = r#"
Host *
  ServerAliveInterval 30

Host dev prod-box
  HostName 10.0.0.2
  User deploy
  Port 2200

Host ignored-*
  HostName ignored.example.com

Host "quoted"
  HostName quoted.example.com
"#;
        let mut hosts = Vec::new();
        let mut index = HashMap::new();
        let mut visited = HashSet::new();
        parse_ssh_config_content(
            content,
            Path::new("/tmp/ssh-config"),
            &mut hosts,
            &mut index,
            &mut visited,
        )
        .unwrap();

        assert_eq!(
            hosts
                .iter()
                .map(|host| host.alias.as_str())
                .collect::<Vec<_>>(),
            vec!["dev", "prod-box", "quoted"]
        );
        assert_eq!(hosts[0].host_name.as_deref(), Some("10.0.0.2"));
        assert_eq!(hosts[0].user.as_deref(), Some("deploy"));
        assert_eq!(hosts[0].port.as_deref(), Some("2200"));
    }

    #[test]
    fn wildcard_match_supports_star_and_question_mark() {
        assert!(wildcard_match("conf.d/*", "conf.d/dev"));
        assert!(wildcard_match("host?.conf", "host1.conf"));
        assert!(!wildcard_match("host?.conf", "host12.conf"));
    }

    #[test]
    fn resolve_manual_ssh_target_builds_label_without_saving_password() -> Result<(), AppError> {
        let target = SshConnectionTarget {
            target_type: Some("manual".to_string()),
            alias: None,
            host: Some("10.0.0.2".to_string()),
            user: Some("deploy".to_string()),
            port: Some(2200),
            password: Some("secret".to_string()),
        };

        let resolved = resolve_ssh_target(&target)?;

        assert_eq!(resolved.label, "deploy@10.0.0.2:2200");
        assert_eq!(resolved.connect_target, "deploy@10.0.0.2");
        assert_eq!(resolved.port, Some(2200));
        assert_eq!(resolved.password.as_deref(), Some("secret"));
        Ok(())
    }

    #[test]
    fn resolve_manual_ssh_target_rejects_unsafe_host() {
        let target = SshConnectionTarget {
            target_type: Some("manual".to_string()),
            alias: None,
            host: Some("-oProxyCommand=bad".to_string()),
            user: Some("deploy".to_string()),
            port: Some(22),
            password: None,
        };

        assert!(resolve_ssh_target(&target).is_err());
    }

    #[test]
    fn import_remote_usage_records_inserts_records_and_sync_state() -> Result<(), AppError> {
        let db = Database::memory()?;
        let options = SessionUsageImportOptions::remote("pjlab", "claude", "remote_claude_session");
        let output = r#"{"kind":"meta","filesScanned":1}
{"kind":"record","requestId":"session:msg_1","sessionId":"session-1","model":"claude-sonnet-4-5","inputTokens":10,"outputTokens":5,"cacheReadTokens":2,"cacheCreationTokens":3,"createdAt":1000}
{"kind":"state","path":"project/session.jsonl","lastModified":123456,"lastOffset":42}
{"kind":"unchangedFile","path":"project/already-synced.jsonl"}
"#;

        let outcome = import_remote_usage_records(&db, &AppType::Claude, &options, output)?;
        let result = outcome.result;

        assert_eq!(result.files_scanned, 1);
        assert_eq!(result.imported, 1);
        assert_eq!(result.skipped, 0);
        assert_eq!(outcome.unchanged_files, 1);

        let conn = lock_conn!(db.conn);
        let (request_id, data_source, provider_id, input_tokens): (String, String, String, i64) =
            conn.query_row(
                "SELECT request_id, data_source, provider_id, input_tokens
                 FROM proxy_request_logs
                 WHERE request_id = 'remote:pjlab:session:msg_1'",
                [],
                |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?, row.get(3)?)),
            )?;
        assert_eq!(request_id, "remote:pjlab:session:msg_1");
        assert_eq!(data_source, "remote:pjlab");
        assert_eq!(provider_id, "_remote:claude:pjlab");
        assert_eq!(input_tokens, 10);

        let (last_modified, last_offset): (i64, i64) = conn.query_row(
            "SELECT last_modified, last_line_offset
             FROM session_log_sync
             WHERE file_path = 'remote://pjlab/claude/project/session.jsonl'",
            [],
            |row| Ok((row.get(0)?, row.get(1)?)),
        )?;
        assert_eq!(last_modified, 123456);
        assert_eq!(last_offset, 42);
        Ok(())
    }

    #[test]
    fn import_remote_usage_records_normalizes_codex_model() -> Result<(), AppError> {
        let db = Database::memory()?;
        let options = SessionUsageImportOptions::remote("pjlab", "codex", "remote_codex_session");
        let output = r#"{"kind":"meta","filesScanned":1}
{"kind":"record","requestId":"codex_session:s1:1","sessionId":"s1","model":"openai/GPT-5.4-2026-03-05","inputTokens":10,"outputTokens":5,"cacheReadTokens":2,"cacheCreationTokens":0,"createdAt":1000}
"#;

        let outcome = import_remote_usage_records(&db, &AppType::Codex, &options, output)?;

        assert_eq!(outcome.result.imported, 1);
        let conn = lock_conn!(db.conn);
        let model: String = conn.query_row(
            "SELECT model
             FROM proxy_request_logs
             WHERE request_id = 'remote:pjlab:codex_session:s1:1'",
            [],
            |row| row.get(0),
        )?;
        assert_eq!(model, "gpt-5.4");
        Ok(())
    }
}

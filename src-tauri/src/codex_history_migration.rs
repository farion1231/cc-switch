//! Codex 第三方历史会话归桶迁移。
//!
//! 只迁移本机 `~/.codex` 历史数据；完成标记写入设备级 `settings.json`，
//! 失败时不写标记，下一次启动自动重试。

use crate::codex_config::{
    get_codex_config_dir, read_codex_config_text, CC_SWITCH_CODEX_MODEL_PROVIDER_ID,
};
use crate::config::{atomic_write, copy_file, get_app_config_dir};
use crate::database::{is_official_seed_id, Database};
use crate::error::AppError;
use crate::session_manager::SessionMessage;
use crate::settings::{
    CodexOfficialHistoryUnifyMigration, CodexProviderTemplateMigration,
    CodexThirdPartyHistoryProviderBucketMigration,
};
use chrono::{Local, SecondsFormat, TimeZone, Utc};
use rusqlite::{backup::Backup, params_from_iter, Connection};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use sha2::{Digest, Sha256};
use std::collections::{BTreeMap, BTreeSet, HashMap, HashSet, VecDeque};
use std::fs;
use std::hash::{Hash, Hasher};
use std::path::{Path, PathBuf};
use std::time::{Duration, SystemTime, UNIX_EPOCH};
use toml_edit::DocumentMut;

const MIGRATION_NAME: &str = "codex-history-provider-migration-v1";
const CURRENT_DESKTOP_HISTORY_REPAIR_NAME: &str =
    "codex-history-current-desktop-visibility-repair-v1";
const CC_SWITCH_CODEX_ROUTER_MODEL_PROVIDER_ID: &str = CC_SWITCH_CODEX_MODEL_PROVIDER_ID;
const OFFICIAL_UNIFY_MIGRATION_NAME: &str = "codex-official-history-unify-v1";
/// 还原操作自身的备份目录（与迁移备份分开，保持迁移账本目录纯净）。
const OFFICIAL_UNIFY_RESTORE_BACKUP_NAME: &str = "codex-official-history-unify-restore-v1";
const CODEX_STATE_DB_FILENAME: &str = "state_5.sqlite";
/// SQLite 变量上限保守值，IN 列表按此分块。
const STATE_DB_ID_CHUNK: usize = 500;

/// 串行化官方历史的迁移与还原：开启迁移（启动重试 + 设置保存后台任务）和
/// 关闭还原可能在毫秒级先后被触发，对同一批 jsonl / state DB 双向改写。
static CODEX_OFFICIAL_HISTORY_OP_LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());

fn lock_codex_official_history_op() -> std::sync::MutexGuard<'static, ()> {
    CODEX_OFFICIAL_HISTORY_OP_LOCK
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
}
/// Codex 内建默认 provider id：config.toml 没有 `model_provider` 键时会话归入此桶。
/// 官方订阅（ChatGPT OAuth / OpenAI API key）的历史会话都记录这个 id。
const OFFICIAL_OPENAI_CODEX_MODEL_PROVIDER_ID: &str = "openai";
const LEGACY_CC_SWITCH_CODEX_MODEL_PROVIDER_ID: &str = "ccswitch";
// If a Codex preset ever used a temporary routing key, keep that old key here
// so local history can be bucketed under the current custom provider id.
const CC_SWITCH_LEGACY_CODEX_MODEL_PROVIDER_IDS: &[&str] = &[
    LEGACY_CC_SWITCH_CODEX_MODEL_PROVIDER_ID,
    "aicodemirror",
    "aicoding",
    "aigocode",
    "aihubmix",
    "ark_agentplan",
    "bailian",
    "bailing",
    "byteplus",
    "claudecn",
    "compshare",
    "compshare_coding",
    "crazyrouter",
    "ctok",
    "cubence",
    "deepseek",
    "dmxapi",
    "doubaoseed",
    "eflowcode",
    "kimi",
    "lemondata",
    "longcat",
    "micu",
    "minimax",
    "minimax_en",
    "modelscope",
    "novita",
    "nvidia",
    "openrouter",
    "packycode",
    "patewayai",
    "pipellm",
    "qianfan_coding",
    "relaxycode",
    "rightcode",
    "runapi",
    "shengsuanyun",
    "siliconflow",
    "siliconflow_en",
    "sssaicode",
    "stepfun",
    "stepfun_en",
    "therouter",
    "xiaomi_mimo",
    "xiaomi_mimo_token_plan",
    "zhipu_glm",
    "zhipu_glm_en",
    "cc_switch_codex_router",
    "codex_model_router",
    "codex_model_router_v2",
];
#[derive(Debug, Clone, Default, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CodexHistoryProviderBucketMigrationOutcome {
    pub source_provider_ids: Vec<String>,
    pub migrated_jsonl_files: usize,
    pub migrated_state_rows: usize,
    pub skipped_reason: Option<String>,
}

/// Codex Desktop 历史可见性修复的调用参数。
///
/// `dry_run=true` 时只统计会被修改的内容；真正写入必须由前端在用户确认后再次调用。
#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CodexHistoryVisibilityRepairOptions {
    pub dry_run: bool,
    pub codex_home: Option<String>,
    pub state_db_path: Option<String>,
    pub project_path: Option<String>,
    pub session_ids: Option<Vec<String>>,
    pub target_provider: Option<String>,
    pub count: Option<usize>,
    pub window_limit: Option<usize>,
    pub balance_recent_window: Option<bool>,
    pub max_per_project: Option<usize>,
    pub max_total: Option<usize>,
    pub source_filter: Option<String>,
    pub include_archived: Option<bool>,
    pub include_subagents: Option<bool>,
    pub skip_provider_bucket_sync: Option<bool>,
}

impl Default for CodexHistoryVisibilityRepairOptions {
    /// 构造保守默认值：预览模式、当前 CC Switch 自定义 provider 桶、可选项目聚焦。
    fn default() -> Self {
        Self {
            dry_run: true,
            codex_home: None,
            state_db_path: None,
            project_path: None,
            session_ids: None,
            target_provider: Some(CC_SWITCH_CODEX_ROUTER_MODEL_PROVIDER_ID.to_string()),
            count: Some(30),
            window_limit: Some(80),
            balance_recent_window: Some(true),
            max_per_project: Some(10),
            max_total: Some(300),
            source_filter: None,
            include_archived: Some(false),
            include_subagents: Some(false),
            skip_provider_bucket_sync: Some(false),
        }
    }
}

/// Codex Desktop 历史可见性修复的统计结果。
///
/// 字段同时用于 dry-run 预览和 apply 结果，便于前端先展示影响范围再确认写入。
#[derive(Debug, Clone, Default, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CodexHistoryVisibilityRepairOutcome {
    pub dry_run: bool,
    pub codex_home: String,
    pub state_db_path: Option<String>,
    pub active_db_kind: Option<String>,
    pub live_config_model_provider: Option<String>,
    pub target_provider: String,
    pub source_provider_ids: Vec<String>,
    pub sqlite_threads: usize,
    pub provider_rows_to_update: usize,
    pub provider_rows_updated: usize,
    pub rollout_first_lines_to_update: usize,
    pub rollout_first_lines_updated: usize,
    pub user_event_rows_to_update: usize,
    pub user_event_rows_updated: usize,
    pub visible_candidate_rows: usize,
    pub session_index_missing_to_append: usize,
    pub session_index_appended: usize,
    pub project_rows: usize,
    pub focus_selected_count: usize,
    pub balanced_recent_window_enabled: bool,
    pub balanced_recent_window_rows: usize,
    pub balanced_recent_window_projects: usize,
    pub max_per_project: usize,
    pub max_total: usize,
    pub source_filter: Option<String>,
    pub sqlite_focus_rows_to_update: usize,
    pub sqlite_focus_rows_updated: usize,
    pub session_index_titles_to_update: usize,
    pub session_index_titles_updated: usize,
    pub session_index_rows_to_move: usize,
    pub session_index_rows_moved: usize,
    pub workspace_hints_to_fix: usize,
    pub workspace_hints_fixed: usize,
    pub projectless_ids_to_remove: usize,
    pub projectless_ids_removed: usize,
    pub saved_workspace_roots_to_add: usize,
    pub saved_workspace_roots_added: usize,
    pub rollout_mtimes_to_touch: usize,
    pub rollout_mtimes_touched: usize,
    pub visible_project_rows_in_window_before: usize,
    pub backup_dir: Option<String>,
    pub skipped_reason: Option<String>,
}

/// Codex 历史列表的查询参数。
///
/// 该查询只读 active SQLite，用于 UI 选择需要定向恢复的 session。
#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CodexHistorySessionListOptions {
    pub codex_home: Option<String>,
    pub state_db_path: Option<String>,
    pub project_path: Option<String>,
    pub provider: Option<String>,
    pub source_filter: Option<String>,
    pub query: Option<String>,
    pub limit: Option<usize>,
    pub include_archived: Option<bool>,
    pub include_subagents: Option<bool>,
}

impl Default for CodexHistorySessionListOptions {
    /// 默认只列出 Codex Desktop 常规交互来源，避免把 subagent 和归档记录混入首屏。
    fn default() -> Self {
        Self {
            codex_home: None,
            state_db_path: None,
            project_path: None,
            provider: None,
            source_filter: None,
            query: None,
            limit: Some(80),
            include_archived: Some(false),
            include_subagents: Some(false),
        }
    }
}

/// UI 历史列表中的单条会话摘要。
#[derive(Debug, Clone, Default, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CodexHistorySessionSummary {
    pub id: String,
    pub title: String,
    pub cwd: Option<String>,
    pub model_provider: Option<String>,
    pub source: Option<String>,
    pub thread_source: Option<String>,
    pub archived: bool,
    pub has_user_event: bool,
    pub updated_at_ms: i64,
    pub updated_at: Option<String>,
    pub rollout_path: Option<String>,
}

/// Codex active SQLite 中某个字段的取值分布，用于前端展示 provider/source 下拉候选。
#[derive(Debug, Clone, Default, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CodexHistoryValueCount {
    pub value: Option<String>,
    pub count: usize,
}

/// Codex 历史列表查询结果。
#[derive(Debug, Clone, Default, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CodexHistorySessionListOutcome {
    pub codex_home: String,
    pub state_db_path: Option<String>,
    pub active_db_kind: Option<String>,
    pub live_config_model_provider: Option<String>,
    pub target_provider_candidates: Vec<String>,
    pub source_counts: Vec<CodexHistoryValueCount>,
    pub provider_counts: Vec<CodexHistoryValueCount>,
    pub total_matched: usize,
    pub items: Vec<CodexHistorySessionSummary>,
    pub skipped_reason: Option<String>,
}

/// 读取单条 Codex 历史详情的参数。
#[derive(Debug, Clone, Default, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CodexHistorySessionDetailOptions {
    pub codex_home: Option<String>,
    pub state_db_path: Option<String>,
    pub session_id: String,
}

/// 单条 Codex 历史详情，正文来自 threads.rollout_path 指向的 JSONL。
#[derive(Debug, Clone, Default, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CodexHistorySessionDetailOutcome {
    pub codex_home: String,
    pub state_db_path: Option<String>,
    pub active_db_kind: Option<String>,
    pub session: Option<CodexHistorySessionSummary>,
    pub messages: Vec<SessionMessage>,
    pub rollout_path: Option<String>,
    pub skipped_reason: Option<String>,
}

#[derive(Debug, Clone, Default)]
pub struct CodexProviderTemplateBucketMigrationOutcome {
    pub migrated_provider_ids: Vec<String>,
    pub skipped_reason: Option<String>,
}

pub fn maybe_migrate_codex_third_party_history_provider_bucket(
    db: &Database,
) -> Result<CodexHistoryProviderBucketMigrationOutcome, AppError> {
    if crate::settings::is_codex_third_party_history_provider_bucket_migrated() {
        return Ok(CodexHistoryProviderBucketMigrationOutcome {
            skipped_reason: Some("already_migrated".to_string()),
            ..Default::default()
        });
    }

    let source_provider_ids = collect_source_model_provider_ids(db)?;
    if source_provider_ids.is_empty() {
        crate::settings::mark_codex_third_party_history_provider_bucket_migrated(
            CodexThirdPartyHistoryProviderBucketMigration {
                completed_at: Utc::now().to_rfc3339(),
                target_provider_id: CC_SWITCH_CODEX_MODEL_PROVIDER_ID.to_string(),
                source_provider_ids: Vec::new(),
                migrated_jsonl_files: 0,
                migrated_state_rows: 0,
                scanned_history_files: true,
            },
        )?;
        return Ok(CodexHistoryProviderBucketMigrationOutcome {
            skipped_reason: Some("no_third_party_provider_ids".to_string()),
            ..Default::default()
        });
    }

    let backup_root = migration_backup_root(MIGRATION_NAME);
    let codex_dir = get_codex_config_dir();
    let migrated_jsonl_files =
        migrate_codex_jsonl_files(&codex_dir, &source_provider_ids, &backup_root)?;
    let migrated_state_rows =
        migrate_codex_state_dbs(&codex_dir, &source_provider_ids, &backup_root)?;

    let source_provider_ids_vec: Vec<String> = source_provider_ids.iter().cloned().collect();
    crate::settings::mark_codex_third_party_history_provider_bucket_migrated(
        CodexThirdPartyHistoryProviderBucketMigration {
            completed_at: Utc::now().to_rfc3339(),
            target_provider_id: CC_SWITCH_CODEX_MODEL_PROVIDER_ID.to_string(),
            source_provider_ids: source_provider_ids_vec.clone(),
            migrated_jsonl_files,
            migrated_state_rows,
            scanned_history_files: true,
        },
    )?;

    Ok(CodexHistoryProviderBucketMigrationOutcome {
        source_provider_ids: source_provider_ids_vec,
        migrated_jsonl_files,
        migrated_state_rows,
        skipped_reason: None,
    })
}

/// 修复当前 Codex Desktop 历史侧边栏可见性。
///
/// 该路径专门覆盖 Codex Desktop 26.609 以后读取 `~/.codex/sqlite/state_5.sqlite` 的实现，
/// 同时处理 provider 桶、`has_user_event`、`session_index.jsonl`、全局工作区提示和项目聚焦。
pub fn repair_codex_history_visibility(
    db: &Database,
    options: CodexHistoryVisibilityRepairOptions,
) -> Result<CodexHistoryVisibilityRepairOutcome, AppError> {
    let codex_dir = options
        .codex_home
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(resolve_user_path)
        .unwrap_or_else(get_codex_config_dir);
    let config_path = codex_dir.join("config.toml");
    let config_text = fs::read_to_string(&config_path)
        .or_else(|_| read_codex_config_text())
        .unwrap_or_default();
    let live_config_model_provider = current_codex_model_provider_from_config_text(&config_text);
    let target_provider = options
        .target_provider
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(str::to_string)
        .or_else(|| live_config_model_provider.clone())
        .unwrap_or_else(|| CC_SWITCH_CODEX_ROUTER_MODEL_PROVIDER_ID.to_string());
    let dry_run = options.dry_run;
    let count = options.count.unwrap_or(30);
    let window_limit = options.window_limit.unwrap_or(80);
    let balance_recent_window = options.balance_recent_window.unwrap_or(true);
    let max_per_project = options.max_per_project.unwrap_or(10).max(1);
    let max_total = options.max_total.unwrap_or(300).max(1);
    let source_filter = options
        .source_filter
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(str::to_string);
    let include_archived = options.include_archived.unwrap_or(false);
    let include_subagents = options.include_subagents.unwrap_or(false);
    let skip_provider_bucket_sync = options.skip_provider_bucket_sync.unwrap_or(false);
    let normalized_project_path = options
        .project_path
        .as_deref()
        .and_then(normalize_history_path);
    let selected_session_ids = options
        .session_ids
        .unwrap_or_default()
        .into_iter()
        .map(|value| value.trim().to_string())
        .filter(|value| !value.is_empty())
        .collect::<BTreeSet<_>>();

    let Some(active_db) = resolve_active_codex_state_db_with_override(
        &codex_dir,
        &config_text,
        options.state_db_path.as_deref(),
    )?
    else {
        return Ok(CodexHistoryVisibilityRepairOutcome {
            dry_run,
            codex_home: codex_dir.to_string_lossy().to_string(),
            target_provider,
            live_config_model_provider,
            skipped_reason: Some("state_db_not_found".to_string()),
            ..Default::default()
        });
    };

    let mut source_provider_ids = collect_history_visibility_source_provider_ids(db)?;
    source_provider_ids.remove(&target_provider);
    if skip_provider_bucket_sync {
        source_provider_ids.clear();
    }

    repair_codex_history_visibility_at(
        &codex_dir,
        active_db,
        &target_provider,
        live_config_model_provider,
        source_provider_ids,
        normalized_project_path,
        HistoryVisibilityRepairRuntimeOptions {
            dry_run,
            count,
            window_limit,
            balance_recent_window,
            max_per_project,
            max_total,
            source_filter,
            include_archived,
            include_subagents,
            selected_session_ids,
            backup_root_override: None,
        },
    )
}

/// 列出 Codex Desktop active SQLite 中的历史摘要，供前端定向选择 session。
///
/// 该函数不读取完整 rollout 内容，只返回侧边栏级别的元数据。
pub fn list_codex_history_sessions(
    options: CodexHistorySessionListOptions,
) -> Result<CodexHistorySessionListOutcome, AppError> {
    let codex_dir = options
        .codex_home
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(resolve_user_path)
        .unwrap_or_else(get_codex_config_dir);
    let config_path = codex_dir.join("config.toml");
    let config_text = fs::read_to_string(&config_path)
        .or_else(|_| read_codex_config_text())
        .unwrap_or_default();
    let live_config_model_provider = current_codex_model_provider_from_config_text(&config_text);
    let Some(active_db) = resolve_active_codex_state_db_with_override(
        &codex_dir,
        &config_text,
        options.state_db_path.as_deref(),
    )?
    else {
        return Ok(CodexHistorySessionListOutcome {
            codex_home: codex_dir.to_string_lossy().to_string(),
            target_provider_candidates: codex_target_provider_candidates(
                live_config_model_provider.as_deref(),
                &[],
            ),
            live_config_model_provider,
            skipped_reason: Some("state_db_not_found".to_string()),
            ..Default::default()
        });
    };
    let conn = Connection::open(&active_db.path)
        .map_err(|e| AppError::Database(format!("open Codex active state DB failed: {e}")))?;
    let rows = load_codex_thread_history_rows(&conn)?;
    let target_provider_candidates =
        codex_target_provider_candidates(live_config_model_provider.as_deref(), &rows);
    let source_counts = history_value_counts(&rows, |row| row.source.clone());
    let provider_counts = history_value_counts(&rows, |row| row.model_provider.clone());
    let project_path = options
        .project_path
        .as_deref()
        .and_then(normalize_history_path);
    let provider = options
        .provider
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(str::to_string);
    let source_filter = options
        .source_filter
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(str::to_string);
    let query = options
        .query
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(|value| value.to_ascii_lowercase());
    let include_archived = options.include_archived.unwrap_or(false);
    let include_subagents = options.include_subagents.unwrap_or(false);
    let limit = options.limit.unwrap_or(80).max(1);
    let mut matched = rows
        .into_iter()
        .filter(|row| include_archived || row.archived.unwrap_or(0) == 0)
        .filter(|row| include_subagents || is_user_thread_source(row.thread_source.as_deref()))
        .filter(|row| {
            source_matches_history_filter(row.source.as_deref(), source_filter.as_deref())
        })
        .filter(|row| {
            provider
                .as_deref()
                .map(|provider| row.model_provider.as_deref() == Some(provider))
                .unwrap_or(true)
        })
        .filter(|row| {
            project_path
                .as_deref()
                .map(|project_path| {
                    row.cwd
                        .as_deref()
                        .and_then(normalize_history_path)
                        .as_deref()
                        == Some(project_path)
                })
                .unwrap_or(true)
        })
        .filter(|row| {
            query
                .as_deref()
                .map(|query| {
                    [
                        row.id.as_str(),
                        row.title.as_deref().unwrap_or_default(),
                        row.preview.as_deref().unwrap_or_default(),
                        row.cwd.as_deref().unwrap_or_default(),
                        row.model_provider.as_deref().unwrap_or_default(),
                    ]
                    .join(" ")
                    .to_ascii_lowercase()
                    .contains(query)
                })
                .unwrap_or(true)
        })
        .collect::<Vec<_>>();
    matched.sort_by(|left, right| {
        row_updated_ms(right)
            .cmp(&row_updated_ms(left))
            .then_with(|| right.id.cmp(&left.id))
    });
    let total_matched = matched.len();
    let items = matched
        .iter()
        .take(limit)
        .map(codex_history_summary_from_row)
        .collect();
    Ok(CodexHistorySessionListOutcome {
        codex_home: codex_dir.to_string_lossy().to_string(),
        state_db_path: Some(active_db.path.to_string_lossy().to_string()),
        active_db_kind: Some(active_db.kind),
        live_config_model_provider,
        target_provider_candidates,
        source_counts,
        provider_counts,
        total_matched,
        items,
        skipped_reason: None,
    })
}

/// 读取单条 Codex 历史正文；SQLite 只负责定位 rollout_path，正文仍来自本地 JSONL。
pub fn read_codex_history_session(
    options: CodexHistorySessionDetailOptions,
) -> Result<CodexHistorySessionDetailOutcome, AppError> {
    let session_id = options.session_id.trim();
    if session_id.is_empty() {
        return Err(AppError::Message("session_id is required".to_string()));
    }

    let codex_dir = options
        .codex_home
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(resolve_user_path)
        .unwrap_or_else(get_codex_config_dir);
    let config_path = codex_dir.join("config.toml");
    let config_text = fs::read_to_string(&config_path)
        .or_else(|_| read_codex_config_text())
        .unwrap_or_default();
    let Some(active_db) = resolve_active_codex_state_db_with_override(
        &codex_dir,
        &config_text,
        options.state_db_path.as_deref(),
    )?
    else {
        return Ok(CodexHistorySessionDetailOutcome {
            codex_home: codex_dir.to_string_lossy().to_string(),
            skipped_reason: Some("state_db_not_found".to_string()),
            ..Default::default()
        });
    };

    let conn = Connection::open(&active_db.path)
        .map_err(|e| AppError::Database(format!("open Codex active state DB failed: {e}")))?;
    let rows = load_codex_thread_history_rows(&conn)?;
    let Some(row) = rows.iter().find(|row| row.id == session_id) else {
        return Ok(CodexHistorySessionDetailOutcome {
            codex_home: codex_dir.to_string_lossy().to_string(),
            state_db_path: Some(active_db.path.to_string_lossy().to_string()),
            active_db_kind: Some(active_db.kind),
            skipped_reason: Some("session_not_found".to_string()),
            ..Default::default()
        });
    };

    let summary = codex_history_summary_from_row(row);
    let rollout_path = summary.rollout_path.clone();
    let Some(path) = resolve_history_path(row.rollout_path.as_deref()) else {
        return Ok(CodexHistorySessionDetailOutcome {
            codex_home: codex_dir.to_string_lossy().to_string(),
            state_db_path: Some(active_db.path.to_string_lossy().to_string()),
            active_db_kind: Some(active_db.kind),
            session: Some(summary),
            rollout_path,
            skipped_reason: Some("rollout_path_not_found".to_string()),
            ..Default::default()
        });
    };
    let messages = crate::session_manager::providers::codex::load_messages(&path)
        .map_err(AppError::Message)?;

    Ok(CodexHistorySessionDetailOutcome {
        codex_home: codex_dir.to_string_lossy().to_string(),
        state_db_path: Some(active_db.path.to_string_lossy().to_string()),
        active_db_kind: Some(active_db.kind),
        session: Some(summary),
        messages,
        rollout_path,
        skipped_reason: None,
    })
}

#[derive(Debug, Clone)]
/// 当前 Codex Desktop 实际读取的 state DB 路径和来源类别。
struct ActiveCodexStateDb {
    path: PathBuf,
    kind: String,
}

#[derive(Debug, Clone)]
/// 历史修复运行时参数；测试可覆盖备份目录，生产路径始终使用真实备份根。
struct HistoryVisibilityRepairRuntimeOptions {
    dry_run: bool,
    count: usize,
    window_limit: usize,
    balance_recent_window: bool,
    max_per_project: usize,
    max_total: usize,
    source_filter: Option<String>,
    include_archived: bool,
    include_subagents: bool,
    selected_session_ids: BTreeSet<String>,
    backup_root_override: Option<PathBuf>,
}

#[derive(Debug, Clone)]
/// SQLite threads 表中参与历史可见性判断的字段快照。
struct ThreadHistoryRow {
    id: String,
    rollout_path: Option<String>,
    model_provider: Option<String>,
    cwd: Option<String>,
    has_user_event: Option<i64>,
    archived: Option<i64>,
    source: Option<String>,
    thread_source: Option<String>,
    title: Option<String>,
    preview: Option<String>,
    first_user_message: Option<String>,
    updated_at_ms: Option<i64>,
    updated_at: Option<i64>,
}

#[derive(Debug, Clone)]
/// 被选中置顶的项目线程，以及即将写入的新时间戳。
struct FocusThreadRow {
    id: String,
    title: String,
    project_path: Option<String>,
    rollout_path: Option<String>,
    new_updated_at: i64,
    new_updated_at_ms: i64,
    updated_iso: String,
}

#[derive(Debug, Clone)]
/// rollout JSONL 中 provider 元数据的延迟写入计划。
struct RolloutProviderUpdate {
    path: PathBuf,
    rewritten: Vec<String>,
    old_mtime_ms: Option<i64>,
}

#[derive(Debug, Clone, Copy, Default)]
/// session_index.jsonl 聚焦移动后的写入统计。
struct SessionIndexMoveCounts {
    rows_moved: usize,
    titles_updated: usize,
}

/// 实际执行历史修复；测试可以直接传入临时 Codex 目录和 state DB。
fn repair_codex_history_visibility_at(
    codex_dir: &Path,
    active_db: ActiveCodexStateDb,
    target_provider: &str,
    live_config_model_provider: Option<String>,
    source_provider_ids: BTreeSet<String>,
    normalized_project_path: Option<String>,
    runtime: HistoryVisibilityRepairRuntimeOptions,
) -> Result<CodexHistoryVisibilityRepairOutcome, AppError> {
    let mut conn = Connection::open(&active_db.path)
        .map_err(|e| AppError::Database(format!("打开 Codex active state DB 失败: {e}")))?;
    conn.busy_timeout(Duration::from_secs(5))
        .map_err(|e| AppError::Database(format!("设置 Codex state DB busy_timeout 失败: {e}")))?;
    if !Database::table_exists(&conn, "threads")?
        || !Database::has_column(&conn, "threads", "id")?
        || !Database::has_column(&conn, "threads", "model_provider")?
    {
        return Ok(CodexHistoryVisibilityRepairOutcome {
            dry_run: runtime.dry_run,
            codex_home: codex_dir.to_string_lossy().to_string(),
            state_db_path: Some(active_db.path.to_string_lossy().to_string()),
            active_db_kind: Some(active_db.kind),
            target_provider: target_provider.to_string(),
            live_config_model_provider,
            source_provider_ids: source_provider_ids.into_iter().collect(),
            skipped_reason: Some("threads_schema_missing_required_columns".to_string()),
            ..Default::default()
        });
    }

    let rows = load_codex_thread_history_rows(&conn)?;
    let source_provider_set: HashSet<String> = source_provider_ids.iter().cloned().collect();
    let provider_update_ids: Vec<String> = rows
        .iter()
        .filter(|row| {
            row.model_provider
                .as_deref()
                .map(|provider| source_provider_set.contains(provider))
                .unwrap_or(false)
        })
        .map(|row| row.id.clone())
        .collect();
    let provider_update_id_set: HashSet<String> = provider_update_ids.iter().cloned().collect();

    let mut rollout_provider_updates = Vec::new();
    for row in rows
        .iter()
        .filter(|row| provider_update_id_set.contains(&row.id))
    {
        if let Some(update) = prepare_rollout_provider_update(row, target_provider)? {
            rollout_provider_updates.push(update);
        }
    }

    let mut user_event_update_ids = Vec::new();
    let mut visible_rows = Vec::new();
    for row in &rows {
        let provider_after = if provider_update_id_set.contains(&row.id) {
            Some(target_provider)
        } else {
            row.model_provider.as_deref()
        };
        if provider_after != Some(target_provider) {
            continue;
        }
        if !runtime.include_archived && row.archived.unwrap_or(0) != 0 {
            continue;
        }
        if !source_matches_history_filter(row.source.as_deref(), runtime.source_filter.as_deref()) {
            continue;
        }
        if !runtime.include_subagents && !is_user_thread_source(row.thread_source.as_deref()) {
            continue;
        }
        let has_user_event_after = row.has_user_event.unwrap_or(0) == 1
            || rollout_contains_user_event(resolve_history_path(row.rollout_path.as_deref()));
        if row.has_user_event.unwrap_or(0) != 1 && has_user_event_after {
            user_event_update_ids.push(row.id.clone());
        }
        if has_user_event_after && row_has_visible_text(row) {
            visible_rows.push(row.clone());
        }
    }

    let index_path = codex_dir.join("session_index.jsonl");
    let global_state_path = codex_dir.join(".codex-global-state.json");
    let session_index_lines = read_jsonl_lines(&index_path)?;
    let session_index_ids = session_index_thread_ids(&session_index_lines);
    let missing_index_rows: Vec<ThreadHistoryRow> = visible_rows
        .iter()
        .filter(|row| !session_index_ids.contains(&row.id))
        .cloned()
        .collect();

    let mut project_rows = Vec::new();
    if let Some(project_path) = normalized_project_path.as_deref() {
        project_rows = visible_rows
            .iter()
            .filter(|row| {
                row.cwd
                    .as_deref()
                    .and_then(normalize_history_path)
                    .as_deref()
                    == Some(project_path)
            })
            .cloned()
            .collect();
        project_rows.sort_by(|left, right| {
            row_updated_ms(right)
                .cmp(&row_updated_ms(left))
                .then_with(|| right.id.cmp(&left.id))
        });
    }
    let mut selected_focus_rows: Vec<FocusThreadRow> = if !runtime.selected_session_ids.is_empty() {
        visible_rows
            .iter()
            .filter(|row| runtime.selected_session_ids.contains(&row.id))
            .map(focus_row_from_thread)
            .collect()
    } else if runtime.balance_recent_window {
        select_balanced_recent_window_rows(
            &visible_rows,
            normalized_project_path.as_deref(),
            runtime.count,
            runtime.max_per_project,
            runtime.max_total,
        )
    } else {
        project_rows
            .iter()
            .take(runtime.count)
            .map(focus_row_from_thread)
            .collect()
    };
    assign_focus_times(&mut selected_focus_rows);
    let balanced_recent_window_projects =
        if runtime.balance_recent_window && runtime.selected_session_ids.is_empty() {
            selected_focus_rows
                .iter()
                .filter_map(|row| row.project_path.as_deref())
                .collect::<BTreeSet<_>>()
                .len()
        } else {
            0
        };

    let visible_project_rows_in_window_before =
        if let Some(project_path) = normalized_project_path.as_deref() {
            let mut recent_rows = visible_rows.clone();
            recent_rows.sort_by(|left, right| {
                row_updated_ms(right)
                    .cmp(&row_updated_ms(left))
                    .then_with(|| right.id.cmp(&left.id))
            });
            recent_rows
                .into_iter()
                .take(runtime.window_limit)
                .filter(|row| {
                    row.cwd
                        .as_deref()
                        .and_then(normalize_history_path)
                        .as_deref()
                        == Some(project_path)
                })
                .count()
        } else {
            0
        };

    let global_state_value = read_json_object(&global_state_path)?;
    let global_plan = plan_global_state_repairs(
        &global_state_value,
        &visible_rows,
        &selected_focus_rows,
        normalized_project_path.as_deref(),
    );
    let rollout_mtimes_to_touch = selected_focus_rows
        .iter()
        .filter(|row| resolve_history_path(row.rollout_path.as_deref()).is_some())
        .count();
    let sqlite_focus_rows_to_update = selected_focus_rows.len();
    let session_index_rows_to_move = selected_focus_rows.len();
    let session_index_titles_to_update =
        count_session_index_title_updates(&session_index_lines, &selected_focus_rows);

    let mut outcome = CodexHistoryVisibilityRepairOutcome {
        dry_run: runtime.dry_run,
        codex_home: codex_dir.to_string_lossy().to_string(),
        state_db_path: Some(active_db.path.to_string_lossy().to_string()),
        active_db_kind: Some(active_db.kind.clone()),
        live_config_model_provider,
        target_provider: target_provider.to_string(),
        source_provider_ids: source_provider_ids.iter().cloned().collect(),
        sqlite_threads: rows.len(),
        provider_rows_to_update: provider_update_ids.len(),
        rollout_first_lines_to_update: rollout_provider_updates.len(),
        user_event_rows_to_update: user_event_update_ids.len(),
        visible_candidate_rows: visible_rows.len(),
        session_index_missing_to_append: missing_index_rows.len(),
        project_rows: project_rows.len(),
        focus_selected_count: selected_focus_rows.len(),
        balanced_recent_window_enabled: runtime.balance_recent_window
            && runtime.selected_session_ids.is_empty(),
        balanced_recent_window_rows: if runtime.balance_recent_window
            && runtime.selected_session_ids.is_empty()
        {
            selected_focus_rows.len()
        } else {
            0
        },
        balanced_recent_window_projects,
        max_per_project: runtime.max_per_project,
        max_total: runtime.max_total,
        source_filter: runtime.source_filter.clone(),
        sqlite_focus_rows_to_update,
        session_index_titles_to_update,
        session_index_rows_to_move,
        workspace_hints_to_fix: global_plan.workspace_hints_to_fix,
        projectless_ids_to_remove: global_plan.projectless_ids_to_remove,
        saved_workspace_roots_to_add: global_plan.saved_workspace_roots_to_add,
        rollout_mtimes_to_touch,
        visible_project_rows_in_window_before,
        ..Default::default()
    };

    if runtime.dry_run {
        return Ok(outcome);
    }

    let has_changes = outcome.provider_rows_to_update > 0
        || outcome.rollout_first_lines_to_update > 0
        || outcome.user_event_rows_to_update > 0
        || outcome.session_index_missing_to_append > 0
        || outcome.sqlite_focus_rows_to_update > 0
        || outcome.session_index_titles_to_update > 0
        || outcome.session_index_rows_to_move > 0
        || outcome.workspace_hints_to_fix > 0
        || outcome.projectless_ids_to_remove > 0
        || outcome.saved_workspace_roots_to_add > 0
        || outcome.rollout_mtimes_to_touch > 0;
    if !has_changes {
        outcome.skipped_reason = Some("already_repaired".to_string());
        return Ok(outcome);
    }

    let backup_root = runtime
        .backup_root_override
        .clone()
        .unwrap_or_else(|| migration_backup_root(CURRENT_DESKTOP_HISTORY_REPAIR_NAME));
    fs::create_dir_all(&backup_root).map_err(|e| AppError::io(&backup_root, e))?;
    backup_codex_state_db(&active_db.path, codex_dir, &backup_root, &conn)?;
    backup_visibility_file_if_exists(&index_path, codex_dir, &backup_root)?;
    backup_visibility_file_if_exists(&global_state_path, codex_dir, &backup_root)?;
    for update in &rollout_provider_updates {
        backup_codex_jsonl_file(&update.path, codex_dir, &backup_root)?;
    }

    let tx = conn
        .transaction()
        .map_err(|e| AppError::Database(format!("开启 Codex 历史修复事务失败: {e}")))?;
    outcome.provider_rows_updated =
        update_thread_provider_rows(&tx, target_provider, &provider_update_ids)?;
    outcome.user_event_rows_updated = update_thread_user_event_rows(&tx, &user_event_update_ids)?;
    outcome.sqlite_focus_rows_updated = update_thread_focus_times(&tx, &selected_focus_rows)?;
    tx.commit()
        .map_err(|e| AppError::Database(format!("提交 Codex 历史修复事务失败: {e}")))?;

    outcome.rollout_first_lines_updated = apply_rollout_provider_updates(rollout_provider_updates)?;
    outcome.session_index_appended =
        append_missing_session_index_rows(&index_path, &missing_index_rows)?;
    let session_index_counts = move_focus_session_index_rows(&index_path, &selected_focus_rows)?;
    outcome.session_index_rows_moved = session_index_counts.rows_moved;
    outcome.session_index_titles_updated = session_index_counts.titles_updated;
    let global_counts = apply_global_state_repairs(
        &global_state_path,
        global_state_value,
        &visible_rows,
        &selected_focus_rows,
        normalized_project_path.as_deref(),
    )?;
    outcome.workspace_hints_fixed = global_counts.workspace_hints_to_fix;
    outcome.projectless_ids_removed = global_counts.projectless_ids_to_remove;
    outcome.saved_workspace_roots_added = global_counts.saved_workspace_roots_to_add;
    outcome.rollout_mtimes_touched =
        touch_focus_rollout_mtimes(&selected_focus_rows, &backup_root)?;
    outcome.backup_dir = Some(backup_root.to_string_lossy().to_string());
    Ok(outcome)
}

/// 收集历史修复时需要并入当前自定义 provider 桶的来源 provider。
fn collect_history_visibility_source_provider_ids(
    db: &Database,
) -> Result<BTreeSet<String>, AppError> {
    let mut ids = collect_source_model_provider_ids(db)?;
    ids.extend(default_history_visibility_source_provider_ids());
    Ok(ids)
}

/// 返回当前历史修复认可的旧 provider 桶。
fn default_history_visibility_source_provider_ids() -> Vec<String> {
    [
        OFFICIAL_OPENAI_CODEX_MODEL_PROVIDER_ID,
        CC_SWITCH_CODEX_MODEL_PROVIDER_ID,
        CC_SWITCH_CODEX_ROUTER_MODEL_PROVIDER_ID,
        "custom",
        "cc_switch_codex_router",
        "codex_model_router",
        "codex_model_router_v2",
    ]
    .into_iter()
    .map(str::to_string)
    .collect()
}

/// 解析当前 Codex Desktop 真正使用的 state DB，优先新版 `sqlite/state_5.sqlite`。
fn resolve_active_codex_state_db(
    codex_dir: &Path,
    config_text: &str,
) -> Option<ActiveCodexStateDb> {
    let sqlite_default = codex_dir.join("sqlite").join(CODEX_STATE_DB_FILENAME);
    if sqlite_default.exists() {
        return Some(ActiveCodexStateDb {
            path: sqlite_default,
            kind: "sqlite_subdir".to_string(),
        });
    }
    if let Some(sqlite_home) = sqlite_home_from_codex_config(config_text) {
        let configured = sqlite_home.join(CODEX_STATE_DB_FILENAME);
        if configured.exists() {
            return Some(ActiveCodexStateDb {
                path: configured,
                kind: "configured_sqlite_home".to_string(),
            });
        }
    }
    let legacy = codex_dir.join(CODEX_STATE_DB_FILENAME);
    if legacy.exists() {
        return Some(ActiveCodexStateDb {
            path: legacy,
            kind: "legacy_root".to_string(),
        });
    }
    None
}

/// 解析 active state DB；独立工具传入显式路径时优先使用该路径。
fn resolve_active_codex_state_db_with_override(
    codex_dir: &Path,
    config_text: &str,
    explicit_path: Option<&str>,
) -> Result<Option<ActiveCodexStateDb>, AppError> {
    if let Some(raw_path) = explicit_path
        .map(str::trim)
        .filter(|value| !value.is_empty())
    {
        let path = resolve_user_path(&strip_long_path_prefix(raw_path));
        if !path.exists() {
            return Err(AppError::Message(format!(
                "指定的 Codex state DB 不存在: {}",
                path.display()
            )));
        }
        return Ok(Some(ActiveCodexStateDb {
            path,
            kind: "explicit".to_string(),
        }));
    }
    Ok(resolve_active_codex_state_db(codex_dir, config_text))
}

/// 读取 live config 顶层 model_provider，用于结果展示和误修排查。
fn current_codex_model_provider_from_config_text(config_text: &str) -> Option<String> {
    let doc = config_text.parse::<DocumentMut>().ok()?;
    doc.get("model_provider")
        .and_then(|item| item.as_str())
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(str::to_string)
}

/// 从 SQLite threads 表读取历史行，缺失的可选列会以 None 参与后续判断。
fn load_codex_thread_history_rows(conn: &Connection) -> Result<Vec<ThreadHistoryRow>, AppError> {
    let mut stmt = conn
        .prepare("SELECT * FROM threads")
        .map_err(|e| AppError::Database(format!("读取 Codex threads 表失败: {e}")))?;
    let columns: Vec<String> = stmt
        .column_names()
        .iter()
        .map(|name| (*name).to_string())
        .collect();
    let rows = stmt
        .query_map([], |row| thread_history_row_from_sql(row, &columns))
        .map_err(|e| AppError::Database(format!("查询 Codex threads 表失败: {e}")))?;
    let mut output = Vec::new();
    for row in rows {
        output
            .push(row.map_err(|e| AppError::Database(format!("解析 Codex threads 行失败: {e}")))?);
    }
    Ok(output)
}

/// 将 SQLite 历史行转换成前端可展示的会话摘要。
fn codex_history_summary_from_row(row: &ThreadHistoryRow) -> CodexHistorySessionSummary {
    let updated_at_ms = row_updated_ms(row);
    let title = row_title(row);
    let cwd = row.cwd.as_deref().and_then(normalize_history_path);
    let rollout_path = row.rollout_path.as_deref().map(strip_long_path_prefix);
    CodexHistorySessionSummary {
        id: row.id.clone(),
        title,
        cwd,
        model_provider: row.model_provider.clone(),
        source: row.source.clone(),
        thread_source: row.thread_source.clone(),
        archived: row.archived.unwrap_or(0) != 0,
        has_user_event: row.has_user_event.unwrap_or(0) == 1,
        updated_at_ms,
        updated_at: Some(iso_from_epoch_millis(updated_at_ms)),
        rollout_path,
    }
}

/// 统计 threads 表字段的实际取值分布，供 UI 构造下拉候选和说明文案。
fn history_value_counts<F>(rows: &[ThreadHistoryRow], mut read: F) -> Vec<CodexHistoryValueCount>
where
    F: FnMut(&ThreadHistoryRow) -> Option<String>,
{
    let mut counts: HashMap<Option<String>, usize> = HashMap::new();
    for row in rows {
        let value = read(row)
            .map(|value| value.trim().to_string())
            .filter(|value| !value.is_empty());
        *counts.entry(value).or_insert(0) += 1;
    }
    let mut output = counts
        .into_iter()
        .map(|(value, count)| CodexHistoryValueCount { value, count })
        .collect::<Vec<_>>();
    output.sort_by(|left, right| {
        right
            .count
            .cmp(&left.count)
            .then_with(|| left.value.cmp(&right.value))
    });
    output
}

/// 生成修复目标 provider 候选；live config 优先，其次 CC Switch custom 桶，再补 DB 现有桶。
fn codex_target_provider_candidates(
    live_config_model_provider: Option<&str>,
    rows: &[ThreadHistoryRow],
) -> Vec<String> {
    fn push_unique(output: &mut Vec<String>, value: Option<&str>) {
        let Some(value) = value.map(str::trim).filter(|value| !value.is_empty()) else {
            return;
        };
        if !output.iter().any(|item| item == value) {
            output.push(value.to_string());
        }
    }

    let mut output = Vec::new();
    push_unique(&mut output, live_config_model_provider);
    push_unique(&mut output, Some(CC_SWITCH_CODEX_ROUTER_MODEL_PROVIDER_ID));
    for row in rows {
        push_unique(&mut output, row.model_provider.as_deref());
    }
    output
}

/// 把动态 SQLite row 转成当前修复逻辑需要的 owned 结构。
fn thread_history_row_from_sql(
    row: &rusqlite::Row<'_>,
    columns: &[String],
) -> rusqlite::Result<ThreadHistoryRow> {
    Ok(ThreadHistoryRow {
        id: sqlite_optional_string(row, columns, "id")?.unwrap_or_default(),
        rollout_path: sqlite_optional_string(row, columns, "rollout_path")?,
        model_provider: sqlite_optional_string(row, columns, "model_provider")?,
        cwd: sqlite_optional_string(row, columns, "cwd")?,
        has_user_event: sqlite_optional_i64(row, columns, "has_user_event")?,
        archived: sqlite_optional_i64(row, columns, "archived")?,
        source: sqlite_optional_string(row, columns, "source")?,
        thread_source: sqlite_optional_string(row, columns, "thread_source")?,
        title: sqlite_optional_string(row, columns, "title")?,
        preview: sqlite_optional_string(row, columns, "preview")?,
        first_user_message: sqlite_optional_string(row, columns, "first_user_message")?,
        updated_at_ms: sqlite_optional_i64(row, columns, "updated_at_ms")?,
        updated_at: sqlite_optional_i64(row, columns, "updated_at")?,
    })
}

/// 按列名读取 SQLite 文本，兼容 NULL 和非文本类型。
fn sqlite_optional_string(
    row: &rusqlite::Row<'_>,
    columns: &[String],
    name: &str,
) -> rusqlite::Result<Option<String>> {
    let Some(index) = columns.iter().position(|column| column == name) else {
        return Ok(None);
    };
    match row.get_ref(index)? {
        rusqlite::types::ValueRef::Null => Ok(None),
        rusqlite::types::ValueRef::Text(value) => {
            Ok(Some(String::from_utf8_lossy(value).to_string()))
        }
        rusqlite::types::ValueRef::Integer(value) => Ok(Some(value.to_string())),
        rusqlite::types::ValueRef::Real(value) => Ok(Some(value.to_string())),
        rusqlite::types::ValueRef::Blob(value) => {
            Ok(Some(String::from_utf8_lossy(value).to_string()))
        }
    }
}

/// 按列名读取 SQLite 整数，兼容布尔、时间戳和文本数字。
fn sqlite_optional_i64(
    row: &rusqlite::Row<'_>,
    columns: &[String],
    name: &str,
) -> rusqlite::Result<Option<i64>> {
    let Some(index) = columns.iter().position(|column| column == name) else {
        return Ok(None);
    };
    match row.get_ref(index)? {
        rusqlite::types::ValueRef::Null => Ok(None),
        rusqlite::types::ValueRef::Integer(value) => Ok(Some(value)),
        rusqlite::types::ValueRef::Real(value) => Ok(Some(value as i64)),
        rusqlite::types::ValueRef::Text(value) => {
            Ok(String::from_utf8_lossy(value).parse::<i64>().ok())
        }
        rusqlite::types::ValueRef::Blob(_) => Ok(None),
    }
}

/// 判断 threads.source 是否属于正常用户可见历史来源。
fn is_interactive_history_source(source: Option<&str>) -> bool {
    matches!(source, Some("cli") | Some("vscode"))
}

/// 按用户指定的来源过滤历史；未指定时保持 Codex Desktop 常规可见来源。
fn source_matches_history_filter(source: Option<&str>, filter: Option<&str>) -> bool {
    match filter.map(str::trim).filter(|value| !value.is_empty()) {
        Some("all") | Some("*") => true,
        Some("interactive") => is_interactive_history_source(source),
        Some(expected) => source == Some(expected),
        None => is_interactive_history_source(source),
    }
}

/// 判断 thread_source 是否为用户主线程，避免默认把 subagent 历史顶上来。
fn is_user_thread_source(thread_source: Option<&str>) -> bool {
    matches!(thread_source, None | Some("") | Some("user"))
}

/// 判断一行历史是否有足够文本可在侧边栏展示。
fn row_has_visible_text(row: &ThreadHistoryRow) -> bool {
    [&row.preview, &row.first_user_message, &row.title]
        .into_iter()
        .flatten()
        .any(|value| !value.trim().is_empty())
}

/// 读取一行历史的更新时间毫秒值，缺失时回退到秒级字段。
fn row_updated_ms(row: &ThreadHistoryRow) -> i64 {
    row.updated_at_ms
        .or_else(|| row.updated_at.map(|value| value.saturating_mul(1000)))
        .unwrap_or(0)
}

/// 提取历史标题，优先使用 title，再回退到 preview/first_user_message。
fn row_title(row: &ThreadHistoryRow) -> String {
    [&row.title, &row.preview, &row.first_user_message]
        .into_iter()
        .flatten()
        .map(|value| value.trim())
        .find(|value| !value.is_empty())
        .unwrap_or("Untitled")
        .to_string()
}

/// 构造一个项目聚焦行，时间戳稍后统一分配。
fn focus_row_from_thread(row: &ThreadHistoryRow) -> FocusThreadRow {
    FocusThreadRow {
        id: row.id.clone(),
        title: row_title(row),
        project_path: row.cwd.as_deref().and_then(normalize_history_path),
        rollout_path: row.rollout_path.clone(),
        new_updated_at: row.updated_at.unwrap_or_default(),
        new_updated_at_ms: row_updated_ms(row),
        updated_iso: iso_from_epoch_millis(row_updated_ms(row)),
    }
}

/// 选择需要推入 Desktop recent 窗口的行：当前项目先保底，再按项目轮询补足全局窗口。
fn select_balanced_recent_window_rows(
    visible_rows: &[ThreadHistoryRow],
    project_path: Option<&str>,
    focus_count: usize,
    max_per_project: usize,
    max_total: usize,
) -> Vec<FocusThreadRow> {
    let mut sorted_rows: Vec<&ThreadHistoryRow> = visible_rows.iter().collect();
    sorted_rows.sort_by(|left, right| {
        row_updated_ms(right)
            .cmp(&row_updated_ms(left))
            .then_with(|| right.id.cmp(&left.id))
    });

    let mut selected_ids = HashSet::new();
    let mut selected = Vec::new();
    if let Some(project_path) = project_path {
        for row in sorted_rows.iter().copied().filter(|row| {
            row.cwd
                .as_deref()
                .and_then(normalize_history_path)
                .as_deref()
                == Some(project_path)
        }) {
            if selected.len() >= max_total || selected.len() >= focus_count {
                break;
            }
            selected_ids.insert(row.id.clone());
            selected.push(focus_row_from_thread(row));
        }
    }

    let mut buckets: BTreeMap<String, VecDeque<&ThreadHistoryRow>> = BTreeMap::new();
    for row in sorted_rows {
        if selected_ids.contains(&row.id) {
            continue;
        }
        let project_key = row
            .cwd
            .as_deref()
            .and_then(normalize_history_path)
            .unwrap_or_else(|| "(no cwd)".to_string());
        let bucket = buckets.entry(project_key).or_default();
        if bucket.len() < max_per_project {
            bucket.push_back(row);
        }
    }

    let mut ordered_projects: Vec<String> = buckets.keys().cloned().collect();
    ordered_projects.sort_by(|left, right| {
        let left_ms = buckets
            .get(left)
            .and_then(|rows| rows.front())
            .map(|row| row_updated_ms(row))
            .unwrap_or_default();
        let right_ms = buckets
            .get(right)
            .and_then(|rows| rows.front())
            .map(|row| row_updated_ms(row))
            .unwrap_or_default();
        right_ms.cmp(&left_ms).then_with(|| right.cmp(left))
    });

    while selected.len() < max_total && buckets.values().any(|bucket| !bucket.is_empty()) {
        let mut progressed = false;
        for project in &ordered_projects {
            if selected.len() >= max_total {
                break;
            }
            let Some(bucket) = buckets.get_mut(project) else {
                continue;
            };
            let Some(row) = bucket.pop_front() else {
                continue;
            };
            if selected_ids.insert(row.id.clone()) {
                selected.push(focus_row_from_thread(row));
            }
            progressed = true;
        }
        if !progressed {
            break;
        }
    }

    selected
}

/// 给聚焦行分配新的递减时间戳，使当前项目会话进入侧边栏可见窗口。
fn assign_focus_times(rows: &mut [FocusThreadRow]) {
    if rows.is_empty() {
        return;
    }
    let now_ms = Utc::now().timestamp_millis();
    let max_existing = rows
        .iter()
        .map(|row| row.new_updated_at_ms)
        .max()
        .unwrap_or_default();
    let base_ms = now_ms.max(max_existing).saturating_add(10_000);
    let total = rows.len() as i64;
    for (index, row) in rows.iter_mut().enumerate() {
        let next_ms = base_ms.saturating_add((total - index as i64) * 250);
        row.new_updated_at_ms = next_ms;
        row.new_updated_at = next_ms / 1000;
        row.updated_iso = iso_from_epoch_millis(next_ms);
    }
}

/// 转成 Codex session_index.jsonl 使用的 UTC 毫秒 ISO 字符串。
fn iso_from_epoch_millis(ms: i64) -> String {
    Utc.timestamp_millis_opt(ms)
        .single()
        .unwrap_or_else(Utc::now)
        .to_rfc3339_opts(SecondsFormat::Millis, true)
}

/// 去掉 Windows 长路径前缀，便于 Path 访问和 cwd 比较。
fn strip_long_path_prefix(value: &str) -> String {
    if let Some(rest) = value.strip_prefix(r"\\?\UNC\") {
        format!(r"\\{rest}")
    } else if let Some(rest) = value.strip_prefix(r"\\?\") {
        rest.to_string()
    } else {
        value.to_string()
    }
}

/// 规范化历史 cwd，只用于比较和写 workspace hint，不改变 SQLite 原始 cwd。
fn normalize_history_path(value: &str) -> Option<String> {
    let mut text = strip_long_path_prefix(value).trim().replace('/', r"\");
    while text.len() > 3 && text.ends_with('\\') {
        text.pop();
    }
    if text.is_empty() {
        return None;
    }
    if text.len() >= 2 && text.as_bytes()[1] == b':' {
        let drive = text[0..1].to_ascii_uppercase();
        text.replace_range(0..1, &drive);
    }
    Some(text)
}

/// 把可能带长路径前缀的 rollout_path 转成可访问路径。
fn resolve_history_path(value: Option<&str>) -> Option<PathBuf> {
    let path = PathBuf::from(strip_long_path_prefix(value?.trim()));
    path.exists().then_some(path)
}

/// 检查 rollout 文件是否包含真实用户消息，用于回填 has_user_event。
fn rollout_contains_user_event(path: Option<PathBuf>) -> bool {
    let Some(path) = path else {
        return false;
    };
    let Ok(content) = fs::read_to_string(path) else {
        return false;
    };
    content.contains("\"type\":\"user_message\"")
        || content.contains("\"role\":\"user\"")
        || content.contains("\"user_input\"")
}

/// 准备改写 rollout 第一行的 provider 元数据，并保留剩余内容。
fn prepare_rollout_provider_update(
    row: &ThreadHistoryRow,
    target_provider: &str,
) -> Result<Option<RolloutProviderUpdate>, AppError> {
    let Some(path) = resolve_history_path(row.rollout_path.as_deref()) else {
        return Ok(None);
    };
    let content = fs::read_to_string(&path).map_err(|e| AppError::io(&path, e))?;
    let old_mtime_ms = fs::metadata(&path)
        .ok()
        .and_then(|metadata| metadata.modified().ok())
        .and_then(|time| time.duration_since(UNIX_EPOCH).ok())
        .map(|duration| duration.as_millis() as i64);
    let mut changed = false;
    let mut rewritten = Vec::new();
    for line in content.lines() {
        let Ok(mut value) = serde_json::from_str::<Value>(line) else {
            rewritten.push(line.to_string());
            continue;
        };
        let is_session_meta = value.get("type").and_then(Value::as_str) == Some("session_meta");
        let Some(payload) = value.get_mut("payload").and_then(Value::as_object_mut) else {
            rewritten.push(line.to_string());
            continue;
        };
        if !is_session_meta && !payload.contains_key("model_provider") {
            rewritten.push(line.to_string());
            continue;
        }
        if payload.get("model_provider").and_then(Value::as_str) != Some(target_provider) {
            payload.insert(
                "model_provider".to_string(),
                Value::String(target_provider.to_string()),
            );
            changed = true;
        }
        rewritten.push(
            serde_json::to_string(&value).map_err(|e| AppError::JsonSerialize { source: e })?,
        );
    }
    if !changed {
        return Ok(None);
    }
    Ok(Some(RolloutProviderUpdate {
        path,
        rewritten,
        old_mtime_ms,
    }))
}

/// 拆出 JSONL 第一行和保留换行符的剩余文本。
/// 读取 JSONL 原始行，解析失败的行在写回时保持原样。
fn read_jsonl_lines(path: &Path) -> Result<Vec<String>, AppError> {
    if !path.exists() {
        return Ok(Vec::new());
    }
    Ok(fs::read_to_string(path)
        .map_err(|e| AppError::io(path, e))?
        .lines()
        .map(str::to_string)
        .collect())
}

/// 从 session_index 原始行里提取已有 thread id。
fn session_index_thread_ids(lines: &[String]) -> HashSet<String> {
    lines
        .iter()
        .filter_map(|line| serde_json::from_str::<Value>(line).ok())
        .filter_map(|value| value.get("id").and_then(Value::as_str).map(str::to_string))
        .collect()
}

/// 把缺失的可见历史追加到 session_index.jsonl。
/// 预估 session_index 中已有行的陈旧标题数量，缺失行由 append 统计单独覆盖。
fn count_session_index_title_updates(lines: &[String], selected: &[FocusThreadRow]) -> usize {
    if selected.is_empty() {
        return 0;
    }
    let selected_by_id: HashMap<&str, &FocusThreadRow> =
        selected.iter().map(|row| (row.id.as_str(), row)).collect();
    lines
        .iter()
        .filter_map(|line| serde_json::from_str::<Value>(line).ok())
        .filter(|value| {
            let Some(thread_id) = value.get("id").and_then(Value::as_str) else {
                return false;
            };
            let Some(selected_row) = selected_by_id.get(thread_id) else {
                return false;
            };
            let selected_title = selected_row.title.trim();
            if selected_title.is_empty() {
                return false;
            }
            value
                .get("thread_name")
                .and_then(Value::as_str)
                .unwrap_or_default()
                .trim()
                != selected_title
        })
        .count()
}

/// 把缺失的可见历史追加到 session_index.jsonl，避免前端只看索引时漏掉会话。
fn append_missing_session_index_rows(
    index_path: &Path,
    rows: &[ThreadHistoryRow],
) -> Result<usize, AppError> {
    if rows.is_empty() {
        return Ok(0);
    }
    let mut lines = read_jsonl_lines(index_path)?;
    let mut existing = session_index_thread_ids(&lines);
    let mut appended = 0;
    for row in rows {
        if !existing.insert(row.id.clone()) {
            continue;
        }
        let value = serde_json::json!({
            "id": row.id,
            "thread_name": row_title(row),
            "updated_at": iso_from_epoch_millis(row_updated_ms(row)),
        });
        lines.push(
            serde_json::to_string(&value).map_err(|e| AppError::JsonSerialize { source: e })?,
        );
        appended += 1;
    }
    write_jsonl_lines(index_path, &lines)?;
    Ok(appended)
}

/// 把聚焦项目的 session_index 行移动到文件尾部，并同步 updated_at。
fn move_focus_session_index_rows(
    index_path: &Path,
    selected: &[FocusThreadRow],
) -> Result<SessionIndexMoveCounts, AppError> {
    if selected.is_empty() {
        return Ok(SessionIndexMoveCounts::default());
    }
    let lines = read_jsonl_lines(index_path)?;
    let selected_by_id: HashMap<&str, &FocusThreadRow> =
        selected.iter().map(|row| (row.id.as_str(), row)).collect();
    let mut seen = HashSet::new();
    let mut kept = Vec::new();
    let mut moved = Vec::new();
    let mut titles_updated = 0;
    for line in lines {
        let mut maybe_value = serde_json::from_str::<Value>(&line).ok();
        let thread_id = maybe_value
            .as_ref()
            .and_then(|value| value.get("id"))
            .and_then(Value::as_str)
            .map(str::to_string);
        if let Some(thread_id) = thread_id.as_deref() {
            seen.insert(thread_id.to_string());
        }
        if let Some(selected_row) = thread_id
            .as_deref()
            .and_then(|thread_id| selected_by_id.get(thread_id))
        {
            if let Some(Value::Object(ref mut object)) = maybe_value {
                object.insert(
                    "updated_at".to_string(),
                    Value::String(selected_row.updated_iso.clone()),
                );
                let existing_title = object
                    .get("thread_name")
                    .and_then(Value::as_str)
                    .unwrap_or_default()
                    .trim();
                if !selected_row.title.trim().is_empty()
                    && existing_title != selected_row.title.trim()
                {
                    object.insert(
                        "thread_name".to_string(),
                        Value::String(selected_row.title.clone()),
                    );
                    titles_updated += 1;
                }
                moved.push(
                    serde_json::to_string(&Value::Object(object.clone()))
                        .map_err(|e| AppError::JsonSerialize { source: e })?,
                );
            }
        } else {
            kept.push(line);
        }
    }
    for selected_row in selected {
        if seen.contains(&selected_row.id) {
            continue;
        }
        let value = serde_json::json!({
            "id": selected_row.id,
            "thread_name": selected_row.title,
            "updated_at": selected_row.updated_iso,
        });
        moved.push(
            serde_json::to_string(&value).map_err(|e| AppError::JsonSerialize { source: e })?,
        );
    }
    let moved_count = moved.len();
    kept.extend(moved);
    write_jsonl_lines(index_path, &kept)?;
    Ok(SessionIndexMoveCounts {
        rows_moved: moved_count,
        titles_updated,
    })
}

/// 原子写回 JSONL 行。
fn write_jsonl_lines(path: &Path, lines: &[String]) -> Result<(), AppError> {
    let mut content = lines.join("\n");
    if !content.is_empty() {
        content.push('\n');
    }
    atomic_write(path, content.as_bytes())
}

#[derive(Debug, Clone, Default)]
/// `.codex-global-state.json` 修复项的预估或实际计数。
struct GlobalStateRepairCounts {
    workspace_hints_to_fix: usize,
    projectless_ids_to_remove: usize,
    saved_workspace_roots_to_add: usize,
}

/// 读取 Codex 全局状态；损坏或非对象时按空对象处理，避免阻断其他修复项。
fn read_json_object(path: &Path) -> Result<Value, AppError> {
    if !path.exists() {
        return Ok(Value::Object(serde_json::Map::new()));
    }
    let text = fs::read_to_string(path).map_err(|e| AppError::io(path, e))?;
    Ok(serde_json::from_str::<Value>(&text)
        .unwrap_or_else(|_| Value::Object(serde_json::Map::new())))
}

/// 预估全局状态需要修复的 workspace hints、projectless ids 和 saved roots。
fn plan_global_state_repairs(
    state: &Value,
    visible_rows: &[ThreadHistoryRow],
    selected: &[FocusThreadRow],
    project_path: Option<&str>,
) -> GlobalStateRepairCounts {
    let hints = state
        .get("thread-workspace-root-hints")
        .and_then(Value::as_object);
    let expected_hints = expected_workspace_hints(visible_rows, selected, project_path);
    let workspace_hints_to_fix = expected_hints
        .iter()
        .filter(|(thread_id, cwd)| {
            hints
                .and_then(|object| object.get(thread_id.as_str()))
                .and_then(Value::as_str)
                != Some(cwd.as_str())
        })
        .count();
    let expected_ids = visible_thread_ids(visible_rows, selected);
    let projectless_ids_to_remove = state
        .get("projectless-thread-ids")
        .and_then(Value::as_array)
        .map(|ids| {
            ids.iter()
                .filter_map(Value::as_str)
                .filter(|id| expected_ids.contains(*id))
                .count()
        })
        .unwrap_or_default();
    let saved_workspace_roots_to_add = project_path
        .filter(|project_path| {
            !state
                .get("electron-saved-workspace-roots")
                .and_then(Value::as_array)
                .map(|roots| {
                    roots
                        .iter()
                        .any(|root| root.as_str() == Some(*project_path))
                })
                .unwrap_or(false)
        })
        .map(|_| 1)
        .unwrap_or_default();
    GlobalStateRepairCounts {
        workspace_hints_to_fix,
        projectless_ids_to_remove,
        saved_workspace_roots_to_add,
    }
}

/// 应用全局状态修复并返回实际变更计数。
fn apply_global_state_repairs(
    global_state_path: &Path,
    mut state: Value,
    visible_rows: &[ThreadHistoryRow],
    selected: &[FocusThreadRow],
    project_path: Option<&str>,
) -> Result<GlobalStateRepairCounts, AppError> {
    if !state.is_object() {
        state = Value::Object(serde_json::Map::new());
    }
    let object = state.as_object_mut().expect("state is object");
    let hints_value = object
        .entry("thread-workspace-root-hints".to_string())
        .or_insert_with(|| Value::Object(serde_json::Map::new()));
    if !hints_value.is_object() {
        *hints_value = Value::Object(serde_json::Map::new());
    }
    let hints = hints_value.as_object_mut().expect("hints is object");
    let mut workspace_hints_to_fix = 0;
    for (thread_id, cwd) in expected_workspace_hints(visible_rows, selected, project_path) {
        if hints.get(&thread_id).and_then(Value::as_str) != Some(cwd.as_str()) {
            hints.insert(thread_id, Value::String(cwd));
            workspace_hints_to_fix += 1;
        }
    }

    let expected_ids = visible_thread_ids(visible_rows, selected);
    let mut projectless_ids_to_remove = 0;
    if let Some(Value::Array(ids)) = object.get_mut("projectless-thread-ids") {
        let mut retained = Vec::new();
        for id in std::mem::take(ids) {
            if id
                .as_str()
                .map(|text| expected_ids.contains(text))
                .unwrap_or(false)
            {
                projectless_ids_to_remove += 1;
            } else {
                retained.push(id);
            }
        }
        *ids = retained;
    }

    let mut saved_workspace_roots_to_add = 0;
    if let Some(project_path) = project_path {
        let roots_value = object
            .entry("electron-saved-workspace-roots".to_string())
            .or_insert_with(|| Value::Array(Vec::new()));
        if !roots_value.is_array() {
            *roots_value = Value::Array(Vec::new());
        }
        let roots = roots_value.as_array_mut().expect("roots is array");
        if !roots.iter().any(|root| root.as_str() == Some(project_path)) {
            roots.push(Value::String(project_path.to_string()));
            saved_workspace_roots_to_add = 1;
        }
    }

    if workspace_hints_to_fix > 0
        || projectless_ids_to_remove > 0
        || saved_workspace_roots_to_add > 0
    {
        let bytes =
            serde_json::to_vec_pretty(&state).map_err(|e| AppError::JsonSerialize { source: e })?;
        atomic_write(global_state_path, &bytes)?;
    }
    Ok(GlobalStateRepairCounts {
        workspace_hints_to_fix,
        projectless_ids_to_remove,
        saved_workspace_roots_to_add,
    })
}

/// 计算每个可见线程应写入的 workspace root hint。
fn expected_workspace_hints(
    visible_rows: &[ThreadHistoryRow],
    selected: &[FocusThreadRow],
    project_path: Option<&str>,
) -> std::collections::BTreeMap<String, String> {
    let mut hints = std::collections::BTreeMap::new();
    for row in visible_rows {
        if let Some(cwd) = row.cwd.as_deref().and_then(normalize_history_path) {
            hints.insert(row.id.clone(), cwd);
        }
    }
    if let Some(project_path) = project_path {
        for row in selected {
            hints.insert(row.id.clone(), project_path.to_string());
        }
    }
    hints
}

/// 汇总可从 projectless-thread-ids 移出的线程 id。
fn visible_thread_ids(
    visible_rows: &[ThreadHistoryRow],
    selected: &[FocusThreadRow],
) -> HashSet<String> {
    let mut ids: HashSet<String> = visible_rows.iter().map(|row| row.id.clone()).collect();
    ids.extend(selected.iter().map(|row| row.id.clone()));
    ids
}

/// 批量更新 provider 桶。
fn update_thread_provider_rows(
    tx: &rusqlite::Transaction<'_>,
    target_provider: &str,
    ids: &[String],
) -> Result<usize, AppError> {
    update_thread_rows_by_ids(
        tx,
        ids,
        |placeholders| {
            format!("UPDATE threads SET model_provider = ? WHERE id IN ({placeholders})")
        },
        |values| {
            values.push(target_provider.to_string());
        },
    )
}

/// 批量回填 has_user_event。
fn update_thread_user_event_rows(
    tx: &rusqlite::Transaction<'_>,
    ids: &[String],
) -> Result<usize, AppError> {
    update_thread_rows_by_ids(
        tx,
        ids,
        |placeholders| {
            format!("UPDATE threads SET has_user_event = 1 WHERE id IN ({placeholders})")
        },
        |_| {},
    )
}

/// 更新项目聚焦行的 SQLite 时间戳。
fn update_thread_focus_times(
    tx: &rusqlite::Transaction<'_>,
    selected: &[FocusThreadRow],
) -> Result<usize, AppError> {
    let mut changed = 0;
    for row in selected {
        changed += tx
            .execute(
                "UPDATE threads SET updated_at = ?1, updated_at_ms = ?2 WHERE id = ?3",
                (&row.new_updated_at, &row.new_updated_at_ms, &row.id),
            )
            .map_err(|e| AppError::Database(format!("更新 Codex 历史聚焦时间失败: {e}")))?;
    }
    Ok(changed)
}

/// 按 id 分块执行更新，避免 SQLite 参数数量上限。
fn update_thread_rows_by_ids(
    tx: &rusqlite::Transaction<'_>,
    ids: &[String],
    sql_for_placeholders: impl Fn(&str) -> String,
    seed_values: impl Fn(&mut Vec<String>),
) -> Result<usize, AppError> {
    if ids.is_empty() {
        return Ok(0);
    }
    let mut changed = 0;
    for chunk in ids.chunks(STATE_DB_ID_CHUNK) {
        let placeholders = placeholders(chunk.len());
        let sql = sql_for_placeholders(&placeholders);
        let mut values = Vec::new();
        seed_values(&mut values);
        values.extend(chunk.iter().cloned());
        changed += tx
            .execute(&sql, params_from_iter(values.iter()))
            .map_err(|e| AppError::Database(format!("更新 Codex threads 行失败: {e}")))?;
    }
    Ok(changed)
}

/// 应用 rollout 第一行 provider 元数据更新。
fn apply_rollout_provider_updates(updates: Vec<RolloutProviderUpdate>) -> Result<usize, AppError> {
    let mut changed = 0;
    for update in updates {
        write_jsonl_lines(&update.path, &update.rewritten)?;
        if let Some(old_mtime_ms) = update.old_mtime_ms {
            set_file_modified_time(&update.path, old_mtime_ms)?;
        }
        changed += 1;
    }
    Ok(changed)
}

/// 备份全局状态、索引等辅助文件。
fn backup_visibility_file_if_exists(
    path: &Path,
    codex_dir: &Path,
    backup_root: &Path,
) -> Result<(), AppError> {
    if path.exists() {
        let backup_path = backup_root
            .join("support")
            .join(relative_backup_path(path, codex_dir));
        copy_existing_file(path, &backup_path)?;
    }
    Ok(())
}

/// 触碰聚焦 rollout 文件 mtime，并写入恢复用 manifest。
fn touch_focus_rollout_mtimes(
    selected: &[FocusThreadRow],
    backup_root: &Path,
) -> Result<usize, AppError> {
    if selected.is_empty() {
        return Ok(0);
    }
    let manifest_path = backup_root.join("rollout-mtime-before.jsonl");
    let mut manifest_lines = Vec::new();
    let mut touched = 0;
    for row in selected {
        let Some(path) = resolve_history_path(row.rollout_path.as_deref()) else {
            continue;
        };
        let metadata = fs::metadata(&path).map_err(|e| AppError::io(&path, e))?;
        let old_mtime_ms = metadata
            .modified()
            .ok()
            .and_then(|time| time.duration_since(UNIX_EPOCH).ok())
            .map(|duration| duration.as_millis() as i64)
            .unwrap_or_default();
        let manifest = serde_json::json!({
            "id": row.id,
            "path": path.to_string_lossy(),
            "old_mtime_ms": old_mtime_ms,
            "new_mtime_ms": row.new_updated_at_ms,
        });
        manifest_lines.push(
            serde_json::to_string(&manifest).map_err(|e| AppError::JsonSerialize { source: e })?,
        );
        set_file_modified_time(&path, row.new_updated_at_ms)?;
        touched += 1;
    }
    if !manifest_lines.is_empty() {
        write_jsonl_lines(&manifest_path, &manifest_lines)?;
    }
    Ok(touched)
}

/// 设置文件 mtime；当前用户场景是 Windows，其他平台使用 filetime crate 同样可工作。
fn set_file_modified_time(path: &Path, epoch_millis: i64) -> Result<(), AppError> {
    let file_time = filetime::FileTime::from_unix_time(
        epoch_millis / 1000,
        ((epoch_millis % 1000) * 1_000_000) as u32,
    );
    filetime::set_file_mtime(path, file_time).map_err(|e| AppError::io(path, e))
}

pub fn maybe_migrate_codex_provider_template_bucket(
    db: &Database,
) -> Result<CodexProviderTemplateBucketMigrationOutcome, AppError> {
    if crate::settings::is_codex_provider_template_migrated() {
        return Ok(CodexProviderTemplateBucketMigrationOutcome {
            skipped_reason: Some("already_migrated".to_string()),
            ..Default::default()
        });
    }

    let backup_root = migration_backup_root(MIGRATION_NAME);
    let outcome = migrate_codex_provider_templates_to_custom(db, &backup_root)?;
    crate::settings::mark_codex_provider_template_migrated(CodexProviderTemplateMigration {
        completed_at: Utc::now().to_rfc3339(),
        migrated_provider_ids: outcome.migrated_provider_ids.clone(),
    })?;

    Ok(outcome)
}

/// 统一会话开关的存量迁移：把官方会话（内建 "openai" 桶）迁入共享 "custom" 桶。
///
/// 仅当用户在开启弹窗里勾选了"迁入既有官方会话"（`unify_codex_migrate_existing`）
/// 且本轮未完成时执行；开关关闭时标记与勾选意愿都会被清除（见 `save_settings`），
/// 重新开启并再次勾选即可补迁关闭期间产生的官方会话。
/// custom 桶里官方与第三方会话无法区分，自动逻辑绝不反向搬回；
/// 用户可在关闭开关时选择按备份账本精确还原（见 `restore_codex_official_history_from_backups`）。
/// 迁移前 jsonl / state DB 均备份到 `~/.cc-switch/backups/codex-official-history-unify-v1/`。
pub fn maybe_migrate_codex_official_history_to_unified_bucket(
) -> Result<CodexHistoryProviderBucketMigrationOutcome, AppError> {
    if !crate::settings::unify_codex_session_history() {
        return Ok(CodexHistoryProviderBucketMigrationOutcome {
            skipped_reason: Some("unify_toggle_off".to_string()),
            ..Default::default()
        });
    }
    if !crate::settings::unify_codex_migrate_existing_requested() {
        return Ok(CodexHistoryProviderBucketMigrationOutcome {
            skipped_reason: Some("stock_migration_not_requested".to_string()),
            ..Default::default()
        });
    }
    let _op_guard = lock_codex_official_history_op();
    let codex_dir = get_codex_config_dir();
    // marker 绑定迁移时的 Codex 目录：切换 codex_config_dir 后旧 marker 不再
    // 挡住新目录的迁移（迁移幂等，重跑无害）。
    let codex_dir_key = canonical_dir_string(&codex_dir);
    if crate::settings::is_codex_official_history_unify_migrated_for_dir(&codex_dir_key) {
        return Ok(CodexHistoryProviderBucketMigrationOutcome {
            skipped_reason: Some("already_migrated".to_string()),
            ..Default::default()
        });
    }
    // live 必须已实际路由到共享 custom 桶才允许迁移：官方配置的注入可能被拒
    // （已有显式 model_provider / 形态冲突的 custom 表，见
    // `inject_codex_unified_session_bucket`），代理接管期间的 live 也不带统一
    // 路由（注入只进备份）。这些状态下新会话仍落 "openai" 桶，迁移只会把
    // 历史搬进当前 live 看不见的桶里。开关与迁移意愿保持不动，待 live 真正
    // 统一后（下次切换 / 接管释放后的启动重试）再迁。
    if !codex_config_text_routes_custom(&read_codex_config_text().unwrap_or_default()) {
        return Ok(CodexHistoryProviderBucketMigrationOutcome {
            skipped_reason: Some("live_not_unified".to_string()),
            ..Default::default()
        });
    }

    let source_provider_ids: BTreeSet<String> =
        std::iter::once(OFFICIAL_OPENAI_CODEX_MODEL_PROVIDER_ID.to_string()).collect();
    let backup_root = migration_backup_root(OFFICIAL_UNIFY_MIGRATION_NAME);
    let migrated_jsonl_files =
        migrate_codex_jsonl_files(&codex_dir, &source_provider_ids, &backup_root)?;
    let migrated_state_rows =
        migrate_codex_state_dbs(&codex_dir, &source_provider_ids, &backup_root)?;
    // 备份代际记录来源目录，restore 据此只取当前目录的账本。
    write_backup_generation_meta(&backup_root, &codex_dir_key)?;

    let outcome = CodexHistoryProviderBucketMigrationOutcome {
        source_provider_ids: source_provider_ids.into_iter().collect(),
        migrated_jsonl_files,
        migrated_state_rows,
        skipped_reason: None,
    };

    // 条件写入在 settings 写锁内原子完成："迁移期间开关被关掉"时不写完成标记，
    // 避免下一次开启被标记挡住而漏迁"关闭期间"新产生的 openai 桶会话。
    // 与关闭路径（update_settings + 清标记）共用同一把锁，无检查-写入窗口。
    let marker_written = crate::settings::mark_codex_official_history_unify_migrated_if_enabled(
        CodexOfficialHistoryUnifyMigration {
            completed_at: Utc::now().to_rfc3339(),
            target_provider_id: CC_SWITCH_CODEX_MODEL_PROVIDER_ID.to_string(),
            migrated_jsonl_files,
            migrated_state_rows,
            codex_config_dir: Some(codex_dir_key),
        },
    )?;
    if !marker_written {
        return Ok(CodexHistoryProviderBucketMigrationOutcome {
            skipped_reason: Some("toggle_disabled_during_migration".to_string()),
            ..outcome
        });
    }

    Ok(outcome)
}

/// live config.toml 是否路由到共享 custom 桶（会话分桶只看这个实态：
/// base_url / 接管与否都不影响 session_meta 记录的 model_provider）。
fn codex_config_text_routes_custom(config_text: &str) -> bool {
    config_text
        .parse::<DocumentMut>()
        .ok()
        .and_then(|doc| {
            doc.get("model_provider")
                .and_then(|item| item.as_str())
                .map(|id| id.trim() == CC_SWITCH_CODEX_MODEL_PROVIDER_ID)
        })
        .unwrap_or(false)
}

/// 目录的规范化字符串形式，用作 marker / 备份代际的目录身份。
/// canonicalize 失败（目录尚不存在等）时退回原始路径字符串。
fn canonical_dir_string(dir: &Path) -> String {
    fs::canonicalize(dir)
        .unwrap_or_else(|_| dir.to_path_buf())
        .to_string_lossy()
        .to_string()
}

/// 在备份代际根目录写入 meta.json，记录这批备份来自哪个 Codex 目录。
/// 代际目录不存在（本轮没有任何文件被迁移）时跳过。
fn write_backup_generation_meta(backup_root: &Path, codex_dir_key: &str) -> Result<(), AppError> {
    if !backup_root.exists() {
        return Ok(());
    }
    let payload = serde_json::json!({ "codexConfigDir": codex_dir_key });
    let bytes =
        serde_json::to_vec_pretty(&payload).map_err(|e| AppError::JsonSerialize { source: e })?;
    atomic_write(&backup_root.join("meta.json"), &bytes)
}

#[derive(Debug, Clone, Default)]
pub struct CodexOfficialHistoryRestoreOutcome {
    pub restored_jsonl_files: usize,
    pub restored_state_rows: usize,
    pub skipped_reason: Option<String>,
}

/// 统一会话开关迁移备份的父目录（其下每次迁移一个时间戳代际目录）。
fn official_history_unify_backup_parent() -> PathBuf {
    get_app_config_dir()
        .join("backups")
        .join(OFFICIAL_UNIFY_MIGRATION_NAME)
}

/// 是否存在可用于还原的迁移备份（给前端决定要不要显示"恢复备份"勾选）。
/// 与 restore 的账本收集共用同一目录匹配口径：只认属于当前 Codex 目录的
/// 代际，避免切换 codex_config_dir 后弹出注定空跑的勾选。
/// 精确账本内容仍在真正还原时才解析。
pub fn has_codex_official_history_unify_backup() -> bool {
    has_official_history_unify_backup_for_dir(
        &official_history_unify_backup_parent(),
        &canonical_dir_string(&get_codex_config_dir()),
    )
}

fn has_official_history_unify_backup_for_dir(ledger_parent: &Path, codex_dir_key: &str) -> bool {
    let Ok(entries) = fs::read_dir(ledger_parent) else {
        return false;
    };
    entries.flatten().any(|entry| {
        let generation = entry.path();
        generation.is_dir() && backup_generation_matches_dir(&generation, codex_dir_key)
    })
}

/// 关闭统一会话开关时的可选还原：按迁移备份账本，把当时迁入共享 custom 桶的
/// 官方会话精确翻回 "openai" 桶。
///
/// 备份是唯一可信的归属证据：备份里 model_provider=="openai" 的会话必定源自
/// 官方桶。开启期间新产生的会话不在任何备份里，**永不触碰**——它们可能来自
/// 第三方，方向无法判定（产品决策：宁可留在第三方历史）。
/// 扫描全部备份代际取并集，多次开关循环后仍能还原早期迁入的会话；
/// 还原前改动目标先备份到独立的 restore 目录（保持迁移账本目录纯净），
/// 且只改写当前仍为 custom 的目标，重复执行无害。
pub fn restore_codex_official_history_from_backups(
) -> Result<CodexOfficialHistoryRestoreOutcome, AppError> {
    let _op_guard = lock_codex_official_history_op();
    // 开关已（重新）开启时拒绝还原：live 正路由 custom，把账本会话翻回
    // openai 桶等于亲手制造分裂。覆盖"关闭保存成功后用户立刻重新开启，
    // 还原排在重开迁移之后才拿到 op lock"的时序。
    if crate::settings::unify_codex_session_history() {
        return Ok(CodexOfficialHistoryRestoreOutcome {
            skipped_reason: Some("unify_toggle_on".to_string()),
            ..Default::default()
        });
    }
    let config_text = read_codex_config_text().unwrap_or_default();
    restore_codex_official_history_inner(
        &get_codex_config_dir(),
        &official_history_unify_backup_parent(),
        &migration_backup_root(OFFICIAL_UNIFY_RESTORE_BACKUP_NAME),
        &config_text,
    )
}

fn restore_codex_official_history_inner(
    codex_dir: &Path,
    ledger_parent: &Path,
    restore_backup_root: &Path,
    config_text: &str,
) -> Result<CodexOfficialHistoryRestoreOutcome, AppError> {
    let codex_dir_key = canonical_dir_string(codex_dir);
    let (official_session_ids, official_thread_ids) =
        collect_official_ledger(ledger_parent, &codex_dir_key)?;
    if official_session_ids.is_empty() && official_thread_ids.is_empty() {
        return Ok(CodexOfficialHistoryRestoreOutcome {
            skipped_reason: Some("no_backup_ledger".to_string()),
            ..Default::default()
        });
    }

    let mut files = Vec::new();
    collect_jsonl_files(&codex_dir.join("sessions"), &mut files, 0, 8);
    collect_jsonl_files(&codex_dir.join("archived_sessions"), &mut files, 0, 4);
    let mut restored_jsonl_files = 0;
    for file_path in files {
        if rewrite_codex_session_file_lines(&file_path, codex_dir, restore_backup_root, |line| {
            rewrite_codex_session_meta_line_for_restore(line, &official_session_ids)
        })? {
            restored_jsonl_files += 1;
        }
    }

    let mut restored_state_rows = 0;
    for db_path in codex_state_db_paths(codex_dir, config_text) {
        restored_state_rows += restore_codex_state_db_official_threads(
            &db_path,
            codex_dir,
            &official_thread_ids,
            restore_backup_root,
        )?;
    }

    if restored_jsonl_files == 0 && restored_state_rows == 0 {
        // 账本非空但没有任何"当前仍为 custom"的目标（如重复还原）：
        // 以 reason 告知前端，避免误报"已还原 0 项"为成功。
        return Ok(CodexOfficialHistoryRestoreOutcome {
            skipped_reason: Some("nothing_to_restore".to_string()),
            ..Default::default()
        });
    }

    Ok(CodexOfficialHistoryRestoreOutcome {
        restored_jsonl_files,
        restored_state_rows,
        skipped_reason: None,
    })
}

/// 从备份代际收集官方会话账本：jsonl 备份里 session_meta 为 "openai" 的
/// 会话 id + state DB 备份里 model_provider 为 "openai" 的 thread id。
/// 只采纳 meta.json 目录与当前 Codex 目录一致的代际，避免切换
/// codex_config_dir 后拿旧目录的账本作用到新目录。
/// 还原操作自身的备份（restore 目录）天然不会混入：那些副本里的 id 都是
/// custom，解析后贡献为空。
fn collect_official_ledger(
    ledger_parent: &Path,
    codex_dir_key: &str,
) -> Result<(HashSet<String>, BTreeSet<String>), AppError> {
    let mut session_ids = HashSet::new();
    let mut thread_ids = BTreeSet::new();
    let entries = match fs::read_dir(ledger_parent) {
        Ok(entries) => entries,
        Err(_) => return Ok((session_ids, thread_ids)),
    };
    for entry in entries.flatten() {
        let generation = entry.path();
        if !generation.is_dir() {
            continue;
        }
        if !backup_generation_matches_dir(&generation, codex_dir_key) {
            continue;
        }
        let mut backup_files = Vec::new();
        collect_jsonl_files(&generation.join("jsonl"), &mut backup_files, 0, 10);
        for backup_file in backup_files {
            collect_official_session_ids_from_backup(&backup_file, &mut session_ids);
        }
        let mut backup_dbs = Vec::new();
        collect_files_with_extension(&generation.join("state"), "sqlite", &mut backup_dbs, 0, 4);
        for backup_db in backup_dbs {
            collect_official_thread_ids_from_backup(&backup_db, &mut thread_ids);
        }
    }
    Ok((session_ids, thread_ids))
}

/// 备份代际是否属于指定 Codex 目录。无 meta.json 或解析失败时宽容接受：
/// 早期版本的备份没有 meta，而那个时期不存在切目录场景；误纳的代价也被
/// "按会话 id 精确匹配 + 仅改写 custom"双重条件兜底。
fn backup_generation_matches_dir(generation: &Path, codex_dir_key: &str) -> bool {
    let Ok(text) = fs::read_to_string(generation.join("meta.json")) else {
        return true;
    };
    serde_json::from_str::<Value>(&text)
        .ok()
        .and_then(|value| {
            value
                .get("codexConfigDir")
                .and_then(Value::as_str)
                .map(|dir| dir == codex_dir_key)
        })
        .unwrap_or(true)
}

fn collect_official_session_ids_from_backup(path: &Path, session_ids: &mut HashSet<String>) {
    let Ok(content) = fs::read_to_string(path) else {
        log::debug!("Failed to read unify backup file {}", path.display());
        return;
    };
    for line in content.lines() {
        if !line.contains("\"session_meta\"") || !line.contains("\"model_provider\"") {
            continue;
        }
        let Ok(value) = serde_json::from_str::<Value>(line) else {
            continue;
        };
        if value.get("type").and_then(Value::as_str) != Some("session_meta") {
            continue;
        }
        let Some(payload) = value.get("payload") else {
            continue;
        };
        if payload.get("model_provider").and_then(Value::as_str)
            != Some(OFFICIAL_OPENAI_CODEX_MODEL_PROVIDER_ID)
        {
            continue;
        }
        if let Some(session_id) = payload.get("id").and_then(Value::as_str) {
            session_ids.insert(session_id.to_string());
        }
    }
}

fn collect_official_thread_ids_from_backup(db_path: &Path, thread_ids: &mut BTreeSet<String>) {
    let conn =
        match Connection::open_with_flags(db_path, rusqlite::OpenFlags::SQLITE_OPEN_READ_ONLY) {
            Ok(conn) => conn,
            Err(err) => {
                log::debug!(
                    "Failed to open unify backup state DB {}: {err}",
                    db_path.display()
                );
                return;
            }
        };
    let has_threads = Database::table_exists(&conn, "threads").unwrap_or(false)
        && Database::has_column(&conn, "threads", "model_provider").unwrap_or(false);
    if !has_threads {
        return;
    }
    let Ok(mut stmt) = conn.prepare("SELECT id FROM threads WHERE model_provider = ?1") else {
        return;
    };
    let Ok(rows) = stmt.query_map([OFFICIAL_OPENAI_CODEX_MODEL_PROVIDER_ID], |row| {
        row.get::<_, String>(0)
    }) else {
        return;
    };
    for thread_id in rows.flatten() {
        thread_ids.insert(thread_id);
    }
}

fn collect_files_with_extension(
    dir: &Path,
    extension: &str,
    files: &mut Vec<PathBuf>,
    depth: u8,
    max_depth: u8,
) {
    if depth > max_depth || !dir.is_dir() {
        return;
    }
    let Ok(entries) = fs::read_dir(dir) else {
        return;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        if path.is_dir() {
            collect_files_with_extension(&path, extension, files, depth + 1, max_depth);
        } else if path.extension().and_then(|ext| ext.to_str()) == Some(extension) {
            files.push(path);
        }
    }
}

fn rewrite_codex_session_meta_line_for_restore(
    line: &str,
    official_session_ids: &HashSet<String>,
) -> Option<String> {
    if !line.contains("\"session_meta\"") || !line.contains("\"model_provider\"") {
        return None;
    }
    let mut value: Value = serde_json::from_str(line).ok()?;
    if value.get("type").and_then(Value::as_str) != Some("session_meta") {
        return None;
    }
    let payload = value.get_mut("payload")?.as_object_mut()?;
    if payload.get("model_provider")?.as_str()? != CC_SWITCH_CODEX_MODEL_PROVIDER_ID {
        return None;
    }
    let session_id = payload.get("id")?.as_str()?;
    if !official_session_ids.contains(session_id) {
        return None;
    }
    payload.insert(
        "model_provider".to_string(),
        Value::String(OFFICIAL_OPENAI_CODEX_MODEL_PROVIDER_ID.to_string()),
    );
    serde_json::to_string(&value).ok()
}

fn restore_codex_state_db_official_threads(
    db_path: &Path,
    codex_dir: &Path,
    official_thread_ids: &BTreeSet<String>,
    backup_root: &Path,
) -> Result<usize, AppError> {
    if !db_path.exists() || official_thread_ids.is_empty() {
        return Ok(0);
    }

    let mut conn = Connection::open(db_path)
        .map_err(|e| AppError::Database(format!("打开 Codex state DB 失败: {e}")))?;
    conn.busy_timeout(Duration::from_secs(5))
        .map_err(|e| AppError::Database(format!("设置 Codex state DB busy_timeout 失败: {e}")))?;

    if !Database::table_exists(&conn, "threads")?
        || !Database::has_column(&conn, "threads", "model_provider")?
    {
        return Ok(0);
    }

    let ids: Vec<&String> = official_thread_ids.iter().collect();
    let mut matching_rows: i64 = 0;
    for chunk in ids.chunks(STATE_DB_ID_CHUNK) {
        let placeholders = placeholders(chunk.len());
        let count_sql = format!(
            "SELECT COUNT(*) FROM threads WHERE model_provider = ? AND id IN ({placeholders})"
        );
        let mut values = Vec::with_capacity(chunk.len() + 1);
        values.push(CC_SWITCH_CODEX_MODEL_PROVIDER_ID.to_string());
        values.extend(chunk.iter().map(|id| (*id).clone()));
        let count: i64 = conn
            .query_row(&count_sql, params_from_iter(values.iter()), |row| {
                row.get(0)
            })
            .map_err(|e| AppError::Database(format!("统计 Codex state DB 待还原行失败: {e}")))?;
        matching_rows += count;
    }
    if matching_rows == 0 {
        return Ok(0);
    }

    backup_codex_state_db(db_path, codex_dir, backup_root, &conn)?;

    let tx = conn
        .transaction()
        .map_err(|e| AppError::Database(format!("开启 Codex state DB 还原事务失败: {e}")))?;
    let mut changed = 0;
    for chunk in ids.chunks(STATE_DB_ID_CHUNK) {
        let placeholders = placeholders(chunk.len());
        let update_sql = format!(
            "UPDATE threads SET model_provider = ? WHERE model_provider = ? AND id IN ({placeholders})"
        );
        let mut values = Vec::with_capacity(chunk.len() + 2);
        values.push(OFFICIAL_OPENAI_CODEX_MODEL_PROVIDER_ID.to_string());
        values.push(CC_SWITCH_CODEX_MODEL_PROVIDER_ID.to_string());
        values.extend(chunk.iter().map(|id| (*id).clone()));
        changed += tx
            .execute(&update_sql, params_from_iter(values.iter()))
            .map_err(|e| AppError::Database(format!("还原 Codex state DB provider 失败: {e}")))?;
    }
    tx.commit()
        .map_err(|e| AppError::Database(format!("提交 Codex state DB 还原事务失败: {e}")))?;
    Ok(changed)
}

fn migrate_codex_provider_templates_to_custom(
    db: &Database,
    backup_root: &Path,
) -> Result<CodexProviderTemplateBucketMigrationOutcome, AppError> {
    let providers = db.get_all_providers("codex")?;
    let mut migrated_provider_ids = Vec::new();

    for (_, provider) in providers {
        if provider.category.as_deref() == Some("official")
            || is_official_seed_id(&provider.id)
            || provider.is_codex_oauth()
        {
            continue;
        }

        let Some(config_text) = provider
            .settings_config
            .get("config")
            .and_then(|value| value.as_str())
        else {
            continue;
        };

        let Some(migrated_config_text) = migrate_provider_config_template_to_custom(config_text)?
        else {
            continue;
        };

        let mut settings = provider.settings_config.clone();
        let Some(obj) = settings.as_object_mut() else {
            log::warn!(
                "Skipping Codex provider template migration for {}: settings_config is not an object",
                provider.id
            );
            continue;
        };
        backup_provider_settings_config(&provider.id, &provider.settings_config, backup_root)?;
        obj.insert("config".to_string(), Value::String(migrated_config_text));
        db.update_provider_settings_config("codex", &provider.id, &settings)?;
        migrated_provider_ids.push(provider.id);
    }

    Ok(CodexProviderTemplateBucketMigrationOutcome {
        migrated_provider_ids,
        skipped_reason: None,
    })
}

fn collect_source_model_provider_ids(db: &Database) -> Result<BTreeSet<String>, AppError> {
    let providers = db.get_all_providers("codex")?;
    let mut ids = BTreeSet::new();

    for provider in providers.values() {
        if provider.category.as_deref() == Some("official")
            || is_official_seed_id(&provider.id)
            || provider.is_codex_oauth()
        {
            continue;
        }

        insert_known_cc_switch_legacy_source_id(&mut ids, &provider.id);

        let Some(config_text) = provider
            .settings_config
            .get("config")
            .and_then(|value| value.as_str())
        else {
            continue;
        };

        for provider_id in trusted_legacy_codex_model_provider_ids_from_config(config_text) {
            insert_known_cc_switch_legacy_source_id(&mut ids, &provider_id);
        }
        if let Some(provider_id) =
            legacy_codex_model_provider_id_from_normalized_config(config_text)
        {
            insert_known_cc_switch_legacy_source_id(&mut ids, &provider_id);
        }
    }

    Ok(ids)
}

fn insert_known_cc_switch_legacy_source_id(ids: &mut BTreeSet<String>, provider_id: &str) {
    let trimmed = provider_id.trim();
    if is_known_cc_switch_legacy_codex_model_provider_id(trimmed) {
        ids.insert(trimmed.to_string());
    }
}

fn migration_backup_root(migration_name: &str) -> PathBuf {
    get_app_config_dir()
        .join("backups")
        .join(migration_name)
        .join(Local::now().format("%Y%m%d_%H%M%S").to_string())
}

fn is_known_cc_switch_legacy_codex_model_provider_id(provider_id: &str) -> bool {
    CC_SWITCH_LEGACY_CODEX_MODEL_PROVIDER_IDS
        .iter()
        .any(|known| known.eq_ignore_ascii_case(provider_id))
}

fn legacy_codex_model_provider_id_from_normalized_config(config_text: &str) -> Option<String> {
    let doc = config_text.parse::<DocumentMut>().ok()?;
    let provider_id = doc
        .get("model_provider")
        .and_then(|item| item.as_str())
        .map(str::trim)?;
    if provider_id != CC_SWITCH_CODEX_MODEL_PROVIDER_ID
        && provider_id != LEGACY_CC_SWITCH_CODEX_MODEL_PROVIDER_ID
    {
        return None;
    }

    let name = doc
        .get("model_providers")
        .and_then(|item| item.as_table())
        .and_then(|table| table.get(provider_id))
        .and_then(|item| item.as_table())
        .and_then(|table| table.get("name"))
        .and_then(|item| item.as_str())?
        .trim();

    normalized_legacy_codex_provider_name(name).map(str::to_string)
}

fn normalized_legacy_codex_provider_name(name: &str) -> Option<&'static str> {
    if is_known_cc_switch_legacy_codex_model_provider_id(name) {
        return CC_SWITCH_LEGACY_CODEX_MODEL_PROVIDER_IDS
            .iter()
            .copied()
            .find(|known| known.eq_ignore_ascii_case(name));
    }

    match name {
        "E-FlowCode" => Some("eflowcode"),
        "PIPELLM" => Some("pipellm"),
        _ => None,
    }
}

fn trusted_legacy_codex_model_provider_ids_from_config(config_text: &str) -> BTreeSet<String> {
    let Ok(doc) = config_text.parse::<DocumentMut>() else {
        return BTreeSet::new();
    };

    trusted_legacy_codex_model_provider_ids_from_doc(&doc)
}

fn trusted_legacy_codex_model_provider_ids_from_doc(doc: &DocumentMut) -> BTreeSet<String> {
    let mut ids = BTreeSet::new();
    insert_trusted_legacy_config_model_provider_id(&mut ids, doc, doc.get("model_provider"));

    if let Some(profiles) = doc.get("profiles").and_then(|item| item.as_table_like()) {
        for (_, profile_item) in profiles.iter() {
            if let Some(profile_table) = profile_item.as_table_like() {
                insert_trusted_legacy_config_model_provider_id(
                    &mut ids,
                    doc,
                    profile_table.get("model_provider"),
                );
            }
        }
    }

    ids
}

fn insert_trusted_legacy_config_model_provider_id(
    ids: &mut BTreeSet<String>,
    doc: &DocumentMut,
    item: Option<&toml_edit::Item>,
) {
    let Some(provider_id) = item.and_then(|item| item.as_str()).map(str::trim) else {
        return;
    };
    if provider_id.is_empty()
        || !is_known_cc_switch_legacy_codex_model_provider_id(provider_id)
        || !config_defines_model_provider(doc, provider_id)
    {
        return;
    }
    ids.insert(provider_id.to_string());
}

fn config_defines_model_provider(doc: &DocumentMut, provider_id: &str) -> bool {
    doc.get("model_providers")
        .and_then(|item| item.as_table())
        .and_then(|table| table.get(provider_id))
        .and_then(|item| item.as_table())
        .is_some()
}

fn migrate_provider_config_template_to_custom(
    config_text: &str,
) -> Result<Option<String>, AppError> {
    if config_text.trim().is_empty() {
        return Ok(None);
    }

    let mut doc = config_text
        .parse::<DocumentMut>()
        .map_err(|e| AppError::Message(format!("Invalid Codex config.toml: {e}")))?;

    let source_provider_ids = trusted_legacy_codex_model_provider_ids_from_doc(&doc);
    if source_provider_ids.is_empty() {
        return Ok(None);
    }

    let active_provider_id = doc
        .get("model_provider")
        .and_then(|item| item.as_str())
        .map(str::trim)
        .filter(|provider_id| !provider_id.is_empty())
        .map(str::to_string);

    let custom_table_exists =
        config_defines_model_provider(&doc, CC_SWITCH_CODEX_MODEL_PROVIDER_ID);
    let source_provider_id_to_move = active_provider_id
        .as_deref()
        .filter(|provider_id| source_provider_ids.contains(*provider_id))
        .map(str::to_string)
        .or_else(|| {
            if custom_table_exists {
                None
            } else {
                source_provider_ids.iter().next().cloned()
            }
        });

    let mut changed = false;

    if let Some(source_provider_id) = source_provider_id_to_move {
        let Some(model_providers) = doc
            .get_mut("model_providers")
            .and_then(|item| item.as_table_mut())
        else {
            return Ok(None);
        };

        let Some(provider_table) = model_providers.remove(source_provider_id.as_str()) else {
            return Ok(None);
        };
        model_providers[CC_SWITCH_CODEX_MODEL_PROVIDER_ID] = provider_table;
        changed = true;
    }

    if active_provider_id
        .as_deref()
        .is_some_and(|provider_id| source_provider_ids.contains(provider_id))
    {
        doc["model_provider"] = toml_edit::value(CC_SWITCH_CODEX_MODEL_PROVIDER_ID);
        changed = true;
    }

    for source_provider_id in source_provider_ids {
        if rewrite_legacy_provider_profile_refs(&mut doc, source_provider_id.as_str()) {
            changed = true;
        }
    }

    if changed {
        Ok(Some(doc.to_string()))
    } else {
        Ok(None)
    }
}

fn rewrite_legacy_provider_profile_refs(doc: &mut DocumentMut, source_provider_id: &str) -> bool {
    let Some(profiles) = doc
        .get_mut("profiles")
        .and_then(|item| item.as_table_like_mut())
    else {
        return false;
    };

    let mut changed = false;
    let profile_keys: Vec<String> = profiles.iter().map(|(key, _)| key.to_string()).collect();
    for profile_key in profile_keys {
        let Some(profile_table) = profiles
            .get_mut(&profile_key)
            .and_then(|item| item.as_table_like_mut())
        else {
            continue;
        };

        let references_legacy = profile_table
            .get("model_provider")
            .and_then(|item| item.as_str())
            == Some(source_provider_id);
        if references_legacy {
            profile_table.insert(
                "model_provider",
                toml_edit::value(CC_SWITCH_CODEX_MODEL_PROVIDER_ID),
            );
            changed = true;
        }
    }
    changed
}

fn migrate_codex_jsonl_files(
    codex_dir: &Path,
    source_provider_ids: &BTreeSet<String>,
    backup_root: &Path,
) -> Result<usize, AppError> {
    migrate_codex_jsonl_files_to_target(
        codex_dir,
        source_provider_ids,
        backup_root,
        CC_SWITCH_CODEX_MODEL_PROVIDER_ID,
    )
}

fn migrate_codex_jsonl_files_to_target(
    codex_dir: &Path,
    source_provider_ids: &BTreeSet<String>,
    backup_root: &Path,
    target_provider_id: &str,
) -> Result<usize, AppError> {
    let mut files = Vec::new();
    collect_jsonl_files(&codex_dir.join("sessions"), &mut files, 0, 8);
    collect_jsonl_files(&codex_dir.join("archived_sessions"), &mut files, 0, 4);

    let source_provider_ids: HashSet<String> = source_provider_ids.iter().cloned().collect();
    let mut migrated = 0;
    for file_path in files {
        if rewrite_codex_session_file_for_provider_bucket_to_target(
            &file_path,
            codex_dir,
            &source_provider_ids,
            backup_root,
            target_provider_id,
        )? {
            migrated += 1;
        }
    }
    Ok(migrated)
}

fn collect_jsonl_files(dir: &Path, files: &mut Vec<PathBuf>, depth: u8, max_depth: u8) {
    if depth > max_depth || !dir.is_dir() {
        return;
    }

    let entries = match fs::read_dir(dir) {
        Ok(entries) => entries,
        Err(err) => {
            log::debug!(
                "Failed to read Codex session directory {}: {err}",
                dir.display()
            );
            return;
        }
    };

    for entry in entries.flatten() {
        let path = entry.path();
        if path.is_dir() {
            collect_jsonl_files(&path, files, depth + 1, max_depth);
        } else if path.extension().and_then(|ext| ext.to_str()) == Some("jsonl") {
            files.push(path);
        }
    }
}

#[cfg(test)]
fn rewrite_codex_session_file_for_provider_bucket(
    path: &Path,
    codex_dir: &Path,
    source_provider_ids: &HashSet<String>,
    backup_root: &Path,
) -> Result<bool, AppError> {
    rewrite_codex_session_file_for_provider_bucket_to_target(
        path,
        codex_dir,
        source_provider_ids,
        backup_root,
        CC_SWITCH_CODEX_MODEL_PROVIDER_ID,
    )
}

fn rewrite_codex_session_file_for_provider_bucket_to_target(
    path: &Path,
    codex_dir: &Path,
    source_provider_ids: &HashSet<String>,
    backup_root: &Path,
    target_provider_id: &str,
) -> Result<bool, AppError> {
    rewrite_codex_session_file_lines(path, codex_dir, backup_root, |line| {
        rewrite_codex_session_meta_line_to_target(line, source_provider_ids, target_provider_id)
    })
}

fn rewrite_codex_session_file_lines(
    path: &Path,
    codex_dir: &Path,
    backup_root: &Path,
    rewrite_line: impl Fn(&str) -> Option<String>,
) -> Result<bool, AppError> {
    let metadata_before = fs::metadata(path).map_err(|e| AppError::io(path, e))?;
    let modified_before = metadata_before.modified().ok();
    let len_before = metadata_before.len();
    let content = fs::read_to_string(path).map_err(|e| AppError::io(path, e))?;

    let mut rewritten = String::with_capacity(content.len());
    let mut changed = false;
    for segment in content.split_inclusive('\n') {
        let (line, newline) = segment
            .strip_suffix('\n')
            .map(|line| (line, "\n"))
            .unwrap_or((segment, ""));
        if let Some(next_line) = rewrite_line(line) {
            rewritten.push_str(&next_line);
            changed = true;
        } else {
            rewritten.push_str(line);
        }
        rewritten.push_str(newline);
    }

    if !changed {
        return Ok(false);
    }

    ensure_codex_session_file_unchanged(path, modified_before, len_before)?;
    backup_codex_jsonl_file(path, codex_dir, backup_root)?;
    ensure_codex_session_file_unchanged(path, modified_before, len_before)?;
    atomic_write(path, rewritten.as_bytes())?;
    Ok(true)
}

fn ensure_codex_session_file_unchanged(
    path: &Path,
    modified_before: Option<SystemTime>,
    len_before: u64,
) -> Result<(), AppError> {
    let metadata_after = fs::metadata(path).map_err(|e| AppError::io(path, e))?;
    if metadata_after.modified().ok() != modified_before || metadata_after.len() != len_before {
        return Err(AppError::Message(format!(
            "Codex session file changed during migration: {}",
            path.display()
        )));
    }
    Ok(())
}

fn rewrite_codex_session_meta_line_to_target(
    line: &str,
    source_provider_ids: &HashSet<String>,
    target_provider_id: &str,
) -> Option<String> {
    if !line.contains("\"session_meta\"") || !line.contains("\"model_provider\"") {
        return None;
    }

    let mut value: Value = serde_json::from_str(line).ok()?;
    if value.get("type").and_then(Value::as_str) != Some("session_meta") {
        return None;
    }

    let payload = value.get_mut("payload")?.as_object_mut()?;
    let current_provider = payload.get("model_provider")?.as_str()?;
    if !source_provider_ids.contains(current_provider) {
        return None;
    }

    payload.insert(
        "model_provider".to_string(),
        Value::String(target_provider_id.to_string()),
    );
    serde_json::to_string(&value).ok()
}

fn migrate_codex_state_dbs(
    codex_dir: &Path,
    source_provider_ids: &BTreeSet<String>,
    backup_root: &Path,
) -> Result<usize, AppError> {
    migrate_codex_state_dbs_to_target(
        codex_dir,
        source_provider_ids,
        backup_root,
        CC_SWITCH_CODEX_MODEL_PROVIDER_ID,
    )
}

fn migrate_codex_state_dbs_to_target(
    codex_dir: &Path,
    source_provider_ids: &BTreeSet<String>,
    backup_root: &Path,
    target_provider_id: &str,
) -> Result<usize, AppError> {
    let config_text = read_codex_config_text().unwrap_or_default();
    let mut migrated = 0;
    for db_path in codex_state_db_paths(codex_dir, &config_text) {
        migrated += migrate_codex_state_db_provider_bucket_to_target(
            &db_path,
            codex_dir,
            source_provider_ids,
            backup_root,
            target_provider_id,
        )?;
    }
    Ok(migrated)
}

fn codex_state_db_paths(codex_dir: &Path, config_text: &str) -> Vec<PathBuf> {
    let mut paths = vec![codex_dir.join(CODEX_STATE_DB_FILENAME)];
    if let Some(sqlite_home) = sqlite_home_from_codex_config(config_text) {
        let db_path = sqlite_home.join(CODEX_STATE_DB_FILENAME);
        if !paths.contains(&db_path) {
            paths.push(db_path);
        }
    }
    paths
}

fn sqlite_home_from_codex_config(config_text: &str) -> Option<PathBuf> {
    let doc = config_text.parse::<DocumentMut>().ok()?;
    let raw = doc.get("sqlite_home")?.as_str()?.trim();
    if raw.is_empty() {
        return None;
    }
    Some(resolve_user_path(raw))
}

fn resolve_user_path(raw: &str) -> PathBuf {
    if raw == "~" {
        return crate::config::get_home_dir();
    }
    if let Some(rest) = raw.strip_prefix("~/") {
        return crate::config::get_home_dir().join(rest);
    }
    if let Some(rest) = raw.strip_prefix("~\\") {
        return crate::config::get_home_dir().join(rest);
    }
    PathBuf::from(raw)
}

#[cfg(test)]
fn migrate_codex_state_db_provider_bucket(
    db_path: &Path,
    codex_dir: &Path,
    source_provider_ids: &BTreeSet<String>,
    backup_root: &Path,
) -> Result<usize, AppError> {
    migrate_codex_state_db_provider_bucket_to_target(
        db_path,
        codex_dir,
        source_provider_ids,
        backup_root,
        CC_SWITCH_CODEX_MODEL_PROVIDER_ID,
    )
}

fn migrate_codex_state_db_provider_bucket_to_target(
    db_path: &Path,
    codex_dir: &Path,
    source_provider_ids: &BTreeSet<String>,
    backup_root: &Path,
    target_provider_id: &str,
) -> Result<usize, AppError> {
    if !db_path.exists() || source_provider_ids.is_empty() {
        return Ok(0);
    }

    let mut conn = Connection::open(db_path)
        .map_err(|e| AppError::Database(format!("打开 Codex state DB 失败: {e}")))?;
    conn.busy_timeout(Duration::from_secs(5))
        .map_err(|e| AppError::Database(format!("设置 Codex state DB busy_timeout 失败: {e}")))?;

    if !Database::table_exists(&conn, "threads")?
        || !Database::has_column(&conn, "threads", "model_provider")?
    {
        return Ok(0);
    }

    let placeholders = placeholders(source_provider_ids.len());
    let count_sql =
        format!("SELECT COUNT(*) FROM threads WHERE model_provider IN ({placeholders})");
    let matching_rows: i64 = conn
        .query_row(
            &count_sql,
            params_from_iter(source_provider_ids.iter()),
            |row| row.get(0),
        )
        .map_err(|e| AppError::Database(format!("统计 Codex state DB 待迁移行失败: {e}")))?;
    if matching_rows == 0 {
        return Ok(0);
    }

    backup_codex_state_db(db_path, codex_dir, backup_root, &conn)?;

    let update_sql =
        format!("UPDATE threads SET model_provider = ? WHERE model_provider IN ({placeholders})");
    let mut values = Vec::with_capacity(source_provider_ids.len() + 1);
    values.push(target_provider_id.to_string());
    values.extend(source_provider_ids.iter().cloned());
    let tx = conn
        .transaction()
        .map_err(|e| AppError::Database(format!("开启 Codex state DB 迁移事务失败: {e}")))?;
    let changed = tx
        .execute(&update_sql, params_from_iter(values.iter()))
        .map_err(|e| AppError::Database(format!("迁移 Codex state DB provider 失败: {e}")))?;
    tx.commit()
        .map_err(|e| AppError::Database(format!("提交 Codex state DB 迁移事务失败: {e}")))?;
    Ok(changed)
}

fn placeholders(count: usize) -> String {
    std::iter::repeat_n("?", count)
        .collect::<Vec<_>>()
        .join(", ")
}

fn backup_codex_jsonl_file(
    path: &Path,
    codex_dir: &Path,
    backup_root: &Path,
) -> Result<(), AppError> {
    let backup_path = backup_root
        .join("jsonl")
        .join(relative_backup_path(path, codex_dir));
    copy_existing_file(path, &backup_path)
}

fn backup_codex_state_db(
    db_path: &Path,
    codex_dir: &Path,
    backup_root: &Path,
    source_conn: &Connection,
) -> Result<(), AppError> {
    let backup_path = backup_root
        .join("state")
        .join(relative_backup_path(db_path, codex_dir));
    if let Some(parent) = backup_path.parent() {
        fs::create_dir_all(parent).map_err(|e| AppError::io(parent, e))?;
    }

    let mut backup_conn = Connection::open(&backup_path)
        .map_err(|e| AppError::Database(format!("创建 Codex state DB 备份失败: {e}")))?;
    let backup = Backup::new(source_conn, &mut backup_conn)
        .map_err(|e| AppError::Database(format!("初始化 Codex state DB 备份失败: {e}")))?;
    backup
        .run_to_completion(5, Duration::from_millis(25), None)
        .map_err(|e| AppError::Database(format!("写入 Codex state DB 备份失败: {e}")))?;
    Ok(())
}

fn backup_provider_settings_config(
    provider_id: &str,
    settings_config: &Value,
    backup_root: &Path,
) -> Result<(), AppError> {
    let backup_path = backup_root
        .join("providers")
        .join(provider_settings_backup_filename(provider_id));
    if let Some(parent) = backup_path.parent() {
        fs::create_dir_all(parent).map_err(|e| AppError::io(parent, e))?;
    }

    let payload = serde_json::json!({
        "providerId": provider_id,
        "settingsConfig": settings_config,
    });
    let bytes =
        serde_json::to_vec_pretty(&payload).map_err(|e| AppError::JsonSerialize { source: e })?;
    atomic_write(&backup_path, &bytes)
}

fn provider_settings_backup_filename(provider_id: &str) -> String {
    let safe_id: String = provider_id
        .chars()
        .map(|ch| {
            if ch.is_ascii_alphanumeric() || matches!(ch, '-' | '_' | '.') {
                ch
            } else {
                '_'
            }
        })
        .collect();
    let safe_id = if safe_id.is_empty() {
        "provider".to_string()
    } else {
        safe_id
    };
    // Keep the hash stable across processes while avoiding collisions after sanitization.
    let digest = Sha256::digest(provider_id.as_bytes());
    let hash = digest[..8]
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect::<String>();
    format!("{hash}-{safe_id}.settings_config.json")
}

fn copy_existing_file(source: &Path, target: &Path) -> Result<(), AppError> {
    if let Some(parent) = target.parent() {
        fs::create_dir_all(parent).map_err(|e| AppError::io(parent, e))?;
    }
    copy_file(source, target)
}

fn relative_backup_path(path: &Path, root: &Path) -> PathBuf {
    if let Ok(relative) = path.strip_prefix(root) {
        return relative.to_path_buf();
    }

    let mut hasher = std::collections::hash_map::DefaultHasher::new();
    path.hash(&mut hasher);
    let hash = hasher.finish();
    let file_name = path
        .file_name()
        .map(|name| name.to_string_lossy().to_string())
        .unwrap_or_else(|| "file".to_string());
    PathBuf::from("external").join(format!("{hash:016x}-{file_name}"))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::provider::Provider;
    use tempfile::tempdir;

    fn source_ids(values: &[&str]) -> BTreeSet<String> {
        values.iter().map(|value| value.to_string()).collect()
    }

    /// 为历史修复测试创建最小 `threads` 表，覆盖当前 UI 需要读取和修复的字段。
    fn create_history_test_threads_table(conn: &Connection) {
        conn.execute_batch(
            "CREATE TABLE threads (
                id TEXT PRIMARY KEY,
                rollout_path TEXT,
                model_provider TEXT,
                cwd TEXT,
                has_user_event INTEGER,
                archived INTEGER,
                source TEXT,
                thread_source TEXT,
                title TEXT,
                preview TEXT,
                first_user_message TEXT,
                updated_at INTEGER,
                updated_at_ms INTEGER
            );",
        )
        .expect("create threads table");
    }

    #[test]
    fn active_state_db_prefers_current_sqlite_subdir() {
        let dir = tempdir().expect("tempdir");
        let codex_dir = dir.path().join(".codex");
        let sqlite_dir = codex_dir.join("sqlite");
        fs::create_dir_all(&sqlite_dir).expect("create sqlite dir");
        fs::write(codex_dir.join(CODEX_STATE_DB_FILENAME), b"legacy").expect("legacy db");
        fs::write(sqlite_dir.join(CODEX_STATE_DB_FILENAME), b"active").expect("active db");

        let active = resolve_active_codex_state_db(&codex_dir, "").expect("active db");

        assert_eq!(active.kind, "sqlite_subdir");
        assert_eq!(active.path, sqlite_dir.join(CODEX_STATE_DB_FILENAME));
    }

    #[test]
    fn history_repair_defaults_target_to_live_config_provider() {
        let dir = tempdir().expect("tempdir");
        let codex_dir = dir.path().join(".codex");
        let sqlite_dir = codex_dir.join("sqlite");
        let session_dir = codex_dir.join("sessions/2026/06/15");
        fs::create_dir_all(&sqlite_dir).expect("create sqlite dir");
        fs::create_dir_all(&session_dir).expect("create session dir");
        fs::write(
            codex_dir.join("config.toml"),
            r#"model_provider = "third_party_live""#,
        )
        .expect("write config");

        let rollout = session_dir.join("rollout-live-target.jsonl");
        fs::write(
            &rollout,
            concat!(
                "{\"type\":\"session_meta\",\"payload\":{\"id\":\"multi-live-target\",\"model_provider\":\"openai\"}}\n",
                "{\"timestamp\":\"2026-03-06T21:50:13Z\",\"type\":\"response_item\",\"payload\":{\"type\":\"message\",\"role\":\"user\",\"content\":\"hello\"}}\n"
            ),
        )
        .expect("write rollout");

        let db_path = sqlite_dir.join(CODEX_STATE_DB_FILENAME);
        let conn = Connection::open(&db_path).expect("open active db");
        create_history_test_threads_table(&conn);
        conn.execute(
            "INSERT INTO threads VALUES (?1, ?2, 'openai', ?3, 0, 0, 'vscode', 'user', 'Live target', 'Live target', NULL, 1000, 1000000)",
            (
                &"multi-live-target",
                &rollout.to_string_lossy().to_string(),
                &"C:\\Users\\sunda\\Documents\\LLMservice",
            ),
        )
        .expect("insert thread");
        drop(conn);

        fs::write(codex_dir.join("session_index.jsonl"), "").expect("write index");
        fs::write(
            codex_dir.join(".codex-global-state.json"),
            serde_json::to_vec_pretty(&serde_json::json!({
                "thread-workspace-root-hints": {},
                "projectless-thread-ids": [],
                "electron-saved-workspace-roots": []
            }))
            .expect("global json"),
        )
        .expect("write global state");

        let db = Database::memory().expect("memory db");
        let result = repair_codex_history_visibility(
            &db,
            CodexHistoryVisibilityRepairOptions {
                dry_run: true,
                codex_home: Some(codex_dir.to_string_lossy().to_string()),
                project_path: Some("C:\\Users\\sunda\\Documents\\LLMservice".to_string()),
                target_provider: None,
                source_filter: Some("vscode".to_string()),
                ..Default::default()
            },
        )
        .expect("history repair dry-run");

        assert_eq!(result.target_provider, "third_party_live");
        assert_eq!(
            result.live_config_model_provider.as_deref(),
            Some("third_party_live")
        );
        assert!(result
            .source_provider_ids
            .iter()
            .any(|provider| provider == OFFICIAL_OPENAI_CODEX_MODEL_PROVIDER_ID));
        assert_eq!(result.provider_rows_to_update, 1);
        assert_eq!(result.user_event_rows_to_update, 1);
        assert_eq!(result.active_db_kind.as_deref(), Some("sqlite_subdir"));
    }

    #[test]
    fn list_history_sessions_returns_provider_source_candidates_and_all_sources() {
        let dir = tempdir().expect("tempdir");
        let codex_dir = dir.path().join(".codex");
        let sqlite_dir = codex_dir.join("sqlite");
        fs::create_dir_all(&sqlite_dir).expect("create sqlite dir");
        fs::write(
            codex_dir.join("config.toml"),
            r#"model_provider = "third_party_live""#,
        )
        .expect("write config");

        let db_path = sqlite_dir.join(CODEX_STATE_DB_FILENAME);
        let conn = Connection::open(&db_path).expect("open active db");
        create_history_test_threads_table(&conn);
        conn.execute(
            "INSERT INTO threads VALUES ('vscode-session', NULL, 'openai', ?1, 1, 0, 'vscode', 'user', 'VS Code', 'VS Code', NULL, 3000, 3000000)",
            (&"C:\\Users\\sunda\\Documents\\LLMservice",),
        )
        .expect("insert vscode row");
        conn.execute(
            "INSERT INTO threads VALUES ('exec-session', NULL, 'custom', ?1, 1, 0, 'exec', 'user', 'Exec', 'Exec', NULL, 2000, 2000000)",
            (&"C:\\Users\\sunda\\Documents\\trace",),
        )
        .expect("insert exec row");
        conn.execute(
            "INSERT INTO threads VALUES ('subagent-session', NULL, 'codex_model_router_v2', ?1, 1, 0, 'cli', 'subagent', 'Subagent', 'Subagent', NULL, 1000, 1000000)",
            (&"C:\\Users\\sunda\\Documents\\LLMservice",),
        )
        .expect("insert subagent row");
        drop(conn);

        let result = list_codex_history_sessions(CodexHistorySessionListOptions {
            codex_home: Some(codex_dir.to_string_lossy().to_string()),
            source_filter: Some("all".to_string()),
            limit: Some(10),
            include_subagents: Some(true),
            ..Default::default()
        })
        .expect("list history sessions");

        assert_eq!(result.total_matched, 3);
        assert_eq!(
            result
                .items
                .iter()
                .map(|item| item.id.as_str())
                .collect::<Vec<_>>(),
            vec!["vscode-session", "exec-session", "subagent-session"]
        );
        assert_eq!(
            result.live_config_model_provider.as_deref(),
            Some("third_party_live")
        );
        assert_eq!(
            result
                .target_provider_candidates
                .first()
                .map(String::as_str),
            Some("third_party_live")
        );
        assert!(result
            .target_provider_candidates
            .iter()
            .any(|provider| provider == CC_SWITCH_CODEX_ROUTER_MODEL_PROVIDER_ID));
        assert!(result
            .source_counts
            .iter()
            .any(|row| row.value.as_deref() == Some("exec") && row.count == 1));
        assert!(result
            .provider_counts
            .iter()
            .any(|row| row.value.as_deref() == Some("custom") && row.count == 1));
        assert_eq!(result.active_db_kind.as_deref(), Some("sqlite_subdir"));
    }

    #[test]
    fn read_history_session_loads_rollout_messages_from_sqlite_path() {
        let dir = tempdir().expect("tempdir");
        let codex_dir = dir.path().join(".codex");
        let sqlite_dir = codex_dir.join("sqlite");
        let session_dir = codex_dir.join("sessions/2026/06/15");
        fs::create_dir_all(&sqlite_dir).expect("create sqlite dir");
        fs::create_dir_all(&session_dir).expect("create session dir");
        fs::write(
            codex_dir.join("config.toml"),
            r#"model_provider = "third_party_live""#,
        )
        .expect("write config");

        let rollout = session_dir.join("rollout-detail-session.jsonl");
        fs::write(
            &rollout,
            concat!(
                "{\"timestamp\":\"2026-03-06T21:50:12Z\",\"type\":\"session_meta\",\"payload\":{\"id\":\"detail-session\",\"cwd\":\"C:\\\\Users\\\\sunda\\\\Documents\\\\LLMservice\"}}\n",
                "{\"timestamp\":\"2026-03-06T21:50:13Z\",\"type\":\"response_item\",\"payload\":{\"type\":\"message\",\"role\":\"user\",\"content\":\"hello detail\"}}\n",
                "{\"timestamp\":\"2026-03-06T21:50:14Z\",\"type\":\"response_item\",\"payload\":{\"type\":\"message\",\"role\":\"assistant\",\"content\":[{\"type\":\"output_text\",\"text\":\"done detail\"}]}}\n"
            ),
        )
        .expect("write rollout");

        let db_path = sqlite_dir.join(CODEX_STATE_DB_FILENAME);
        let conn = Connection::open(&db_path).expect("open active db");
        create_history_test_threads_table(&conn);
        conn.execute(
            "INSERT INTO threads VALUES ('detail-session', ?1, 'third_party_live', ?2, 1, 0, 'vscode', 'user', 'Detail title', 'Detail title', NULL, 1000, 1000000)",
            (
                &rollout.to_string_lossy().to_string(),
                &"C:\\Users\\sunda\\Documents\\LLMservice",
            ),
        )
        .expect("insert detail row");
        drop(conn);

        let result = read_codex_history_session(CodexHistorySessionDetailOptions {
            codex_home: Some(codex_dir.to_string_lossy().to_string()),
            session_id: "detail-session".to_string(),
            ..Default::default()
        })
        .expect("read history session");

        assert_eq!(
            result.session.as_ref().map(|session| session.id.as_str()),
            Some("detail-session")
        );
        let expected_rollout_path = rollout.to_string_lossy().to_string();
        assert_eq!(
            result.rollout_path.as_deref(),
            Some(expected_rollout_path.as_str())
        );
        assert_eq!(result.messages.len(), 2);
        assert_eq!(result.messages[0].role, "user");
        assert_eq!(result.messages[0].content, "hello detail");
        assert_eq!(result.messages[1].role, "assistant");
        assert_eq!(result.messages[1].content, "done detail");
    }

    #[test]
    fn repairs_current_desktop_history_visibility_end_to_end() {
        let dir = tempdir().expect("tempdir");
        let codex_dir = dir.path().join(".codex");
        let sqlite_dir = codex_dir.join("sqlite");
        let session_dir = codex_dir.join("sessions/2026/06/14");
        fs::create_dir_all(&sqlite_dir).expect("create sqlite dir");
        fs::create_dir_all(&session_dir).expect("create session dir");

        let rollout_one = session_dir.join("rollout-one.jsonl");
        let rollout_two = session_dir.join("rollout-two.jsonl");
        fs::write(
            &rollout_one,
            concat!(
                "{\"type\":\"session_meta\",\"payload\":{\"id\":\"t1\",\"model_provider\":\"openai\"}}\n",
                "{\"type\":\"session_meta\",\"payload\":{\"id\":\"t1-extra\",\"model_provider\":\"openai\"}}\n",
                "{\"type\":\"user_message\",\"message\":\"hello\"}\n"
            ),
        )
        .expect("write rollout one");
        fs::write(
            &rollout_two,
            concat!(
                "{\"type\":\"session_meta\",\"payload\":{\"id\":\"t2\",\"model_provider\":\"custom\"}}\n",
                "{\"role\":\"user\",\"content\":\"hi\"}\n"
            ),
        )
        .expect("write rollout two");

        let db_path = sqlite_dir.join(CODEX_STATE_DB_FILENAME);
        let conn = Connection::open(&db_path).expect("open active db");
        let project_with_long_prefix = r"\\?\C:\Users\sunda\Documents\LLMservice";
        conn.execute_batch(
            "CREATE TABLE threads (
                id TEXT PRIMARY KEY,
                rollout_path TEXT,
                model_provider TEXT,
                cwd TEXT,
                has_user_event INTEGER,
                archived INTEGER,
                source TEXT,
                thread_source TEXT,
                title TEXT,
                preview TEXT,
                first_user_message TEXT,
                updated_at INTEGER,
                updated_at_ms INTEGER
            );",
        )
        .expect("create threads table");
        conn.execute(
            "INSERT INTO threads VALUES (?1, ?2, 'openai', ?3, 0, 0, 'vscode', 'user', '回复1234', '回复1234', NULL, 1000, 1000000)",
            (&"t1", &rollout_one.to_string_lossy().to_string(), &project_with_long_prefix),
        )
        .expect("insert t1");
        conn.execute(
            "INSERT INTO threads VALUES (?1, ?2, 'custom', ?3, 0, 0, 'cli', 'user', '测试', '测试', NULL, 900, 900000)",
            (&"t2", &rollout_two.to_string_lossy().to_string(), &project_with_long_prefix),
        )
        .expect("insert t2");
        drop(conn);

        fs::write(
            codex_dir.join("session_index.jsonl"),
            "{\"id\":\"old\",\"thread_name\":\"old\",\"updated_at\":\"2026-01-01T00:00:00.000Z\"}\n",
        )
        .expect("write session index");
        fs::write(
            codex_dir.join(".codex-global-state.json"),
            serde_json::to_vec_pretty(&serde_json::json!({
                "thread-workspace-root-hints": {"t2": "C:\\wrong"},
                "projectless-thread-ids": ["t1", "t2", "untouched"],
                "electron-saved-workspace-roots": []
            }))
            .expect("global json"),
        )
        .expect("write global state");

        let active = ActiveCodexStateDb {
            path: db_path.clone(),
            kind: "sqlite_subdir".to_string(),
        };
        let project_path = "C:\\Users\\sunda\\Documents\\LLMservice".to_string();
        let source_providers = source_ids(&["openai", "custom", "cc_switch_codex_router"]);
        let dry_run = repair_codex_history_visibility_at(
            &codex_dir,
            active.clone(),
            CC_SWITCH_CODEX_ROUTER_MODEL_PROVIDER_ID,
            Some(CC_SWITCH_CODEX_ROUTER_MODEL_PROVIDER_ID.to_string()),
            source_providers.clone(),
            Some(project_path.clone()),
            HistoryVisibilityRepairRuntimeOptions {
                dry_run: true,
                count: 20,
                window_limit: 50,
                balance_recent_window: true,
                max_per_project: 10,
                max_total: 300,
                source_filter: None,
                include_archived: false,
                include_subagents: false,
                selected_session_ids: BTreeSet::new(),
                backup_root_override: Some(dir.path().join("dry-run-backup")),
            },
        )
        .expect("dry run repair");

        assert_eq!(dry_run.provider_rows_to_update, 2);
        assert_eq!(dry_run.user_event_rows_to_update, 2);
        assert_eq!(dry_run.visible_candidate_rows, 2);
        assert_eq!(dry_run.session_index_missing_to_append, 2);
        assert_eq!(dry_run.workspace_hints_to_fix, 2);
        assert_eq!(dry_run.projectless_ids_to_remove, 2);
        assert_eq!(dry_run.saved_workspace_roots_to_add, 1);
        assert_eq!(dry_run.focus_selected_count, 2);
        assert_eq!(dry_run.rollout_mtimes_to_touch, 2);
        assert!(dry_run.backup_dir.is_none());

        let backup_root = dir.path().join("history-repair-backup");
        let applied = repair_codex_history_visibility_at(
            &codex_dir,
            active,
            CC_SWITCH_CODEX_ROUTER_MODEL_PROVIDER_ID,
            Some(CC_SWITCH_CODEX_ROUTER_MODEL_PROVIDER_ID.to_string()),
            source_providers,
            Some(project_path.clone()),
            HistoryVisibilityRepairRuntimeOptions {
                dry_run: false,
                count: 20,
                window_limit: 50,
                balance_recent_window: true,
                max_per_project: 10,
                max_total: 300,
                source_filter: None,
                include_archived: false,
                include_subagents: false,
                selected_session_ids: BTreeSet::new(),
                backup_root_override: Some(backup_root.clone()),
            },
        )
        .expect("apply repair");

        assert_eq!(applied.provider_rows_updated, 2);
        assert_eq!(applied.user_event_rows_updated, 2);
        assert_eq!(applied.session_index_appended, 2);
        assert_eq!(applied.workspace_hints_fixed, 2);
        assert_eq!(applied.projectless_ids_removed, 2);
        assert_eq!(applied.saved_workspace_roots_added, 1);
        assert_eq!(applied.sqlite_focus_rows_updated, 2);
        assert_eq!(applied.session_index_rows_moved, 2);
        assert_eq!(applied.rollout_mtimes_touched, 2);
        assert_eq!(
            applied.backup_dir,
            Some(backup_root.to_string_lossy().to_string())
        );

        let conn = Connection::open(&db_path).expect("reopen active db");
        let fixed_rows: i64 = conn
            .query_row(
                "SELECT COUNT(*) FROM threads WHERE model_provider = ?1 AND has_user_event = 1 AND updated_at_ms > 1000000",
                [CC_SWITCH_CODEX_ROUTER_MODEL_PROVIDER_ID],
                |row| row.get(0),
            )
            .expect("count repaired rows");
        assert_eq!(fixed_rows, 2);

        let index_text = fs::read_to_string(codex_dir.join("session_index.jsonl")).expect("index");
        assert!(index_text.contains("\"id\":\"t1\""));
        assert!(index_text.contains("\"id\":\"t2\""));

        let global_state: Value = serde_json::from_str(
            &fs::read_to_string(codex_dir.join(".codex-global-state.json")).expect("global state"),
        )
        .expect("parse global state");
        assert_eq!(
            global_state
                .get("thread-workspace-root-hints")
                .and_then(|value| value.get("t1"))
                .and_then(Value::as_str),
            Some(project_path.as_str())
        );
        assert_eq!(
            global_state
                .get("thread-workspace-root-hints")
                .and_then(|value| value.get("t2"))
                .and_then(Value::as_str),
            Some(project_path.as_str())
        );
        assert_eq!(
            global_state
                .get("projectless-thread-ids")
                .and_then(Value::as_array)
                .expect("projectless")
                .iter()
                .filter_map(Value::as_str)
                .collect::<Vec<_>>(),
            vec!["untouched"]
        );
        assert!(global_state
            .get("electron-saved-workspace-roots")
            .and_then(Value::as_array)
            .expect("roots")
            .iter()
            .any(|value| value.as_str() == Some(project_path.as_str())));

        let rollout_text = fs::read_to_string(&rollout_one).expect("rollout one after");
        assert!(rollout_text.contains("\"model_provider\":\"custom\""));
        assert!(!rollout_text.contains("\"model_provider\":\"openai\""));
        assert!(backup_root
            .join("state/sqlite")
            .join(CODEX_STATE_DB_FILENAME)
            .exists());
    }

    #[test]
    fn selected_session_ids_focus_only_requested_rows() {
        let dir = tempdir().expect("tempdir");
        let codex_dir = dir.path().join(".codex");
        let sqlite_dir = codex_dir.join("sqlite");
        let session_dir = codex_dir.join("sessions/2026/06/15");
        fs::create_dir_all(&sqlite_dir).expect("create sqlite dir");
        fs::create_dir_all(&session_dir).expect("create session dir");

        let rollout_one = session_dir.join("rollout-selected-one.jsonl");
        let rollout_two = session_dir.join("rollout-selected-two.jsonl");
        fs::write(
            &rollout_one,
            concat!(
                "{\"type\":\"session_meta\",\"payload\":{\"id\":\"selected-one\",\"model_provider\":\"openai\"}}\n",
                "{\"type\":\"user_message\",\"message\":\"one\"}\n"
            ),
        )
        .expect("write rollout one");
        fs::write(
            &rollout_two,
            concat!(
                "{\"type\":\"session_meta\",\"payload\":{\"id\":\"selected-two\",\"model_provider\":\"openai\"}}\n",
                "{\"role\":\"user\",\"content\":\"two\"}\n"
            ),
        )
        .expect("write rollout two");

        let db_path = sqlite_dir.join(CODEX_STATE_DB_FILENAME);
        let conn = Connection::open(&db_path).expect("open active db");
        conn.execute_batch(
            "CREATE TABLE threads (
                id TEXT PRIMARY KEY,
                rollout_path TEXT,
                model_provider TEXT,
                cwd TEXT,
                has_user_event INTEGER,
                archived INTEGER,
                source TEXT,
                thread_source TEXT,
                title TEXT,
                preview TEXT,
                first_user_message TEXT,
                updated_at INTEGER,
                updated_at_ms INTEGER
            );",
        )
        .expect("create threads table");
        let project_path = r"C:\Users\sunda\Documents\LLMservice".to_string();
        conn.execute(
            "INSERT INTO threads VALUES (?1, ?2, 'openai', ?3, 0, 0, 'vscode', 'user', 'One', 'One', NULL, 1000, 1000000)",
            (&"selected-one", &rollout_one.to_string_lossy().to_string(), &project_path),
        )
        .expect("insert selected-one");
        conn.execute(
            "INSERT INTO threads VALUES (?1, ?2, 'openai', ?3, 0, 0, 'vscode', 'user', 'Two', 'Two', NULL, 900, 900000)",
            (&"selected-two", &rollout_two.to_string_lossy().to_string(), &project_path),
        )
        .expect("insert selected-two");
        drop(conn);

        fs::write(codex_dir.join("session_index.jsonl"), "").expect("write session index");
        fs::write(
            codex_dir.join(".codex-global-state.json"),
            serde_json::to_vec_pretty(&serde_json::json!({
                "thread-workspace-root-hints": {},
                "projectless-thread-ids": [],
                "electron-saved-workspace-roots": []
            }))
            .expect("global json"),
        )
        .expect("write global state");

        let active = ActiveCodexStateDb {
            path: db_path,
            kind: "sqlite_subdir".to_string(),
        };
        let selected = source_ids(&["selected-two"]);
        let dry_run = repair_codex_history_visibility_at(
            &codex_dir,
            active,
            CC_SWITCH_CODEX_ROUTER_MODEL_PROVIDER_ID,
            Some(CC_SWITCH_CODEX_ROUTER_MODEL_PROVIDER_ID.to_string()),
            source_ids(&["openai"]),
            Some(project_path),
            HistoryVisibilityRepairRuntimeOptions {
                dry_run: true,
                count: 20,
                window_limit: 50,
                balance_recent_window: true,
                max_per_project: 10,
                max_total: 300,
                source_filter: Some("vscode".to_string()),
                include_archived: false,
                include_subagents: false,
                selected_session_ids: selected,
                backup_root_override: Some(dir.path().join("dry-run-backup")),
            },
        )
        .expect("dry run selected repair");

        assert_eq!(dry_run.visible_candidate_rows, 2);
        assert_eq!(dry_run.project_rows, 2);
        assert_eq!(dry_run.focus_selected_count, 1);
        assert!(!dry_run.balanced_recent_window_enabled);
        assert_eq!(dry_run.balanced_recent_window_rows, 0);
        assert_eq!(dry_run.sqlite_focus_rows_to_update, 1);
        assert_eq!(dry_run.session_index_rows_to_move, 1);
        assert_eq!(dry_run.rollout_mtimes_to_touch, 1);
    }

    #[test]
    fn balances_recent_window_and_overwrites_stale_session_index_titles() {
        let dir = tempdir().expect("tempdir");
        let codex_dir = dir.path().join(".codex");
        let sqlite_dir = codex_dir.join("sqlite");
        let session_dir = codex_dir.join("sessions/2026/06/15");
        fs::create_dir_all(&sqlite_dir).expect("create sqlite dir");
        fs::create_dir_all(&session_dir).expect("create session dir");

        let db_path = sqlite_dir.join(CODEX_STATE_DB_FILENAME);
        let conn = Connection::open(&db_path).expect("open active db");
        conn.execute_batch(
            "CREATE TABLE threads (
                id TEXT PRIMARY KEY,
                rollout_path TEXT,
                model_provider TEXT,
                cwd TEXT,
                has_user_event INTEGER,
                archived INTEGER,
                source TEXT,
                thread_source TEXT,
                title TEXT,
                preview TEXT,
                first_user_message TEXT,
                updated_at INTEGER,
                updated_at_ms INTEGER
            );",
        )
        .expect("create threads table");

        let projects = [
            ("llm", r"C:\Users\sunda\Documents\LLMservice"),
            ("trace", r"C:\Users\sunda\Documents\trace"),
            ("openclaw", r"C:\Users\sunda\Documents\Codex repo\openclaw"),
        ];
        let mut timestamp_ms = 10_000_000_i64;
        for (project_key, cwd) in projects {
            for index in 0..12 {
                let id = format!("{project_key}-{index:02}");
                let title = format!("{project_key} title {index:02}");
                let rollout = session_dir.join(format!("rollout-{id}.jsonl"));
                fs::write(
                    &rollout,
                    format!(
                        "{{\"type\":\"session_meta\",\"payload\":{{\"id\":\"{id}\",\"model_provider\":\"openai\"}}}}\n{{\"type\":\"user_message\",\"message\":\"hello\"}}\n"
                    ),
                )
                .expect("write rollout");
                conn.execute(
                    "INSERT INTO threads VALUES (?1, ?2, 'openai', ?3, 1, 0, 'vscode', 'user', ?4, ?4, NULL, ?5, ?6)",
                    (
                        &id,
                        &rollout.to_string_lossy().to_string(),
                        &cwd,
                        &title,
                        &(timestamp_ms / 1000),
                        &timestamp_ms,
                    ),
                )
                .expect("insert thread");
                timestamp_ms -= 1000;
            }
        }

        let cli_rollout = session_dir.join("rollout-llm-cli.jsonl");
        fs::write(
            &cli_rollout,
            "{\"type\":\"session_meta\",\"payload\":{\"id\":\"llm-cli\",\"model_provider\":\"openai\"}}\n{\"type\":\"user_message\",\"message\":\"cli\"}\n",
        )
        .expect("write cli rollout");
        conn.execute(
            "INSERT INTO threads VALUES ('llm-cli', ?1, 'openai', ?2, 1, 0, 'cli', 'user', 'CLI title', 'CLI title', NULL, 1, 1000)",
            (
                &cli_rollout.to_string_lossy().to_string(),
                &r"C:\Users\sunda\Documents\LLMservice",
            ),
        )
        .expect("insert cli thread");
        drop(conn);

        fs::write(
            codex_dir.join("session_index.jsonl"),
            "{\"id\":\"llm-00\",\"thread_name\":\"stale title\",\"updated_at\":\"2026-01-01T00:00:00.000Z\"}\n",
        )
        .expect("write session index");
        fs::write(
            codex_dir.join(".codex-global-state.json"),
            serde_json::to_vec_pretty(&serde_json::json!({
                "thread-workspace-root-hints": {},
                "projectless-thread-ids": [],
                "electron-saved-workspace-roots": []
            }))
            .expect("global json"),
        )
        .expect("write global state");

        let active = ActiveCodexStateDb {
            path: db_path.clone(),
            kind: "sqlite_subdir".to_string(),
        };
        let project_path = r"C:\Users\sunda\Documents\LLMservice".to_string();
        let source_providers = source_ids(&["openai"]);
        let dry_run = repair_codex_history_visibility_at(
            &codex_dir,
            active.clone(),
            CC_SWITCH_CODEX_ROUTER_MODEL_PROVIDER_ID,
            Some(CC_SWITCH_CODEX_ROUTER_MODEL_PROVIDER_ID.to_string()),
            source_providers.clone(),
            Some(project_path.clone()),
            HistoryVisibilityRepairRuntimeOptions {
                dry_run: true,
                count: 5,
                window_limit: 8,
                balance_recent_window: true,
                max_per_project: 3,
                max_total: 8,
                source_filter: Some("vscode".to_string()),
                include_archived: false,
                include_subagents: false,
                selected_session_ids: BTreeSet::new(),
                backup_root_override: Some(dir.path().join("dry-run-backup")),
            },
        )
        .expect("dry run repair");

        assert_eq!(dry_run.provider_rows_to_update, 37);
        assert_eq!(dry_run.visible_candidate_rows, 36);
        assert_eq!(dry_run.project_rows, 12);
        assert_eq!(dry_run.focus_selected_count, 8);
        assert_eq!(dry_run.balanced_recent_window_rows, 8);
        assert_eq!(dry_run.balanced_recent_window_projects, 3);
        assert_eq!(dry_run.session_index_titles_to_update, 1);
        assert_eq!(dry_run.source_filter.as_deref(), Some("vscode"));

        let backup_root = dir.path().join("history-repair-backup");
        let applied = repair_codex_history_visibility_at(
            &codex_dir,
            active,
            CC_SWITCH_CODEX_ROUTER_MODEL_PROVIDER_ID,
            Some(CC_SWITCH_CODEX_ROUTER_MODEL_PROVIDER_ID.to_string()),
            source_providers,
            Some(project_path),
            HistoryVisibilityRepairRuntimeOptions {
                dry_run: false,
                count: 5,
                window_limit: 8,
                balance_recent_window: true,
                max_per_project: 3,
                max_total: 8,
                source_filter: Some("vscode".to_string()),
                include_archived: false,
                include_subagents: false,
                selected_session_ids: BTreeSet::new(),
                backup_root_override: Some(backup_root),
            },
        )
        .expect("apply repair");

        assert_eq!(applied.session_index_titles_updated, 1);
        assert_eq!(applied.session_index_rows_moved, 8);
        assert_eq!(applied.rollout_mtimes_touched, 8);

        let index_text = fs::read_to_string(codex_dir.join("session_index.jsonl")).expect("index");
        assert!(index_text.contains("\"id\":\"llm-00\""));
        assert!(index_text.contains("\"thread_name\":\"llm title 00\""));
        assert!(!index_text.contains("stale title"));
    }

    #[test]
    fn detects_custom_routed_codex_config_for_unify_gate() {
        // 注入产物（官方 + 统一开关）
        assert!(codex_config_text_routes_custom(
            r#"model_provider = "custom"

[model_providers.custom]
name = "OpenAI"
requires_openai_auth = true
supports_websockets = true
wire_api = "responses"
"#
        ));
        // 第三方供应商的常规 custom 路由（带 base_url）同样算已统一
        assert!(codex_config_text_routes_custom(
            r#"model_provider = "custom"

[model_providers.custom]
name = "AIHubMix"
base_url = "https://aihubmix.example/v1"
"#
        ));
        // 注入被拒的形态：显式 openai 路由 / 无 model_provider（接管期间、空配置）
        assert!(!codex_config_text_routes_custom(
            "model_provider = \"openai\"\n"
        ));
        assert!(!codex_config_text_routes_custom(
            "base_url = \"http://127.0.0.1:15721/codex\"\n"
        ));
        assert!(!codex_config_text_routes_custom(""));
        assert!(!codex_config_text_routes_custom("not toml ["));
    }

    fn migrate_provider_templates_for_test(
        db: &Database,
    ) -> (
        CodexProviderTemplateBucketMigrationOutcome,
        tempfile::TempDir,
    ) {
        let backup_dir = tempdir().expect("backup dir");
        let outcome = migrate_codex_provider_templates_to_custom(db, backup_dir.path())
            .expect("migrate template");
        (outcome, backup_dir)
    }

    #[test]
    fn simulates_local_codex_provider_bucket_migration_end_to_end() {
        let dir = tempdir().expect("tempdir");
        let codex_dir = dir.path().join(".codex");
        let backup_root = dir.path().join("backup");
        fs::create_dir_all(&codex_dir).expect("create codex dir");

        let db = Database::memory().expect("memory db");
        let providers = [
            Provider::with_id(
                "rightcode".to_string(),
                "RightCode".to_string(),
                serde_json::json!({
                    "auth": {},
                    "config": r#"model_provider = "aihubmix"

[model_providers.aihubmix]
name = "AIHubMix"
base_url = "https://aihubmix.example/v1"
"#
                }),
                None,
            ),
            Provider::with_id(
                "legacy-ccswitch".to_string(),
                "Legacy CC Switch".to_string(),
                serde_json::json!({
                    "auth": {},
                    "config": r#"model_provider = "ccswitch"

[model_providers.ccswitch]
name = "AIHubMix"
base_url = "https://aihubmix.example/v1"
"#
                }),
                None,
            ),
            Provider::with_id(
                "normalized-aihubmix".to_string(),
                "Already Normalized".to_string(),
                serde_json::json!({
                    "auth": {},
                    "config": r#"model_provider = "custom"

[model_providers.custom]
name = "AIHubMix"
base_url = "https://aihubmix.example/v1"
"#
                }),
                None,
            ),
            Provider::with_id(
                "manual-relay".to_string(),
                "Manual Relay".to_string(),
                serde_json::json!({
                    "auth": {},
                    "config": r#"model_provider = "my-private-relay"

[model_providers.my-private-relay]
name = "Manual Relay"
base_url = "http://localhost:8080/v1"
"#
                }),
                None,
            ),
            Provider::with_id(
                "custom-openai".to_string(),
                "Custom OpenAI".to_string(),
                serde_json::json!({
                    "auth": {},
                    "config": r#"model_provider = "openai"

[model_providers.openai]
name = "Custom OpenAI"
base_url = "https://proxy.example/v1"
"#
                }),
                None,
            ),
        ];
        for provider in providers {
            db.save_provider("codex", &provider).expect("save provider");
        }

        let mut official = Provider::with_id(
            "codex-official".to_string(),
            "OpenAI Official".to_string(),
            serde_json::json!({"auth": {}, "config": "model_provider = \"openai\""}),
            None,
        );
        official.category = Some("official".to_string());
        db.save_provider("codex", &official).expect("save official");

        let source_provider_ids = collect_source_model_provider_ids(&db).expect("collect ids");
        assert_eq!(
            source_provider_ids,
            source_ids(&["aihubmix", "ccswitch", "rightcode"])
        );

        let session_dir = codex_dir.join("sessions/2026/05/28");
        fs::create_dir_all(&session_dir).expect("create session dir");
        let session_path = session_dir.join("local-sim.jsonl");
        fs::write(
            &session_path,
            concat!(
                "{\"type\":\"session_meta\",\"payload\":{\"id\":\"s1\",\"model_provider\":\"rightcode\"}}\n",
                "{\"type\":\"session_meta\",\"payload\":{\"id\":\"s2\",\"model_provider\":\"aihubmix\"}}\n",
                "{\"type\":\"session_meta\",\"payload\":{\"id\":\"s3\",\"model_provider\":\"ccswitch\"}}\n",
                "{\"type\":\"session_meta\",\"payload\":{\"id\":\"s4\",\"model_provider\":\"my-private-relay\"}}\n",
                "{\"type\":\"session_meta\",\"payload\":{\"id\":\"s5\",\"model_provider\":\"openai\"}}\n",
                "{\"type\":\"session_meta\",\"payload\":{\"id\":\"s6\",\"model_provider\":\"custom\"}}\n",
            ),
        )
        .expect("write session");

        let migrated_jsonl =
            migrate_codex_jsonl_files(&codex_dir, &source_provider_ids, &backup_root)
                .expect("migrate jsonl");
        assert_eq!(migrated_jsonl, 1);
        let session_text = fs::read_to_string(&session_path).expect("read session");
        assert_eq!(
            session_text
                .matches("\"model_provider\":\"custom\"")
                .count(),
            4
        );
        assert!(session_text.contains("\"model_provider\":\"my-private-relay\""));
        assert!(session_text.contains("\"model_provider\":\"openai\""));
        assert!(backup_root
            .join("jsonl/sessions/2026/05/28/local-sim.jsonl")
            .exists());

        let state_db_path = codex_dir.join(CODEX_STATE_DB_FILENAME);
        let conn = Connection::open(&state_db_path).expect("open state db");
        conn.execute_batch(
            "CREATE TABLE threads (
                id TEXT PRIMARY KEY,
                model_provider TEXT NOT NULL
            );
            INSERT INTO threads (id, model_provider) VALUES
                ('rightcode-thread', 'rightcode'),
                ('aihubmix-thread', 'aihubmix'),
                ('ccswitch-thread', 'ccswitch'),
                ('manual-thread', 'my-private-relay'),
                ('openai-thread', 'openai'),
                ('custom-thread', 'custom');",
        )
        .expect("seed state db");
        drop(conn);

        let migrated_state_rows = migrate_codex_state_db_provider_bucket(
            &state_db_path,
            &codex_dir,
            &source_provider_ids,
            &backup_root,
        )
        .expect("migrate state db");
        assert_eq!(migrated_state_rows, 3);

        let conn = Connection::open(&state_db_path).expect("reopen state db");
        let count_provider = |provider_id: &str| -> i64 {
            conn.query_row(
                "SELECT COUNT(*) FROM threads WHERE model_provider = ?1",
                [provider_id],
                |row| row.get(0),
            )
            .expect("count provider")
        };
        assert_eq!(count_provider("custom"), 4);
        assert_eq!(count_provider("my-private-relay"), 1);
        assert_eq!(count_provider("openai"), 1);
        assert!(backup_root
            .join("state")
            .join(CODEX_STATE_DB_FILENAME)
            .exists());
        drop(conn);

        let template_outcome = migrate_codex_provider_templates_to_custom(&db, &backup_root)
            .expect("migrate provider templates");
        assert!(!template_outcome
            .migrated_provider_ids
            .iter()
            .any(|id| id == "normalized-aihubmix"));
        assert_eq!(
            source_ids(
                &template_outcome
                    .migrated_provider_ids
                    .iter()
                    .map(String::as_str)
                    .collect::<Vec<_>>()
            ),
            source_ids(&["legacy-ccswitch", "rightcode"])
        );

        let config_provider_id = |provider_id: &str| -> String {
            db.get_provider_by_id(provider_id, "codex")
                .expect("get provider")
                .expect("provider exists")
                .settings_config
                .get("config")
                .and_then(Value::as_str)
                .expect("config text")
                .to_string()
        };

        let rightcode_config: toml::Value =
            toml::from_str(&config_provider_id("rightcode")).expect("parse rightcode config");
        assert_eq!(
            rightcode_config
                .get("model_provider")
                .and_then(|value| value.as_str()),
            Some("custom")
        );
        assert!(rightcode_config
            .get("model_providers")
            .and_then(|value| value.get("aihubmix"))
            .is_none());

        let ccswitch_config: toml::Value =
            toml::from_str(&config_provider_id("legacy-ccswitch")).expect("parse ccswitch config");
        assert_eq!(
            ccswitch_config
                .get("model_provider")
                .and_then(|value| value.as_str()),
            Some("custom")
        );
        assert!(ccswitch_config
            .get("model_providers")
            .and_then(|value| value.get("ccswitch"))
            .is_none());

        let manual_config: toml::Value =
            toml::from_str(&config_provider_id("manual-relay")).expect("parse manual config");
        assert_eq!(
            manual_config
                .get("model_provider")
                .and_then(|value| value.as_str()),
            Some("my-private-relay")
        );

        let openai_config: toml::Value =
            toml::from_str(&config_provider_id("custom-openai")).expect("parse openai config");
        assert_eq!(
            openai_config
                .get("model_provider")
                .and_then(|value| value.as_str()),
            Some("openai")
        );

        let normalized_config: toml::Value =
            toml::from_str(&config_provider_id("normalized-aihubmix"))
                .expect("parse normalized config");
        assert_eq!(
            normalized_config
                .get("model_provider")
                .and_then(|value| value.as_str()),
            Some("custom")
        );
    }

    #[test]
    fn simulates_official_history_unify_migration_end_to_end() {
        let dir = tempdir().expect("tempdir");
        let codex_dir = dir.path().join(".codex");
        let backup_root = dir.path().join("backup");
        fs::create_dir_all(&codex_dir).expect("create codex dir");

        let source_provider_ids = source_ids(&[OFFICIAL_OPENAI_CODEX_MODEL_PROVIDER_ID]);

        let session_dir = codex_dir.join("sessions/2026/06/12");
        fs::create_dir_all(&session_dir).expect("create session dir");
        let session_path = session_dir.join("official-sim.jsonl");
        fs::write(
            &session_path,
            concat!(
                "{\"type\":\"session_meta\",\"payload\":{\"id\":\"s1\",\"model_provider\":\"openai\"}}\n",
                "{\"type\":\"session_meta\",\"payload\":{\"id\":\"s2\",\"model_provider\":\"custom\"}}\n",
                "{\"type\":\"session_meta\",\"payload\":{\"id\":\"s3\",\"model_provider\":\"my-private-relay\"}}\n",
                "{\"type\":\"response_item\",\"payload\":{\"text\":\"openai\"}}\n",
            ),
        )
        .expect("write session");

        let migrated_jsonl =
            migrate_codex_jsonl_files(&codex_dir, &source_provider_ids, &backup_root)
                .expect("migrate jsonl");
        assert_eq!(migrated_jsonl, 1);
        let session_text = fs::read_to_string(&session_path).expect("read session");
        assert_eq!(
            session_text
                .matches("\"model_provider\":\"custom\"")
                .count(),
            2
        );
        assert!(!session_text.contains("\"model_provider\":\"openai\""));
        assert!(session_text.contains("\"model_provider\":\"my-private-relay\""));
        assert!(
            session_text.contains("{\"type\":\"response_item\",\"payload\":{\"text\":\"openai\"}}")
        );
        assert!(backup_root
            .join("jsonl/sessions/2026/06/12/official-sim.jsonl")
            .exists());

        // 第二次执行应当无事可做（幂等）
        let rerun = migrate_codex_jsonl_files(&codex_dir, &source_provider_ids, &backup_root)
            .expect("rerun migrate jsonl");
        assert_eq!(rerun, 0);

        let state_db_path = codex_dir.join(CODEX_STATE_DB_FILENAME);
        let conn = Connection::open(&state_db_path).expect("open state db");
        conn.execute_batch(
            "CREATE TABLE threads (
                id TEXT PRIMARY KEY,
                model_provider TEXT NOT NULL
            );
            INSERT INTO threads (id, model_provider) VALUES
                ('openai-thread', 'openai'),
                ('custom-thread', 'custom'),
                ('manual-thread', 'my-private-relay');",
        )
        .expect("seed state db");
        drop(conn);

        let migrated_state_rows = migrate_codex_state_db_provider_bucket(
            &state_db_path,
            &codex_dir,
            &source_provider_ids,
            &backup_root,
        )
        .expect("migrate state db");
        assert_eq!(migrated_state_rows, 1);

        let conn = Connection::open(&state_db_path).expect("reopen state db");
        let count_provider = |provider_id: &str| -> i64 {
            conn.query_row(
                "SELECT COUNT(*) FROM threads WHERE model_provider = ?1",
                [provider_id],
                |row| row.get(0),
            )
            .expect("count provider")
        };
        assert_eq!(count_provider("custom"), 2);
        assert_eq!(count_provider("openai"), 0);
        assert_eq!(count_provider("my-private-relay"), 1);
    }

    #[test]
    fn restores_only_ledgered_official_sessions_from_backups() {
        let dir = tempdir().expect("tempdir");
        let codex_dir = dir.path().join(".codex");
        let ledger_parent = dir.path().join("ledger");
        let restore_backup_root = dir.path().join("restore-backup");

        // 备份账本：一个代际，jsonl 备份里 s1 是 openai；state 备份里 t1 是 openai
        let generation = ledger_parent.join("20260612_010101");
        let backup_session_dir = generation.join("jsonl/sessions/2026/06/01");
        fs::create_dir_all(&backup_session_dir).expect("create backup session dir");
        fs::write(
            backup_session_dir.join("official.jsonl"),
            "{\"type\":\"session_meta\",\"payload\":{\"id\":\"s1\",\"model_provider\":\"openai\"}}\n",
        )
        .expect("write backup session");
        let backup_state_dir = generation.join("state");
        fs::create_dir_all(&backup_state_dir).expect("create backup state dir");
        let backup_db = Connection::open(backup_state_dir.join(CODEX_STATE_DB_FILENAME))
            .expect("open backup db");
        backup_db
            .execute_batch(
                "CREATE TABLE threads (id TEXT PRIMARY KEY, model_provider TEXT NOT NULL);
                INSERT INTO threads (id, model_provider) VALUES ('t1', 'openai');",
            )
            .expect("seed backup db");
        drop(backup_db);

        // 当前数据：s1（账本内，custom）应还原；s2（开启期间新会话，不在账本）
        // 与 s3（手工 relay）必须原样保留
        let session_dir = codex_dir.join("sessions/2026/06/01");
        fs::create_dir_all(&session_dir).expect("create session dir");
        let official_path = session_dir.join("official.jsonl");
        fs::write(
            &official_path,
            "{\"type\":\"session_meta\",\"payload\":{\"id\":\"s1\",\"model_provider\":\"custom\"}}\n",
        )
        .expect("write official session");
        let on_period_dir = codex_dir.join("sessions/2026/06/12");
        fs::create_dir_all(&on_period_dir).expect("create on-period dir");
        let on_period_path = on_period_dir.join("on-period.jsonl");
        fs::write(
            &on_period_path,
            concat!(
                "{\"type\":\"session_meta\",\"payload\":{\"id\":\"s2\",\"model_provider\":\"custom\"}}\n",
                "{\"type\":\"session_meta\",\"payload\":{\"id\":\"s3\",\"model_provider\":\"my-private-relay\"}}\n",
            ),
        )
        .expect("write on-period session");

        let state_db_path = codex_dir.join(CODEX_STATE_DB_FILENAME);
        let conn = Connection::open(&state_db_path).expect("open state db");
        conn.execute_batch(
            "CREATE TABLE threads (id TEXT PRIMARY KEY, model_provider TEXT NOT NULL);
            INSERT INTO threads (id, model_provider) VALUES
                ('t1', 'custom'),
                ('t2', 'custom'),
                ('t3', 'openai');",
        )
        .expect("seed state db");
        drop(conn);

        // 代际 meta 指向当前 Codex 目录：精确匹配分支生效（而非无 meta 的宽容分支）
        fs::write(
            generation.join("meta.json"),
            serde_json::to_vec_pretty(&serde_json::json!({
                "codexConfigDir": canonical_dir_string(&codex_dir)
            }))
            .expect("serialize meta"),
        )
        .expect("write meta");

        let outcome = restore_codex_official_history_inner(
            &codex_dir,
            &ledger_parent,
            &restore_backup_root,
            "",
        )
        .expect("restore");
        assert_eq!(outcome.restored_jsonl_files, 1);
        assert_eq!(outcome.restored_state_rows, 1);
        assert!(outcome.skipped_reason.is_none());

        let official_text = fs::read_to_string(&official_path).expect("read official");
        assert!(official_text.contains("\"model_provider\":\"openai\""));
        let on_period_text = fs::read_to_string(&on_period_path).expect("read on-period");
        assert!(on_period_text.contains("\"id\":\"s2\",\"model_provider\":\"custom\""));
        assert!(on_period_text.contains("\"model_provider\":\"my-private-relay\""));

        let conn = Connection::open(&state_db_path).expect("reopen state db");
        let provider_of = |thread_id: &str| -> String {
            conn.query_row(
                "SELECT model_provider FROM threads WHERE id = ?1",
                [thread_id],
                |row| row.get(0),
            )
            .expect("thread provider")
        };
        assert_eq!(provider_of("t1"), "openai");
        assert_eq!(provider_of("t2"), "custom");
        assert_eq!(provider_of("t3"), "openai");
        drop(conn);

        // 还原前的现场已备份到独立目录
        assert!(restore_backup_root
            .join("jsonl/sessions/2026/06/01/official.jsonl")
            .exists());
        assert!(restore_backup_root
            .join("state")
            .join(CODEX_STATE_DB_FILENAME)
            .exists());

        // 幂等：第二次还原无事可做
        let rerun = restore_codex_official_history_inner(
            &codex_dir,
            &ledger_parent,
            &dir.path().join("restore-backup-2"),
            "",
        )
        .expect("rerun restore");
        assert_eq!(rerun.restored_jsonl_files, 0);
        assert_eq!(rerun.restored_state_rows, 0);
        assert_eq!(rerun.skipped_reason.as_deref(), Some("nothing_to_restore"));
    }

    #[test]
    fn restore_ignores_backup_generations_from_other_codex_dirs() {
        let dir = tempdir().expect("tempdir");
        let codex_dir = dir.path().join(".codex");
        let ledger_parent = dir.path().join("ledger");

        // 账本代际属于另一个 Codex 目录
        let generation = ledger_parent.join("20260612_010101");
        let backup_session_dir = generation.join("jsonl/sessions/2026/06/01");
        fs::create_dir_all(&backup_session_dir).expect("create backup session dir");
        fs::write(
            backup_session_dir.join("official.jsonl"),
            "{\"type\":\"session_meta\",\"payload\":{\"id\":\"s1\",\"model_provider\":\"openai\"}}\n",
        )
        .expect("write backup session");
        fs::write(
            generation.join("meta.json"),
            "{\n  \"codexConfigDir\": \"/some/other/codex-dir\"\n}",
        )
        .expect("write meta");

        let session_dir = codex_dir.join("sessions/2026/06/01");
        fs::create_dir_all(&session_dir).expect("create session dir");
        let session_path = session_dir.join("official.jsonl");
        fs::write(
            &session_path,
            "{\"type\":\"session_meta\",\"payload\":{\"id\":\"s1\",\"model_provider\":\"custom\"}}\n",
        )
        .expect("write session");

        let outcome = restore_codex_official_history_inner(
            &codex_dir,
            &ledger_parent,
            &dir.path().join("restore-backup"),
            "",
        )
        .expect("restore");
        assert_eq!(outcome.skipped_reason.as_deref(), Some("no_backup_ledger"));
        let text = fs::read_to_string(&session_path).expect("read session");
        assert!(text.contains("\"model_provider\":\"custom\""));
    }

    #[test]
    fn backup_probe_only_counts_generations_for_current_dir() {
        let dir = tempdir().expect("tempdir");
        let ledger_parent = dir.path().join("ledger");
        let codex_dir_key = "/current/codex-dir";

        // 空父目录 / 父目录不存在：无备份
        assert!(!has_official_history_unify_backup_for_dir(
            &ledger_parent,
            codex_dir_key
        ));

        // 只有其他目录的代际：不算有备份
        let other = ledger_parent.join("20260612_010101");
        fs::create_dir_all(&other).expect("create generation");
        fs::write(
            other.join("meta.json"),
            "{\n  \"codexConfigDir\": \"/some/other/codex-dir\"\n}",
        )
        .expect("write meta");
        assert!(!has_official_history_unify_backup_for_dir(
            &ledger_parent,
            codex_dir_key
        ));

        // 无 meta 的早期代际：宽容接受（与 restore 的账本口径一致）
        fs::create_dir_all(ledger_parent.join("20260612_020202")).expect("create legacy gen");
        assert!(has_official_history_unify_backup_for_dir(
            &ledger_parent,
            codex_dir_key
        ));

        // 精确匹配当前目录的代际
        fs::remove_dir_all(ledger_parent.join("20260612_020202")).expect("remove legacy gen");
        let matched = ledger_parent.join("20260612_030303");
        fs::create_dir_all(&matched).expect("create matched gen");
        fs::write(
            matched.join("meta.json"),
            format!("{{\n  \"codexConfigDir\": \"{codex_dir_key}\"\n}}"),
        )
        .expect("write matched meta");
        assert!(has_official_history_unify_backup_for_dir(
            &ledger_parent,
            codex_dir_key
        ));
    }

    #[test]
    fn restore_skips_when_no_backup_ledger_exists() {
        let dir = tempdir().expect("tempdir");
        let codex_dir = dir.path().join(".codex");
        let session_dir = codex_dir.join("sessions/2026/06/01");
        fs::create_dir_all(&session_dir).expect("create session dir");
        fs::write(
            session_dir.join("session.jsonl"),
            "{\"type\":\"session_meta\",\"payload\":{\"id\":\"s1\",\"model_provider\":\"custom\"}}\n",
        )
        .expect("write session");

        let outcome = restore_codex_official_history_inner(
            &codex_dir,
            &dir.path().join("missing-ledger"),
            &dir.path().join("restore-backup"),
            "",
        )
        .expect("restore");
        assert_eq!(outcome.skipped_reason.as_deref(), Some("no_backup_ledger"));
        assert_eq!(outcome.restored_jsonl_files, 0);
        assert_eq!(outcome.restored_state_rows, 0);

        let text = fs::read_to_string(session_dir.join("session.jsonl")).expect("read session");
        assert!(text.contains("\"model_provider\":\"custom\""));
    }

    #[test]
    fn rewrites_only_codex_session_meta_provider_ids() {
        let dir = tempdir().expect("tempdir");
        let codex_dir = dir.path().join(".codex");
        let backup_root = dir.path().join("backup");
        let session_dir = codex_dir.join("sessions/2026/05/20");
        fs::create_dir_all(&session_dir).expect("create session dir");
        let path = session_dir.join("rollout-test.jsonl");
        fs::write(
            &path,
            concat!(
                "{\"type\":\"session_meta\",\"payload\":{\"id\":\"s1\",\"model_provider\":\"rightcode\"}}\n",
                "{\"type\":\"response_item\",\"payload\":{\"type\":\"message\",\"role\":\"user\",\"content\":\"hi\"}}\n"
            ),
        )
        .expect("write session");

        let changed = rewrite_codex_session_file_for_provider_bucket(
            &path,
            &codex_dir,
            &HashSet::from(["rightcode".to_string()]),
            &backup_root,
        )
        .expect("rewrite");

        assert!(changed);
        let next = fs::read_to_string(&path).expect("read rewritten");
        assert!(next.contains("\"model_provider\":\"custom\""));
        assert!(backup_root
            .join("jsonl/sessions/2026/05/20/rollout-test.jsonl")
            .exists());
    }

    #[test]
    fn does_not_rewrite_unknown_jsonl_history_without_trusted_source_id() {
        let dir = tempdir().expect("tempdir");
        let codex_dir = dir.path().join(".codex");
        let session_dir = codex_dir.join("sessions/2026/05/20");
        fs::create_dir_all(&session_dir).expect("create session dir");
        let path = session_dir.join("rollout-rightcode.jsonl");
        fs::write(
            &path,
            concat!(
                "{\"type\":\"session_meta\",\"payload\":{\"id\":\"s1\",\"model_provider\":\"rightcode\"}}\n",
                "{\"type\":\"response_item\",\"payload\":{\"type\":\"message\",\"role\":\"user\",\"content\":\"hi\"}}\n"
            ),
        )
        .expect("write session");

        let backup_root = dir.path().join("backup");
        let changed = migrate_codex_jsonl_files(
            &codex_dir,
            &source_ids(&["some-trusted-provider"]),
            &backup_root,
        )
        .expect("migrate jsonl");

        assert_eq!(changed, 0);
        let next = fs::read_to_string(&path).expect("read session");
        assert!(next.contains("\"model_provider\":\"rightcode\""));
        assert!(!backup_root.exists());
    }

    #[test]
    fn does_not_update_unknown_state_db_history_without_trusted_source_id() {
        let dir = tempdir().expect("tempdir");
        let codex_dir = dir.path().join(".codex");
        fs::create_dir_all(&codex_dir).expect("create codex dir");
        let db_path = codex_dir.join(CODEX_STATE_DB_FILENAME);
        let conn = Connection::open(&db_path).expect("open db");
        conn.execute_batch(
            "CREATE TABLE threads (
                id TEXT PRIMARY KEY,
                model_provider TEXT NOT NULL
            );
            INSERT INTO threads (id, model_provider) VALUES
                ('a', 'aihubmix'),
                ('b', 'openai'),
                ('c', 'custom');",
        )
        .expect("seed db");
        drop(conn);

        let backup_root = dir.path().join("backup");
        let changed = migrate_codex_state_db_provider_bucket(
            &db_path,
            &codex_dir,
            &source_ids(&["rightcode"]),
            &backup_root,
        )
        .expect("migrate state db");

        assert_eq!(changed, 0);
        let conn = Connection::open(&db_path).expect("reopen db");
        let aihubmix_count: i64 = conn
            .query_row(
                "SELECT COUNT(*) FROM threads WHERE model_provider = 'aihubmix'",
                [],
                |row| row.get(0),
            )
            .expect("count aihubmix");
        assert_eq!(aihubmix_count, 1);
        assert!(!backup_root.exists());
    }

    #[test]
    fn updates_codex_state_db_thread_provider_ids() {
        let dir = tempdir().expect("tempdir");
        let codex_dir = dir.path().join(".codex");
        fs::create_dir_all(&codex_dir).expect("create codex dir");
        let db_path = codex_dir.join(CODEX_STATE_DB_FILENAME);
        let conn = Connection::open(&db_path).expect("open db");
        conn.execute_batch(
            "CREATE TABLE threads (
                id TEXT PRIMARY KEY,
                model_provider TEXT NOT NULL
            );
            INSERT INTO threads (id, model_provider) VALUES
                ('a', 'rightcode'),
                ('b', 'openai'),
                ('c', 'aihubmix');",
        )
        .expect("seed db");
        drop(conn);

        let backup_root = dir.path().join("backup");
        let changed = migrate_codex_state_db_provider_bucket(
            &db_path,
            &codex_dir,
            &source_ids(&["rightcode", "aihubmix"]),
            &backup_root,
        )
        .expect("migrate state db");

        assert_eq!(changed, 2);
        let conn = Connection::open(&db_path).expect("reopen db");
        let custom_count: i64 = conn
            .query_row(
                "SELECT COUNT(*) FROM threads WHERE model_provider = 'custom'",
                [],
                |row| row.get(0),
            )
            .expect("count custom");
        let openai_count: i64 = conn
            .query_row(
                "SELECT COUNT(*) FROM threads WHERE model_provider = 'openai'",
                [],
                |row| row.get(0),
            )
            .expect("count openai");
        assert_eq!(custom_count, 2);
        assert_eq!(openai_count, 1);

        let backup_path = backup_root.join("state").join(CODEX_STATE_DB_FILENAME);
        let backup_conn = Connection::open(&backup_path).expect("open backup db");
        let backed_up_source_count: i64 = backup_conn
            .query_row(
                "SELECT COUNT(*) FROM threads WHERE model_provider IN ('rightcode', 'aihubmix')",
                [],
                |row| row.get(0),
            )
            .expect("count backed up source providers");
        assert_eq!(backed_up_source_count, 2);
    }

    #[test]
    fn collects_third_party_provider_ids_from_codex_providers() {
        let db = Database::memory().expect("memory db");
        let third_party = Provider::with_id(
            "rightcode".to_string(),
            "RightCode".to_string(),
            serde_json::json!({
                "auth": {},
                "config": "model_provider = \"aihubmix\"\n\n[model_providers.aihubmix]\nname = \"AIHubMix\"\nbase_url = \"https://example.com/v1\""
            }),
            None,
        );
        let mut official = Provider::with_id(
            "codex-official".to_string(),
            "OpenAI Official".to_string(),
            serde_json::json!({"auth": {}, "config": "model_provider = \"openai\""}),
            None,
        );
        official.category = Some("official".to_string());

        db.save_provider("codex", &third_party)
            .expect("save third-party");
        db.save_provider("codex", &official).expect("save official");

        let ids = collect_source_model_provider_ids(&db).expect("collect ids");
        assert!(ids.contains("rightcode"));
        assert!(ids.contains("aihubmix"));
        assert!(!ids.contains("openai"));
        assert!(!ids.contains("codex-official"));
    }

    #[test]
    fn skips_unknown_provider_model_provider_id_from_existing_config() {
        let db = Database::memory().expect("memory db");
        let mut provider = Provider::with_id(
            "manual-aggregator".to_string(),
            "Manual Aggregator".to_string(),
            serde_json::json!({
                "auth": {},
                "config": "model_provider = \"my-private-relay\"\n\n[model_providers.my-private-relay]\nname = \"Manual Relay\"\nbase_url = \"http://localhost:8080/v1\""
            }),
            None,
        );
        provider.category = Some("aggregator".to_string());

        db.save_provider("codex", &provider).expect("save provider");

        let ids = collect_source_model_provider_ids(&db).expect("collect ids");
        assert!(!ids.contains("my-private-relay"));
    }

    #[test]
    fn skips_undefined_provider_model_provider_id_from_existing_config() {
        let db = Database::memory().expect("memory db");
        let mut provider = Provider::with_id(
            "manual-aggregator".to_string(),
            "Manual Aggregator".to_string(),
            serde_json::json!({
                "auth": {},
                "config": "model_provider = \"my-private-relay\"\n"
            }),
            None,
        );
        provider.category = Some("aggregator".to_string());

        db.save_provider("codex", &provider).expect("save provider");

        let ids = collect_source_model_provider_ids(&db).expect("collect ids");
        assert!(!ids.contains("my-private-relay"));
    }

    #[test]
    fn skips_unknown_profile_model_provider_id_from_existing_config() {
        let db = Database::memory().expect("memory db");
        let mut provider = Provider::with_id(
            "manual-aggregator".to_string(),
            "Manual Aggregator".to_string(),
            serde_json::json!({
                "auth": {},
                "config": r#"profile = "work"

[model_providers.my-private-relay]
name = "Manual Relay"
base_url = "http://localhost:8080/v1"

[profiles.work]
model_provider = "my-private-relay"
"#
            }),
            None,
        );
        provider.category = Some("aggregator".to_string());

        db.save_provider("codex", &provider).expect("save provider");

        let ids = collect_source_model_provider_ids(&db).expect("collect ids");
        assert!(!ids.contains("my-private-relay"));
    }

    #[test]
    fn collects_known_legacy_provider_id_from_normalized_preset_config() {
        let db = Database::memory().expect("memory db");
        let mut provider = Provider::with_id(
            "generated-uuid".to_string(),
            "AIHubMix".to_string(),
            serde_json::json!({
                "auth": {},
                "config": "model_provider = \"custom\"\n\n[model_providers.custom]\nname = \"AIHubMix\"\nbase_url = \"https://aihubmix.example/v1\""
            }),
            None,
        );
        provider.category = Some("aggregator".to_string());

        db.save_provider("codex", &provider).expect("save provider");

        let ids = collect_source_model_provider_ids(&db).expect("collect ids");
        assert!(ids.contains("aihubmix"));
        assert!(!ids.contains("generated-uuid"));
    }

    #[test]
    fn collects_legacy_ccswitch_provider_id_from_stored_config() {
        let db = Database::memory().expect("memory db");
        let mut provider = Provider::with_id(
            "generated-uuid".to_string(),
            "Legacy Stable".to_string(),
            serde_json::json!({
                "auth": {},
                "config": "model_provider = \"ccswitch\"\n\n[model_providers.ccswitch]\nname = \"AIHubMix\"\nbase_url = \"https://aihubmix.example/v1\""
            }),
            None,
        );
        provider.category = Some("aggregator".to_string());

        db.save_provider("codex", &provider).expect("save provider");

        let ids = collect_source_model_provider_ids(&db).expect("collect ids");
        assert!(ids.contains("ccswitch"));
        assert!(ids.contains("aihubmix"));
        assert!(!ids.contains("generated-uuid"));
    }

    #[test]
    fn migrates_stored_provider_template_to_custom() {
        let db = Database::memory().expect("memory db");
        let provider = Provider::with_id(
            "legacy".to_string(),
            "Legacy Stable".to_string(),
            serde_json::json!({
                "auth": {},
                "config": r#"model_provider = "aihubmix"
model = "gpt-5.4"
profile = "work"

[model_providers.aihubmix]
name = "AIHubMix"
base_url = "https://aihubmix.example/v1"
wire_api = "responses"

[profiles.work]
model_provider = "aihubmix"
model = "gpt-5.4"
"#
            }),
            None,
        );
        db.save_provider("codex", &provider).expect("save provider");

        let (outcome, backup_dir) = migrate_provider_templates_for_test(&db);
        assert_eq!(outcome.migrated_provider_ids, vec!["legacy".to_string()]);

        let saved = db
            .get_provider_by_id("legacy", "codex")
            .expect("get provider")
            .expect("provider exists");
        let config_text = saved
            .settings_config
            .get("config")
            .and_then(Value::as_str)
            .expect("config text");
        let parsed: toml::Value = toml::from_str(config_text).expect("parse config");

        assert_eq!(
            parsed
                .get("model_provider")
                .and_then(|value| value.as_str()),
            Some("custom")
        );
        assert!(parsed
            .get("model_providers")
            .and_then(|value| value.get("aihubmix"))
            .is_none());
        assert_eq!(
            parsed
                .get("model_providers")
                .and_then(|value| value.get("custom"))
                .and_then(|value| value.get("base_url"))
                .and_then(|value| value.as_str()),
            Some("https://aihubmix.example/v1")
        );
        assert_eq!(
            parsed
                .get("profiles")
                .and_then(|value| value.get("work"))
                .and_then(|value| value.get("model_provider"))
                .and_then(|value| value.as_str()),
            Some("custom")
        );

        let backups: Vec<_> = fs::read_dir(backup_dir.path().join("providers"))
            .expect("provider backups")
            .flatten()
            .collect();
        assert_eq!(backups.len(), 1);
        let backup_text = fs::read_to_string(backups[0].path()).expect("read provider backup");
        assert!(backup_text.contains(r#""providerId": "legacy""#));
        assert!(backup_text.contains(r#"model_provider = \"aihubmix\""#));

        let (second, _second_backup_dir) = migrate_provider_templates_for_test(&db);
        assert!(second.migrated_provider_ids.is_empty());
    }

    #[test]
    fn migrates_legacy_ccswitch_provider_template_to_custom() {
        let db = Database::memory().expect("memory db");
        let provider = Provider::with_id(
            "legacy-ccswitch".to_string(),
            "Legacy CC Switch".to_string(),
            serde_json::json!({
                "auth": {},
                "config": r#"model_provider = "ccswitch"

[model_providers.ccswitch]
name = "AIHubMix"
base_url = "https://aihubmix.example/v1"
"#
            }),
            None,
        );
        db.save_provider("codex", &provider).expect("save provider");

        let (outcome, _backup_dir) = migrate_provider_templates_for_test(&db);
        assert_eq!(
            outcome.migrated_provider_ids,
            vec!["legacy-ccswitch".to_string()]
        );

        let saved = db
            .get_provider_by_id("legacy-ccswitch", "codex")
            .expect("get provider")
            .expect("provider exists");
        let config_text = saved
            .settings_config
            .get("config")
            .and_then(Value::as_str)
            .expect("config text");
        let parsed: toml::Value = toml::from_str(config_text).expect("parse config");

        assert_eq!(
            parsed
                .get("model_provider")
                .and_then(|value| value.as_str()),
            Some("custom")
        );
        assert!(parsed
            .get("model_providers")
            .and_then(|value| value.get("ccswitch"))
            .is_none());
        assert_eq!(
            parsed
                .get("model_providers")
                .and_then(|value| value.get("custom"))
                .and_then(|value| value.get("base_url"))
                .and_then(|value| value.as_str()),
            Some("https://aihubmix.example/v1")
        );
    }

    #[test]
    fn skips_unknown_stored_provider_template() {
        let db = Database::memory().expect("memory db");
        let provider = Provider::with_id(
            "manual".to_string(),
            "Manual Relay".to_string(),
            serde_json::json!({
                "auth": {},
                "config": r#"model_provider = "my-private-relay"

[model_providers.my-private-relay]
name = "Manual Relay"
base_url = "http://localhost:8080/v1"
"#
            }),
            None,
        );
        db.save_provider("codex", &provider).expect("save provider");

        let (outcome, _backup_dir) = migrate_provider_templates_for_test(&db);
        assert!(outcome.migrated_provider_ids.is_empty());

        let saved = db
            .get_provider_by_id("manual", "codex")
            .expect("get provider")
            .expect("provider exists");
        let config_text = saved
            .settings_config
            .get("config")
            .and_then(Value::as_str)
            .expect("config text");
        let parsed: toml::Value = toml::from_str(config_text).expect("parse config");

        assert_eq!(
            parsed
                .get("model_provider")
                .and_then(|value| value.as_str()),
            Some("my-private-relay")
        );
        assert_eq!(
            parsed
                .get("model_providers")
                .and_then(|value| value.get("my-private-relay"))
                .and_then(|value| value.get("base_url"))
                .and_then(|value| value.as_str()),
            Some("http://localhost:8080/v1")
        );
    }

    #[test]
    fn skips_reserved_key_in_non_official_stored_provider_template() {
        let db = Database::memory().expect("memory db");
        let provider = Provider::with_id(
            "custom-openai".to_string(),
            "Custom OpenAI".to_string(),
            serde_json::json!({
                "auth": {},
                "config": r#"model_provider = "openai"

[model_providers.openai]
name = "Custom OpenAI"
base_url = "https://proxy.example/v1"
"#
            }),
            None,
        );
        db.save_provider("codex", &provider).expect("save provider");

        let (outcome, _backup_dir) = migrate_provider_templates_for_test(&db);
        assert!(outcome.migrated_provider_ids.is_empty());

        let saved = db
            .get_provider_by_id("custom-openai", "codex")
            .expect("get provider")
            .expect("provider exists");
        let config_text = saved
            .settings_config
            .get("config")
            .and_then(Value::as_str)
            .expect("config text");
        let parsed: toml::Value = toml::from_str(config_text).expect("parse config");

        assert_eq!(
            parsed
                .get("model_provider")
                .and_then(|value| value.as_str()),
            Some("openai")
        );
        assert_eq!(
            parsed
                .get("model_providers")
                .and_then(|value| value.get("openai"))
                .and_then(|value| value.get("base_url"))
                .and_then(|value| value.as_str()),
            Some("https://proxy.example/v1")
        );
    }

    #[test]
    fn migrates_profile_model_provider_refs_to_custom_when_top_level_is_already_custom() {
        let db = Database::memory().expect("memory db");
        let provider = Provider::with_id(
            "profiled".to_string(),
            "Profiled Relay".to_string(),
            serde_json::json!({
                "auth": {},
                "config": r#"model_provider = "custom"
profile = "work"

[model_providers.custom]
name = "Current"
base_url = "https://current.example/v1"

[model_providers.aihubmix]
name = "AIHubMix"
base_url = "https://aihubmix.example/v1"

[profiles.work]
model_provider = "aihubmix"
"#
            }),
            None,
        );
        db.save_provider("codex", &provider).expect("save provider");

        let (outcome, _backup_dir) = migrate_provider_templates_for_test(&db);
        assert_eq!(outcome.migrated_provider_ids, vec!["profiled".to_string()]);

        let saved = db
            .get_provider_by_id("profiled", "codex")
            .expect("get provider")
            .expect("provider exists");
        let config_text = saved
            .settings_config
            .get("config")
            .and_then(Value::as_str)
            .expect("config text");
        let parsed: toml::Value = toml::from_str(config_text).expect("parse config");

        assert_eq!(
            parsed
                .get("profiles")
                .and_then(|value| value.get("work"))
                .and_then(|value| value.get("model_provider"))
                .and_then(|value| value.as_str()),
            Some("custom")
        );
        assert_eq!(
            parsed
                .get("model_providers")
                .and_then(|value| value.get("custom"))
                .and_then(|value| value.get("base_url"))
                .and_then(|value| value.as_str()),
            Some("https://current.example/v1")
        );
    }

    #[test]
    fn skips_custom_category_unknown_provider_when_created_by_cc_switch() {
        let db = Database::memory().expect("memory db");
        let mut provider = Provider::with_id(
            "generated-uuid".to_string(),
            "Manual Relay".to_string(),
            serde_json::json!({
                "auth": {},
                "config": "model_provider = \"my-private-relay\"\n\n[model_providers.my-private-relay]\nname = \"Manual Relay\"\nbase_url = \"http://localhost:8080/v1\""
            }),
            None,
        );
        provider.category = Some("custom".to_string());
        provider.created_at = Some(1);

        db.save_provider("codex", &provider).expect("save provider");

        let ids = collect_source_model_provider_ids(&db).expect("collect ids");
        assert!(!ids.contains("my-private-relay"));
        assert!(!ids.contains("generated-uuid"));
    }

    #[test]
    fn skips_custom_category_unknown_provider_model_provider_id() {
        let db = Database::memory().expect("memory db");
        let mut provider = Provider::with_id(
            "manual".to_string(),
            "Manual Relay".to_string(),
            serde_json::json!({
                "auth": {},
                "config": "model_provider = \"my-local-relay\"\n\n[model_providers.my-local-relay]\nname = \"Manual Relay\"\nbase_url = \"http://localhost:8080/v1\""
            }),
            None,
        );
        provider.category = Some("custom".to_string());

        db.save_provider("codex", &provider).expect("save provider");

        let ids = collect_source_model_provider_ids(&db).expect("collect ids");
        assert!(!ids.contains("my-local-relay"));
    }
}

//! Codex 第三方历史会话归桶迁移。
//!
//! 只迁移本机 `~/.codex` 历史数据；完成标记写入设备级 `settings.json`，
//! 失败时不写标记，下一次启动自动重试。

use crate::codex_config::{
    get_codex_config_dir, read_codex_config_text, CC_SWITCH_CODEX_MODEL_PROVIDER_ID,
    CC_SWITCH_CODEX_OFFICIAL_PROXY_PROVIDER_ID,
};
use crate::codex_rollout_file;
use crate::codex_state_db::{codex_state_db_is_lockable, codex_state_db_paths};
use crate::config::{atomic_write, copy_file, get_app_config_dir};
use crate::database::{is_official_seed_id, Database};
use crate::error::AppError;
use crate::settings::{
    CodexLegacyOfficialHistoryBucketMigration, CodexOfficialHistoryUnifyMigration,
    CodexProviderTemplateMigration, CodexThirdPartyHistoryProviderBucketMigration,
};
use chrono::{Local, Utc};
use rusqlite::{backup::Backup, params_from_iter, Connection};
use serde_json::Value;
use sha2::{Digest, Sha256};
use std::collections::{BTreeSet, HashSet};
use std::fs;
use std::hash::{Hash, Hasher};
use std::io::{Read, Seek, SeekFrom, Write};
use std::path::{Path, PathBuf};
use std::time::Duration;
use toml_edit::DocumentMut;

pub(crate) const MIGRATION_NAME: &str = "codex-history-provider-migration-v1";
pub(crate) const OFFICIAL_UNIFY_MIGRATION_NAME: &str = "codex-official-history-unify-v1";
/// 还原操作自身的备份目录（与迁移备份分开，保持迁移账本目录纯净）。
pub(crate) const OFFICIAL_UNIFY_RESTORE_BACKUP_NAME: &str =
    "codex-official-history-unify-restore-v1";

/// 串行化官方历史的迁移与还原：开启迁移（启动重试 + 设置保存后台任务）和
/// 关闭还原可能在毫秒级先后被触发，对同一批 jsonl / state DB 双向改写。
static CODEX_OFFICIAL_HISTORY_OP_LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());

fn lock_codex_official_history_op() -> std::sync::MutexGuard<'static, ()> {
    CODEX_OFFICIAL_HISTORY_OP_LOCK
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
}

/// 用户在「备份与恢复」里删除迁移备份目录时拿同一把锁，避免抽掉正在写的备份。
pub(crate) fn lock_history_op_for_backup_cleanup() -> std::sync::MutexGuard<'static, ()> {
    lock_codex_official_history_op()
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
    "etok",
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
];

#[derive(Debug, Clone, Default, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CodexHistoryProviderBucketMigrationOutcome {
    pub source_provider_ids: Vec<String>,
    pub migrated_jsonl_files: usize,
    pub migrated_state_rows: usize,
    pub skipped_jsonl_files: usize,
    pub skipped_state_rows: usize,
    pub retry_pending: bool,
    pub skipped_reason: Option<String>,
}

#[derive(Debug, Clone, Default)]
pub struct CodexProviderTemplateBucketMigrationOutcome {
    pub migrated_provider_ids: Vec<String>,
    pub skipped_reason: Option<String>,
}

/// Until an inventory can prove otherwise, saved history may still reference
/// an old route. Check only the presence of history stores, not their contents,
/// so switching a provider never scans or rewrites all saved conversations.
pub(crate) fn history_may_reference_legacy_routes() -> bool {
    let root = get_codex_config_dir();
    [root.join("sessions"), root.join("archived_sessions")]
        .into_iter()
        .chain(codex_state_db_paths(
            &root,
            &read_codex_config_text().unwrap_or_default(),
        ))
        .any(|path| path.try_exists().unwrap_or(true))
}

pub fn maybe_migrate_codex_third_party_history_provider_bucket(
    db: &Database,
) -> Result<CodexHistoryProviderBucketMigrationOutcome, AppError> {
    migrate_third_party_history(db, false)
}

/// Only called by the settings action after the user confirms moving managed
/// custom histories to the currently selected shared connection.
pub fn migrate_managed_codex_history_to_unified_bucket(
    db: &Database,
) -> Result<CodexHistoryProviderBucketMigrationOutcome, AppError> {
    migrate_third_party_history(db, true)
}

fn migrate_third_party_history(
    db: &Database,
    include_managed_custom: bool,
) -> Result<CodexHistoryProviderBucketMigrationOutcome, AppError> {
    let _op_guard = lock_codex_official_history_op();
    let codex_dir = get_codex_config_dir();
    let config_text = read_history_config(&codex_dir)?;
    let codex_dir_key = canonical_dir_string(&codex_dir);
    if !include_managed_custom
        && crate::settings::is_codex_third_party_history_provider_bucket_migrated_for_dir(
            &codex_dir_key,
        )
    {
        return Ok(CodexHistoryProviderBucketMigrationOutcome {
            skipped_reason: Some("already_migrated".to_string()),
            ..Default::default()
        });
    }
    if include_managed_custom
        && (!crate::settings::unify_codex_session_history()
            || !codex_config_text_routes_custom(&config_text))
    {
        return Err(AppError::Message(
            "Enable unified Codex session history and apply a shared connection before migrating"
                .into(),
        ));
    }
    let source_provider_ids =
        collect_source_model_provider_ids_for_unify(db, include_managed_custom, &codex_dir)?;
    if source_provider_ids.is_empty() {
        crate::settings::mark_codex_third_party_history_provider_bucket_migrated(
            CodexThirdPartyHistoryProviderBucketMigration {
                codex_config_dir: Some(canonical_dir_string(&codex_dir)),
                completed_at: Utc::now().to_rfc3339(),
                target_provider_id: CC_SWITCH_CODEX_MODEL_PROVIDER_ID.to_string(),
                source_provider_ids: remembered_source_provider_ids(&BTreeSet::new(), &codex_dir),
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

    if state_db_unavailable(&codex_dir, &config_text) {
        return Ok(CodexHistoryProviderBucketMigrationOutcome {
            skipped_reason: Some("state_db_unlockable".into()),
            retry_pending: true,
            ..Default::default()
        });
    }
    if include_managed_custom {
        // Unknown index layouts need manual recovery; do not migrate files and
        // only then discover that their corresponding rows cannot be verified.
        for path in codex_state_db_paths(&codex_dir, &config_text) {
            if !path.exists() {
                continue;
            }
            let conn =
                Connection::open_with_flags(&path, rusqlite::OpenFlags::SQLITE_OPEN_READ_ONLY)?;
            if !Database::table_exists(&conn, "threads")?
                || !Database::has_column(&conn, "threads", "rollout_path")?
            {
                return Err(AppError::Message(format!(
                    "Codex index layout cannot be verified safely: {}. Restore the original provider definition instead", path.display()
                )));
            }
        }
    }
    let backup_root = migration_backup_root(MIGRATION_NAME);
    let files = migrate_codex_jsonl_files_to(
        &codex_dir,
        &source_provider_ids,
        CC_SWITCH_CODEX_MODEL_PROVIDER_ID,
        &backup_root,
    )?;
    let state = migrate_codex_state_dbs_to(
        &codex_dir,
        &source_provider_ids,
        CC_SWITCH_CODEX_MODEL_PROVIDER_ID,
        &backup_root,
        &files.skipped_paths,
        &config_text,
    )?;
    let migrated_jsonl_files = files.migrated;
    let migrated_state_rows = state.migrated;

    let source_provider_ids_vec: Vec<String> = source_provider_ids.iter().cloned().collect();
    crate::settings::mark_codex_third_party_history_provider_bucket_migrated(
        CodexThirdPartyHistoryProviderBucketMigration {
            codex_config_dir: Some(canonical_dir_string(&codex_dir)),
            completed_at: Utc::now().to_rfc3339(),
            target_provider_id: CC_SWITCH_CODEX_MODEL_PROVIDER_ID.to_string(),
            source_provider_ids: remembered_source_provider_ids(&source_provider_ids, &codex_dir),
            migrated_jsonl_files,
            migrated_state_rows,
            scanned_history_files: !files.retry_pending && !state.retry_pending,
        },
    )?;

    Ok(CodexHistoryProviderBucketMigrationOutcome {
        source_provider_ids: source_provider_ids_vec,
        migrated_jsonl_files,
        migrated_state_rows,
        skipped_jsonl_files: files.skipped_paths.len(),
        skipped_state_rows: state.skipped,
        retry_pending: files.retry_pending || state.retry_pending,
        skipped_reason: None,
    })
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
    migrate_official_history_with_lock(lock_codex_official_history_op)
}

fn migrate_official_history_with_lock(
    acquire: impl FnOnce() -> std::sync::MutexGuard<'static, ()>,
) -> Result<CodexHistoryProviderBucketMigrationOutcome, AppError> {
    let _op_guard = acquire();
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
    let codex_dir = get_codex_config_dir();
    let config_text = read_history_config(&codex_dir)?;
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
    // `inject_codex_unified_session_bucket`），live 也可能还没按开关重写。
    // 这些状态下新会话仍落 "openai" 桶，迁移只会把历史搬进当前 live 看不见的
    // 桶里。开关与迁移意愿保持不动，待 live 真正统一后（下次切换 / 启动重试）
    // 再迁。
    if !codex_config_text_routes_custom(&config_text) {
        return Ok(CodexHistoryProviderBucketMigrationOutcome {
            skipped_reason: Some("live_not_unified".to_string()),
            ..Default::default()
        });
    }

    let source_provider_ids: BTreeSet<String> =
        std::iter::once(OFFICIAL_OPENAI_CODEX_MODEL_PROVIDER_ID.to_string()).collect();
    if state_db_unavailable(&codex_dir, &config_text) {
        return Ok(CodexHistoryProviderBucketMigrationOutcome {
            skipped_reason: Some("state_db_unlockable".into()),
            retry_pending: true,
            ..Default::default()
        });
    }
    let backup_root = migration_backup_root(OFFICIAL_UNIFY_MIGRATION_NAME);
    let files = migrate_codex_jsonl_files_to(
        &codex_dir,
        &source_provider_ids,
        CC_SWITCH_CODEX_MODEL_PROVIDER_ID,
        &backup_root,
    )?;
    let state = migrate_codex_state_dbs_to(
        &codex_dir,
        &source_provider_ids,
        CC_SWITCH_CODEX_MODEL_PROVIDER_ID,
        &backup_root,
        &files.skipped_paths,
        &config_text,
    )?;
    let migrated_jsonl_files = files.migrated;
    let migrated_state_rows = state.migrated;
    // 备份代际记录来源目录，restore 据此只取当前目录的账本。
    write_backup_generation_meta(&backup_root, &codex_dir_key)?;

    let outcome = CodexHistoryProviderBucketMigrationOutcome {
        source_provider_ids: source_provider_ids.into_iter().collect(),
        migrated_jsonl_files,
        migrated_state_rows,
        skipped_jsonl_files: files.skipped_paths.len(),
        skipped_state_rows: state.skipped,
        retry_pending: files.retry_pending || state.retry_pending,
        skipped_reason: None,
    };

    // 条件写入在 settings 写锁内原子完成："迁移期间开关被关掉"时不写完成标记，
    // 避免下一次开启被标记挡住而漏迁"关闭期间"新产生的 openai 桶会话。
    // 与关闭路径（update_settings + 清标记）共用同一把锁，无检查-写入窗口。
    if outcome.retry_pending {
        return Ok(outcome);
    }
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

/// v3.17–v3.20 官方账号走本地路由时，会话记在 `cc-switch-official` 桶下；v4 起这张表
/// 不再写、旧表被清理，这些会话恢复时报「Model provider `cc-switch-official` not found」
/// （#7948）。它们用的是官方登录、表名本就是 "OpenAI"，迁回内置 "openai" 桶后能力不变，
/// 且这个 id 永远能解析。统一会话开关开着时，随后的统一迁移会再把它们按正常流程搬进
/// custom 桶（备份里记为 openai，关开关时可精确还原），所以这里不看开关。
///
/// config.toml 一个字节都不动。当前选路或某个 profile 仍选着这个 id（用户手动修过）时
/// 不迁；state DB 打不开（WSL 路径）时不迁也不写标记，只改 jsonl 恢复时仍会报错。
pub fn maybe_migrate_codex_legacy_official_history_to_openai_bucket(
) -> Result<CodexHistoryProviderBucketMigrationOutcome, AppError> {
    let _op_guard = lock_codex_official_history_op();
    let codex_dir = get_codex_config_dir();
    let codex_dir_key = canonical_dir_string(&codex_dir);
    if crate::settings::is_codex_legacy_official_history_bucket_migrated_for_dir(&codex_dir_key) {
        return Ok(CodexHistoryProviderBucketMigrationOutcome {
            skipped_reason: Some("already_migrated".to_string()),
            ..Default::default()
        });
    }

    let config_text = read_history_config(&codex_dir)?;
    if codex_config_selects_provider(&config_text, CC_SWITCH_CODEX_OFFICIAL_PROXY_PROVIDER_ID) {
        return Ok(CodexHistoryProviderBucketMigrationOutcome {
            skipped_reason: Some("legacy_route_in_use".to_string()),
            ..Default::default()
        });
    }
    if state_db_unavailable(&codex_dir, &config_text) {
        return Ok(CodexHistoryProviderBucketMigrationOutcome {
            skipped_reason: Some("state_db_unlockable".to_string()),
            retry_pending: true,
            ..Default::default()
        });
    }

    let source_provider_ids: BTreeSet<String> =
        std::iter::once(CC_SWITCH_CODEX_OFFICIAL_PROXY_PROVIDER_ID.to_string()).collect();
    let backup_root = migration_backup_root(MIGRATION_NAME);
    let files = migrate_codex_jsonl_files_to(
        &codex_dir,
        &source_provider_ids,
        OFFICIAL_OPENAI_CODEX_MODEL_PROVIDER_ID,
        &backup_root,
    )?;
    let state = migrate_codex_state_dbs_to(
        &codex_dir,
        &source_provider_ids,
        OFFICIAL_OPENAI_CODEX_MODEL_PROVIDER_ID,
        &backup_root,
        &files.skipped_paths,
        &config_text,
    )?;
    let migrated_jsonl_files = files.migrated;
    let migrated_state_rows = state.migrated;

    if !files.retry_pending && !state.retry_pending {
        crate::settings::mark_codex_legacy_official_history_bucket_migrated(
            CodexLegacyOfficialHistoryBucketMigration {
                completed_at: Utc::now().to_rfc3339(),
                target_provider_id: OFFICIAL_OPENAI_CODEX_MODEL_PROVIDER_ID.to_string(),
                migrated_jsonl_files,
                migrated_state_rows,
                codex_config_dir: Some(codex_dir_key),
            },
        )?;
    }

    Ok(CodexHistoryProviderBucketMigrationOutcome {
        source_provider_ids: source_provider_ids.into_iter().collect(),
        migrated_jsonl_files,
        migrated_state_rows,
        skipped_jsonl_files: files.skipped_paths.len(),
        skipped_state_rows: state.skipped,
        retry_pending: files.retry_pending || state.retry_pending,
        skipped_reason: None,
    })
}

/// 顶层 `model_provider` 或任一 `[profiles.*]` 的 `model_provider` 是否选着 `provider_id`。
fn codex_config_selects_provider(config_text: &str, provider_id: &str) -> bool {
    let Ok(doc) = config_text.parse::<DocumentMut>() else {
        return false;
    };
    let selects = |item: Option<&toml_edit::Item>| {
        item.and_then(|item| item.as_str())
            .is_some_and(|id| id.trim() == provider_id)
    };
    selects(doc.get("model_provider"))
        || doc
            .get("profiles")
            .and_then(|item| item.as_table_like())
            .is_some_and(|profiles| {
                profiles.iter().any(|(_, profile)| {
                    profile
                        .as_table_like()
                        .is_some_and(|profile| selects(profile.get("model_provider")))
                })
            })
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
    pub skipped_jsonl_files: usize,
    pub skipped_state_rows: usize,
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
    let codex_dir = get_codex_config_dir();
    let config_text = read_history_config(&codex_dir)?;
    restore_codex_official_history_inner(
        &codex_dir,
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

    if state_db_unavailable(codex_dir, config_text) {
        return Ok(CodexOfficialHistoryRestoreOutcome {
            skipped_reason: Some("state_db_unlockable".into()),
            ..Default::default()
        });
    }
    let mut files = Vec::new();
    collect_jsonl_files(&codex_dir.join("sessions"), &mut files, 0, 8)?;
    collect_jsonl_files(&codex_dir.join("archived_sessions"), &mut files, 0, 4)?;
    let mut restored_jsonl_files = 0;
    let mut skipped_rollouts = HashSet::new();
    for file_path in codex_rollout_file::dedupe_siblings(files) {
        match rewrite_codex_session_file_lines(&file_path, codex_dir, restore_backup_root, |line| {
            rewrite_codex_session_meta_line_for_restore(line, &official_session_ids)
        }) {
            Ok(changed) => restored_jsonl_files += usize::from(changed),
            Err(error) => {
                log::warn!(
                    "Skipping Codex history restore file {}: {error}",
                    file_path.display()
                );
                skipped_rollouts.insert(fs::canonicalize(&file_path).unwrap_or(file_path));
            }
        }
    }

    let mut restored_state_rows = 0;
    let mut skipped_state_rows = 0;
    let source_ids = BTreeSet::from([CC_SWITCH_CODEX_MODEL_PROVIDER_ID.to_string()]);
    for db_path in codex_state_db_paths(codex_dir, config_text) {
        if !codex_state_db_is_lockable(&db_path) {
            continue;
        }
        let state = migrate_codex_state_db_provider_bucket_to(
            &db_path,
            codex_dir,
            &source_ids,
            OFFICIAL_OPENAI_CODEX_MODEL_PROVIDER_ID,
            restore_backup_root,
            &skipped_rollouts,
            Some(&official_thread_ids),
        )?;
        restored_state_rows += state.migrated;
        skipped_state_rows += state.skipped;
    }

    if restored_jsonl_files == 0
        && restored_state_rows == 0
        && skipped_rollouts.is_empty()
        && skipped_state_rows == 0
    {
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
        skipped_jsonl_files: skipped_rollouts.len(),
        skipped_state_rows,
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
        collect_jsonl_files(&generation.join("jsonl"), &mut backup_files, 0, 10)?;
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
    let Ok(content) = codex_rollout_file::read_to_string(path) else {
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
    if depth > max_depth
        || !dir.is_dir()
        || fs::symlink_metadata(dir).is_ok_and(|m| m.file_type().is_symlink())
    {
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
) -> Result<Option<String>, AppError> {
    if !line.contains("\"session_meta\"") || !line.contains("\"model_provider\"") {
        return Ok(None);
    }
    let Ok(value) = serde_json::from_str::<Value>(line) else {
        if line.contains("\"session_meta\"") && line.contains("\"model_provider\"") {
            return Err(AppError::Message(
                "Incomplete or invalid Codex metadata; retry after the rollout is idle".into(),
            ));
        }
        return Ok(None);
    };
    if value.get("type").and_then(Value::as_str) != Some("session_meta") {
        return Ok(None);
    }
    let Some(payload) = value.get("payload") else {
        return Ok(None);
    };
    if payload.get("model_provider").and_then(Value::as_str)
        != Some(CC_SWITCH_CODEX_MODEL_PROVIDER_ID)
        || !payload
            .get("id")
            .and_then(Value::as_str)
            .is_some_and(|id| official_session_ids.contains(id))
    {
        return Ok(None);
    }
    replace_metadata_provider(line, &value, OFFICIAL_OPENAI_CODEX_MODEL_PROVIDER_ID).map(Some)
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

#[cfg(test)]
fn collect_source_model_provider_ids(db: &Database) -> Result<BTreeSet<String>, AppError> {
    collect_source_model_provider_ids_for_unify(db, false, &get_codex_config_dir())
}

fn collect_source_model_provider_ids_for_unify(
    db: &Database,
    include_managed_custom: bool,
    codex_dir: &Path,
) -> Result<BTreeSet<String>, AppError> {
    let providers = db.get_all_providers("codex")?;
    let mut ids = BTreeSet::new();
    if include_managed_custom {
        ids.extend(remembered_source_provider_ids(&BTreeSet::new(), codex_dir));
    }

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

        if include_managed_custom {
            if let Ok(doc) = config_text.parse::<DocumentMut>() {
                if let Some(id) = doc
                    .get("model_provider")
                    .and_then(|v| v.as_str())
                    .map(str::trim)
                {
                    if is_migratable_custom_id(id) && config_defines_model_provider(&doc, id) {
                        ids.insert(id.to_string());
                    }
                }
            }
        }

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

fn remembered_source_provider_ids(current: &BTreeSet<String>, codex_dir: &Path) -> Vec<String> {
    let mut ids = current.clone();
    if let Some(previous) = crate::settings::codex_third_party_history_migration_for_dir(
        &canonical_dir_string(codex_dir),
    ) {
        ids.extend(
            previous
                .source_provider_ids
                .into_iter()
                .filter(|id| is_migratable_custom_id(id)),
        );
    }
    ids.into_iter().collect()
}

fn is_migratable_custom_id(id: &str) -> bool {
    !id.trim().is_empty()
        && !crate::live::project::codex::BUILT_IN_IDS.contains(&id)
        && !["custom", "cc-switch-official", "codex_local_access"].contains(&id)
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
        .join(Local::now().format("%Y%m%d_%H%M%S_%f").to_string())
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

#[derive(Default)]
struct RolloutMigrationReport {
    migrated: usize,
    skipped_paths: HashSet<PathBuf>,
    retry_pending: bool,
}

#[derive(Default)]
struct StateMigrationReport {
    migrated: usize,
    skipped: usize,
    retry_pending: bool,
}

#[cfg(test)]
fn migrate_codex_jsonl_files(
    codex_dir: &Path,
    source_provider_ids: &BTreeSet<String>,
    backup_root: &Path,
) -> Result<usize, AppError> {
    migrate_codex_jsonl_files_to(
        codex_dir,
        source_provider_ids,
        CC_SWITCH_CODEX_MODEL_PROVIDER_ID,
        backup_root,
    )
    .map(|report| report.migrated)
}

fn migrate_codex_jsonl_files_to(
    codex_dir: &Path,
    source_provider_ids: &BTreeSet<String>,
    target_provider_id: &str,
    backup_root: &Path,
) -> Result<RolloutMigrationReport, AppError> {
    let mut files = Vec::new();
    collect_jsonl_files(&codex_dir.join("sessions"), &mut files, 0, 8)?;
    collect_jsonl_files(&codex_dir.join("archived_sessions"), &mut files, 0, 4)?;

    let source_provider_ids: HashSet<String> = source_provider_ids.iter().cloned().collect();
    let mut report = RolloutMigrationReport::default();
    for file_path in codex_rollout_file::dedupe_siblings(files) {
        match rewrite_codex_session_file_lines(&file_path, codex_dir, backup_root, |line| {
            rewrite_codex_session_meta_line(line, &source_provider_ids, target_provider_id)
        }) {
            Ok(changed) => report.migrated += usize::from(changed),
            Err(error) => {
                report.retry_pending |= !matches!(error, AppError::InvalidInput(_));
                log::warn!(
                    "Skipping Codex history file {}: {error}",
                    file_path.display()
                );
                report
                    .skipped_paths
                    .insert(fs::canonicalize(&file_path).unwrap_or(file_path));
            }
        }
    }
    Ok(report)
}

fn collect_jsonl_files(
    dir: &Path,
    files: &mut Vec<PathBuf>,
    depth: u8,
    max_depth: u8,
) -> Result<(), AppError> {
    let metadata = match fs::symlink_metadata(dir) {
        Ok(metadata) => metadata,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(()),
        Err(error) => return Err(AppError::io(dir, error)),
    };
    if metadata.file_type().is_symlink() {
        return Ok(());
    }
    if !metadata.is_dir() || depth > max_depth {
        return Err(AppError::Message(format!(
            "Cannot completely scan Codex history directory: {}",
            dir.display()
        )));
    }
    for entry in fs::read_dir(dir).map_err(|error| AppError::io(dir, error))? {
        let entry = entry.map_err(|error| AppError::io(dir, error))?;
        let path = entry.path();
        let kind = entry
            .file_type()
            .map_err(|error| AppError::io(&path, error))?;
        if kind.is_symlink() {
            continue;
        }
        if kind.is_dir() {
            collect_jsonl_files(&path, files, depth + 1, max_depth)?;
        } else if codex_rollout_file::is_rollout_file(&path) {
            files.push(path);
        }
    }
    Ok(())
}

#[cfg(test)]
fn rewrite_codex_session_file_for_provider_bucket(
    path: &Path,
    codex_dir: &Path,
    source_provider_ids: &HashSet<String>,
    backup_root: &Path,
) -> Result<bool, AppError> {
    rewrite_codex_session_file_lines(path, codex_dir, backup_root, |line| {
        rewrite_codex_session_meta_line(
            line,
            source_provider_ids,
            CC_SWITCH_CODEX_MODEL_PROVIDER_ID,
        )
    })
}

/// Byte offsets refer to uncompressed JSONL; changing even one header length
/// invalidates every subsequent paginated item. Never reserialize a whole file.
fn rewrite_codex_session_file_lines(
    path: &Path,
    codex_dir: &Path,
    backup_root: &Path,
    rewrite_line: impl Fn(&str) -> Result<Option<String>, AppError>,
) -> Result<bool, AppError> {
    // Retain the original handle: Windows creation times can be reused by
    // filesystem tunneling and cannot identify a replacement at the same path.
    let original = fs::File::open(path).map_err(|e| AppError::io(path, e))?;
    let metadata_before = original.metadata().map_err(|e| AppError::io(path, e))?;
    let content = codex_rollout_file::read_to_string(path).map_err(|e| AppError::io(path, e))?;
    let mut edits = Vec::new();
    let mut offset = 0usize;
    for segment in content.split_inclusive('\n') {
        let line = segment.strip_suffix('\n').unwrap_or(segment);
        if let Some(next) = rewrite_line(line)? {
            if next.len() != line.len() {
                return Err(AppError::Message(
                    "Codex metadata patch would invalidate paginated byte offsets".into(),
                ));
            }
            edits.push((offset, line.as_bytes().to_vec(), next.into_bytes()));
        }
        offset += segment.len();
    }
    if edits.is_empty() {
        return Ok(false);
    }

    if codex_rollout_file::is_compressed(path) {
        ensure_codex_session_file_unchanged(path, &original, &metadata_before)?;
        if codex_rollout_file::sibling(path).is_some_and(|p| p.exists()) {
            return Err(AppError::Message(
                "Codex rollout is being decompressed; retry after it is idle".into(),
            ));
        }
        backup_codex_jsonl_file(path, codex_dir, backup_root)?;
        let mut rewritten = content.into_bytes();
        for (offset, _, next) in &edits {
            rewritten[*offset..*offset + next.len()].copy_from_slice(next);
        }
        ensure_codex_session_file_unchanged(path, &original, &metadata_before)?;
        if codex_rollout_file::sibling(path).is_some_and(|p| p.exists()) {
            return Err(AppError::Message(
                "Codex rollout changed representation during migration".into(),
            ));
        }
        codex_rollout_file::write_rollout(path, &rewritten)?;
    } else {
        backup_codex_jsonl_file(path, codex_dir, backup_root)?;
        let mut file = fs::OpenOptions::new()
            .read(true)
            .write(true)
            .open(path)
            .map_err(|e| AppError::io(path, e))?;
        apply_plain_metadata_edits(path, &mut file, &original, content.len() as u64, &edits)?;
    }
    Ok(true)
}

type MetadataEdit = (usize, Vec<u8>, Vec<u8>);

fn same_file_identity(left: &fs::File, right: &fs::File) -> bool {
    #[cfg(unix)]
    {
        use std::os::unix::fs::MetadataExt;
        match (left.metadata(), right.metadata()) {
            (Ok(left), Ok(right)) => left.dev() == right.dev() && left.ino() == right.ino(),
            _ => false,
        }
    }
    #[cfg(windows)]
    {
        use std::os::windows::io::AsRawHandle;
        use windows_sys::Win32::Storage::FileSystem::{
            FileIdInfo, GetFileInformationByHandleEx, FILE_ID_INFO,
        };
        fn identity(file: &fs::File) -> Option<(u64, [u8; 16])> {
            let mut info = FILE_ID_INFO::default();
            // SAFETY: the file owns a valid handle throughout this call; info
            // is a writable FILE_ID_INFO buffer with the supplied exact size.
            let ok = unsafe {
                GetFileInformationByHandleEx(
                    file.as_raw_handle(),
                    FileIdInfo,
                    std::ptr::addr_of_mut!(info).cast(),
                    std::mem::size_of::<FILE_ID_INFO>() as u32,
                )
            };
            if ok == 0 {
                log::warn!(
                    "Cannot verify Codex rollout file identity: {}",
                    std::io::Error::last_os_error()
                );
                return None;
            }
            Some((info.VolumeSerialNumber, info.FileId.Identifier))
        }
        match (identity(left), identity(right)) {
            (Some(left), Some(right)) => left == right,
            _ => false,
        }
    }
    #[cfg(not(any(unix, windows)))]
    {
        let _ = (left, right);
        false
    }
}

fn apply_plain_metadata_edits(
    path: &Path,
    file: &mut fs::File,
    original: &fs::File,
    original_len: u64,
    edits: &[MetadataEdit],
) -> Result<(), AppError> {
    let check = |file: &fs::File| -> Result<(), AppError> {
        let opened = file.metadata().map_err(|e| AppError::io(path, e))?;
        let current = fs::File::open(path).map_err(|e| AppError::io(path, e))?;
        let current_metadata = current.metadata().map_err(|e| AppError::io(path, e))?;
        if opened.len() < original_len
            || current_metadata.len() < original_len
            || !same_file_identity(original, file)
            || !same_file_identity(file, &current)
        {
            return Err(AppError::Message(format!(
                "Codex rollout replaced or truncated during migration: {}",
                path.display()
            )));
        }
        Ok(())
    };
    check(file)?;
    // Validate every old range before the first write; tolerate only tail appends.
    for (offset, old, next) in edits {
        if old.len() != next.len() {
            return Err(AppError::Message(
                "Unequal Codex metadata patch lengths".into(),
            ));
        }
        file.seek(SeekFrom::Start(*offset as u64))
            .map_err(|e| AppError::io(path, e))?;
        let mut bytes = vec![0; old.len()];
        file.read_exact(&mut bytes)
            .map_err(|e| AppError::io(path, e))?;
        if bytes != *old {
            return Err(AppError::Message(format!(
                "Codex metadata changed during migration: {}",
                path.display()
            )));
        }
    }
    for (offset, old, next) in edits {
        check(file)?;
        file.seek(SeekFrom::Start(*offset as u64))
            .map_err(|e| AppError::io(path, e))?;
        let mut bytes = vec![0; old.len()];
        file.read_exact(&mut bytes)
            .map_err(|e| AppError::io(path, e))?;
        if bytes != *old {
            return Err(AppError::Message(
                "Codex metadata changed before write; retry migration".into(),
            ));
        }
        file.seek(SeekFrom::Start(*offset as u64))
            .map_err(|e| AppError::io(path, e))?;
        file.write_all(next).map_err(|e| AppError::io(path, e))?;
    }
    file.sync_data().map_err(|e| AppError::io(path, e))?;
    check(file)
}

fn ensure_codex_session_file_unchanged(
    path: &Path,
    original: &fs::File,
    metadata_before: &fs::Metadata,
) -> Result<(), AppError> {
    let current = fs::File::open(path).map_err(|e| AppError::io(path, e))?;
    let metadata_after = current.metadata().map_err(|e| AppError::io(path, e))?;
    if !same_file_identity(original, &current)
        || metadata_after.modified().ok() != metadata_before.modified().ok()
        || metadata_after.len() != metadata_before.len()
    {
        return Err(AppError::Message(format!(
            "Codex session file changed during migration: {}",
            path.display()
        )));
    }
    Ok(())
}

fn replace_metadata_provider(line: &str, before: &Value, target: &str) -> Result<String, AppError> {
    static FIELD: std::sync::LazyLock<regex::Regex> = std::sync::LazyLock::new(|| {
        regex::Regex::new(r#""model_provider"\s*:\s*("(?:\\.|[^"\\])*")[ \t]*"#)
            .expect("static JSON field regex")
    });
    let fields: Vec<_> = FIELD.captures_iter(line).collect();
    if fields.len() != 1 {
        return Err(AppError::InvalidInput(
            "Ambiguous Codex provider metadata; refusing migration".into(),
        ));
    }
    let field = &fields[0];
    let value = field.get(1).expect("capture");
    let end = field.get(0).expect("match").end();
    let replacement =
        serde_json::to_string(target).map_err(|e| AppError::JsonSerialize { source: e })?;
    if replacement.len() > end - value.start() {
        return Err(AppError::InvalidInput("Codex provider id cannot fit without changing paginated offsets; use native resume or an explicit index migration".into()));
    }
    let mut next = line.to_string();
    let padded = format!(
        "{replacement}{}",
        " ".repeat(end - value.start() - replacement.len())
    );
    next.replace_range(value.start()..end, &padded);
    let mut expected = before.clone();
    expected["payload"]["model_provider"] = Value::String(target.to_string());
    if next.len() != line.len()
        || serde_json::from_str::<Value>(&next).ok().as_ref() != Some(&expected)
    {
        return Err(AppError::InvalidInput(
            "Codex metadata patch changed non-provider data".into(),
        ));
    }
    Ok(next)
}

fn rewrite_codex_session_meta_line(
    line: &str,
    source_provider_ids: &HashSet<String>,
    target_provider_id: &str,
) -> Result<Option<String>, AppError> {
    if !line.contains("\"session_meta\"") || !line.contains("\"model_provider\"") {
        return Ok(None);
    }
    let Ok(value) = serde_json::from_str::<Value>(line) else {
        if line.contains("\"session_meta\"") && line.contains("\"model_provider\"") {
            return Err(AppError::Message(
                "Incomplete or invalid Codex metadata; retry after the rollout is idle".into(),
            ));
        }
        return Ok(None);
    };
    if value.get("type").and_then(Value::as_str) != Some("session_meta") {
        return Ok(None);
    }
    let Some(provider) = value
        .get("payload")
        .and_then(|v| v.get("model_provider"))
        .and_then(Value::as_str)
    else {
        return Ok(None);
    };
    if provider == target_provider_id || !source_provider_ids.contains(provider) {
        return Ok(None);
    }
    replace_metadata_provider(line, &value, target_provider_id).map(Some)
}

fn read_history_config(codex_dir: &Path) -> Result<String, AppError> {
    let path = codex_dir.join("config.toml");
    let text = match fs::read_to_string(&path) {
        Ok(text) => text,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => String::new(),
        Err(error) => return Err(AppError::io(&path, error)),
    };
    text.parse::<DocumentMut>().map_err(|error| {
        AppError::Message(format!(
            "Cannot verify Codex history config {}: {error}",
            path.display()
        ))
    })?;
    Ok(text)
}

fn state_db_unavailable(codex_dir: &Path, config_text: &str) -> bool {
    codex_state_db_paths(codex_dir, config_text)
        .iter()
        .any(|path| path.exists() && !codex_state_db_is_lockable(path))
}

fn migrate_codex_state_dbs_to(
    codex_dir: &Path,
    source_provider_ids: &BTreeSet<String>,
    target_provider_id: &str,
    backup_root: &Path,
    skipped_rollouts: &HashSet<PathBuf>,
    config_text: &str,
) -> Result<StateMigrationReport, AppError> {
    migrate_codex_state_dbs_with_lock_check(
        codex_dir,
        source_provider_ids,
        target_provider_id,
        backup_root,
        skipped_rollouts,
        config_text,
        codex_state_db_is_lockable,
    )
}

fn migrate_codex_state_dbs_with_lock_check(
    codex_dir: &Path,
    source_provider_ids: &BTreeSet<String>,
    target_provider_id: &str,
    backup_root: &Path,
    skipped_rollouts: &HashSet<PathBuf>,
    config_text: &str,
    can_lock: impl Fn(&Path) -> bool,
) -> Result<StateMigrationReport, AppError> {
    let mut report = StateMigrationReport::default();
    for db_path in codex_state_db_paths(codex_dir, config_text) {
        if db_path.exists() && !can_lock(&db_path) {
            report.retry_pending = true;
            continue;
        }
        let state = migrate_codex_state_db_provider_bucket_to(
            &db_path,
            codex_dir,
            source_provider_ids,
            target_provider_id,
            backup_root,
            skipped_rollouts,
            None,
        )?;
        report.migrated += state.migrated;
        report.skipped += state.skipped;
        report.retry_pending |= state.retry_pending;
    }
    Ok(report)
}

#[cfg(test)]
fn migrate_codex_state_db_provider_bucket(
    db_path: &Path,
    codex_dir: &Path,
    source_provider_ids: &BTreeSet<String>,
    backup_root: &Path,
) -> Result<usize, AppError> {
    migrate_codex_state_db_provider_bucket_to(
        db_path,
        codex_dir,
        source_provider_ids,
        CC_SWITCH_CODEX_MODEL_PROVIDER_ID,
        backup_root,
        &HashSet::new(),
        None,
    )
    .map(|report| report.migrated)
}

fn migrate_codex_state_db_provider_bucket_to(
    db_path: &Path,
    codex_dir: &Path,
    source_provider_ids: &BTreeSet<String>,
    target_provider_id: &str,
    backup_root: &Path,
    skipped_rollouts: &HashSet<PathBuf>,
    eligible_thread_ids: Option<&BTreeSet<String>>,
) -> Result<StateMigrationReport, AppError> {
    if !db_path.exists() || source_provider_ids.is_empty() {
        return Ok(StateMigrationReport::default());
    }

    let mut conn = Connection::open(db_path)
        .map_err(|e| AppError::Database(format!("打开 Codex state DB 失败: {e}")))?;
    conn.busy_timeout(Duration::from_secs(5))
        .map_err(|e| AppError::Database(format!("设置 Codex state DB busy_timeout 失败: {e}")))?;

    if !Database::table_exists(&conn, "threads")?
        || !Database::has_column(&conn, "threads", "model_provider")?
    {
        return Ok(StateMigrationReport::default());
    }

    let placeholders = placeholders(source_provider_ids.len());
    // A no-op scan must not require a writer lock. Re-read the actual plan
    // inside the immediate transaction below before updating any candidates.
    let has_candidate = {
        let mut statement = conn.prepare(&format!(
            "SELECT id FROM threads WHERE model_provider IN ({placeholders})"
        ))?;
        let ids = statement.query_map(params_from_iter(source_provider_ids.iter()), |row| {
            row.get::<_, String>(0)
        })?;
        let mut found = false;
        for id in ids {
            let id = id?;
            if eligible_thread_ids.is_none_or(|eligible| eligible.contains(&id)) {
                found = true;
                break;
            }
        }
        found
    };
    if !has_candidate {
        return Ok(StateMigrationReport::default());
    }
    let has_path = Database::has_column(&conn, "threads", "rollout_path")?;
    let tx = conn
        .transaction_with_behavior(rusqlite::TransactionBehavior::Immediate)
        .map_err(|e| AppError::Database(format!("开启 Codex state DB 迁移事务失败: {e}")))?;
    let path_column = if has_path { "rollout_path" } else { "NULL" };
    let mut rows: Vec<(String, String, Option<String>)> = {
        let mut statement = tx.prepare(&format!("SELECT id, model_provider, {path_column} FROM threads WHERE model_provider IN ({placeholders})"))?;
        let mapped = statement.query_map(params_from_iter(source_provider_ids.iter()), |row| {
            Ok((row.get(0)?, row.get(1)?, row.get(2)?))
        })?;
        mapped.collect::<Result<_, _>>()?
    };
    if let Some(ids) = eligible_thread_ids {
        rows.retain(|(id, _, _)| ids.contains(id));
    }
    if !has_path && !skipped_rollouts.is_empty() {
        log::warn!(
            "Skipping {} Codex index rows without rollout paths after file safety skips: {}",
            rows.len(),
            db_path.display()
        );
        return Ok(StateMigrationReport {
            skipped: rows.len(),
            ..Default::default()
        });
    }
    if rows.is_empty() {
        return Ok(StateMigrationReport::default());
    }
    // Back up via a separate read connection while the immediate transaction
    // prevents another writer from adding an unplanned index row.
    let mut backed_up = false;
    let mut changed = 0;
    let mut unresolved = 0;
    let mut retry_pending = false;
    for (id, old_provider, rollout) in rows {
        if has_path {
            let verification = match rollout.as_deref() {
                Some(raw) => {
                    rollout_is_migrated(codex_dir, raw, &id, target_provider_id, skipped_rollouts)
                }
                None => Ok(false),
            };
            match verification {
                Ok(true) => {}
                result => {
                    if let Err(error) = result {
                        retry_pending = true;
                        log::warn!("Deferring Codex index row {id}: {error}");
                    } else {
                        log::warn!("Skipping Codex index row {id}: no verified migrated rollout ({rollout:?})");
                    }
                    unresolved += 1;
                    continue;
                }
            }
        }
        if !backed_up {
            let snapshot =
                Connection::open_with_flags(db_path, rusqlite::OpenFlags::SQLITE_OPEN_READ_ONLY)
                    .map_err(|e| {
                        AppError::Database(format!("打开 Codex state DB 备份源失败: {e}"))
                    })?;
            backup_codex_state_db(db_path, codex_dir, backup_root, &snapshot)?;
            backed_up = true;
        }
        changed += tx.execute(
            "UPDATE threads SET model_provider = ?1 WHERE id = ?2 AND model_provider = ?3",
            rusqlite::params![target_provider_id, id, old_provider],
        )?;
    }
    tx.commit()
        .map_err(|e| AppError::Database(format!("提交 Codex state DB 迁移事务失败: {e}")))?;
    Ok(StateMigrationReport {
        migrated: changed,
        skipped: unresolved,
        retry_pending,
    })
}

fn rollout_is_migrated(
    codex_dir: &Path,
    raw: &str,
    expected_id: &str,
    target: &str,
    skipped_rollouts: &HashSet<PathBuf>,
) -> Result<bool, AppError> {
    use std::io::BufRead;
    let raw = Path::new(raw);
    let mut path = if raw.is_absolute() {
        raw.to_path_buf()
    } else {
        codex_dir.join(raw)
    };
    if codex_rollout_file::is_compressed(&path) && codex_rollout_file::logical_path(&path).is_file()
    {
        path = codex_rollout_file::logical_path(&path);
    } else if !path.is_file() {
        if let Some(other) = codex_rollout_file::sibling(&path) {
            path = other;
        }
    }
    let root = fs::canonicalize(codex_dir).map_err(|error| AppError::io(codex_dir, error))?;
    let actual = fs::canonicalize(&path).map_err(|error| AppError::io(&path, error))?;
    if skipped_rollouts.contains(&actual)
        || !codex_rollout_file::is_rollout_file(&actual)
        || !(actual.starts_with(root.join("sessions"))
            || actual.starts_with(root.join("archived_sessions")))
    {
        return Ok(false);
    }
    let reader =
        codex_rollout_file::open_reader(&actual).map_err(|error| AppError::io(&actual, error))?;
    let mut found_target = false;
    for line in reader.lines() {
        let line = line.map_err(|error| AppError::io(&actual, error))?;
        if !line.contains("\"session_meta\"") {
            continue;
        }
        let value = serde_json::from_str::<Value>(&line).map_err(|error| {
            AppError::Message(format!(
                "Cannot verify Codex metadata in {}: {error}",
                actual.display()
            ))
        })?;
        if value.get("type").and_then(Value::as_str) != Some("session_meta") {
            continue;
        }
        if value["payload"]["id"].as_str() != Some(expected_id) {
            continue;
        }
        if let Some(provider) = value
            .get("payload")
            .and_then(|v| v.get("model_provider"))
            .and_then(Value::as_str)
        {
            if provider == target {
                found_target = true;
            } else {
                return Ok(false);
            }
        }
    }
    Ok(found_target)
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
    use crate::codex_state_db::CODEX_STATE_DB_FILENAME;
    use crate::provider::Provider;
    use serial_test::serial;
    use std::ffi::OsString;
    use tempfile::tempdir;

    struct EnvVarGuard {
        key: &'static str,
        previous: Option<OsString>,
    }

    impl EnvVarGuard {
        fn set(key: &'static str, value: &Path) -> Self {
            let previous = std::env::var_os(key);
            std::env::set_var(key, value);
            Self { key, previous }
        }
    }

    impl Drop for EnvVarGuard {
        fn drop(&mut self) {
            if let Some(previous) = &self.previous {
                std::env::set_var(self.key, previous);
            } else {
                std::env::remove_var(self.key);
            }
        }
    }

    fn source_ids(values: &[&str]) -> BTreeSet<String> {
        values.iter().map(|value| value.to_string()).collect()
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
        if crate::config::sqlite_unsupported_in_temp_dir() {
            return;
        }
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
        if crate::config::sqlite_unsupported_in_temp_dir() {
            return;
        }
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
        if crate::config::sqlite_unsupported_in_temp_dir() {
            return;
        }
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
    fn rewrites_compressed_rollout_in_place_and_keeps_it_compressed() {
        let dir = tempdir().expect("tempdir");
        let codex_dir = dir.path().join(".codex");
        let backup_root = dir.path().join("backup");
        let session_dir = codex_dir.join("sessions/2026/05/20");
        fs::create_dir_all(&session_dir).expect("create session dir");
        let path = session_dir.join("rollout-test.jsonl.zst");
        let original = concat!(
            "{\"type\":\"session_meta\",\"payload\":{\"id\":\"s1\",\"model_provider\":\"rightcode\"}}\n",
            "{\"type\":\"response_item\",\"payload\":{\"type\":\"message\",\"role\":\"user\",\"content\":\"hi\"}}\n"
        );
        fs::write(
            &path,
            zstd::stream::encode_all(original.as_bytes(), 3).expect("encode"),
        )
        .expect("write session");

        let old_mtime = std::time::UNIX_EPOCH + std::time::Duration::from_secs(1_700_000_000);
        fs::File::options()
            .write(true)
            .open(&path)
            .expect("open")
            .set_times(fs::FileTimes::new().set_modified(old_mtime))
            .expect("set mtime");

        let mut files = Vec::new();
        collect_jsonl_files(&codex_dir.join("sessions"), &mut files, 0, 8).unwrap();
        assert_eq!(files, vec![path.clone()]);

        let changed = rewrite_codex_session_file_for_provider_bucket(
            &path,
            &codex_dir,
            &HashSet::from(["rightcode".to_string()]),
            &backup_root,
        )
        .expect("rewrite");

        assert!(changed);
        // 压缩会话的最后活跃时间取 mtime，改写不能让它变成「刚刚」
        assert_eq!(
            fs::metadata(&path)
                .expect("meta")
                .modified()
                .expect("mtime"),
            old_mtime
        );
        let raw = fs::read(&path).expect("read raw");
        assert_eq!(&raw[..4], &[0x28, 0xB5, 0x2F, 0xFD], "still zstd");
        let next = codex_rollout_file::read_to_string(&path).expect("read rewritten");
        assert!(next.contains("\"model_provider\":\"custom\""));
        assert!(next.ends_with("\"content\":\"hi\"}}\n"));
        let backup = fs::read(backup_root.join("jsonl/sessions/2026/05/20/rollout-test.jsonl.zst"))
            .expect("backup kept as zst");
        assert_eq!(
            zstd::stream::decode_all(backup.as_slice()).expect("decode backup"),
            original.as_bytes()
        );
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
        if crate::config::sqlite_unsupported_in_temp_dir() {
            return;
        }
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
        if crate::config::sqlite_unsupported_in_temp_dir() {
            return;
        }
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
    #[serial]
    fn state_db_paths_include_codex_sqlite_home_env() {
        let dir = tempdir().expect("tempdir");
        let codex_dir = dir.path().join(".codex");
        let sqlite_home = dir.path().join("sqlite-home");
        let _guard = EnvVarGuard::set("CODEX_SQLITE_HOME", &sqlite_home);

        let paths = codex_state_db_paths(&codex_dir, "");

        assert_eq!(
            paths,
            vec![
                codex_dir.join(CODEX_STATE_DB_FILENAME),
                sqlite_home.join(CODEX_STATE_DB_FILENAME),
            ]
        );
    }

    #[test]
    #[serial]
    fn config_sqlite_home_takes_precedence_over_codex_sqlite_home_env() {
        let dir = tempdir().expect("tempdir");
        let codex_dir = dir.path().join(".codex");
        let env_sqlite_home = dir.path().join("env-sqlite-home");
        let config_sqlite_home = dir.path().join("config-sqlite-home");
        let _guard = EnvVarGuard::set("CODEX_SQLITE_HOME", &env_sqlite_home);
        // TOML 字面量字符串(单引号)：Windows 路径含反斜杠，basic string 会解析失败。
        let config_text = format!("sqlite_home = '{}'\n", config_sqlite_home.display());

        let paths = codex_state_db_paths(&codex_dir, &config_text);

        assert_eq!(
            paths,
            vec![
                codex_dir.join(CODEX_STATE_DB_FILENAME),
                config_sqlite_home.join(CODEX_STATE_DB_FILENAME),
            ]
        );
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

    /// 端到端测试用的隔离 HOME：CC Switch 设置与 `~/.codex` 都落在临时目录。
    struct IsolatedHome {
        dir: tempfile::TempDir,
        original_test_home: Option<OsString>,
    }

    impl IsolatedHome {
        fn new() -> Self {
            let dir = tempdir().expect("tempdir");
            let original_test_home = std::env::var_os("CC_SWITCH_TEST_HOME");
            std::env::set_var("CC_SWITCH_TEST_HOME", dir.path());
            crate::settings::reload_settings().expect("reload settings for isolated home");
            Self {
                dir,
                original_test_home,
            }
        }

        fn codex_dir(&self) -> PathBuf {
            self.dir.path().join(".codex")
        }
    }

    impl Drop for IsolatedHome {
        fn drop(&mut self) {
            match &self.original_test_home {
                Some(value) => std::env::set_var("CC_SWITCH_TEST_HOME", value),
                None => std::env::remove_var("CC_SWITCH_TEST_HOME"),
            }
            let _ = crate::settings::reload_settings();
        }
    }

    fn session_meta_line(id: &str, provider: &str) -> String {
        format!(
            "{{\"type\":\"session_meta\",\"payload\":{{\"id\":\"{id}\",\"model_provider\":\"{provider}\"}}}}\n"
        )
    }

    fn seed_threads(db_path: &Path, rows: &[(&str, &str)]) {
        let conn = Connection::open(db_path).expect("open state db");
        conn.execute_batch(
            "CREATE TABLE threads (id TEXT PRIMARY KEY, model_provider TEXT NOT NULL);",
        )
        .expect("create threads");
        for (id, provider) in rows {
            conn.execute(
                "INSERT INTO threads (id, model_provider) VALUES (?1, ?2)",
                [id, provider],
            )
            .expect("insert thread");
        }
    }

    fn thread_provider(db_path: &Path, id: &str) -> String {
        Connection::open(db_path)
            .expect("open state db")
            .query_row(
                "SELECT model_provider FROM threads WHERE id = ?1",
                [id],
                |row| row.get(0),
            )
            .expect("thread provider")
    }

    #[test]
    #[serial]
    fn missing_rollout_index_verification_keeps_the_startup_retry_open() {
        let home = IsolatedHome::new();
        let codex = home.codex_dir();
        fs::create_dir_all(codex.join("sessions")).unwrap();
        let db = Database::memory().unwrap();
        db.save_provider(
            "codex",
            &Provider::with_id(
                "rightcode".into(),
                "Synthetic relay".into(),
                serde_json::json!({"config": "model_provider = \"rightcode\"\n"}),
                None,
            ),
        )
        .unwrap();
        let state = codex.join(CODEX_STATE_DB_FILENAME);
        let conn = Connection::open(&state).unwrap();
        conn.execute_batch("CREATE TABLE threads (id TEXT PRIMARY KEY, model_provider TEXT, rollout_path TEXT); INSERT INTO threads VALUES ('late','rightcode','sessions/late.jsonl');").unwrap();
        drop(conn);
        let first = maybe_migrate_codex_third_party_history_provider_bucket(&db).unwrap();
        assert!(first.retry_pending, "missing rollout must remain retryable");
        fs::write(
            codex.join("sessions/late.jsonl"),
            session_meta_line("late", "custom"),
        )
        .unwrap();
        let second = maybe_migrate_codex_third_party_history_provider_bucket(&db).unwrap();
        assert_eq!(second.migrated_state_rows, 1);
        assert_eq!(thread_provider(&state, "late"), "custom");
    }

    #[test]
    #[serial]
    fn incomplete_directory_inventory_never_marks_startup_complete() {
        let home = IsolatedHome::new();
        let codex = home.codex_dir();
        fs::create_dir_all(&codex).unwrap();
        fs::write(codex.join("sessions"), "not a directory").unwrap();
        let db = Database::memory().unwrap();
        db.save_provider(
            "codex",
            &Provider::with_id(
                "rightcode".into(),
                "Synthetic relay".into(),
                serde_json::json!({"config": "model_provider = \"rightcode\"\n"}),
                None,
            ),
        )
        .unwrap();
        assert!(maybe_migrate_codex_third_party_history_provider_bucket(&db).is_err());
        // Replace only this synthetic malformed store with an accessible one.
        fs::remove_file(codex.join("sessions")).unwrap();
        fs::create_dir_all(codex.join("sessions")).unwrap();
        fs::write(
            codex.join("sessions/recovered.jsonl"),
            session_meta_line("recovered", "rightcode"),
        )
        .unwrap();
        assert_eq!(
            maybe_migrate_codex_third_party_history_provider_bucket(&db)
                .unwrap()
                .migrated_jsonl_files,
            1
        );
    }

    #[test]
    #[serial]
    fn third_party_markers_do_not_hide_another_codex_directory() {
        let home = IsolatedHome::new();
        let db = Database::memory().unwrap();
        db.save_provider(
            "codex",
            &Provider::with_id(
                "rightcode".into(),
                "Synthetic relay".into(),
                serde_json::json!({"config": "model_provider = \"rightcode\"\n"}),
                None,
            ),
        )
        .unwrap();
        assert!(maybe_migrate_codex_third_party_history_provider_bucket(&db).is_ok());
        let other = home.dir.path().join("other-codex");
        fs::create_dir_all(other.join("sessions")).unwrap();
        fs::write(
            other.join("sessions/imported.jsonl"),
            session_meta_line("imported", "rightcode"),
        )
        .unwrap();
        let mut settings = crate::settings::get_settings();
        settings.codex_config_dir = Some(other.to_string_lossy().to_string());
        crate::settings::update_settings(settings).unwrap();
        let result = maybe_migrate_codex_third_party_history_provider_bucket(&db).unwrap();
        assert_eq!(result.migrated_jsonl_files, 1);
    }

    #[test]
    #[serial]
    fn queued_official_migration_rechecks_cancellation_after_acquiring_the_lock() {
        let home = IsolatedHome::new();
        let codex = home.codex_dir();
        fs::create_dir_all(codex.join("sessions")).unwrap();
        fs::write(codex.join("config.toml"), "model_provider = \"custom\"\n").unwrap();
        let path = codex.join("sessions/official.jsonl");
        let before = session_meta_line("official", "openai");
        fs::write(&path, &before).unwrap();
        let mut settings = crate::settings::get_settings();
        settings.unify_codex_session_history = true;
        settings.unify_codex_migrate_existing = Some(true);
        crate::settings::update_settings(settings).unwrap();
        let guard = lock_codex_official_history_op();
        let (ready, waiting) = std::sync::mpsc::channel();
        let worker = std::thread::spawn(move || {
            migrate_official_history_with_lock(|| {
                ready.send(()).unwrap();
                lock_codex_official_history_op()
            })
        });
        waiting.recv_timeout(Duration::from_secs(5)).unwrap();
        let mut settings = crate::settings::get_settings();
        settings.unify_codex_session_history = false;
        settings.unify_codex_migrate_existing = Some(false);
        crate::settings::update_settings(settings).unwrap();
        drop(guard);
        let result = worker.join().unwrap().unwrap();
        assert_eq!(result.migrated_jsonl_files, 0);
        assert_eq!(result.skipped_reason.as_deref(), Some("unify_toggle_off"));
        assert_eq!(fs::read_to_string(path).unwrap(), before);
    }

    #[test]
    #[serial]
    fn custom_source_ledgers_are_isolated_and_survive_directory_round_trips() {
        let home = IsolatedHome::new();
        let codex = home.codex_dir();
        fs::create_dir_all(codex.join("sessions")).unwrap();
        fs::write(codex.join("config.toml"), "model_provider = \"custom\"\n").unwrap();
        let db = Database::memory().unwrap();
        db.save_provider("codex", &Provider::with_id("synthetic-card".into(), "Synthetic relay".into(), serde_json::json!({"config": "model_provider = \"legacy-relay\"\n[model_providers.legacy-relay]\nbase_url = \"https://api.example/v1\"\n"}), None)).unwrap();
        let mut settings = crate::settings::get_settings();
        settings.unify_codex_session_history = true;
        crate::settings::update_settings(settings).unwrap();
        migrate_managed_codex_history_to_unified_bucket(&db).unwrap();
        db.update_provider_settings_config(
            "codex",
            "synthetic-card",
            &serde_json::json!({"config": "model_provider = \"custom\"\n"}),
        )
        .unwrap();
        let other = home.dir.path().join("other-codex");
        fs::create_dir_all(other.join("sessions")).unwrap();
        fs::write(other.join("config.toml"), "model_provider = \"custom\"\n").unwrap();
        let other_path = other.join("sessions/unknown.jsonl");
        let original = session_meta_line("unknown", "legacy-relay");
        fs::write(&other_path, &original).unwrap();
        let mut settings = crate::settings::get_settings();
        settings.codex_config_dir = Some(other.to_string_lossy().to_string());
        crate::settings::update_settings(settings).unwrap();
        assert_eq!(
            migrate_managed_codex_history_to_unified_bucket(&db)
                .unwrap()
                .migrated_jsonl_files,
            0
        );
        assert_eq!(fs::read_to_string(&other_path).unwrap(), original);
        let mut settings = crate::settings::get_settings();
        settings.codex_config_dir = None;
        crate::settings::update_settings(settings).unwrap();
        fs::write(
            codex.join("sessions/imported.jsonl"),
            session_meta_line("imported", "legacy-relay"),
        )
        .unwrap();
        assert_eq!(
            migrate_managed_codex_history_to_unified_bucket(&db)
                .unwrap()
                .migrated_jsonl_files,
            1
        );
    }

    #[test]
    #[serial]
    fn unscoped_legacy_marker_does_not_prove_a_completed_current_directory() {
        let home = IsolatedHome::new();
        let codex = home.codex_dir();
        fs::create_dir_all(codex.join("sessions")).unwrap();
        let mut settings = crate::settings::get_settings();
        settings.local_migrations = Some(crate::settings::LocalMigrations {
            codex_third_party_history_provider_bucket_v1: Some(
                CodexThirdPartyHistoryProviderBucketMigration {
                    codex_config_dir: None,
                    completed_at: "legacy".into(),
                    target_provider_id: "custom".into(),
                    source_provider_ids: vec!["legacy-relay".into()],
                    migrated_jsonl_files: 0,
                    migrated_state_rows: 0,
                    scanned_history_files: true,
                },
            ),
            ..Default::default()
        });
        crate::settings::update_settings(settings).unwrap();
        assert!(
            !crate::settings::is_codex_third_party_history_provider_bucket_migrated_for_dir(
                &canonical_dir_string(&codex)
            )
        );
        assert!(!collect_source_model_provider_ids_for_unify(
            &Database::memory().unwrap(),
            true,
            &codex
        )
        .unwrap()
        .contains("legacy-relay"));
    }

    #[test]
    #[serial]
    fn transient_metadata_skips_retry_on_each_startup_migration_path() {
        for kind in ["third-party", "official", "legacy-official"] {
            let home = IsolatedHome::new();
            let codex = home.codex_dir();
            fs::create_dir_all(codex.join("sessions")).unwrap();
            fs::write(
                codex.join("config.toml"),
                "model_provider = \"custom\"\n[model_providers.custom]\nname = \"Shared\"\n",
            )
            .unwrap();
            let mut settings = crate::settings::get_settings();
            settings.unify_codex_session_history = true;
            settings.unify_codex_migrate_existing = Some(true);
            crate::settings::update_settings(settings).unwrap();
            let db = Database::memory().unwrap();
            db.save_provider(
                "codex",
                &Provider::with_id(
                    "rightcode".into(),
                    "Synthetic relay".into(),
                    serde_json::json!({"config": "model_provider = \"rightcode\"\n"}),
                    None,
                ),
            )
            .unwrap();
            let source = match kind {
                "third-party" => "rightcode",
                "official" => "openai",
                _ => "cc-switch-official",
            };
            let run = || match kind {
                "third-party" => maybe_migrate_codex_third_party_history_provider_bucket(&db),
                "official" => maybe_migrate_codex_official_history_to_unified_bucket(),
                _ => maybe_migrate_codex_legacy_official_history_to_openai_bucket(),
            };
            let path = codex.join("sessions/active.jsonl");
            fs::write(&path, format!("{{\"type\":\"session_meta\",\"payload\":{{\"id\":\"active\",\"model_provider\":\"{source}\"")).unwrap();
            let first = run().unwrap();
            assert_eq!(first.skipped_jsonl_files, 1, "{kind}");
            fs::write(&path, session_meta_line("active", source)).unwrap();
            let second = run().unwrap();
            assert_eq!(second.migrated_jsonl_files, 1, "{kind}: {second:?}");
            assert_eq!(
                run().unwrap().skipped_reason.as_deref(),
                Some("already_migrated")
            );
        }
    }

    #[test]
    fn restore_skips_bad_rollouts_and_updates_only_verified_ledger_rows() {
        let dir = tempdir().unwrap();
        let codex = dir.path().join("codex");
        let ledger = dir.path().join("ledger");
        let generation = ledger.join("generation");
        fs::create_dir_all(codex.join("sessions")).unwrap();
        fs::create_dir_all(generation.join("jsonl/sessions")).unwrap();
        fs::create_dir_all(generation.join("state")).unwrap();
        let broken = "{\"type\":\"session_meta\",\"payload\":{\"model_provider\":\"custom\"";
        for id in ["good", "broken", "unrelated", "unledgered", "missing"] {
            if id != "missing" {
                fs::write(
                    codex.join(format!("sessions/{id}.jsonl")),
                    if ["broken", "unrelated"].contains(&id) {
                        broken.to_string()
                    } else {
                        session_meta_line(id, "custom")
                    },
                )
                .unwrap();
            }
            if ["good", "broken", "missing"].contains(&id) {
                fs::write(
                    generation.join(format!("jsonl/sessions/{id}.jsonl")),
                    session_meta_line(id, "openai"),
                )
                .unwrap();
            }
        }
        fs::write(
            codex.join("sessions/good.jsonl"),
            session_meta_line("good", "custom") + &session_meta_line("new-in-shared", "custom"),
        )
        .unwrap();
        seed_threads(
            &generation.join("state").join(CODEX_STATE_DB_FILENAME),
            &[
                ("good", "openai"),
                ("broken", "openai"),
                ("missing", "openai"),
            ],
        );
        let state = codex.join(CODEX_STATE_DB_FILENAME);
        let conn = Connection::open(&state).unwrap();
        conn.execute_batch("CREATE TABLE threads (id TEXT PRIMARY KEY, model_provider TEXT, rollout_path TEXT);
            INSERT INTO threads VALUES ('good','custom','sessions/good.jsonl'), ('broken','custom','sessions/broken.jsonl'), ('missing','custom','sessions/missing.jsonl'), ('unledgered','custom','sessions/unledgered.jsonl');").unwrap();
        drop(conn);
        let result =
            restore_codex_official_history_inner(&codex, &ledger, &dir.path().join("restore"), "")
                .unwrap();
        assert_eq!(
            (result.restored_jsonl_files, result.restored_state_rows),
            (1, 1)
        );
        assert_eq!(thread_provider(&state, "good"), "openai");
        for id in ["broken", "missing", "unledgered"] {
            assert_eq!(thread_provider(&state, id), "custom");
        }
        assert_eq!(
            fs::read_to_string(codex.join("sessions/broken.jsonl")).unwrap(),
            broken
        );
        assert_eq!(
            (result.skipped_jsonl_files, result.skipped_state_rows),
            (2, 2)
        );
        assert!(fs::read_to_string(codex.join("sessions/good.jsonl"))
            .unwrap()
            .contains(&session_meta_line("new-in-shared", "custom")));
    }

    #[test]
    #[serial]
    fn unavailable_state_db_is_deferred_without_aborting_other_indexes() {
        let home = IsolatedHome::new();
        let codex = home.codex_dir();
        let other = home.dir.path().join("sqlite");
        fs::create_dir_all(codex.join("sessions")).unwrap();
        fs::create_dir_all(&other).unwrap();
        fs::write(
            codex.join("config.toml"),
            format!("sqlite_home = '{}'\n", other.display()),
        )
        .unwrap();
        fs::write(
            codex.join("sessions/good.jsonl"),
            session_meta_line("good", "custom"),
        )
        .unwrap();
        let blocked = codex.join(CODEX_STATE_DB_FILENAME);
        let accessible = other.join(CODEX_STATE_DB_FILENAME);
        for path in [&blocked, &accessible] {
            let conn = Connection::open(path).unwrap();
            conn.execute_batch("CREATE TABLE threads (id TEXT PRIMARY KEY, model_provider TEXT, rollout_path TEXT); INSERT INTO threads VALUES ('good', 'legacy-relay', 'sessions/good.jsonl');").unwrap();
        }
        let result = migrate_codex_state_dbs_with_lock_check(
            &codex,
            &source_ids(&["legacy-relay"]),
            "custom",
            &home.dir.path().join("backup"),
            &HashSet::new(),
            &read_history_config(&codex).unwrap(),
            |path| path != blocked,
        )
        .unwrap();
        assert!(result.retry_pending);
        assert_eq!(result.migrated, 1);
        assert_eq!(thread_provider(&blocked, "good"), "legacy-relay");
        assert_eq!(thread_provider(&accessible, "good"), "custom");
    }

    #[test]
    #[serial]
    fn pending_custom_sources_survive_startup_without_automatic_expansion() {
        let home = IsolatedHome::new();
        let codex = home.codex_dir();
        fs::create_dir_all(codex.join("sessions")).unwrap();
        fs::write(codex.join("config.toml"), "model_provider = \"custom\"\n").unwrap();
        let mut settings = crate::settings::get_settings();
        settings.unify_codex_session_history = true;
        crate::settings::update_settings(settings).unwrap();
        let db = Database::memory().unwrap();
        db.save_provider("codex", &Provider::with_id("synthetic-card".into(), "Synthetic relay".into(), serde_json::json!({"config": "model_provider = \"legacy-relay\"\n[model_providers.legacy-relay]\nbase_url = \"https://api.example/v1\"\n"}), None)).unwrap();
        let path = codex.join("sessions/active.jsonl");
        fs::write(&path, "{\"type\":\"session_meta\",\"model_provider\":").unwrap();
        assert!(
            migrate_managed_codex_history_to_unified_bucket(&db)
                .unwrap()
                .retry_pending
        );
        db.update_provider_settings_config(
            "codex",
            "synthetic-card",
            &serde_json::json!({"config": "model_provider = \"custom\"\n"}),
        )
        .unwrap();
        fs::write(&path, session_meta_line("active", "legacy-relay")).unwrap();
        let startup = maybe_migrate_codex_third_party_history_provider_bucket(&db).unwrap();
        assert_eq!(startup.migrated_jsonl_files, 0);
        assert_eq!(
            fs::read_to_string(&path).unwrap(),
            session_meta_line("active", "legacy-relay")
        );
        assert_eq!(
            migrate_managed_codex_history_to_unified_bucket(&db)
                .unwrap()
                .migrated_jsonl_files,
            1
        );
    }

    #[test]
    fn no_op_index_restore_does_not_wait_for_a_writer_lock() {
        let dir = tempdir().unwrap();
        let state = dir.path().join(CODEX_STATE_DB_FILENAME);
        seed_threads(&state, &[("unledgered", "custom")]);
        let conn = Connection::open(&state).unwrap();
        conn.execute_batch("BEGIN IMMEDIATE").unwrap();
        let report = migrate_codex_state_db_provider_bucket_to(
            &state,
            dir.path(),
            &source_ids(&["custom"]),
            "openai",
            &dir.path().join("backup"),
            &HashSet::new(),
            Some(&source_ids(&["ledgered"])),
        )
        .unwrap();
        assert_eq!((report.migrated, report.skipped), (0, 0));
        assert!(!dir.path().join("backup").exists());
    }

    #[test]
    #[serial]
    fn migrates_legacy_official_route_sessions_to_openai_bucket() {
        if crate::config::sqlite_unsupported_in_temp_dir() {
            return;
        }
        let home = IsolatedHome::new();
        let codex_dir = home.codex_dir();
        let session_dir = codex_dir.join("sessions/2026/09/01");
        fs::create_dir_all(&session_dir).expect("create session dir");
        // 休眠 custom 表保持原样：迁移不碰 config.toml。
        let config = "model = \"gpt-5.5\"\n\n[model_providers.custom]\nname = \"custom\"\nbase_url = \"http://127.0.0.1:15721/v1\"\nexperimental_bearer_token = \"PROXY_MANAGED\"\n";
        fs::write(codex_dir.join("config.toml"), config).expect("write config");

        let plain = session_dir.join("rollout-legacy.jsonl");
        fs::write(
            &plain,
            session_meta_line("s1", "cc-switch-official")
                + "{\"type\":\"response_item\",\"payload\":{\"text\":\"cc-switch-official\"}}\n",
        )
        .expect("write plain session");
        let compressed = session_dir.join("rollout-legacy-zst.jsonl.zst");
        fs::write(
            &compressed,
            zstd::stream::encode_all(session_meta_line("s2", "cc-switch-official").as_bytes(), 3)
                .expect("encode"),
        )
        .expect("write zst session");
        let third_party = session_dir.join("rollout-custom.jsonl");
        fs::write(&third_party, session_meta_line("s3", "custom")).expect("write custom session");

        let state_db = codex_dir.join(CODEX_STATE_DB_FILENAME);
        seed_threads(
            &state_db,
            &[
                ("s1", "cc-switch-official"),
                ("s2", "cc-switch-official"),
                ("s3", "custom"),
                ("s4", "openai"),
            ],
        );

        let outcome = maybe_migrate_codex_legacy_official_history_to_openai_bucket()
            .expect("migrate legacy official bucket");
        assert_eq!(outcome.skipped_reason, None);
        assert_eq!(outcome.migrated_jsonl_files, 2);
        assert_eq!(outcome.migrated_state_rows, 2);

        let plain_text = fs::read_to_string(&plain).expect("read plain");
        assert!(plain_text.contains("\"model_provider\":\"openai\""));
        // 只改 session_meta，正文里出现的同名字符串不动。
        assert!(plain_text.contains(
            "{\"type\":\"response_item\",\"payload\":{\"text\":\"cc-switch-official\"}}"
        ));
        let raw = fs::read(&compressed).expect("read zst");
        assert_eq!(&raw[..4], &[0x28, 0xB5, 0x2F, 0xFD], "still zstd");
        let decoded = String::from_utf8(zstd::stream::decode_all(raw.as_slice()).expect("decode"))
            .expect("utf8");
        assert!(decoded.contains("\"model_provider\":\"openai\""));
        assert_eq!(
            fs::read_to_string(&third_party).expect("read custom"),
            session_meta_line("s3", "custom")
        );

        assert_eq!(thread_provider(&state_db, "s1"), "openai");
        assert_eq!(thread_provider(&state_db, "s2"), "openai");
        assert_eq!(thread_provider(&state_db, "s3"), "custom");
        assert_eq!(thread_provider(&state_db, "s4"), "openai");

        assert_eq!(
            fs::read_to_string(codex_dir.join("config.toml")).expect("read config"),
            config
        );
        let backups = get_app_config_dir().join("backups").join(MIGRATION_NAME);
        assert!(
            backups.is_dir(),
            "backup written under {}",
            backups.display()
        );

        let rerun = maybe_migrate_codex_legacy_official_history_to_openai_bucket().expect("rerun");
        assert_eq!(rerun.skipped_reason.as_deref(), Some("already_migrated"));
    }

    #[test]
    #[serial]
    fn skips_legacy_official_migration_while_a_profile_still_selects_it() {
        if crate::config::sqlite_unsupported_in_temp_dir() {
            return;
        }
        let home = IsolatedHome::new();
        let codex_dir = home.codex_dir();
        fs::create_dir_all(&codex_dir).expect("create codex dir");
        fs::write(
            codex_dir.join("config.toml"),
            "[profiles.legacy]\nmodel_provider = \"cc-switch-official\"\n\n[model_providers.cc-switch-official]\nname = \"OpenAI\"\nrequires_openai_auth = true\n",
        )
        .expect("write config");
        let state_db = codex_dir.join(CODEX_STATE_DB_FILENAME);
        seed_threads(&state_db, &[("s1", "cc-switch-official")]);

        let outcome = maybe_migrate_codex_legacy_official_history_to_openai_bucket()
            .expect("check legacy official bucket");
        assert_eq!(
            outcome.skipped_reason.as_deref(),
            Some("legacy_route_in_use")
        );
        assert_eq!(thread_provider(&state_db, "s1"), "cc-switch-official");
        // 没写完成标记：用户把 profile 改掉后下次启动照常迁移。
        assert!(
            !crate::settings::is_codex_legacy_official_history_bucket_migrated_for_dir(
                &canonical_dir_string(&codex_dir)
            )
        );
    }

    #[test]
    fn detects_provider_selected_at_top_level_or_by_a_profile() {
        let id = "cc-switch-official";
        assert!(codex_config_selects_provider(
            "model_provider = \"cc-switch-official\"\n",
            id
        ));
        assert!(codex_config_selects_provider(
            "[profiles.a]\nmodel = \"x\"\n\n[profiles.b]\nmodel_provider = \"cc-switch-official\"\n",
            id
        ));
        assert!(!codex_config_selects_provider(
            "model_provider = \"custom\"\n\n[model_providers.cc-switch-official]\nname = \"OpenAI\"\n",
            id
        ));
        assert!(!codex_config_selects_provider("not = [valid", id));
    }

    #[test]
    #[serial]
    fn unified_history_retries_imports_after_a_completed_legacy_migration() {
        let home = IsolatedHome::new();
        let codex = home.codex_dir();
        fs::create_dir_all(codex.join("sessions")).unwrap();
        fs::write(codex.join("config.toml"), "model_provider = \"custom\"\n[model_providers.custom]\nname = \"OpenAI\"\nrequires_openai_auth = true\n").unwrap();
        let db = Database::memory().unwrap();
        assert!(migrate_managed_codex_history_to_unified_bucket(&db).is_err());
        let mut settings = crate::settings::get_settings();
        settings.unify_codex_session_history = true;
        crate::settings::update_settings(settings).unwrap();
        crate::settings::mark_codex_third_party_history_provider_bucket_migrated(
            CodexThirdPartyHistoryProviderBucketMigration {
                codex_config_dir: Some(canonical_dir_string(&codex)),
                completed_at: "old".into(),
                target_provider_id: "custom".into(),
                source_provider_ids: vec![],
                migrated_jsonl_files: 0,
                migrated_state_rows: 0,
                scanned_history_files: true,
            },
        )
        .unwrap();
        let card = Provider::with_id(
            "card-id".into(),
            "Legacy relay".into(),
            serde_json::json!({"config": "model_provider = \"legacy-relay\"\n[model_providers.legacy-relay]\nbase_url = \"https://api.example/v1\"\n"}),
            None,
        );
        db.save_provider("codex", &card).unwrap();
        assert!(
            !collect_source_model_provider_ids_for_unify(&db, false, &get_codex_config_dir())
                .unwrap()
                .contains("legacy-relay")
        );
        assert!(
            collect_source_model_provider_ids_for_unify(&db, true, &get_codex_config_dir())
                .unwrap()
                .contains("legacy-relay")
        );
        let first = codex.join("sessions/first.jsonl");
        fs::write(&first, session_meta_line("first", "legacy-relay")).unwrap();
        seed_threads(
            &codex.join(CODEX_STATE_DB_FILENAME),
            &[("first", "legacy-relay")],
        );
        let state = Connection::open(codex.join(CODEX_STATE_DB_FILENAME)).unwrap();
        state
            .execute("ALTER TABLE threads ADD COLUMN rollout_path TEXT", [])
            .unwrap();
        state
            .execute(
                "UPDATE threads SET rollout_path = 'sessions/first.jsonl'",
                [],
            )
            .unwrap();
        drop(state);
        // Startup must not extend the old migration to custom ids implicitly.
        let startup = maybe_migrate_codex_third_party_history_provider_bucket(&db).unwrap();
        assert_eq!(startup.skipped_reason.as_deref(), Some("already_migrated"));
        assert!(fs::read_to_string(&first).unwrap().contains("legacy-relay"));
        let migrated = migrate_managed_codex_history_to_unified_bucket(&db).unwrap();
        assert_eq!(
            (migrated.migrated_jsonl_files, migrated.migrated_state_rows),
            (1, 1)
        );
        let again = migrate_managed_codex_history_to_unified_bucket(&db).unwrap();
        assert_eq!(
            (again.migrated_jsonl_files, again.migrated_state_rows),
            (0, 0)
        );
        // The card can be normalized or renamed without losing proven old ids.
        db.update_provider_settings_config("codex", "card-id", &serde_json::json!({"config":"model_provider = \"custom\"\n[model_providers.custom]\nname = \"Renamed\"\n"})).unwrap();
        let imported = codex.join("sessions/imported.jsonl");
        fs::write(&imported, session_meta_line("imported", "legacy-relay")).unwrap();
        let state = Connection::open(codex.join(CODEX_STATE_DB_FILENAME)).unwrap();
        state
            .execute(
                "INSERT INTO threads VALUES ('imported', 'legacy-relay', 'sessions/imported.jsonl')",
                [],
            )
            .unwrap();
        let last = migrate_managed_codex_history_to_unified_bucket(&db).unwrap();
        assert_eq!(
            (last.migrated_jsonl_files, last.migrated_state_rows),
            (1, 1)
        );
        assert_eq!(
            thread_provider(&codex.join(CODEX_STATE_DB_FILENAME), "imported"),
            "custom"
        );
    }

    #[test]
    fn migrated_index_requires_a_verified_rollout_and_keeps_unresolved_rows() {
        let dir = tempdir().unwrap();
        fs::create_dir_all(dir.path().join("sessions")).unwrap();
        let good = dir.path().join("sessions/good.jsonl");
        fs::write(&good, session_meta_line("good", "custom")).unwrap();
        let state_path = dir.path().join(CODEX_STATE_DB_FILENAME);
        let conn = Connection::open(&state_path).unwrap();
        conn.execute_batch(
            "CREATE TABLE threads (id TEXT PRIMARY KEY, model_provider TEXT, rollout_path TEXT);",
        )
        .unwrap();
        conn.execute("INSERT INTO threads VALUES ('good','legacy-relay',?1), ('missing','legacy-relay','sessions/missing.jsonl')", [good.to_string_lossy().as_ref()]).unwrap();
        let result = migrate_codex_state_db_provider_bucket_to(
            &state_path,
            dir.path(),
            &source_ids(&["legacy-relay"]),
            "custom",
            &dir.path().join("backup"),
            &HashSet::new(),
            None,
        );
        let report = result.unwrap();
        assert_eq!((report.migrated, report.skipped), (1, 1));
        assert_eq!(thread_provider(&state_path, "good"), "custom");
        assert_eq!(thread_provider(&state_path, "missing"), "legacy-relay");
    }
    #[test]
    #[serial]
    fn review_builtin_bedrock_routes_are_not_custom_migration_sources() {
        let _home = IsolatedHome::new();
        let db = Database::memory().unwrap();
        for id in [
            "amazon-bedrock",
            "amazon-bedrock-runtime",
            "openai",
            "ollama",
            "lmstudio",
        ] {
            let card = Provider::with_id(
                format!("card-{id}"),
                "Built-in route".into(),
                serde_json::json!({"config": format!("model_provider = \"{id}\"\n[model_providers.{id}]\nregion = \"example-region\"\n")}),
                None,
            );
            db.save_provider("codex", &card).unwrap();
        }
        let ids = collect_source_model_provider_ids_for_unify(&db, true, &get_codex_config_dir())
            .unwrap();
        assert!(!ids.contains("amazon-bedrock"));
        assert!(!ids.contains("amazon-bedrock-runtime"));
        assert!(
            ids.is_empty(),
            "built-in routes must stay out of custom history"
        );
    }

    #[test]
    #[serial]
    fn review_short_and_ambiguous_files_do_not_block_healthy_migration() {
        let home = IsolatedHome::new();
        let codex = home.codex_dir();
        fs::create_dir_all(codex.join("sessions")).unwrap();
        fs::write(
            codex.join("config.toml"),
            "model_provider = \"custom\"\n[model_providers.custom]\nname = \"Shared\"\n",
        )
        .unwrap();
        let mut settings = crate::settings::get_settings();
        settings.unify_codex_session_history = true;
        crate::settings::update_settings(settings).unwrap();
        let db = Database::memory().unwrap();
        for (id, route) in [("short-card", "kimi"), ("legacy-card", "legacy-relay")] {
            let card = Provider::with_id(
                id.into(),
                "Legacy test connection".into(),
                serde_json::json!({"config": format!("model_provider = \"{route}\"\n[model_providers.{route}]\nbase_url = \"https://api.example/v1\"\n")}),
                None,
            );
            db.save_provider("codex", &card).unwrap();
        }
        let short = session_meta_line("short", "kimi");
        let ambiguous = "{\"type\":\"session_meta\",\"payload\":{\"id\":\"ambiguous\",\"model_provider\":\"legacy-relay\",\"model_provider\":\"legacy-relay\"}}\n";
        fs::write(codex.join("sessions/a-short.jsonl"), &short).unwrap();
        fs::write(codex.join("sessions/b-ambiguous.jsonl"), ambiguous).unwrap();
        fs::write(
            codex.join("sessions/c-good.jsonl"),
            session_meta_line("good", "legacy-relay"),
        )
        .unwrap();
        let state_path = codex.join(CODEX_STATE_DB_FILENAME);
        let conn = Connection::open(&state_path).unwrap();
        conn.execute_batch(
            "CREATE TABLE threads (id TEXT PRIMARY KEY, model_provider TEXT, rollout_path TEXT);
            INSERT INTO threads VALUES
            ('short','kimi','sessions/a-short.jsonl'),
            ('ambiguous','legacy-relay','sessions/b-ambiguous.jsonl'),
            ('good','legacy-relay','sessions/c-good.jsonl');",
        )
        .unwrap();
        drop(conn);
        let outcome = migrate_managed_codex_history_to_unified_bucket(&db).unwrap();
        assert_eq!(
            (outcome.migrated_jsonl_files, outcome.migrated_state_rows),
            (1, 1)
        );
        let wire = serde_json::to_value(&outcome).unwrap();
        assert_eq!(wire["skippedJsonlFiles"], 2);
        assert_eq!(wire["skippedStateRows"], 2);
        assert_eq!(thread_provider(&state_path, "good"), "custom");
        assert_eq!(thread_provider(&state_path, "short"), "kimi");
        assert_eq!(thread_provider(&state_path, "ambiguous"), "legacy-relay");
        assert_eq!(
            fs::read_to_string(codex.join("sessions/a-short.jsonl")).unwrap(),
            short
        );
        assert_eq!(
            fs::read_to_string(codex.join("sessions/b-ambiguous.jsonl")).unwrap(),
            ambiguous
        );
        // The background path must not keep retrying expected safety skips.
        let startup = maybe_migrate_codex_third_party_history_provider_bucket(&db).unwrap();
        assert_eq!(startup.skipped_reason.as_deref(), Some("already_migrated"));
        let retried = migrate_managed_codex_history_to_unified_bucket(&db).unwrap();
        assert_eq!(
            (retried.migrated_jsonl_files, retried.migrated_state_rows),
            (0, 0)
        );
    }

    #[test]
    #[serial]
    fn review_startup_records_a_completed_scan_with_short_ids_skipped() {
        let home = IsolatedHome::new();
        let codex = home.codex_dir();
        fs::create_dir_all(codex.join("sessions")).unwrap();
        fs::write(
            codex.join("sessions/a-short.jsonl"),
            session_meta_line("short", "kimi"),
        )
        .unwrap();
        fs::write(
            codex.join("sessions/b-good.jsonl"),
            session_meta_line("good", "rightcode"),
        )
        .unwrap();
        let db = Database::memory().unwrap();
        for route in ["kimi", "rightcode"] {
            let card = Provider::with_id(
                route.into(),
                "Test connection".into(),
                serde_json::json!({"config": format!("model_provider = \"{route}\"\n[model_providers.{route}]\nbase_url = \"https://api.example/v1\"\n")}),
                None,
            );
            db.save_provider("codex", &card).unwrap();
        }
        let state_path = codex.join(CODEX_STATE_DB_FILENAME);
        let conn = Connection::open(&state_path).unwrap();
        conn.execute_batch("CREATE TABLE threads (id TEXT PRIMARY KEY, model_provider TEXT, rollout_path TEXT);
            INSERT INTO threads VALUES ('short','kimi','sessions/a-short.jsonl'), ('good','rightcode','sessions/b-good.jsonl');").unwrap();
        drop(conn);
        let outcome = maybe_migrate_codex_third_party_history_provider_bucket(&db).unwrap();
        assert_eq!(
            (outcome.migrated_jsonl_files, outcome.migrated_state_rows),
            (1, 1)
        );
        assert_eq!(
            serde_json::to_value(outcome).unwrap()["skippedJsonlFiles"],
            1
        );
        assert_eq!(thread_provider(&state_path, "good"), "custom");
        assert_eq!(thread_provider(&state_path, "short"), "kimi");
        assert_eq!(
            maybe_migrate_codex_third_party_history_provider_bucket(&db)
                .unwrap()
                .skipped_reason
                .as_deref(),
            Some("already_migrated")
        );
    }
}

#[cfg(test)]
mod switch_attribution_regression {
    use super::*;

    #[test]
    fn switch_attribution_migration_keeps_metadata_byte_offsets() {
        let line = r#"{ "type": "session_meta", "payload": { "id": "x", "model_provider": "legacy-relay", "cwd": "/工作" } }"#;
        let ids = HashSet::from(["legacy-relay".to_string()]);
        let changed = rewrite_codex_session_meta_line(line, &ids, "custom")
            .unwrap()
            .unwrap();
        assert_eq!(changed.len(), line.len());
        let mut expected: Value = serde_json::from_str(line).unwrap();
        expected["payload"]["model_provider"] = Value::String("custom".into());
        assert_eq!(serde_json::from_str::<Value>(&changed).unwrap(), expected);
    }

    #[cfg(unix)]
    #[test]
    fn switch_attribution_migration_keeps_plain_inode_and_all_metadata() {
        use std::os::unix::fs::MetadataExt;
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("sessions/test.jsonl");
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        let meta =
            r#"{"type":"session_meta","payload":{"id":"x","model_provider":"legacy-relay"}}"#;
        let original = format!(
            "{meta}\n{{\"type\":\"event_msg\",\"payload\":{{\"text\":\"unchanged\"}}}}\n{meta}\n"
        );
        fs::write(&path, &original).unwrap();
        let inode = fs::metadata(&path).unwrap().ino();
        let backup = dir.path().join("backup");
        rewrite_codex_session_file_for_provider_bucket(
            &path,
            dir.path(),
            &HashSet::from(["legacy-relay".into()]),
            &backup,
        )
        .unwrap();
        assert_eq!(fs::metadata(&path).unwrap().ino(), inode);
        let changed = fs::read_to_string(&path).unwrap();
        assert_eq!(changed.len(), original.len());
        assert!(!changed.contains("legacy-relay"));
        assert!(changed.contains("unchanged"));
    }

    #[test]
    fn metadata_that_cannot_preserve_offsets_is_left_untouched() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("short.jsonl");
        let original =
            "{\"type\":\"session_meta\",\"payload\":{\"id\":\"x\",\"model_provider\":\"kimi\"}}\n";
        fs::write(&path, original).unwrap();
        assert!(rewrite_codex_session_file_for_provider_bucket(
            &path,
            dir.path(),
            &HashSet::from(["kimi".into()]),
            &dir.path().join("backup")
        )
        .is_err());
        assert_eq!(fs::read_to_string(&path).unwrap(), original);
        assert!(!dir.path().join("backup").exists());
    }

    #[test]
    fn plain_metadata_patch_keeps_a_concurrent_tail_append() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("append.jsonl");
        fs::write(&path, b"OLD\noriginal\n").unwrap();
        let original = fs::File::open(&path).unwrap();
        let original_len = original.metadata().unwrap().len();
        let mut file = fs::OpenOptions::new()
            .read(true)
            .write(true)
            .open(&path)
            .unwrap();
        fs::OpenOptions::new()
            .append(true)
            .open(&path)
            .unwrap()
            .write_all(b"new tail\n")
            .unwrap();
        apply_plain_metadata_edits(
            &path,
            &mut file,
            &original,
            original_len,
            &[(0, b"OLD".to_vec(), b"NEW".to_vec())],
        )
        .unwrap();
        assert_eq!(fs::read(&path).unwrap(), b"NEW\noriginal\nnew tail\n");
    }

    #[test]
    fn plain_metadata_patch_rejects_a_replaced_file() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("replace.jsonl");
        fs::write(&path, b"OLD").unwrap();
        let original = fs::File::open(&path).unwrap();
        let original_len = original.metadata().unwrap().len();
        let mut file = fs::OpenOptions::new()
            .read(true)
            .write(true)
            .open(&path)
            .unwrap();
        fs::rename(&path, dir.path().join("previous.jsonl")).unwrap();
        fs::write(&path, b"OLD").unwrap();
        assert!(apply_plain_metadata_edits(
            &path,
            &mut file,
            &original,
            original_len,
            &[(0, b"OLD".to_vec(), b"NEW".to_vec())]
        )
        .is_err());
        assert_eq!(fs::read(&path).unwrap(), b"OLD");
        assert_eq!(fs::read(dir.path().join("previous.jsonl")).unwrap(), b"OLD");
    }

    #[test]
    fn malformed_and_ambiguous_metadata_do_not_advance_migration() {
        let ids = HashSet::from(["legacy-relay".to_string()]);
        assert!(rewrite_codex_session_meta_line(
            "{\"type\":\"session_meta\",\"payload\":{\"model_provider\":",
            &ids,
            "custom"
        )
        .is_err());
        let ambiguous = r#"{"type":"session_meta","payload":{"model_provider":"legacy-relay","model_provider":"legacy-relay"}}"#;
        assert!(rewrite_codex_session_meta_line(ambiguous, &ids, "custom").is_err());
    }

    #[cfg(unix)]
    #[test]
    fn history_inventory_does_not_follow_directory_symlinks() {
        let dir = tempfile::tempdir().unwrap();
        fs::create_dir(dir.path().join("outside")).unwrap();
        fs::write(dir.path().join("outside/secret.jsonl"), b"unchanged").unwrap();
        std::os::unix::fs::symlink(dir.path().join("outside"), dir.path().join("sessions"))
            .unwrap();
        let mut found = vec![];
        collect_jsonl_files(&dir.path().join("sessions"), &mut found, 0, 8).unwrap();
        assert!(found.is_empty());
    }
}

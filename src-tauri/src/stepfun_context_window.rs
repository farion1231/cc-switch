//! StepFun 存量供应商的上下文窗口回填。
//!
//! Step Plan 的 3.x 系列窗口是 262,144 token，但预设早期没有声明窗口，长会话会在
//! Claude Code 自动压缩前撞上上游的 400。预设修正只覆盖之后新建的供应商；库里已有的
//! 行仍带着旧配置，所以这里做一次性回填。
//!
//! **不是所有 StepFun 模型都是 256K。** `step-5-preview` 是 1M 窗口 / 64K 输出
//! （platform.stepfun.com 模型页，2026-10 盘点），把它按 256K 钉死会白白丢掉四分之三
//! 的可用上下文。因此回填只在模型档位全部指向 256K 模型时才生效：出现
//! `step-5-preview`、或任何带 `[1M]` 标记的模型，整行跳过，留给用户自己声明。
//!
//! 与预设保持一致的两个键都钉在模型窗口上（`Kimi For Coding`、`Mistral`、
//! `ChatGPT Codex` 等预设同样写法）：Claude Code 自己会预留输出 token，压缩触发点
//! 已经低于窗口值；显式钉住是为了屏蔽远程实验可能下发的更小压缩点。
//!
//! 回填只看 provider 行自己的 `settings_config`（上游重构后不再有运行时的「有效配置
//! 注入」层，`live/project/claude.rs` 只投影行里的键，`live/residue.rs` 还会把
//! 262144 这类残留值清掉）。改行前按 Codex 迁移的同一目录约定备份原始配置。

use std::fs;
use std::path::{Path, PathBuf};

use chrono::{Local, Utc};
use serde_json::{Map, Value};
use sha2::{Digest, Sha256};

use crate::config::{atomic_write, get_app_config_dir};
use crate::database::Database;
use crate::error::AppError;
use crate::settings::StepFunContextWindowMigration;

/// 备份目录名，与 Codex 迁移的 `backups/<name>/<timestamp>/providers/` 布局一致。
const MIGRATION_NAME: &str = "stepfun-context-window-v1";

/// Step Plan 3.x 的硬上限：262,144 token。
pub(crate) const STEPFUN_CONTEXT_TOKENS: &str = "262144";
/// 与窗口同值：Claude Code 的压缩触发点已自带输出预留，钉住只为屏蔽远端更小的实验值。
pub(crate) const STEPFUN_AUTO_COMPACT_WINDOW: &str = "262144";

const APP_TYPE: &str = "claude";

/// Step Plan 的两个区域端点（末尾斜杠忽略）。
const STEPFUN_STEP_PLAN_ENDPOINTS: [&str; 2] = [
    "https://api.stepfun.com/step_plan",
    "https://api.stepfun.ai/step_plan",
];

/// 1M 窗口的 StepFun 模型：不能用 256K 的默认值。
const STEPFUN_ONE_M_MODELS: [&str; 1] = ["step-5-preview"];

/// Claude Code 可能用来选路的所有模型 env。
const CLAUDE_MODEL_ENV_KEYS: [&str; 6] = [
    "ANTHROPIC_MODEL",
    "ANTHROPIC_DEFAULT_HAIKU_MODEL",
    "ANTHROPIC_DEFAULT_SONNET_MODEL",
    "ANTHROPIC_DEFAULT_OPUS_MODEL",
    "ANTHROPIC_DEFAULT_FABLE_MODEL",
    "CLAUDE_CODE_SUBAGENT_MODEL",
];

#[derive(Debug, Default)]
pub struct StepFunContextWindowOutcome {
    /// 本次补过默认值的供应商 id。
    pub migrated_provider_ids: Vec<String>,
    /// `None` 表示真的跑了；`Some` 是跳过原因。
    pub skipped_reason: Option<String>,
}

/// 启动时调用一次：给存量 StepFun 供应商补上窗口默认值。
pub fn maybe_backfill_stepfun_context_window(
    db: &Database,
) -> Result<StepFunContextWindowOutcome, AppError> {
    if crate::settings::is_stepfun_context_window_migrated() {
        return Ok(StepFunContextWindowOutcome {
            skipped_reason: Some("already_migrated".to_string()),
            ..Default::default()
        });
    }

    let outcome = backfill_stepfun_context_window(db, &migration_backup_root())?;
    crate::settings::mark_stepfun_context_window_migrated(StepFunContextWindowMigration {
        completed_at: Utc::now().to_rfc3339(),
        migrated_provider_ids: outcome.migrated_provider_ids.clone(),
    })?;

    Ok(outcome)
}

fn migration_backup_root() -> PathBuf {
    get_app_config_dir()
        .join("backups")
        .join(MIGRATION_NAME)
        .join(Local::now().format("%Y%m%d_%H%M%S").to_string())
}

/// 改行本身。拆出来是为了让测试能用内存库和临时备份目录跑完整流程。
fn backfill_stepfun_context_window(
    db: &Database,
    backup_root: &Path,
) -> Result<StepFunContextWindowOutcome, AppError> {
    let providers = db.get_all_providers(APP_TYPE)?;
    let mut migrated_provider_ids = Vec::new();

    for (_, provider) in &providers {
        let Some(updated) = backfilled_settings(&provider.settings_config) else {
            continue;
        };
        backup_provider_settings_config(&provider.id, &provider.settings_config, backup_root)?;
        db.update_provider_settings_config(APP_TYPE, &provider.id, &updated)?;
        migrated_provider_ids.push(provider.id.clone());
    }

    Ok(StepFunContextWindowOutcome {
        migrated_provider_ids,
        skipped_reason: None,
    })
}

/// 纯函数：命中的 StepFun 供应商返回补齐后的配置，否则返回 `None`。
///
/// 只有真的补了至少一个键才返回 `Some`，避免把没变化的行写回库。
fn backfilled_settings(settings: &Value) -> Option<Value> {
    let base_url = settings
        .get("env")
        .and_then(|env| env.get("ANTHROPIC_BASE_URL"))
        .and_then(Value::as_str)
        .map(str::trim)
        .map(|url| url.trim_end_matches('/'))?;
    if !STEPFUN_STEP_PLAN_ENDPOINTS.contains(&base_url) {
        return None;
    }

    let mut updated = settings.clone();
    let env = updated.get_mut("env")?.as_object_mut()?;
    if routes_any_one_m_model(env) {
        return None;
    }
    let mut changed = false;

    for (key, default_value) in [
        ("CLAUDE_CODE_MAX_CONTEXT_TOKENS", STEPFUN_CONTEXT_TOKENS),
        (
            "CLAUDE_CODE_AUTO_COMPACT_WINDOW",
            STEPFUN_AUTO_COMPACT_WINDOW,
        ),
    ] {
        if missing_or_blank(env, key) {
            env.insert(key.to_string(), Value::String(default_value.to_string()));
            changed = true;
        }
    }

    changed.then_some(updated)
}

/// 缺键或空串视为「用户没设」；数字和任何非空字符串都算显式值。
fn missing_or_blank(env: &Map<String, Value>, key: &str) -> bool {
    match env.get(key) {
        None | Some(Value::Null) => true,
        Some(Value::String(text)) => text.trim().is_empty(),
        Some(_) => false,
    }
}

/// 任一模型档位指向 1M 模型时不能用 256K 的默认值：`step-5-preview` 本身，或
/// 任何带 `[1M]` 标记的模型名（Claude Code 以该标记为准，见 `has_one_m_marker`）。
fn routes_any_one_m_model(env: &Map<String, Value>) -> bool {
    CLAUDE_MODEL_ENV_KEYS.iter().any(|key| {
        env.get(*key)
            .and_then(Value::as_str)
            .map(str::trim)
            .filter(|model| !model.is_empty())
            .is_some_and(|model| {
                model.to_ascii_lowercase().ends_with("[1m]")
                    || STEPFUN_ONE_M_MODELS
                        .iter()
                        .any(|name| model.eq_ignore_ascii_case(name))
            })
    })
}

/// 改行前留一份原始配置，布局与 Codex 迁移一致：
/// `backups/<迁移名>/<时间戳>/providers/<hash>-<id>.settings_config.json`。
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
    // 保留哈希，避免清洗后同名冲突。
    let digest = Sha256::digest(provider_id.as_bytes());
    let hash = digest[..8]
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect::<String>();
    format!("{hash}-{safe_id}.settings_config.json")
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::provider::Provider;
    use serde_json::json;
    use tempfile::tempdir;

    fn stepfun_env(base_url: &str, extra: Value) -> Value {
        let mut env = json!({
            "ANTHROPIC_BASE_URL": base_url,
            "ANTHROPIC_AUTH_TOKEN": "sk-test",
            "ANTHROPIC_MODEL": "step-3.5-flash-2603"
        });
        if let Some(extra) = extra.as_object() {
            for (key, value) in extra {
                env[key.clone()] = value.clone();
            }
        }
        json!({ "env": env })
    }

    fn save_provider(db: &Database, id: &str, settings: Value) {
        let provider = Provider::with_id(id.to_string(), format!("StepFun {id}"), settings, None);
        db.save_provider(APP_TYPE, &provider)
            .expect("save provider");
    }

    #[test]
    fn backfills_only_the_two_step_plan_endpoints() {
        for base_url in [
            "https://api.stepfun.com/step_plan",
            "https://api.stepfun.com/step_plan/",
            "https://api.stepfun.ai/step_plan",
        ] {
            let updated = backfilled_settings(&stepfun_env(base_url, json!({})))
                .expect("Step Plan providers must be backfilled");
            assert_eq!(
                updated["env"]["CLAUDE_CODE_MAX_CONTEXT_TOKENS"],
                json!("262144")
            );
            assert_eq!(
                updated["env"]["CLAUDE_CODE_AUTO_COMPACT_WINDOW"],
                json!("262144")
            );
        }

        // 按量付费的 /v1 与其它域名都不是 Step Plan，不能改。
        for base_url in [
            "https://api.stepfun.com/v1",
            "https://api.stepfun.example.com/step_plan",
            "https://api.anthropic.com",
        ] {
            assert!(
                backfilled_settings(&stepfun_env(base_url, json!({}))).is_none(),
                "{base_url} must not be treated as Step Plan"
            );
        }
    }

    #[test]
    fn preserves_explicit_user_values() {
        let updated = backfilled_settings(&stepfun_env(
            "https://api.stepfun.com/step_plan",
            json!({
                "CLAUDE_CODE_MAX_CONTEXT_TOKENS": "240000",
                "CLAUDE_CODE_AUTO_COMPACT_WINDOW": 200000
            }),
        ));
        assert!(
            updated.is_none(),
            "a fully configured provider must not be rewritten"
        );
    }

    #[test]
    fn fills_missing_or_blank_keys_only() {
        let partial = backfilled_settings(&stepfun_env(
            "https://api.stepfun.ai/step_plan",
            json!({ "CLAUDE_CODE_MAX_CONTEXT_TOKENS": "200000" }),
        ))
        .expect("the missing compact window must be filled");
        assert_eq!(
            partial["env"]["CLAUDE_CODE_MAX_CONTEXT_TOKENS"],
            json!("200000"),
            "the user's explicit window wins"
        );
        assert_eq!(
            partial["env"]["CLAUDE_CODE_AUTO_COMPACT_WINDOW"],
            json!("262144")
        );

        let blank = backfilled_settings(&stepfun_env(
            "https://api.stepfun.com/step_plan",
            json!({
                "CLAUDE_CODE_MAX_CONTEXT_TOKENS": "   ",
                "CLAUDE_CODE_AUTO_COMPACT_WINDOW": ""
            }),
        ))
        .expect("blank values count as unset");
        assert_eq!(
            blank["env"]["CLAUDE_CODE_MAX_CONTEXT_TOKENS"],
            json!("262144")
        );
        assert_eq!(
            blank["env"]["CLAUDE_CODE_AUTO_COMPACT_WINDOW"],
            json!("262144")
        );
    }

    /// step-5-preview 是 1M 窗口：按 256K 回填会白白砍掉四分之三的可用上下文。
    /// `[1M]` 标记同理，它在任一模型档位上都算数。
    #[test]
    fn skips_one_m_routes() {
        for model in ["step-5-preview", "STEP-5-PREVIEW", " step-5-preview "] {
            let settings = json!({ "env": {
                "ANTHROPIC_BASE_URL": "https://api.stepfun.com/step_plan",
                "ANTHROPIC_MODEL": model,
                "ANTHROPIC_DEFAULT_HAIKU_MODEL": "step-5-preview",
            }});
            assert!(
                backfilled_settings(&settings).is_none(),
                "{model} must keep its 1M window"
            );
        }

        for key in CLAUDE_MODEL_ENV_KEYS {
            let settings = json!({ "env": {
                "ANTHROPIC_BASE_URL": "https://api.stepfun.ai/step_plan",
                "ANTHROPIC_MODEL": "step-3.7-flash",
                key: "step-3.7-flash[1M]",
            }});
            assert!(
                backfilled_settings(&settings).is_none(),
                "{key} carrying [1M] must suppress the 256K defaults"
            );
        }
    }

    #[test]
    fn rewrites_rows_and_keeps_a_backup_of_the_original() {
        let db = Database::memory().expect("memory db");
        let backup_dir = tempdir().expect("tempdir");
        save_provider(
            &db,
            "stepfun",
            stepfun_env("https://api.stepfun.com/step_plan", json!({})),
        );
        save_provider(
            &db,
            "kimi",
            stepfun_env("https://api.kimi.com/coding", json!({})),
        );

        let outcome = backfill_stepfun_context_window(&db, backup_dir.path()).expect("backfill");
        assert_eq!(outcome.migrated_provider_ids, vec!["stepfun".to_string()]);
        assert!(outcome.skipped_reason.is_none());

        let saved = db
            .get_provider_by_id("stepfun", APP_TYPE)
            .expect("get provider")
            .expect("provider exists");
        assert_eq!(
            saved.settings_config["env"]["CLAUDE_CODE_MAX_CONTEXT_TOKENS"],
            json!("262144")
        );
        assert_eq!(
            saved.settings_config["env"]["CLAUDE_CODE_AUTO_COMPACT_WINDOW"],
            json!("262144")
        );

        let untouched = db
            .get_provider_by_id("kimi", APP_TYPE)
            .expect("get provider")
            .expect("provider exists");
        assert!(untouched
            .settings_config
            .get("env")
            .and_then(|env| env.get("CLAUDE_CODE_MAX_CONTEXT_TOKENS"))
            .is_none());

        let backups: Vec<_> = fs::read_dir(backup_dir.path().join("providers"))
            .expect("provider backups")
            .flatten()
            .collect();
        assert_eq!(backups.len(), 1, "only the rewritten row is backed up");
        let backup: Value =
            serde_json::from_str(&fs::read_to_string(backups[0].path()).expect("read backup"))
                .expect("parse backup");
        assert_eq!(backup["providerId"], json!("stepfun"));
        assert!(backup["settingsConfig"]["env"]
            .get("CLAUDE_CODE_MAX_CONTEXT_TOKENS")
            .is_none());
    }
}

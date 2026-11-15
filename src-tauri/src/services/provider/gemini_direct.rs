//! 写 Gemini CLI 的 `.env` 和 `settings.json`：只替换关键字段，其余行、键和注释不碰。
//!
//! 写 Gemini live 的入口（切换、新增第一个供应商、编辑当前供应商、同步、统一供应商、
//! 进入 / 退出代理）都走这里：先拿应用写锁，再经 `mode::operation` 记下 pending，两个
//! 文件和状态在同一个操作里提交。不回填、不合并通用配置片段：用户的设置本来就留在
//! live 里，MCP 在 `settings.json` 里也不受影响。

use crate::app_config::AppType;
use crate::database::Database;
use crate::error::AppError;
use crate::gemini_config::{
    get_gemini_env_path, get_gemini_settings_path, validate_gemini_settings,
    validate_gemini_settings_strict,
};
use crate::live::engine::LiveFile;
use crate::live::project::gemini::GeminiProjection;
use crate::mode::operation::{AppWrite, FileChange, OperationReport};
use crate::mode::state::{op, PendingTarget};
use crate::provider::Provider;

use super::gemini_auth::is_google_official_gemini;
use super::gemini_editor::{EnvWrite, GeminiEdits, SettingsWrite};

fn app() -> &'static str {
    AppType::Gemini.as_str()
}

/// `~/.gemini/.env`：里面有 Key，按 0600 写。
pub(crate) fn env_file() -> LiveFile {
    LiveFile::private(get_gemini_env_path())
}

/// `~/.gemini/settings.json`。
pub(crate) fn settings_file() -> LiveFile {
    LiveFile::shared(get_gemini_settings_path())
}

/// 官方卡（Google 登录）：分类是官方，或按名称、推广标记认出来的 Google 官方。
pub(crate) fn is_official(provider: &Provider) -> bool {
    provider.category.as_deref() == Some("official") || is_google_official_gemini(provider)
}

/// 供应商的关键字段。第三方卡要带 `GEMINI_API_KEY`（环境变量全空的是走 Google 登录的
/// 卡，不要求），写之前校验，校验不过什么都不写。
pub(crate) fn projection(provider: &Provider) -> Result<GeminiProjection, AppError> {
    validate_gemini_settings(&provider.settings_config)?;
    let official = is_official(provider);
    if !official {
        validate_gemini_settings_strict(&provider.settings_config)?;
    }
    Ok(GeminiProjection::of(&provider.settings_config, official))
}

/// 切到 `target`：同一个操作里写 live、再把当前供应商改成它。
pub(crate) fn switch_to(db: &Database, target: &Provider) -> Result<OperationReport, AppError> {
    let projection = projection(target)?;
    run(
        db,
        op::SWITCH,
        Some(&projection),
        PendingTarget::pointer(Some(target.id.clone())),
    )
}

/// 把当前供应商重新投影到 live，不改指针。
pub(crate) fn reapply(db: &Database, target: &Provider) -> Result<OperationReport, AppError> {
    let projection = projection(target)?;
    run(db, op::APPLY, Some(&projection), PendingTarget::default())
}

/// 把 `projection` 写进两个文件，和 `target` 在同一个操作里提交；`projection` 为空时
/// 只落定状态、不读也不写文件。
pub(crate) fn run(
    db: &Database,
    op: &str,
    projection: Option<&GeminiProjection>,
    target: PendingTarget,
) -> Result<OperationReport, AppError> {
    run_with_edits(db, op, projection, target, None)
}

/// 同 [`run`]，另把编辑器保存的全局改动在同一次写入里写进去（先改动，后关键字段）。
pub(crate) fn run_with_edits(
    db: &Database,
    op: &str,
    projection: Option<&GeminiProjection>,
    target: PendingTarget,
    edits: Option<&GeminiEdits>,
) -> Result<OperationReport, AppError> {
    // 写不写、写成谁是调用方按读到的指针定的：先补完上一次的操作。
    let write = AppWrite::begin(db, app())?;
    let env = EnvWrite {
        edits,
        key_fields: projection.map(GeminiProjection::env_patch),
    };
    let settings = SettingsWrite {
        edits,
        key_fields: projection.map(GeminiProjection::settings_patch),
    };
    let mut changes: Vec<FileChange<'_>> = Vec::new();
    if projection.is_some() || edits.is_some_and(GeminiEdits::touches_env) {
        changes.push(FileChange {
            file: env_file(),
            patch: &env,
        });
    }
    if projection.is_some() || edits.is_some_and(GeminiEdits::touches_settings) {
        changes.push(FileChange {
            file: settings_file(),
            patch: &settings,
        });
    }
    write.run(op, &changes, target)
}

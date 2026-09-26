//! Claude Code 直连模式下写 `settings.json`：只替换关键字段和独有字段，其余字节不碰。
//!
//! 写 Claude live 的入口（切换、新增第一个供应商、编辑当前供应商、同步、统一供应商、
//! 退出代理时写回）都走这里：先拿应用写锁，再经 `mode::operation` 记下 pending、发布。
//! 不回填、不合并通用配置片段、不注入上下文默认值：用户的设置本来就留在 live 里。

use crate::app_config::AppType;
use crate::config::get_claude_settings_path;
use crate::database::Database;
use crate::error::AppError;
use crate::live::engine::{lock_app, AppWriteGuard, DeviceStore, LiveFile};
use crate::live::project::claude::{direct_patch, ClaudeProjection};
use crate::mode::operation::{self, FileChange, OperationReport};
use crate::mode::state::{op, PendingTarget};
use crate::provider::Provider;

fn app() -> &'static str {
    AppType::Claude.as_str()
}

/// `settings.json`（旧安装可能是 `claude.json`）。里面有 Key，按 0600 写。
pub(crate) fn settings_file() -> LiveFile {
    LiveFile::private(get_claude_settings_path())
}

/// 从 `prev` 切到 `target`：同一个操作里写 live、再把当前供应商改成 `target`。
///
/// `prev` 是 live 现在对应的供应商（直连指针指向的那家），用来删它带进来的独有字段。
pub(crate) fn switch_to(
    db: &Database,
    prev: Option<&Provider>,
    target: &Provider,
) -> Result<OperationReport, AppError> {
    let guard = lock_app(app());
    write(db, &guard, prev, target, Some(&target.id))
}

/// 把当前供应商 `target` 重新投影到 live，不改指针。`prev` 是 live 现在对应的那一版
/// 行（编辑前的行；没改过就是它自己）。
pub(crate) fn reapply(
    db: &Database,
    prev: Option<&Provider>,
    target: &Provider,
) -> Result<OperationReport, AppError> {
    let guard = lock_app(app());
    write(db, &guard, prev, target, None)
}

fn write(
    db: &Database,
    guard: &AppWriteGuard,
    prev: Option<&Provider>,
    target: &Provider,
    pointer: Option<&str>,
) -> Result<OperationReport, AppError> {
    let prev = prev.map(|provider| ClaudeProjection::of(&provider.settings_config));
    let patch = direct_patch(
        prev.as_ref(),
        &ClaudeProjection::of(&target.settings_config),
    );
    let pending_target = PendingTarget {
        pointer: pointer.map(str::to_string),
        ..PendingTarget::default()
    };
    operation::run(
        &DeviceStore::for_device(),
        guard,
        if pointer.is_some() {
            op::SWITCH
        } else {
            op::APPLY
        },
        &[FileChange {
            file: settings_file(),
            patch: &patch,
        }],
        pending_target,
        &|target| operation::commit_pointer(db, app(), target),
    )
}

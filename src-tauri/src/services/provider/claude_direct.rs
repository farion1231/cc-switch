//! 写 Claude Code 的 `settings.json`：只替换关键字段和独有字段，其余字节不碰。
//!
//! 写 Claude live 的入口（切换、新增第一个供应商、编辑当前供应商、同步、统一供应商、
//! 进入 / 退出代理）都走这里：先拿应用写锁，再经 `mode::operation` 记下 pending、发布。
//! 不回填、不合并通用配置片段、不注入上下文默认值：用户的设置本来就留在 live 里。

use crate::app_config::AppType;
use crate::config::get_claude_settings_path;
use crate::database::Database;
use crate::error::AppError;
use serde_json::Value;

use crate::live::engine::{sha256_hex, DeviceStore, LiveFile};
use crate::live::patch::json::JsonPatch;
use crate::live::patch::KeyPath;
use crate::live::project::claude::{direct_patch, ClaudeProjection, MODEL_PICKER_KEY};
use crate::mode::operation::{AppWrite, FileChange, OperationReport};
use crate::mode::state::{self, op, PendingTarget, Written};
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
    write(db, prev, target, Some(&target.id))
}

/// 把当前供应商 `target` 重新投影到 live，不改指针。`prev` 是 live 现在对应的那一版
/// 行（编辑前的行；没改过就是它自己）。
pub(crate) fn reapply(
    db: &Database,
    prev: Option<&Provider>,
    target: &Provider,
) -> Result<OperationReport, AppError> {
    write(db, prev, target, None)
}

fn write(
    db: &Database,
    prev: Option<&Provider>,
    target: &Provider,
    pointer: Option<&str>,
) -> Result<OperationReport, AppError> {
    let prev = prev.map(|provider| ClaudeProjection::of(&provider.settings_config));
    let mut patch = direct_patch(
        prev.as_ref(),
        &ClaudeProjection::of(&target.settings_config),
    );
    // 删掉以前开发版写进去的账号目录（见 claude_official）
    super::claude_official::strip_from_patch(&mut patch);
    // 先换登录（卡绑了本机账号、没设配置目录覆盖时）：失败就整个不切，指针和
    // settings.json 都不动；登录换了、后面写失败时再换回去
    super::claude_official::with_login_switch(Some(target), || {
        run(
            db,
            if pointer.is_some() {
                op::SWITCH
            } else {
                op::APPLY
            },
            Some(&patch),
            PendingTarget::pointer(pointer.map(str::to_string)),
        )
    })
}

/// 用 `patch` 改写 `settings.json`，和 `target` 在同一个操作里提交；`patch` 为空时只
/// 落定状态、不读也不写文件。
pub(crate) fn run(
    db: &Database,
    op: &str,
    patch: Option<&JsonPatch>,
    mut target: PendingTarget,
) -> Result<OperationReport, AppError> {
    // 补丁是按调用方读到的指针算的（要删上一家的独有字段）：先补完上一次的操作。
    let write = AppWrite::begin(db, app())?;
    let patch = match patch {
        Some(patch) => {
            let recorded =
                state::written(&write.store, app())?.and_then(|written| written.model_picker);
            record_picker(&write.store, patch, &mut target)?;
            let live_picker = read_settings().and_then(|live| live.get(MODEL_PICKER_KEY).cloned());
            Some(with_users_picker(patch, live_picker, recorded.as_deref()))
        }
        None => None,
    };
    let changes: Vec<FileChange<'_>> = patch
        .iter()
        .map(|patch| FileChange {
            file: settings_file(),
            patch,
        })
        .collect();
    write.run(op, &changes, target)
}

/// 读 live 的 `settings.json`；不存在或解析不了时 `None`。
pub(crate) fn read_settings() -> Option<Value> {
    let bytes = std::fs::read(settings_file().path).ok()?;
    serde_json::from_slice(&bytes).ok()
}

fn picker_digest(picker: &Value) -> String {
    sha256_hex(&serde_json::to_vec(picker).expect("modelPicker serializes"))
}

fn picker_path() -> KeyPath {
    KeyPath::root().child(MODEL_PICKER_KEY)
}

/// 把这次要写进去的 `modelPicker`（补丁里没有就是空串）记进写入记录，和文件在同一个操作
/// 里落定。所有写关键字段的入口（切换、进出代理、编辑器保存当前供应商）都要记，否则
/// 下次会把 CC Switch 自己写的菜单当成用户的留下。
pub(crate) fn record_picker(
    store: &DeviceStore,
    patch: &JsonPatch,
    target: &mut PendingTarget,
) -> Result<(), AppError> {
    let path = picker_path();
    let record = patch
        .set
        .iter()
        .find(|(key, _)| *key == path)
        .map(|(_, picker)| picker_digest(picker))
        .unwrap_or_default();
    let base = match target.written.take() {
        Some(written) => written,
        None => state::written(store, app())?.unwrap_or_default(),
    };
    target.written = Some(Written {
        model_picker: Some(record),
        ..base
    });
    Ok(())
}

/// `modelPicker` 是关键字段，补丁默认清掉 live 里的再写目标的。这里只把 CC Switch 自己写的
/// 当成关键字段：live 里的和上次的写入记录（`recorded`）对不上，就是用户自己写的，目标
/// 没有菜单时原样留下。目标带着菜单（聚合模式、行里配了菜单）时照旧以目标为准。没有记录
/// （升级前）按旧规矩当成 CC Switch 的。
fn with_users_picker(
    patch: &JsonPatch,
    live_picker: Option<Value>,
    recorded: Option<&str>,
) -> JsonPatch {
    let path = picker_path();
    let mut patch = patch.clone();
    if patch.set.iter().any(|(key, _)| *key == path) {
        return patch;
    }
    if let (Some(live), Some(recorded)) = (live_picker, recorded) {
        if picker_digest(&live) != recorded {
            patch.set.push((path, live));
        }
    }
    patch
}

//! 写 Hermes 的 `config.yaml`：只动 CC Switch 自己的路由条目和顶层 `model:` 段，其余
//! 字节不碰（投影与补丁见 `live::project::hermes`）。
//!
//! 写 Hermes live 的入口（进入 / 退出代理、换路由、同步、退出 CC Switch 分离）都走这里：
//! 和 `hermes_config` 的其它写路径共用 `hermes_write_lock`，再经 `mode::operation` 记下
//! pending，文件、模式和写入记录在同一个操作里提交，崩溃后可前滚或丢弃。
//!
//! Hermes 是累加式应用：供应商条目本来就在 `custom_providers` 里共存，直连没有「独占
//! 写」的概念。退出代理不重写直连那家的条目，只删路由条目、把 `model:` 段指回去。

use crate::app_config::AppType;
use crate::database::Database;
use crate::error::AppError;
use crate::live::engine::LiveFile;
use crate::live::project::claude::PROXY_TOKEN_PLACEHOLDER;
use crate::live::project::hermes::{HermesConfigPatch, HermesProjection};
use crate::mode::operation::{AppWrite, FileChange, OperationReport};
use crate::mode::state::{PendingTarget, Written};
use crate::provider::Provider;

fn app() -> &'static str {
    AppType::Hermes.as_str()
}

/// `~/.hermes/config.yaml`（供应商条目里带 Key，按 0600 写）。
pub(crate) fn config_file() -> LiveFile {
    LiveFile::private(crate::hermes_config::get_hermes_config_path())
}

/// 代理契约：`{proxy_base}/hermes/v1` 上的 Responses 路由条目 + `model:` 段指向它。
pub(crate) fn proxy_contract(
    route: &Provider,
    proxy_base_url: &str,
) -> Result<HermesProjection, AppError> {
    HermesProjection::proxy_contract(
        &route.settings_config,
        proxy_base_url,
        PROXY_TOKEN_PLACEHOLDER,
    )
}

/// 直连投影：删路由条目，`model:` 段指回直连指针那家（没有就清掉指向）。
pub(crate) fn direct_projection(direct: Option<&Provider>) -> HermesProjection {
    HermesProjection::direct(
        direct.map(|provider| (provider.id.as_str(), &provider.settings_config)),
    )
}

/// 把 `projection` 写进 `config.yaml`，和目标状态在同一个操作里提交；`None` 时只落定
/// 状态、不读也不写文件。
///
/// 进入 / 换路由记下写入记录（路由条目名）；退出代理清掉记录。
pub(crate) fn run(
    db: &Database,
    op: &str,
    projection: Option<&HermesProjection>,
    mut target: PendingTarget,
) -> Result<OperationReport, AppError> {
    // 与 hermes_config 的其它写路径（set_provider 等）互斥，避免两条路径交错的半截写。
    let _hermes_guard = crate::hermes_config::hermes_write_lock()
        .lock()
        .map_err(|e| AppError::Lock(e.to_string()))?;

    let app_write = AppWrite::begin(db, app())?;
    let patch = projection.map(HermesConfigPatch::new);
    if let Some(projection) = projection {
        target.written = Some(match projection.entry.as_ref() {
            Some(_) => Written {
                tables: vec![crate::live::project::hermes::ROUTE_ENTRY_NAME.to_string()],
                ..Written::default()
            },
            // 直连：没有 CC Switch 写的条目了，清掉写入记录。
            None => Written::default(),
        });
    }
    let changes: Vec<FileChange<'_>> = patch
        .as_ref()
        .map(|patch| FileChange {
            file: config_file(),
            patch,
        })
        .into_iter()
        .collect();
    app_write.run(op, &changes, target)
}

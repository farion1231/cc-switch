//! 「当前供应商」有两个：直连指针和代理路由，互相独立。
//!
//! 读当前供应商一律经 [`provider_for`]，并说明读它做什么。直接读直连指针
//! （`settings::get_effective_current_provider`、`Database::get_current_provider`）
//! 只允许在这里和它们自己的定义处，由下面的测试把关。

use crate::app_config::AppType;
use crate::database::Database;
use crate::error::AppError;
use crate::live::engine::DeviceStore;
use crate::provider::Provider;

use super::state::{self, ModeState};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Purpose {
    /// 直连指针：直连模式下写进客户端文件的那家，代理模式下退出代理时写回它。
    Direct,
    /// 请求实际发往的那家：代理模式下是代理路由，直连模式下是直连指针。界面上的
    /// 「当前」、用量、测速、项目快照都按它。
    InUse,
}

/// 这个应用的模式状态；不支持本地路由的应用恒为直连。
pub fn mode_state(app: &AppType) -> ModeState {
    if !app.supports_local_proxy() {
        return ModeState::default();
    }
    state::mode_state(&DeviceStore::for_device(), app.as_str()).unwrap_or_else(|err| {
        log::warn!("读取 {} 的模式状态失败，按直连处理: {err}", app.as_str());
        ModeState::default()
    })
}

pub fn is_proxy(app: &AppType) -> bool {
    mode_state(app).is_proxy()
}

/// 同 [`is_proxy`]，一次看几个应用，状态文件只读一次（状态面板每 2 秒轮询一次）。
pub fn proxy_flags<const N: usize>(apps: [AppType; N]) -> [bool; N] {
    let names = apps.each_ref().map(AppType::as_str);
    match state::mode_states(&DeviceStore::for_device(), names) {
        Ok(states) => {
            std::array::from_fn(|i| apps[i].supports_local_proxy() && states[i].is_proxy())
        }
        Err(err) => {
            log::warn!("读取模式状态失败，按直连处理: {err}");
            [false; N]
        }
    }
}

pub fn provider_for(
    db: &Database,
    app: &AppType,
    purpose: Purpose,
) -> Result<Option<String>, AppError> {
    if purpose == Purpose::InUse {
        if let Some(route) = proxy_route_row(db, app)? {
            return Ok(Some(route.id));
        }
    }
    crate::settings::get_effective_current_provider(db, app)
}

/// 正在用的那一行（见 [`Purpose::InUse`]）。
pub fn provider_in_use(db: &Database, app: &AppType) -> Result<Option<Provider>, AppError> {
    match proxy_route_row(db, app)? {
        Some(route) => Ok(Some(route)),
        None => direct_provider(db, app),
    }
}

/// 代理模式下代理路由那一行；不在代理模式、或者路由那家在库里不存在（按直连处理）时
/// 为 `None`。
fn proxy_route_row(db: &Database, app: &AppType) -> Result<Option<Provider>, AppError> {
    let mode = mode_state(app);
    if !mode.is_proxy() {
        return Ok(None);
    }
    let Some(route) = mode.proxy_route else {
        return Ok(None);
    };
    let row = db.get_provider_by_id(&route, app.as_str())?;
    if row.is_none() {
        log::warn!(
            "{} 的代理路由 {route} 在数据库中不存在，按直连指针处理",
            app.as_str()
        );
    }
    Ok(row)
}

/// 直连指针指向的那一行。
pub fn direct_provider(db: &Database, app: &AppType) -> Result<Option<Provider>, AppError> {
    match provider_for(db, app, Purpose::Direct)? {
        Some(id) => db.get_provider_by_id(&id, app.as_str()),
        None => Ok(None),
    }
}

/// 设备本地记录的直连指针（不验证是否存在、不回落到 DB）。只给切换失败时的回滚用：
/// 回滚要把本地记录恢复成原样。
pub fn local_direct_pointer(app: &AppType) -> Option<String> {
    crate::settings::get_current_provider(app)
}

/// 删除前检查：本地记录、DB 的 `is_current`、代理路由里任何一处指着它，就算正在用。
pub fn is_referenced(db: &Database, app: &AppType, id: &str) -> Result<bool, AppError> {
    if crate::settings::get_current_provider(app).as_deref() == Some(id)
        || db.get_current_provider(app.as_str())?.as_deref() == Some(id)
    {
        return Ok(true);
    }
    Ok(mode_state(app).routes_to(id))
}

#[cfg(test)]
mod tests {
    use std::fs;
    use std::path::Path;

    /// 直接读直连指针的地方：定义处、这里，以及只关心直连指针的写入路径。
    const ALLOWED: &[&str] = &[
        "settings.rs",
        "database/dao/providers.rs",
        "mode/current.rs",
    ];

    fn scan(dir: &Path, root: &Path, hits: &mut Vec<String>) {
        for entry in fs::read_dir(dir).unwrap() {
            let path = entry.unwrap().path();
            if path.is_dir() {
                scan(&path, root, hits);
                continue;
            }
            if path.extension().and_then(|ext| ext.to_str()) != Some("rs") {
                continue;
            }
            let relative = path
                .strip_prefix(root)
                .unwrap()
                .to_string_lossy()
                .replace('\\', "/");
            if ALLOWED.contains(&relative.as_str()) {
                continue;
            }
            let text = fs::read_to_string(&path).unwrap();
            // 测试模块里造数据可以直接读写，只查生产代码：跳过顶层的
            // `#[cfg(test)] mod xxx { … }`（到下一个顶格的 `}` 为止）。
            let mut in_tests = false;
            let mut previous = "";
            for (index, line) in text.lines().enumerate() {
                if in_tests {
                    in_tests = line != "}";
                    previous = line;
                    continue;
                }
                if previous == "#[cfg(test)]" && line.starts_with("mod ") && line.ends_with('{') {
                    in_tests = true;
                    previous = line;
                    continue;
                }
                previous = line;
                if line.contains("get_effective_current_provider(")
                    || line.contains(".get_current_provider(")
                    || line.contains("settings::get_current_provider(")
                {
                    hits.push(format!("{relative}:{}: {}", index + 1, line.trim()));
                }
            }
        }
    }

    #[test]
    fn current_provider_is_read_through_provider_for() {
        let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("src");
        let mut hits = Vec::new();
        scan(&root, &root, &mut hits);
        assert!(
            hits.is_empty(),
            "读当前供应商要经 mode::current::provider_for 并说明用途（直连指针还是正在用的那家）:\n{}",
            hits.join("\n")
        );
    }
}

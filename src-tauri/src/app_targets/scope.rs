//! 跟随环境的写入作用域。
//!
//! 现有的写入链路（直连写、MCP、提示词、Skills）都通过 `settings::get_*_override_dir`
//! 找客户端目录、通过 `DeviceStore::for_device` 找设备状态。在作用域里，这两处分别换成
//! 这个目标的目录和这个目标自己的状态目录：写前意图、首写备份、Codex 登录暂存按目标隔离，
//! 不能用一份快照覆盖两边。作用域是线程局部的，只影响当前线程上的这一次写入。

use std::cell::RefCell;
use std::path::PathBuf;

use crate::app_config::AppType;

#[derive(Debug, Clone)]
pub struct TargetScope {
    pub app: AppType,
    /// 目标标识（`windows` / `wsl:<发行版>`）。
    pub key: String,
    /// 目标目录；`None` 表示用应用自己的默认目录（忽略旧单目录设置）。
    pub dir: Option<PathBuf>,
    /// 这个目标自己的设备状态目录（`live-state.json`、备份、登录暂存）。
    pub store_root: PathBuf,
    /// 目标在 WSL 里：Windows 专用的 MCP 命令等不能照搬。
    pub wsl: bool,
}

thread_local! {
    static CURRENT: RefCell<Option<TargetScope>> = const { RefCell::new(None) };
    static NOTES: RefCell<Vec<String>> = const { RefCell::new(Vec::new()) };
}

struct Restore(Option<TargetScope>);

impl Drop for Restore {
    fn drop(&mut self) {
        let previous = self.0.take();
        CURRENT.with(|cell| *cell.borrow_mut() = previous);
    }
}

/// 在 `scope` 里运行 `f`；返回 `f` 的结果和期间记下的提示。
pub fn run<R>(scope: TargetScope, f: impl FnOnce() -> R) -> (R, Vec<String>) {
    let previous = CURRENT.with(|cell| cell.borrow_mut().replace(scope));
    let saved_notes = NOTES.with(|cell| std::mem::take(&mut *cell.borrow_mut()));
    let _restore = Restore(previous);
    let result = f();
    let notes = NOTES.with(|cell| std::mem::replace(&mut *cell.borrow_mut(), saved_notes));
    (result, notes)
}

pub fn active() -> bool {
    CURRENT.with(|cell| cell.borrow().is_some())
}

/// 作用域对 `app` 的目录覆盖：`Some(dir)` 表示在作用域里（`dir` 为 `None` 时用默认目录）。
pub fn override_dir(app: &AppType) -> Option<Option<PathBuf>> {
    CURRENT.with(|cell| {
        cell.borrow()
            .as_ref()
            .filter(|scope| &scope.app == app)
            .map(|scope| scope.dir.clone())
    })
}

pub fn store_root() -> Option<PathBuf> {
    CURRENT.with(|cell| cell.borrow().as_ref().map(|scope| scope.store_root.clone()))
}

/// 应用写锁的键：跟随环境用自己的锁，和主环境、其他目标的写入互不等待（文件和状态都
/// 不重叠），同一目标的两次写入仍然串行。
pub fn lock_key(app: &str) -> String {
    CURRENT.with(|cell| match cell.borrow().as_ref() {
        Some(scope) => format!("{app}@{}", scope.key),
        None => app.to_string(),
    })
}

pub fn is_wsl() -> bool {
    CURRENT.with(|cell| cell.borrow().as_ref().is_some_and(|scope| scope.wsl))
}

/// 记一条提示（比如跳过了 Windows 专用的 MCP 命令），写入结束后随结果返回。
pub fn note(message: impl Into<String>) {
    if active() {
        NOTES.with(|cell| cell.borrow_mut().push(message.into()));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn scope(key: &str) -> TargetScope {
        TargetScope {
            app: AppType::Claude,
            key: key.to_string(),
            dir: Some(PathBuf::from("/tmp/x")),
            store_root: PathBuf::from("/tmp/store"),
            wsl: true,
        }
    }

    #[test]
    fn scope_applies_only_to_its_app_and_thread() {
        assert!(!active());
        assert_eq!(override_dir(&AppType::Claude), None);
        let ((), notes) = run(scope("wsl:Ubuntu"), || {
            assert_eq!(
                override_dir(&AppType::Claude),
                Some(Some(PathBuf::from("/tmp/x")))
            );
            assert_eq!(override_dir(&AppType::Codex), None);
            assert_eq!(lock_key("claude"), "claude@wsl:Ubuntu");
            assert!(is_wsl());
            note("mcp_skipped:a");
            std::thread::spawn(|| assert!(!active())).join().unwrap();
        });
        assert_eq!(notes, vec!["mcp_skipped:a".to_string()]);
        assert!(!active());
        assert_eq!(lock_key("claude"), "claude");
    }

    #[test]
    fn scope_is_restored_after_a_panic() {
        let result = std::panic::catch_unwind(|| {
            run(scope("windows"), || panic!("boom"));
        });
        assert!(result.is_err());
        assert!(!active());
    }
}

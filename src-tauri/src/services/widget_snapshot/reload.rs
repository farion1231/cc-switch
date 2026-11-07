//! 写完快照后通知桌面小组件重读（`WidgetCenter.reloadAllTimelines()`，Swift 桥见
//! `src-tauri/macos/WidgetReload.swift`）。不通知的话小组件只按自己的时间线重读，系统还会按
//! 刷新额度往后推，桌面上看到的就是旧数据。
//!
//! 只在内容（不算写入时间）变了、或距上次通知超过 `HEARTBEAT` 时通知，并且两次通知至少隔
//! `MIN_INTERVAL`：窗口内的变化攒到窗口结束再通知一次，免得把系统给的刷新额度用光。

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Mutex;
use std::time::{Duration, Instant};

const MIN_INTERVAL: Duration = Duration::from_secs(60);
/// 内容没变也至少隔这么久通知一次：小组件到「写入时间 + 30 分钟」会标「数据已过时」，
/// 只靠它自己的时间线重读会被系统按刷新额度往后推，主程序开着也可能被误标。
const HEARTBEAT: Duration = Duration::from_secs(20 * 60);

static LAST_CONTENT: Mutex<Option<u64>> = Mutex::new(None);
static LAST_RELOAD: Mutex<Option<Instant>> = Mutex::new(None);
static PENDING: AtomicBool = AtomicBool::new(false);

fn lock<T>(m: &Mutex<T>) -> std::sync::MutexGuard<'_, T> {
    m.lock().unwrap_or_else(|e| e.into_inner())
}

#[cfg(all(target_os = "macos", not(widget_reload_unavailable)))]
fn reload_now() {
    extern "C" {
        fn ccswitch_reload_widget_timelines();
    }
    // SAFETY: 无参无返回的 Swift `@_cdecl` 函数，只调 WidgetCenter，可在任意线程调用。
    unsafe { ccswitch_reload_widget_timelines() };
    log::debug!("[Widget] 已通知小组件重读");
}

#[cfg(not(all(target_os = "macos", not(widget_reload_unavailable))))]
fn reload_now() {}

fn content_hash(content: &[u8]) -> u64 {
    use std::hash::{Hash, Hasher};
    let mut hasher = std::collections::hash_map::DefaultHasher::new();
    content.hash(&mut hasher);
    hasher.finish()
}

/// 刚写完快照。`content` 是去掉写入时间后的内容，用来判断有没有变化。
pub fn snapshot_written(content: &[u8]) {
    let hash = content_hash(content);
    {
        let mut last = lock(&LAST_CONTENT);
        let recently = lock(&LAST_RELOAD).is_some_and(|at| at.elapsed() < HEARTBEAT);
        if *last == Some(hash) && recently {
            return;
        }
        *last = Some(hash);
    }
    let wait = lock(&LAST_RELOAD)
        .map(|at| MIN_INTERVAL.saturating_sub(at.elapsed()))
        .unwrap_or(Duration::ZERO);
    if wait.is_zero() {
        *lock(&LAST_RELOAD) = Some(Instant::now());
        reload_now();
        return;
    }
    // 窗口内已经排了一次就不再排：到点时读到的是最新的文件
    if PENDING.swap(true, Ordering::AcqRel) {
        return;
    }
    std::thread::spawn(move || {
        std::thread::sleep(wait);
        PENDING.store(false, Ordering::Release);
        *lock(&LAST_RELOAD) = Some(Instant::now());
        reload_now();
    });
}

/// 关掉小组件、删了快照：下次打开时无论内容是否相同都要通知。
pub fn forget() {
    *lock(&LAST_CONTENT) = None;
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn same_content_hashes_equal() {
        assert_eq!(content_hash(b"abc"), content_hash(b"abc"));
        assert_ne!(content_hash(b"abc"), content_hash(b"abd"));
    }
}

//! Linux 专用的主窗口恢复补丁。
//!
//! 解决 Tauri 2.x 在部分 Linux 发行版（尤其是 Wayland / 某些 WebKitGTK
//! 版本）上启动或从托盘/隐藏状态恢复后「点不到」的问题：
//!
//! - **失效模式 A**（Tauri #10746 / wry #637）：webview 在 `show()` 后
//!   没有获得 keyboard focus，导致首次点击被 X11/Wayland 用作
//!   click-to-activate 而非传给 webview。
//! - **失效模式 B**：GTK surface 与 WebKitWebView 的 input region 尺寸
//!   协商在 `visible:false` → `show()` 的路径上失败，整窗永远不响应
//!   点击，只有重新 `size_allocate` 才能恢复。
//! - **失效模式 C**（#7405 / #2736 / #7499）：GTK HeaderBar 的 Wayland
//!   事件子表面未挂上，表现为网页内容仍可点、原生标题栏按钮全死。
//!
//! 本模块导出 [`nudge_main_window`]。序列是 fire-and-forget，零抽搐：
//! 1. 显式 `set_focus`（realize 前后各一次，不循环抢焦点）；
//! 2. 装饰对账：仅在与设置不一致时 `set_decorations`；
//! 3. 短暂翻转 `set_resizable`，触发 Tao `HeaderBar::set_decoration_layout`
//!    重绑 CSD 按钮（最大化窗口也走这条，不改几何）；
//! 4. 非最大化窗口再用 `LogicalSize` ±1 逻辑像素刷新 WebKit input region。
//!    最大化窗口严禁 `set_size`（合成器硬约束，会 drift / 抖动）。
//!
//! 并发策略是 queue-latest 而不是中途取消：一旦已经 `set_size(bumped)`，
//! 必须先 restore 再处理下一轮，避免窗口永久停在 width+1。
//!
//! 所有「让主窗口出现在用户面前」的路径（正常启动、deeplink、
//! single_instance、托盘 show_main、lightweight 退出/重建）都应在现有
//! `set_focus()` 之后追加一次调用。

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Mutex;
use std::time::Duration;

use tauri::{LogicalSize, PhysicalSize, WebviewWindow};

static IS_NUDGING: AtomicBool = AtomicBool::new(false);

/// 只保留最新一次 nudge 请求。轻量模式销毁重建窗口时，旧任务仍在跑，
/// 新窗口的请求必须覆盖排队，而不是被互斥直接丢掉。
static PENDING_NUDGE: LatestSlot<PendingNudge> = LatestSlot::new();

/// 在 webview realize 之后的延迟，等 GTK 主循环把 realize 事件处理完。
const REALIZE_WAIT: Duration = Duration::from_millis(200);

/// 翻转 resizable 的间隔（覆盖 1~2 个显示刷新周期）。
const TOGGLE_GAP: Duration = Duration::from_millis(30);

/// 伪 resize 两步之间的间隔。Tao Linux 的尺寸 API 是异步的
///（`gtk_window_resize` → 合成器 configure），太短会被 coalesce。
const RESIZE_GAP: Duration = Duration::from_millis(100);

/// 尺寸对账回读前的额外等待，确保合成器处理完 resize 消息队列。
const RECONCILE_WAIT: Duration = Duration::from_millis(500);

struct PendingNudge {
    window: WebviewWindow,
    reason: &'static str,
}

/// 覆盖式槽：并发写入只保留最新值，供当前 nudge 任务结束后取出。
struct LatestSlot<T> {
    inner: Mutex<Option<T>>,
}

impl<T> LatestSlot<T> {
    const fn new() -> Self {
        Self {
            inner: Mutex::new(None),
        }
    }

    fn store(&self, value: T) {
        *self.lock() = Some(value);
    }

    fn take(&self) -> Option<T> {
        self.lock().take()
    }

    fn has(&self) -> bool {
        self.lock().is_some()
    }

    fn lock(&self) -> std::sync::MutexGuard<'_, Option<T>> {
        self.inner
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
    }
}

fn try_acquire_nudge() -> bool {
    IS_NUDGING
        .compare_exchange(false, true, Ordering::SeqCst, Ordering::SeqCst)
        .is_ok()
}

fn calculate_nudge_sizes(
    physical: PhysicalSize<u32>,
    scale_factor: f64,
) -> (LogicalSize<f64>, LogicalSize<f64>) {
    let scale = if scale_factor > 0.0 {
        scale_factor
    } else {
        1.0
    };
    let logical = physical.to_logical::<f64>(scale);
    let bumped = LogicalSize::new(logical.width + 1.0, logical.height);
    (logical, bumped)
}

fn has_size_drift(current: LogicalSize<f64>, expected: LogicalSize<f64>) -> bool {
    (current.width - expected.width).abs() >= 0.5 || (current.height - expected.height).abs() >= 0.5
}

fn decorations_need_restore(current: bool, desired: bool) -> bool {
    current != desired
}

fn is_window_alive(window: &WebviewWindow) -> bool {
    window.is_visible().is_ok()
}

fn is_window_visible(window: &WebviewWindow) -> bool {
    window.is_visible().unwrap_or(false)
}

fn sample_focus(window: &WebviewWindow, ever_focused: &mut bool) {
    if window.is_focused().unwrap_or(false) {
        *ever_focused = true;
    }
}

struct NudgeGuard;

impl Drop for NudgeGuard {
    fn drop(&mut self) {
        IS_NUDGING.store(false, Ordering::SeqCst);
    }
}

/// 对主窗口执行 Linux 专用的「focus + HeaderBar + surface 重激活」序列。
///
/// 调用是 fire-and-forget：内部 spawn 一个异步任务并立即返回，不阻塞 UI。
/// `reason` 标识触发来源（startup / tray-show-main 等），用于日志排障。
///
/// 同一时刻只跑一条序列。后续请求覆盖排队，当前序列（含任何已发出的
/// bump→restore）结束后再对最新窗口跑一遍。
pub(crate) fn nudge_main_window(window: WebviewWindow, reason: &'static str) {
    let _ = window.set_focus();
    PENDING_NUDGE.store(PendingNudge { window, reason });

    if !try_acquire_nudge() {
        log::debug!("Linux: 正在执行主窗口重激活，已排队最新请求 (reason: {reason})");
        return;
    }

    tauri::async_runtime::spawn(async move {
        loop {
            {
                let _guard = NudgeGuard;
                while let Some(PendingNudge { window, reason }) = PENDING_NUDGE.take() {
                    run_nudge_sequence(window, reason).await;
                }
            }
            // Guard 已释放 IS_NUDGING。若释放窗口期内又有请求入队，重新抢执行权；
            // 抢不到说明另一个 runner 已经接手。
            if !PENDING_NUDGE.has() {
                break;
            }
            if !try_acquire_nudge() {
                break;
            }
        }
    });
}

async fn run_nudge_sequence(window: WebviewWindow, reason: &'static str) {
    if !is_window_alive(&window) {
        log::debug!("Linux: 窗口已销毁，跳过重激活 (reason: {reason})");
        return;
    }

    tokio::time::sleep(REALIZE_WAIT).await;

    if !is_window_alive(&window) {
        log::debug!("Linux: 窗口在等待 realize 期间被销毁，跳过重激活 (reason: {reason})");
        return;
    }

    // 用户已再次藏进托盘：不要 set_focus / 改装饰把窗口 map 回来。
    if !is_window_visible(&window) {
        log::debug!("Linux: 窗口已隐藏，跳过重激活 (reason: {reason})");
        return;
    }

    let _ = window.set_focus();
    let mut ever_focused = false;
    sample_focus(&window, &mut ever_focused);

    restore_decorations_if_needed(&window);

    // 修复 GTK HeaderBar 按钮输入路由（失效模式 C）。
    // 通过微秒级翻转 set_resizable 触发 Tao connect_resizable_notify →
    // HeaderBar::set_decoration_layout，就地重挂 CSD 事件子表面。
    // 不改变窗口尺寸与最大化状态。
    let is_resizable = window.is_resizable().unwrap_or(true);
    let _ = window.set_resizable(!is_resizable);
    tokio::time::sleep(TOGGLE_GAP).await;
    let _ = window.set_resizable(is_resizable);
    sample_focus(&window, &mut ever_focused);

    let is_maximized = window.is_maximized().unwrap_or(false);

    // 失效模式 B：仅非最大化窗口做 LogicalSize 微调。
    // 最大化尺寸受合成器硬约束，set_size 无效且会引发 drift 与抖动。
    // 一旦 bump 已经发出，后面即使窗口被隐藏也必须 restore。
    if !is_maximized {
        pseudo_resize_logical(&window).await;
        sample_focus(&window, &mut ever_focused);
    }

    // 最多再补一次 focus。已经拿到过焦点、或用户已切走，都不再抢。
    if is_window_visible(&window) && !ever_focused && !window.is_focused().unwrap_or(false) {
        let _ = window.set_focus();
    }

    log::info!(
        "Linux: 已对主窗口执行 focus + HeaderBar 控制按钮与 surface 重激活 (reason: {reason}, maximized={is_maximized})"
    );
}

fn restore_decorations_if_needed(window: &WebviewWindow) {
    let desired = !crate::settings::get_settings().use_app_window_controls;
    match window.is_decorated() {
        Ok(current) if decorations_need_restore(current, desired) => {
            if let Err(e) = window.set_decorations(desired) {
                log::warn!("Linux: 恢复窗口装饰失败: {e}");
            } else {
                log::info!("Linux: 已恢复窗口装饰 decorated={desired}");
            }
        }
        Ok(_) => {}
        Err(e) => log::warn!("Linux: 读取窗口装饰状态失败: {e}"),
    }
}

/// 以 LogicalSize 做 ±1 逻辑像素伪 resize，避免 HiDPI 下 PhysicalSize +1
/// 被逻辑换算截断成 no-op。
///
/// 一旦 `set_size(bumped)` 已经发出，本函数保证随后 `set_size(original)`，
/// 即使窗口在 RESIZE_GAP 期间被隐藏。这是刻意不做 generation-cancel 的原因。
async fn pseudo_resize_logical(window: &WebviewWindow) {
    let (physical, scale) = match (window.inner_size(), window.scale_factor()) {
        (Ok(physical), Ok(scale)) => (physical, scale),
        (Err(e), _) => {
            log::warn!("Linux nudge: 读取 inner_size 失败，跳过伪 resize: {e}");
            return;
        }
        (_, Err(e)) => {
            log::warn!("Linux nudge: 读取 scale_factor 失败，跳过伪 resize: {e}");
            return;
        }
    };

    let (logical, bumped) = calculate_nudge_sizes(physical, scale);
    let _ = window.set_size(bumped);
    tokio::time::sleep(RESIZE_GAP).await;
    let _ = window.set_size(logical);

    tokio::time::sleep(RECONCILE_WAIT).await;
    if !is_window_alive(window) {
        return;
    }

    match (window.inner_size(), window.scale_factor()) {
        (Ok(after_physical), Ok(after_scale)) => {
            let after_scale = if after_scale > 0.0 { after_scale } else { 1.0 };
            let after_logical = after_physical.to_logical::<f64>(after_scale);
            if has_size_drift(after_logical, logical) {
                log::info!(
                    "Linux nudge 尺寸 drift: expected={:?}, got={:?}，已补偿",
                    logical,
                    after_logical
                );
                let _ = window.set_size(logical);

                if let (Ok(final_physical), Ok(final_scale)) =
                    (window.inner_size(), window.scale_factor())
                {
                    let final_scale = if final_scale > 0.0 { final_scale } else { 1.0 };
                    let final_logical = final_physical.to_logical::<f64>(final_scale);
                    if has_size_drift(final_logical, logical) {
                        log::warn!(
                            "Linux nudge 尺寸 drift 补偿后仍不一致: expected={:?}, got={:?}",
                            logical,
                            final_logical
                        );
                    }
                }
            }
        }
        (Err(e), _) => log::warn!("Linux nudge: 对账回读 inner_size 失败: {e}"),
        (_, Err(e)) => log::warn!("Linux nudge: 对账回读 scale_factor 失败: {e}"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn latest_slot_keeps_newest_while_busy() {
        let slot = LatestSlot::new();
        slot.store("destroyed-window");
        slot.store("replacement-window");
        assert!(slot.has());
        assert_eq!(slot.take(), Some("replacement-window"));
        assert!(!slot.has());
        assert_eq!(slot.take(), None);
    }

    #[test]
    fn calculate_nudge_sizes_various_hidpi_scales() {
        let (logical, bumped) = calculate_nudge_sizes(PhysicalSize::new(800, 600), 1.0);
        assert_eq!(logical.width, 800.0);
        assert_eq!(bumped.width, 801.0);
        assert_eq!(bumped.height, 600.0);

        let (logical, bumped) = calculate_nudge_sizes(PhysicalSize::new(1000, 750), 1.25);
        assert_eq!(logical.width, 800.0);
        assert_eq!(bumped.width, 801.0);
        assert!((bumped.width - logical.width - 1.0).abs() < 1e-6);

        let (logical, bumped) = calculate_nudge_sizes(PhysicalSize::new(1600, 1200), 2.0);
        assert_eq!(logical.width, 800.0);
        assert_eq!(bumped.width, 801.0);
        assert_eq!(bumped.height, 600.0);

        let (logical, bumped) = calculate_nudge_sizes(PhysicalSize::new(800, 600), 0.0);
        assert_eq!(logical.width, 800.0);
        assert_eq!(bumped.width, 801.0);
    }

    #[test]
    fn has_size_drift_uses_half_logical_pixel() {
        let expected = LogicalSize::new(800.0, 600.0);
        assert!(!has_size_drift(LogicalSize::new(800.0, 600.0), expected));
        assert!(!has_size_drift(LogicalSize::new(800.3, 600.2), expected));
        assert!(has_size_drift(LogicalSize::new(801.0, 600.0), expected));
        assert!(has_size_drift(LogicalSize::new(800.0, 601.0), expected));
    }

    #[test]
    fn decorations_are_only_restored_on_mismatch() {
        assert!(!decorations_need_restore(true, true));
        assert!(!decorations_need_restore(false, false));
        assert!(decorations_need_restore(true, false));
        assert!(decorations_need_restore(false, true));
    }
}

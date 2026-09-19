//! Linux 专用的主窗口恢复补丁。
//!
//! 解决 Tauri 2.x 在部分 Linux 发行版（尤其是 Wayland / 某些 WebKitGTK
//! 版本）上「窗口重新出现后无法交互」的问题：
//!
//! - **失效模式 A**（Tauri #10746 / wry #637）：webview 在 `show()` 后
//!   没有获得 keyboard focus，导致首次点击被 X11/Wayland 用作
//!   click-to-activate 而非传给 webview。
//! - **失效模式 B**：GTK surface 与 WebKitWebView 的 input region 尺寸
//!   协商在 `visible:false → show()` 的路径上失败，整窗永远不响应
//!   点击，只有重新 `size_allocate`（例如最大化-还原）才能恢复。
//! - **失效模式 C**（#7499）：从托盘 `hide()` 后再 `show()`，GTK 客户端
//!   装饰（标题栏的关闭 / 最大化按钮）的输入区域没有重新协商——此时
//!   webview 内容往往还能点，只有标题栏按钮「死掉」，直观表现就是
//!   「从托盘打开后关不掉窗口」。反复 `hide()/show()` 会让此前的重激活
//!   序列与新窗口状态竞争，进一步放大该问题。
//!
//! 本模块导出 [`reactivate_main_window`]：它先显式 `set_focus`（带重试），
//! 再对账 decorated，最后做一次无视觉版本的 ±1px 伪 resize，精确模拟用户
//! 手动最大化再还原的 workaround，但肉眼无法察觉。所有"让主窗口出现在用户
//! 面前"的路径（正常启动、deeplink 唤起、single_instance 回调、托盘
//! show_main、lightweight 退出）都应在现有 `set_focus()` 之后追加一次调用。
//!
//! 有意不做 `unmaximize → maximize`：GTK 的 `gtk_window_resize` 在最大化时
//! 仍会把 resize 请求排进主循环并在 widget 上跑 `size_allocate`，因此 ±1px
//! 伪 resize 已足够重挂 input region；而真实的最大化往返会让窗口在每次从
//! 托盘恢复时肉眼可见地闪一下，属于回归。
//!
//! 模块本身在所有平台编译（便于单测覆盖平台无关的决策逻辑），但只在
//! Linux 上被调用；非 Linux 平台上的死代码由 `allow(dead_code)` 抑制。

#![cfg_attr(not(target_os = "linux"), allow(dead_code))]

use std::sync::atomic::{AtomicU64, Ordering};
use std::time::Duration;

use tauri::{PhysicalSize, WebviewWindow};

/// 在 webview realize 之后的延迟，等 GTK 主循环把 realize 事件处理完。
/// 200ms 是社区经验值；太短 set_focus 仍会无效，太长会让首屏可交互
/// 时间被肉眼感知到。
const REALIZE_WAIT: Duration = Duration::from_millis(200);

/// ±1px 伪 resize 两步之间的间隔，确保 GTK 先处理了第一次
/// `size_allocate` 再收到第二次 resize。放宽到 100ms 是因为 Tao 在 Linux
/// 上的尺寸 API 是异步的（底层走 `gtk_window_resize` → 合成器 configure），
/// 太短会让合成器把两次连续 resize coalesce 成一次。
const RESIZE_GAP: Duration = Duration::from_millis(100);

/// 尺寸对账回读前的额外等待。200ms + 100ms + 500ms = 总共 ~800ms 后
/// 校验窗口尺寸是否回到 original。这个时间足够所有合成器处理完
/// resize 消息队列。
const RECONCILE_WAIT: Duration = Duration::from_millis(500);

/// 焦点兜底重试的间隔；隐藏再显示后 window manager 可能还没把焦点交还，
/// 用户此刻点击标题栏按钮同样会「失效」（第一次点击只用于激活窗口）。
const FOCUS_RETRY_WAIT: Duration = Duration::from_millis(250);

/// 焦点兜底重试次数。
const FOCUS_RETRY_ATTEMPTS: u8 = 3;

/// 单调递增的调用代次：每次 [`reactivate_main_window`] 都让上一轮尚未完成的
/// 异步序列失效，避免「上一次显示排队的伪 resize / 焦点重试」在窗口再次
/// 隐藏后仍然执行——那会在用户已经关闭到托盘之后又把 GTK 窗口映射回屏幕上，
/// 表现就是「关不掉」（#7499）。
static REACTIVATE_GENERATION: AtomicU64 = AtomicU64::new(0);

/// 重显示后 GTK 可能把窗口装饰状态重置回默认值，这里与设置里的期望值对账，
/// 只有不一致才调用 `set_decorations`——避免每次显示都无谓地重建标题栏。
fn decorations_need_restore(current: bool, desired: bool) -> bool {
    current != desired
}

fn is_current(generation: u64) -> bool {
    REACTIVATE_GENERATION.load(Ordering::Acquire) == generation
}

/// 对主窗口执行 Linux 专用的「focus + surface 重激活」序列。
///
/// 调用是 fire-and-forget：内部 spawn 一个异步任务在 ~250ms 后完成。
/// 调用线程立即返回，不阻塞 UI。`reason` 只用于日志，便于线上定位是哪条
/// 显示路径触发的重激活。
pub(crate) fn reactivate_main_window(window: WebviewWindow, reason: &'static str) {
    // 开启新一轮：让上一次尚未跑完的序列作废。
    let generation = REACTIVATE_GENERATION.fetch_add(1, Ordering::AcqRel) + 1;

    // 第一次 set_focus：webview 可能还没 realize，这一次通常是无效的，
    // 但成本极低（线程安全，内部 run_on_main_thread），顺手做掉。
    let _ = window.set_focus();

    tauri::async_runtime::spawn(async move {
        if !is_current(generation) {
            return;
        }
        tokio::time::sleep(REALIZE_WAIT).await;
        if !is_current(generation) {
            return;
        }

        // 第二次 set_focus：此时 webview realize 已完成，在绝大多数
        // 发行版上这一次会真的生效，消除失效模式 A。
        let _ = window.set_focus();

        restore_decorations_if_needed(&window);

        pseudo_resize(&window, generation).await;

        let focused = retry_focus(&window, generation).await;

        if is_current(generation) {
            log::info!(
                "Linux: 已对主窗口执行 focus + surface 重激活（原因: {reason}, focused={focused}）"
            );
        }
    });
}

/// 与设置里的期望值对账窗口装饰；不一致时才修正（避免标题栏闪烁）。
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

/// 重激活：读取当前 inner_size，先加 1px 再还原。这会触发 GTK 的
/// size-allocate → WebKitWebViewBase::size_allocate → 重新 attach input
/// surface，消除失效模式 B/C。
///
/// 使用 PhysicalSize 避免跨 DPI 的逻辑坐标漂移；saturating_add
/// 防止极端尺寸溢出。
async fn pseudo_resize(window: &WebviewWindow, generation: u64) {
    let original = match window.inner_size() {
        Ok(size) => size,
        Err(e) => {
            // 极罕见的失败路径；只做了 set_focus 也比什么都不做强，
            // 不要让 resize 失败把整个补丁吞掉。
            log::warn!("Linux nudge: 读取 inner_size 失败，跳过伪 resize: {e}");
            return;
        }
    };

    let bumped = PhysicalSize::new(original.width.saturating_add(1), original.height);
    let _ = window.set_size(bumped);
    tokio::time::sleep(RESIZE_GAP).await;
    if !is_current(generation) {
        return;
    }
    let _ = window.set_size(original);

    // 尺寸对账回读：Tao Linux 的尺寸 API 是异步的，`set_size` 只是把
    // resize 请求送进 GTK 主循环队列，合成器可能会 coalesce 两次连续
    // 请求（尤其是第二次 `set_size(original)`），导致窗口永久停留在
    // width+1。这里等合成器处理完队列后读一次实际尺寸，发现 drift 就
    // 再补一次 `set_size(original)` 兜底。
    //
    // 已知限制：tiling Wayland 合成器（sway/river/hyprland）会完全忽略
    // `set_size`，此时对账永远 drift=0（因为两次 set_size 都是 no-op），
    // 看起来"没问题"但失效模式 B/C 其实没被修复；这是已知限制，需要用户
    // 侧用 GDK_BACKEND=x11 绕过，README 应该有说明。
    tokio::time::sleep(RECONCILE_WAIT).await;
    if !is_current(generation) {
        return;
    }
    match window.inner_size() {
        Ok(after) => {
            if after.width != original.width || after.height != original.height {
                log::info!(
                    "Linux nudge 尺寸 drift: expected={}x{}, got={}x{}，已补偿",
                    original.width,
                    original.height,
                    after.width,
                    after.height
                );
                let _ = window.set_size(original);
                // 最终校验：如果补偿后仍然不一致，记 warn 让用户/开发者
                // 知道对账失败。这时窗口会停在非预期尺寸（通常是 +1px），
                // 属于极端兜底场景。
                if let Ok(final_size) = window.inner_size() {
                    if final_size.width != original.width || final_size.height != original.height {
                        log::warn!(
                            "Linux nudge 尺寸 drift 补偿后仍不一致: expected={}x{}, got={}x{}",
                            original.width,
                            original.height,
                            final_size.width,
                            final_size.height
                        );
                    }
                }
            }
        }
        Err(e) => {
            log::warn!("Linux nudge: 对账回读 inner_size 失败: {e}");
        }
    }
}

/// 焦点兜底：`show_main` 里的 `set_focus()` 紧跟 `show()`，而 Tao Linux 的
/// `set_focus` 在 GTK window 变为可见前会被静默跳过（内部检查
/// `get_visible()`），200ms 后的那一次也可能因为主循环繁忙而落空。这里在
/// 窗口未获得焦点时按固定间隔重试，直到拿到焦点或次数用尽。返回最终是否
/// 获得焦点，仅用于日志。
async fn retry_focus(window: &WebviewWindow, generation: u64) -> bool {
    for _ in 0..FOCUS_RETRY_ATTEMPTS {
        if !is_current(generation) {
            return false;
        }
        if window.is_focused().unwrap_or(false) {
            return true;
        }
        let _ = window.set_focus();
        tokio::time::sleep(FOCUS_RETRY_WAIT).await;
    }
    is_current(generation) && window.is_focused().unwrap_or(false)
}

#[cfg(test)]
mod tests {
    use super::{decorations_need_restore, is_current, REACTIVATE_GENERATION};
    use std::sync::atomic::Ordering;

    #[test]
    fn decorations_are_only_restored_on_mismatch() {
        assert!(!decorations_need_restore(true, true));
        assert!(!decorations_need_restore(false, false));
        assert!(decorations_need_restore(true, false));
        assert!(decorations_need_restore(false, true));
    }

    #[test]
    fn newer_reactivation_invalidates_older_generation() {
        let older = REACTIVATE_GENERATION.fetch_add(1, Ordering::AcqRel) + 1;
        assert!(is_current(older));

        let newer = REACTIVATE_GENERATION.fetch_add(1, Ordering::AcqRel) + 1;
        assert_ne!(older, newer);
        assert!(!is_current(older));
        assert!(is_current(newer));
    }
}

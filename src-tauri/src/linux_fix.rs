//! Linux 专用的主窗口恢复补丁。
//!
//! 解决 Tauri 2.x 在部分 Linux 发行版（尤其是 Wayland / 某些 WebKitGTK
//! 版本）上启动或从托盘/隐藏状态恢复后 UI 无法响应点击的问题：
//!
//! - **失效模式 A**（Tauri #10746 / wry #637）：webview 在 `show()` 后
//!   没有获得 keyboard focus，导致首次点击被 X11/Wayland 用作
//!   click-to-activate 而非传给 webview。
//! - **失效模式 B**：GTK surface 与 WebKitWebView 的 input region 尺寸
//!   协商在 `visible:false` → `show()` 的路径上失败，整窗永远不响应
//!   点击，只有重新 `size_allocate` 才能恢复。
//! - **失效模式 C**（Issue #7405）：在 Wayland 下当窗口经由 `visible:false` →
//!   `show()` 或从托盘隐藏恢复时，GTK HeaderBar 的事件子表面未能建立
//!   正确的输入路由，导致窗口控制按钮（最小化、最大化/还原、关闭）无法交互，
//!   之前必须手动最大化/还原才能恢复。
//!
//! 本模块导出 [`nudge_main_window`]。针对上述失效模式，采用零抽搐（Zero-Twitch）
//! 的方式优雅解决：
//! 1. 显式多阶段 `set_focus`，解决失效模式 A；
//! 2. 短暂（~30ms）切换 `set_resizable` 状态，触发 Tao 底层 `connect_resizable_notify`
//!    就地重建 `HeaderBar::set_decoration_layout()`，重新绑定窗口控制按钮的 Wayland
//!    事件子表面与输入路由，无需窗口发生任何宏观缩放/动画，肉眼完全无感，彻底杜绝抽搐，
//!    解决失效模式 C；
//! 3. 针对未最大化窗口，以 `LogicalSize` 进行 1 逻辑像素的微调刷新 WebKitWebView
//!    input region，避免 HiDPI 下物理像素截断为 0 的问题，解决失效模式 B；
//! 4. 针对已最大化窗口，严禁调用 `set_size`（最大化窗口尺寸受合成器硬约束，调用
//!    `set_size` 无效且会引发 drift 告警与合成器重绘抖动）。

use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Duration;

use tauri::{LogicalSize, WebviewWindow};

static IS_NUDGING: AtomicBool = AtomicBool::new(false);

/// 在 webview realize 之后的延迟，等 GTK 主循环把 realize 事件处理完。
const REALIZE_WAIT: Duration = Duration::from_millis(200);

/// 状态切换微调间隔（~30ms，覆盖 1~2 个显示刷新周期）。
const TOGGLE_GAP: Duration = Duration::from_millis(30);

/// 对主窗口执行 Linux 专用的「focus + surface 重激活」序列。
///
/// 调用是 fire-and-forget：内部 spawn 一个异步任务并在防抖互斥保护下完成。
/// 调用线程立即返回，不阻塞 UI。
pub(crate) fn nudge_main_window(window: WebviewWindow) {
    // 第一次 set_focus：webview 可能还没 realize，这一次通常成本极低，顺手做掉。
    let _ = window.set_focus();

    // 防抖互斥：若当前已有 nudge 任务正在进行，直接返回，避免连续事件引发并发竞争。
    if IS_NUDGING
        .compare_exchange(false, true, Ordering::SeqCst, Ordering::SeqCst)
        .is_err()
    {
        return;
    }

    tauri::async_runtime::spawn(async move {
        // 使用 RAII Guard 确保退出任务时总能释放互斥标志
        struct NudgeGuard;
        impl Drop for NudgeGuard {
            fn drop(&mut self) {
                IS_NUDGING.store(false, Ordering::SeqCst);
            }
        }
        let _guard = NudgeGuard;

        tokio::time::sleep(REALIZE_WAIT).await;

        // 第二次 set_focus：此时 webview realize 已完成，消除失效模式 A。
        let _ = window.set_focus();

        let is_maximized = window.is_maximized().unwrap_or(false);

        // 1. 修复 GTK HeaderBar 按钮输入路由 (Issue #7405 / 失效模式 C)：
        // 在 Wayland 下，Tao 的 WlHeader 通过 connect_resizable_notify 监听 resizable 变化
        // 并在变化时调用 header.set_decoration_layout() 重建并重新挂载窗口控制按钮。
        // 通过短暂翻转 resizable 状态（~30ms 内完成），GTK 会就地销毁并以正确的 Wayland
        // 子表面事件掩码重新构建最小化/最大化/关闭按钮。
        // 这一过程完全不改变窗口尺寸与最大化状态，肉眼完全无感知，彻底杜绝窗口“抽搐/闪烁”。
        let is_resizable = window.is_resizable().unwrap_or(true);
        let _ = window.set_resizable(!is_resizable);
        tokio::time::sleep(TOGGLE_GAP).await;
        let _ = window.set_resizable(is_resizable);

        // 2. 修复 WebKitWebView input region 协商问题 (失效模式 B)：
        // 仅在非最大化窗口下执行微调（以 LogicalSize 为基准，避免 HiDPI 下物理像素截断为 0）。
        // 最大化窗口尺寸受合成器硬约束，严禁调用 set_size，否则会引发 drift 告警与画面撕裂。
        if !is_maximized {
            if let (Ok(physical), Ok(scale)) = (window.inner_size(), window.scale_factor()) {
                let scale = if scale > 0.0 { scale } else { 1.0 };
                let logical = physical.to_logical::<f64>(scale);
                let bumped = LogicalSize::new(logical.width + 1.0, logical.height);
                let _ = window.set_size(bumped);
                tokio::time::sleep(TOGGLE_GAP).await;
                let _ = window.set_size(logical);
            }
        }

        let _ = window.set_focus();
        log::info!(
            "Linux: 已对主窗口执行 focus + HeaderBar 控制按钮与 surface 重激活 (maximized={is_maximized})"
        );
    });
}

//! Linux 专用的主窗口恢复补丁。
//!
//! 解决 Tauri 2.x 在部分 Linux 发行版（尤其是 Wayland / 某些 WebKitGTK
//! 版本）上启动后 UI 无法响应点击的问题：
//!
//! - **失效模式 A**（Tauri #10746 / wry #637）：webview 在 `show()` 后
//!   没有获得 keyboard focus，导致首次点击被 X11/Wayland 用作
//!   click-to-activate 而非传给 webview。
//! - **失效模式 B**：GTK surface 与 WebKitWebView 的 input region 尺寸
//!   协商在 `visible:false` → `show()` 的路径上失败，整窗永远不响应
//!   点击，只有重新 `size_allocate`（例如最大化-还原）才能恢复。
//!
//! 本模块导出 [`nudge_main_window`]，它通过「显式 set_focus + 无视觉
//! 版本的 ±1px 伪 resize」精确模拟用户手动最大化再还原的 workaround，
//! 但肉眼无法察觉。所有"让主窗口出现在用户面前"的路径（正常启动、
//! deeplink 唤起、single_instance 回调、托盘 show_main、lightweight
//! 退出）都应在现有 `set_focus()` 之后追加一次调用。

use std::{
    sync::atomic::{AtomicU64, Ordering},
    time::Duration,
};

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

/// Resize events arrive in bursts while dragging a border or moving between
/// monitors. Only repaint once after the burst settles.
const RESIZE_REPAINT_WAIT: Duration = Duration::from_millis(180);

static RESIZE_REPAINT_GENERATION: AtomicU64 = AtomicU64::new(0);

const RESIZE_REPAINT_SCRIPT: &str = r#"
(() => {
  const body = document.body;
  if (!body) return;

  const previousTransform = body.style.transform;
  const previousBackfaceVisibility = body.style.backfaceVisibility;

  body.style.transform = previousTransform
    ? `${previousTransform} translateZ(0)`
    : "translateZ(0)";
  body.style.backfaceVisibility = "hidden";
  void body.offsetHeight;

  window.dispatchEvent(new Event("resize"));

  requestAnimationFrame(() => {
    body.style.transform = previousTransform;
    body.style.backfaceVisibility = previousBackfaceVisibility;
    void body.offsetHeight;
  });
})();
"#;

fn queue_webview_repaint(window: &WebviewWindow) {
    let _ = window.with_webview(|webview| {
        use gtk::prelude::WidgetExt;

        webview.inner().queue_draw();
    });

    if let Err(err) = window.eval(RESIZE_REPAINT_SCRIPT) {
        log::debug!("Linux repaint: 前端重绘脚本执行失败: {err}");
    }
}

fn is_effective_wayland_backend(gdk_backend: Option<&str>, has_wayland_display: bool) -> bool {
    let requested_backend = gdk_backend
        .and_then(|value| value.split(',').find(|part| !part.trim().is_empty()))
        .map(|value| value.trim().to_ascii_lowercase());

    match requested_backend.as_deref() {
        Some("x11") => false,
        Some("wayland") => true,
        _ => has_wayland_display,
    }
}

/// 对主窗口执行 Linux 专用的「focus + surface 重激活」序列。
///
/// 调用是 fire-and-forget：内部 spawn 一个异步任务在 ~250ms 后完成。
/// 调用线程立即返回，不阻塞 UI。
pub(crate) fn nudge_main_window(window: WebviewWindow) {
    // 第一次 set_focus：webview 可能还没 realize，这一次通常是无效的，
    // 但成本极低（线程安全，内部 run_on_main_thread），顺手做掉。
    let _ = window.set_focus();

    tauri::async_runtime::spawn(async move {
        tokio::time::sleep(REALIZE_WAIT).await;

        // 第二次 set_focus：此时 webview realize 已完成，在绝大多数
        // 发行版上这一次会真的生效，消除失效模式 A。
        let _ = window.set_focus();
        queue_webview_repaint(&window);

        let is_wayland = is_effective_wayland_backend(
            std::env::var("GDK_BACKEND").ok().as_deref(),
            std::env::var_os("WAYLAND_DISPLAY").is_some(),
        );
        let skip_pseudo_resize = is_wayland
            || matches!(window.is_maximized(), Ok(true))
            || matches!(window.is_fullscreen(), Ok(true));
        if skip_pseudo_resize {
            log::info!("Linux: 已对主窗口执行 focus + repaint，跳过 Wayland/最大化/全屏伪 resize");
            return;
        }

        // 伪 resize：读取当前 inner_size，先加 1px 再还原。这会触发
        // GTK 的 size-allocate → WebKitWebViewBase::size_allocate →
        // 重新 attach input surface，消除失效模式 B。
        //
        // 使用 PhysicalSize 避免跨 DPI 的逻辑坐标漂移；saturating_add
        // 防止极端尺寸溢出。
        match window.inner_size() {
            Ok(original) => {
                let bumped = PhysicalSize::new(original.width.saturating_add(1), original.height);
                let _ = window.set_size(bumped);
                tokio::time::sleep(RESIZE_GAP).await;
                let _ = window.set_size(original);
                log::info!("Linux: 已对主窗口执行 focus + surface 重激活");

                // 尺寸对账回读：Tao Linux 的尺寸 API 是异步的，`set_size` 只是把
                // resize 请求送进 GTK 主循环队列，合成器可能会 coalesce 两次连续
                // 请求（尤其是第二次 `set_size(original)`），导致窗口永久停留在
                // width+1。这里等合成器处理完队列后读一次实际尺寸，发现 drift 就
                // 再补一次 `set_size(original)` 兜底。
                //
                // 已知限制：tiling Wayland 合成器（sway/river/hyprland）会完全忽略
                // `set_size`，此时对账永远 drift=0（因为两次 set_size 都是 no-op），
                // 看起来"没问题"但失效模式 B 其实没被修复；这是已知限制，需要用户
                // 侧用 GDK_BACKEND=x11 绕过，README 应该有说明。
                tokio::time::sleep(RECONCILE_WAIT).await;
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
                                if final_size.width != original.width
                                    || final_size.height != original.height
                                {
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
            Err(e) => {
                // 极罕见的失败路径；只做了 set_focus 也比什么都不做强，
                // 不要让 resize 失败把整个补丁吞掉。
                log::warn!("Linux nudge: 读取 inner_size 失败，跳过伪 resize: {e}");
            }
        }
    });
}

/// Linux WebKitGTK can keep a stale/black backing surface after manual resize,
/// monitor moves, or scale-factor changes. This is a lighter repaint than
/// `nudge_main_window`: it does not call `set_size`, so it is safe to trigger
/// from resize events without creating a resize loop.
pub(crate) fn repaint_main_window_after_resize(window: WebviewWindow) {
    let generation = RESIZE_REPAINT_GENERATION.fetch_add(1, Ordering::AcqRel) + 1;

    tauri::async_runtime::spawn(async move {
        tokio::time::sleep(RESIZE_REPAINT_WAIT).await;
        if RESIZE_REPAINT_GENERATION.load(Ordering::Acquire) != generation {
            return;
        }

        if matches!(window.is_visible(), Ok(false)) {
            return;
        }

        let _ = window.set_focus();
        queue_webview_repaint(&window);
        log::debug!("Linux: 已请求 resize 后 WebView 重绘");
    });
}

#[cfg(test)]
mod tests {
    use super::is_effective_wayland_backend;

    #[test]
    fn gdk_backend_x11_overrides_wayland_session() {
        assert!(!is_effective_wayland_backend(Some("x11"), true));
    }

    #[test]
    fn gdk_backend_first_entry_controls_backend() {
        assert!(!is_effective_wayland_backend(Some("x11,wayland"), true));
        assert!(is_effective_wayland_backend(Some("wayland,x11"), false));
    }

    #[test]
    fn wayland_display_is_used_when_backend_is_unset() {
        assert!(is_effective_wayland_backend(None, true));
        assert!(!is_effective_wayland_backend(None, false));
    }

    #[test]
    fn unknown_backend_falls_back_to_session_probe() {
        assert!(is_effective_wayland_backend(Some("broadway"), true));
        assert!(!is_effective_wayland_backend(Some("broadway"), false));
    }
}

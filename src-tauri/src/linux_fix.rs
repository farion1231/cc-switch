//! Linux 专用的主窗口恢复补丁。
//!
//! ## 根因
//!
//! Tauri 2.x 在 Wayland 下（通过 Tao 的 `WlHeader::setup`）会无条件安装
//! GTK HeaderBar 作为自定义标题栏。当 `decorations=true` 时，KWin 也通过
//! xdg-decoration 协议提供 SSDF 原生装饰。两者的 button 区域重叠：
//! - 用户看到并点击的是 KWin 原生按钮
//! - 但 GTK HeaderBar 的 EventBox 在 Z 序上覆盖在 KWin 按钮上方
//! - 点击事件被 HeaderBar 拦截，KWin 按钮无法响应
//!
//! 最大化→还原可修复是因为 size_allocate 级联强制了 HeaderBar/KWin 的
//! 重对齐。hide→show 后 HeaderBar 重新渲染，再次覆盖 KWin 按钮。
//!
//! ## 修复方案
//!
//! 当 `decorations=true`（SD 模式）时，移除 GTK HeaderBar，让 KWin SSDF
//! 独占用标题栏区域。仅在 Wayland、decorations=true 时执行。

use std::time::Duration;

use tauri::{Manager, WebviewWindow};
use webkit2gtk::glib::ObjectType;

/// 在 webview realize 之后的延迟，等 GTK 主循环把 realize 事件处理完。
const REALIZE_WAIT: Duration = Duration::from_millis(200);

/// 对主窗口执行 Linux 专用的修复序列。
///
/// 调用是 fire-and-forget：内部 spawn 异步任务，调用线程立即返回。
pub(crate) fn nudge_main_window(window: WebviewWindow) {
    let _ = window.set_focus();

    tauri::async_runtime::spawn(async move {
        tokio::time::sleep(REALIZE_WAIT).await;

        // 第二次 set_focus：此时 webview realize 已完成，消除失效模式 A
        let _ = window.set_focus();

        let decorations = !crate::settings::get_settings().use_app_window_controls;

        // 仅在 decorations=true（SD 模式）时移除 GTK HeaderBar。
        // 否则（app 自绘按钮模式）保留 HeaderBar 供前端使用。
        if decorations {
            let webview_handle = window.clone();
            let _ = window.app_handle().run_on_main_thread(move || {
                let _ = webview_handle.with_webview(|wv| {
                    // Cast via gobject pointer. WebView ISA GtkWidget so
                    // the GObject pointer is also a valid *mut GtkWidget.
                    let widget_ptr: *mut gtk_sys::GtkWidget = wv.inner().as_ptr() as *mut _;
                    // Use gtk_widget_get_toplevel to navigate from WebView
                    // up to GtkApplicationWindow, without importing gtk.
                    let toplevel = unsafe { gtk_sys::gtk_widget_get_toplevel(widget_ptr) };
                    if !toplevel.is_null() {
                        unsafe {
                            gtk_sys::gtk_window_set_titlebar(
                                toplevel as *mut gtk_sys::GtkWindow,
                                std::ptr::null_mut(),
                            );
                        }
                        log::info!("Linux: 已移除 GTK HeaderBar，仅保留 KWin SSDF 原生装饰");
                    }
                });
            });
        } else {
            log::info!("Linux: 已对主窗口执行 focus (app 自绘按钮模式，保留 HeaderBar)");
        }
    });
}

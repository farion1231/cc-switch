//! Windows 专用：WebView2 渲染进程死亡后的自动恢复。
//!
//! 系统级内存压力/卡顿会让 Windows 杀掉 WebView2 的渲染进程：浏览器主进程
//! 与本应用都活着，托盘、代理一切正常，唯独主窗口从此一片白。wry 0.54 /
//! tauri 2.10 都没有把 `ProcessFailed` 事件暴露出来，前端 JS 也已随进程死亡，
//! 错误边界与任何 show/focus 都救不回来——只能由 Rust 侧直接挂 COM 事件，
//! 死亡发生时 `reload()`（WebView2 会自动重启渲染进程）。
//!
//! 事件回调里只做三件事：记录故障标志（供单实例二次启动兜底重载）、按冷却
//! 窗口节流地自动 reload、写日志。其余进程类型（浏览器主进程、GPU、子框架
//! 等）交给 WebView2 自己的默认处理，避免过度干预。
use std::sync::atomic::{AtomicBool, AtomicI64, Ordering};
use std::time::{SystemTime, UNIX_EPOCH};

/// 渲染进程死亡后尚未成功恢复的标志：单实例二次启动时据此补一次 reload
/// （自动恢复失败、或用户从未触发过自动路径时的逃生门）。
pub static RENDERER_FAILURE_PENDING: AtomicBool = AtomicBool::new(false);

/// 上次自动 reload 的毫秒时间戳；防止坏循环（reload 后立刻又死）刷屏。
static LAST_AUTO_RELOAD_MS: AtomicI64 = AtomicI64::new(0);

/// 自动恢复的冷却窗口：两次自动 reload 至少间隔这么久。
const AUTO_RELOAD_COOLDOWN_MS: i64 = 10_000;

/// 渲染进程死亡（主框架）。这是白屏的直接原因；其余类型（浏览器主进程退
/// 出、GPU、子框架、无响应等）不由这里处理——主进程退出时 reload 无处发生，
/// GPU/子框架 WebView2 会自行重建，UNRESPONSIVE 是"卡"不是"死"，抢跑 reload
/// 反而会打断可自行恢复的页面。
fn renderer_process_exited(kind_code: i32) -> bool {
    kind_code == 1 // COREWEBVIEW2_PROCESS_FAILED_KIND_RENDER_PROCESS_EXITED
}

fn cooldown_elapsed(now_ms: i64, last_reload_ms: i64) -> bool {
    // 系统时钟被回拨（now < last）时视为冷却已过：宁可多恢复一次，
    // 也不能把恢复卡死在一段永远等不完的时间里
    now_ms < last_reload_ms || now_ms.saturating_sub(last_reload_ms) >= AUTO_RELOAD_COOLDOWN_MS
}

fn now_ms() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_millis() as i64)
        .unwrap_or(0)
}

/// 渲染进程死亡的处理入口（COM 回调线程调用）。返回是否触发了 reload。
fn handle_renderer_exit(app: &tauri::AppHandle) -> bool {
    use tauri::Manager;
    RENDERER_FAILURE_PENDING.store(true, Ordering::SeqCst);
    let now = now_ms();
    let last = LAST_AUTO_RELOAD_MS.load(Ordering::SeqCst);
    if !cooldown_elapsed(now, last) {
        log::warn!(
            "[WebViewRecovery] 渲染进程死亡后 {}ms 内已有一次自动恢复，跳过本次（等待单实例/托盘手动路径）",
            now.saturating_sub(last)
        );
        return false;
    }
    LAST_AUTO_RELOAD_MS.store(now, Ordering::SeqCst);
    if let Some(window) = app.get_webview_window("main") {
        match window.reload() {
            Ok(()) => {
                log::warn!("[WebViewRecovery] 渲染进程死亡，已自动重载界面");
                RENDERER_FAILURE_PENDING.store(false, Ordering::SeqCst);
                true
            }
            Err(error) => {
                log::error!("[WebViewRecovery] 自动重载失败: {error}；下次启动或托盘可恢复");
                false
            }
        }
    } else {
        log::warn!("[WebViewRecovery] 渲染进程死亡但主窗口不存在，跳过恢复");
        false
    }
}

/// 在主窗口的 WebView2 上挂 ProcessFailed 监听。失败只记日志——这条路径
/// 的任何问题都不该拦住应用启动。
pub fn install(app: &tauri::AppHandle) {
    use tauri::Manager;
    let Some(window) = app.get_webview_window("main") else {
        return;
    };
    let handle = app.clone();
    let result = window.with_webview(move |webview| {
        use webview2_com::Microsoft::Web::WebView2::Win32::{
            ICoreWebView2ProcessFailedEventArgs, COREWEBVIEW2_PROCESS_FAILED_KIND,
        };
        use webview2_com::ProcessFailedEventHandler;

        let controller = webview.controller();
        let core_webview = match unsafe { controller.CoreWebView2() } {
            Ok(core) => core,
            Err(error) => {
                log::warn!("[WebViewRecovery] 取 CoreWebView2 失败，跳过崩溃监听: {error}");
                return;
            }
        };
        let mut token = 0i64;
        let registration = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| unsafe {
            core_webview.add_ProcessFailed(
                &ProcessFailedEventHandler::create(Box::new(move |_, args| {
                    let Some(args): Option<ICoreWebView2ProcessFailedEventArgs> = args else {
                        return Ok(());
                    };
                    let mut kind = COREWEBVIEW2_PROCESS_FAILED_KIND::default();
                    if let Err(error) = args.ProcessFailedKind(&mut kind) {
                        log::warn!("[WebViewRecovery] 读取进程失败类型出错: {error}");
                        return Ok(());
                    }
                    if renderer_process_exited(kind.0) {
                        handle_renderer_exit(&handle);
                    } else {
                        log::debug!(
                            "[WebViewRecovery] 非渲染进程失败（kind={}），交由 WebView2 自处理",
                            kind.0
                        );
                    }
                    Ok(())
                })),
                &mut token,
            )
        }));
        match registration {
            Ok(Ok(())) => log::info!("[WebViewRecovery] 已注册渲染进程崩溃自动恢复"),
            Ok(Err(error)) => {
                log::warn!("[WebViewRecovery] 注册 ProcessFailed 失败: {error}")
            }
            Err(_) => log::warn!("[WebViewRecovery] 注册 ProcessFailed 时发生 panic，跳过"),
        }
    });
    if let Err(error) = result {
        log::warn!("[WebViewRecovery] with_webview 调用失败，跳过崩溃监听: {error}");
    }
}

/// 单实例二次启动时的兜底：若渲染进程死后自动恢复没能完成，补一次 reload。
/// 正常路径（自动恢复已成功）标志已清空，这里不会多动。
pub fn reload_if_renderer_failure_pending(window: &tauri::WebviewWindow) {
    if RENDERER_FAILURE_PENDING.swap(false, Ordering::SeqCst) {
        log::warn!("[WebViewRecovery] 检测到未恢复的渲染进程故障，借二次启动重载界面");
        if let Err(error) = window.reload() {
            log::error!("[WebViewRecovery] 二次启动重载失败: {error}");
        }
    }
}

/// 托盘手动重载的入口。
pub fn reload_main_window(app: &tauri::AppHandle) -> Result<(), String> {
    use tauri::Manager;
    let Some(window) = app.get_webview_window("main") else {
        return Err("主窗口不存在".into());
    };
    window.reload().map_err(|e| e.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_the_main_render_process_kinds_trigger_reload() {
        assert!(renderer_process_exited(1)); // RENDER_PROCESS_EXITED
        assert!(!renderer_process_exited(0)); // BROWSER_PROCESS_EXITED：reload 无处发生
        assert!(!renderer_process_exited(2)); // RENDER_PROCESS_UNRESPONSIVE：卡而非死
        assert!(!renderer_process_exited(3)); // FRAME_RENDER_PROCESS_EXITED：子框架
        assert!(!renderer_process_exited(6)); // GPU_PROCESS_EXITED：WebView2 自重建
    }

    #[test]
    fn cooldown_gates_repeated_reloads() {
        assert!(cooldown_elapsed(10_000, 0));
        assert!(!cooldown_elapsed(10_000, 5_000));
        assert!(cooldown_elapsed(20_000, 5_000));
        // 时钟回拨时宁可多恢复一次，也不卡死在永远等不到的冷却上
        assert!(cooldown_elapsed(1_000, 5_000));
    }
}

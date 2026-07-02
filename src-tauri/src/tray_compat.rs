//! 托盘右键菜单第三方兼容层。
//!
//! ## 背景
//! `tray-icon` 0.21.x 的 Windows 窗口过程 `tray_proc` 仅在收到 Explorer 风格的
//! 托盘回调（`uCallbackMessage` + `WM_RBUTTONDOWN`）时才 `TrackPopupMenu` 弹出
//! 菜单，且中途依赖 `Shell_NotifyIconGetRect` 取图标矩形——取不到就直接 `return`
//! 不弹菜单。它完全不理会 `WM_CONTEXTMENU`。
//!
//! 第三方托盘 / Dock（MyDockFinder、StartAllBack、RetroBar 等）拦截
//! `Shell_NotifyIconW` 自绘图标后，右键时按 Win32 通用约定**直接向应用注册的
//! HWND 发送 `WM_CONTEXTMENU`**（`wParam`=HWND，`lParam`=MAKELONG(x,y) 屏幕坐标），
//! 而不是回放 Explorer 的 `uCallbackMessage + WM_RBUTTONDOWN` 协议。结果 `tray_proc`
//! 收不到能触发菜单的消息 → 菜单不弹 → 用户"右键没有任何反应"。
//!
//! ## 方案
//! 用 comctl32 的 `SetWindowSubclass` 给托盘隐藏窗口挂一个独立 subclass（id 与 muda
//! 的 200/202 错开），拦截 `WM_CONTEXTMENU`：用当前菜单的 HMENU 直接 `TrackPopupMenu`
//! 弹出，**绕开 tray-icon 对 `Shell_NotifyIconGetRect` 的依赖**（第三方托盘下该 API
//! 多半返回失败）。其余消息走 `DefSubclassProc` 原样转发给 muda / `tray_proc`，保持
//! Explorer 流程零行为变化。
//!
//! ## 为什么不用 `SetWindowLongPtrW`
//! muda 的菜单子类化也走 `SetWindowSubclass`（comctl32，id=200）。若用
//! `SetWindowLongPtrW` 直接替换 `GWLP_WNDPROC`，则每次 `tray.set_menu` 刷新菜单时
//! muda 的 `RemoveWindowSubclass`（最后一个 subclasser 被移除时）会把 `GWLP_WNDPROC`
//! 恢复成它内部缓存的原始 `tray_proc`，**把我们的 wndproc 吹掉**——刷新一次后兼容层即失效。
//! 走 `SetWindowSubclass` 用独立 id 则与 muda 互不干扰，刷新菜单也不会丢。

#[cfg(target_os = "windows")]
mod windows_impl {
    use std::sync::OnceLock;

    use tauri::menu::{ContextMenu, Menu};
    use windows_sys::Win32::Foundation::{HWND, LPARAM, LRESULT, POINT, WPARAM};
    use windows_sys::Win32::UI::Shell::{DefSubclassProc, SetWindowSubclass};
    use windows_sys::Win32::UI::WindowsAndMessaging::{
        GetCursorPos, PostMessageW, SetForegroundWindow, TrackPopupMenu, TPM_BOTTOMALIGN,
        TPM_LEFTALIGN, WM_CONTEXTMENU, WM_NULL,
    };

    use crate::tray::TRAY_ID;

    /// 我们的 comctl32 subclass id，避开 muda 的 200 / 202。
    const TRAY_CONTEXT_COMPAT_SUBCLASS_ID: usize = 0x5357; // 'SW'

    /// 当前托盘菜单 + 其 HMENU 的快照。
    ///
    /// 存 `Menu` clone 是为了让底层 `muda::Menu`（及其 HMENU）保持存活：
    /// muda 文档明确“HMENU 在 ContextMenu 存活期间有效”。每次 `create_tray_menu`
    /// 重建都会整表覆盖写入，保证句柄始终指向当前活跃菜单。
    static CURRENT_TRAY_MENU: OnceLock<std::sync::Mutex<Option<(Menu<tauri::Wry>, isize)>>> =
        OnceLock::new();

    fn menu_slot() -> &'static std::sync::Mutex<Option<(Menu<tauri::Wry>, isize)>> {
        CURRENT_TRAY_MENU.get_or_init(|| std::sync::Mutex::new(None))
    }

    /// 记录当前托盘菜单（`create_tray_menu` 构建完成后调用）。
    ///
    /// 此处提前在主线程上下文抽取 HMENU，避免在 subclass 热路径里再做
    /// `run_on_main_thread` marshal（subclass proc 本身就跑在主线程，再 marshal 到主线程有死锁风险）。
    pub fn set_current_tray_menu(menu: &Menu<tauri::Wry>) {
        let hmenu = menu.hpopupmenu().unwrap_or(0);
        let mut slot = menu_slot()
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        *slot = Some((menu.clone(), hmenu));
    }

    fn current_hmenu() -> Option<isize> {
        let slot = menu_slot().lock().ok()?;
        let (_, h) = slot.as_ref()?;
        (*h != 0).then_some(*h)
    }

    unsafe extern "system" fn compat_subclass_proc(
        hwnd: HWND,
        msg: u32,
        wparam: WPARAM,
        lparam: LPARAM,
        _uidsubclass: usize,
        _dwrefdata: usize,
    ) -> LRESULT {
        // 第三方托盘右键走这里：直接弹当前菜单，不依赖 Shell_NotifyIconGetRect。
        if msg == WM_CONTEXTMENU {
            if let Some(hmenu) = current_hmenu() {
                let lparam_i32 = lparam as i32;
                // GET_X_LPARAM / GET_Y_LPARAM（带符号扩展）
                let mut x = (lparam_i32 as u16) as i16 as i32;
                let mut y = ((lparam_i32 >> 16) as u16) as i16 as i32;
                // 键盘触发（Shift+F10 / 菜单键）时 lParam == -1，回退到光标位置
                if lparam_i32 == -1 {
                    let mut pt = POINT { x: 0, y: 0 };
                    if GetCursorPos(&mut pt) != 0 {
                        x = pt.x;
                        y = pt.y;
                    }
                }
                // MSDN 推荐的托盘弹出序列：先 SetForegroundWindow，再 TrackPopupMenu，
                // 最后 PostMessage(WM_NULL) —— 保证点击菜单外区域时能自动收起
                // （仅 TrackPopupMenu 在部分场景下会出现“点外部不消失”）。
                SetForegroundWindow(hwnd);
                TrackPopupMenu(
                    hmenu as *mut core::ffi::c_void,
                    TPM_LEFTALIGN | TPM_BOTTOMALIGN,
                    x,
                    y,
                    0,
                    hwnd,
                    std::ptr::null(),
                );
                PostMessageW(hwnd, WM_NULL, 0, 0);
                return 0;
            }
        }

        // 其余消息原样转发给 muda / tray_proc，保持 Explorer 流程零变化
        DefSubclassProc(hwnd, msg, wparam, lparam)
    }

    /// 给托盘隐藏窗口挂 comctl32 subclass 以拦截 `WM_CONTEXTMENU`。
    ///
    /// 必须在托盘构建后调用；comctl32 subclass 要求在窗口所属线程（主线程）安装，
    /// 当前调用点在 `setup()` 闭包内，满足该约束（与 `create_tray_menu` 同处主线程上下文，
    /// 其内部 `MenuItem::with_id` 等同样走 `run_on_main_thread` marshal 且线上工作正常）。
    pub fn install(app: &tauri::AppHandle) {
        let Some(tray) = app.tray_by_id(TRAY_ID) else {
            log::warn!("[tray_compat] 托盘未找到，跳过 WM_CONTEXTMENU 兼容安装");
            return;
        };
        let hwnd = match tray.with_inner_tray_icon(|t| t.window_handle() as isize) {
            Ok(hwnd) => hwnd,
            Err(e) => {
                log::warn!("[tray_compat] 获取托盘 HWND 失败: {e}");
                return;
            }
        };
        if hwnd == 0 {
            log::warn!("[tray_compat] 托盘 HWND 为空");
            return;
        }
        let ok = unsafe {
            SetWindowSubclass(
                hwnd as HWND,
                Some(compat_subclass_proc),
                TRAY_CONTEXT_COMPAT_SUBCLASS_ID,
                0,
            )
        };
        if ok == 0 {
            log::warn!(
                "[tray_compat] SetWindowSubclass 失败: {}",
                std::io::Error::last_os_error()
            );
            return;
        }
        log::info!("[tray_compat] 已安装 WM_CONTEXTMENU 兼容层（HWND={hwnd}）");
    }
}

#[cfg(target_os = "windows")]
pub use windows_impl::{install, set_current_tray_menu};

#[cfg(not(target_os = "windows"))]
mod stub {
    use tauri::menu::Menu;
    /// 非 Windows 平台空实现，保持 `tray.rs` 跨平台编译。
    pub fn set_current_tray_menu(_menu: &Menu<tauri::Wry>) {}
}

#[cfg(not(target_os = "windows"))]
pub use stub::set_current_tray_menu;

//! Windows 专用的主窗口尺寸修正。
//!
//! 解决在缩放比例不同的多显示器之间拖动主窗口后，窗口尺寸被放大到
//! Win32 坐标上限、导致界面永久不可见的问题：
//!
//! - **触发条件**：系统接有两块及以上缩放比例不同的显示器（例如主屏
//!   100%、副屏 175%）。跨屏拖动时 Windows 发出 `WM_DPICHANGED`，窗口
//!   尺寸在按新缩放因子重算的过程中被放大，最终撞上 Win32 消息里以
//!   16 位表示坐标的上限 65535。
//! - **失效表现**：窗口对象本身完全有效——进程存活、`IsWindowVisible`
//!   为真、托盘图标正常、`GetWindowRect` 能读到矩形——但 65535x53251
//!   的绘制表面远超 WebView2 的渲染上限，整个窗口不显示任何内容，用户
//!   看到的就是"应用还在跑但主界面消失了"。
//! - **故障永久化**：`tauri-plugin-window-state` 会把这个尺寸原样写进
//!   `.window-state.json`（实测 `width: 65519, height: 53212`，同时
//!   `prev_x` 为 `-32768`），此后每次启动都从坏值恢复。重启应用、重启
//!   系统都无法自愈，用户只能手动删除该文件——而这个文件在 `%APPDATA%`
//!   下且以点开头，普通用户不可能找到。
//!
//! 插件在恢复 `POSITION` 时会用 `Monitor::intersects` 检查窗口是否落在
//! 某块显示器上，不在就交给系统摆放；但恢复 `SIZE` 时是无条件
//! `set_size`，没有任何上限校验。尺寸一旦溢出，那个巨大的矩形又必然与
//! 每块显示器相交，连带把位置校验也绕了过去。本模块补上缺失的尺寸校验。
//!
//! 导出的 [`clamp_main_window_size`] 在「窗口状态恢复之后」与「DPI 变化
//! 之后」各调用一次：前者让已经损坏的配置能自愈，后者让窗口在跨屏拖动
//! 当场就恢复。只有尺寸真的越界时才会动窗口，正常尺寸不受影响。

use tauri::{Monitor, PhysicalPosition, PhysicalSize, WebviewWindow, WindowEvent};

/// 需要重置尺寸时的目标逻辑尺寸，与 `tauri.conf.json` 中主窗口的
/// `width` / `height` 保持一致。
const DEFAULT_LOGICAL_WIDTH: f64 = 1000.0;
const DEFAULT_LOGICAL_HEIGHT: f64 = 650.0;

/// 注册 DPI 变化监听。
///
/// 跨不同缩放的显示器拖动窗口时 Windows 发出 `WM_DPICHANGED`，Tauri 将其转成
/// [`WindowEvent::ScaleFactorChanged`]——这正是尺寸溢出的发生时机。在事件之后
/// 立刻校验一次，窗口可以当场恢复，用户不必等到下次启动。
///
/// 回调在主线程事件循环中执行，可以安全调用窗口 API；`set_size` 只会触发
/// `Resized`，不会再次触发 `ScaleFactorChanged`，因此不存在递归。
pub fn watch_scale_factor_changes(window: &WebviewWindow) {
    let target = window.clone();
    window.on_window_event(move |event| {
        if matches!(event, WindowEvent::ScaleFactorChanged { .. }) {
            clamp_main_window_size(&target);
        }
    });
}

/// 校验并修正主窗口尺寸。失败只记日志，不影响启动流程。
pub fn clamp_main_window_size(window: &WebviewWindow) {
    if let Err(err) = clamp_size(window) {
        log::warn!("Windows: 主窗口尺寸校验失败: {err}");
    }
}

fn clamp_size(window: &WebviewWindow) -> tauri::Result<()> {
    // 最小化时 Win32 报告的不是真实几何，跳过，等窗口还原后再校验。
    if window.is_minimized()? {
        return Ok(());
    }

    // 最大化窗口的尺寸完全由系统管理，不会溢出。而且它的 outer_size 会略大于
    // 屏幕（实测 1936x1048 对 1920x1080 的显示器，四边各多出 8px 的隐藏边框），
    // 参与校验反而会被误判成越界。
    if window.is_maximized()? {
        return Ok(());
    }

    let monitors = window.available_monitors()?;
    if monitors.is_empty() {
        return Ok(());
    }

    let size = window.outer_size()?;

    // 整个虚拟桌面的外接矩形。窗口尺寸不可能合法地超过它，超过就是溢出。
    // 这里用显示器的完整尺寸而非工作区：窗口可以合法地盖住任务栏，拿工作区
    // 当上限会把接近全屏的正常窗口误判成溢出。
    let (virtual_width, virtual_height) = virtual_bounds(&monitors);
    if i64::from(size.width) <= virtual_width && i64::from(size.height) <= virtual_height {
        return Ok(());
    }

    // 优先落到主显示器：尺寸已经异常时 `current_monitor()` 的结果不可靠，
    // 主屏是用户最可能正在看的地方。
    let target = window
        .primary_monitor()?
        .or(window.current_monitor()?)
        .unwrap_or_else(|| monitors[0].clone());
    let work_area = target.work_area();
    let scale = target.scale_factor();

    let new_size = PhysicalSize::new(
        ((DEFAULT_LOGICAL_WIDTH * scale).round() as u32).min(work_area.size.width),
        ((DEFAULT_LOGICAL_HEIGHT * scale).round() as u32).min(work_area.size.height),
    );
    window.set_size(new_size)?;

    // 尺寸变了，原位置多半已经没有意义，直接摆回工作区中央。
    let x = work_area.position.x
        + (i64::from(work_area.size.width) - i64::from(new_size.width)).max(0) as i32 / 2;
    let y = work_area.position.y
        + (i64::from(work_area.size.height) - i64::from(new_size.height)).max(0) as i32 / 2;
    window.set_position(PhysicalPosition::new(x, y))?;

    log::warn!(
        "Windows: 主窗口尺寸越界，已从 {}x{} 修正为 {}x{}@({},{})",
        size.width,
        size.height,
        new_size.width,
        new_size.height,
        x,
        y
    );

    Ok(())
}

/// 整个虚拟桌面的外接矩形尺寸，即所有显示器完整区域的并集。
fn virtual_bounds(monitors: &[Monitor]) -> (i64, i64) {
    let mut left = i64::MAX;
    let mut top = i64::MAX;
    let mut right = i64::MIN;
    let mut bottom = i64::MIN;

    for monitor in monitors {
        let position = monitor.position();
        let size = monitor.size();
        let l = i64::from(position.x);
        let t = i64::from(position.y);
        left = left.min(l);
        top = top.min(t);
        right = right.max(l + i64::from(size.width));
        bottom = bottom.max(t + i64::from(size.height));
    }

    ((right - left).max(0), (bottom - top).max(0))
}

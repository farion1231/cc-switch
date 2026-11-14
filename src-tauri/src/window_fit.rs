//! 恢复主窗口的可用尺寸，并收回显示器工作区内。
//!
//! window-state 插件恢复位置时只检查窗口是否和某个显示器的整块区域有交集，
//! 不看任务栏 / Dock 占掉的部分。窗口底部若曾被拖到任务栏下面，每次启动都会
//! 恢复到同一位置，底部按钮（例如更新弹窗的按钮）一直被挡住。
//! 插件也会原样恢复异常小的非零尺寸（#9496），不能只依赖系统的最小尺寸限制。

use tauri::{LogicalSize, Manager, PhysicalPosition, PhysicalSize, Runtime, WebviewWindow};

/// 物理像素下的矩形。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct Rect {
    x: i32,
    y: i32,
    width: u32,
    height: u32,
}

/// 配置是逻辑内容区尺寸；工作区和窗口外框是物理像素。
fn outer_size(
    logical: LogicalSize<f64>,
    scale: f64,
    frame: PhysicalSize<u32>,
) -> PhysicalSize<u32> {
    let inner = logical.to_physical::<u32>(scale);
    PhysicalSize::new(
        inner.width.saturating_add(frame.width),
        inner.height.saturating_add(frame.height),
    )
}

/// 过小的维度恢复默认值，过大的缩到工作区，再把位置夹到工作区内。
/// 正常尺寸保持不变；小屏幕以工作区为限，避免每次显示都重复修正。
fn fit_into(
    window: Rect,
    work: Rect,
    minimum: PhysicalSize<u32>,
    default: PhysicalSize<u32>,
) -> Option<Rect> {
    if work.width == 0 || work.height == 0 {
        return None;
    }
    let fit_dimension = |current: u32, minimum: u32, default: u32, available: u32| {
        if current < minimum.min(available) {
            default.max(minimum).min(available)
        } else {
            current.min(available)
        }
    };
    let width = fit_dimension(window.width, minimum.width, default.width, work.width);
    let height = fit_dimension(window.height, minimum.height, default.height, work.height);
    let max_x = work.x.saturating_add((work.width - width) as i32);
    let max_y = work.y.saturating_add((work.height - height) as i32);
    let fitted = Rect {
        x: window.x.clamp(work.x, max_x),
        y: window.y.clamp(work.y, max_y),
        width,
        height,
    };
    (fitted != window).then_some(fitted)
}

/// 窗口状态恢复后、首次显示或从托盘唤回时调用。
pub fn fit_to_work_area<R: Runtime>(window: &WebviewWindow<R>) {
    // 不把最小化的临时尺寸当成损坏；最大化 / 全屏窗口由系统负责。
    if window.is_minimized().unwrap_or(false)
        || window.is_maximized().unwrap_or(false)
        || window.is_fullscreen().unwrap_or(false)
    {
        return;
    }
    // Wayland 下拿不到窗口位置，交给合成器
    let (Ok(position), Ok(size), Ok(inner)) = (
        window.outer_position(),
        window.outer_size(),
        window.inner_size(),
    ) else {
        return;
    };
    let Some(monitor) = window
        .current_monitor()
        .ok()
        .flatten()
        .or_else(|| window.primary_monitor().ok().flatten())
    else {
        return;
    };
    let work = monitor.work_area();
    let current = Rect {
        x: position.x,
        y: position.y,
        width: size.width,
        height: size.height,
    };
    let work = Rect {
        x: work.position.x,
        y: work.position.y,
        width: work.size.width,
        height: work.size.height,
    };

    let Some(config) = window
        .app_handle()
        .config()
        .app
        .windows
        .iter()
        .find(|config| config.label == window.label())
    else {
        return;
    };
    let scale = window.scale_factor().unwrap_or(monitor.scale_factor());
    let frame = PhysicalSize::new(
        size.width.saturating_sub(inner.width),
        size.height.saturating_sub(inner.height),
    );
    let minimum = outer_size(
        LogicalSize::new(
            config.min_width.unwrap_or(0.0),
            config.min_height.unwrap_or(0.0),
        ),
        scale,
        frame,
    );
    let default = outer_size(LogicalSize::new(config.width, config.height), scale, frame);
    let Some(fitted) = fit_into(current, work, minimum, default) else {
        return;
    };
    if (fitted.width, fitted.height) != (current.width, current.height) {
        // set_size 接收内容区尺寸；同时支持放大和缩小，避免无符号减法下溢。
        if let Err(error) = window.set_size(PhysicalSize::new(
            fitted.width.saturating_sub(frame.width).max(1),
            fitted.height.saturating_sub(frame.height).max(1),
        )) {
            log::warn!("修正主窗口尺寸失败: {error}");
            return;
        }
    }
    if let Err(error) = window.set_position(PhysicalPosition::new(fitted.x, fitted.y)) {
        log::warn!("修正主窗口位置失败: {error}");
        return;
    }
    log::info!("已请求修正主窗口: {current:?} -> {fitted:?} (工作区 {work:?}, 缩放 {scale})");
}

#[cfg(test)]
mod tests {
    use super::*;

    const fn rect(x: i32, y: i32, width: u32, height: u32) -> Rect {
        Rect {
            x,
            y,
            width,
            height,
        }
    }

    // 1920×1080 显示器，底部 48px 任务栏
    const WORK: Rect = rect(0, 0, 1920, 1032);
    const MINIMUM: PhysicalSize<u32> = PhysicalSize::new(1000, 640);
    const DEFAULT: PhysicalSize<u32> = PhysicalSize::new(1100, 650);

    #[test]
    fn corrupted_saved_strip_recovers_to_default_size() {
        // #9496: a nonzero strip inside the work area must not survive restoration.
        for (width, height) in [(180, 30), (0, 0), (1, 1)] {
            assert_eq!(
                fit_into(rect(100, 100, width, height), WORK, MINIMUM, DEFAULT),
                Some(rect(100, 100, 1100, 650))
            );
        }
    }

    #[test]
    fn window_inside_the_work_area_is_left_alone() {
        assert_eq!(
            fit_into(rect(100, 100, 1000, 650), WORK, MINIMUM, DEFAULT),
            None
        );
        assert_eq!(
            fit_into(rect(0, 0, 1920, 1032), WORK, MINIMUM, DEFAULT),
            None
        );
    }

    #[test]
    fn window_under_the_taskbar_moves_up() {
        // #9206：窗口底部压在任务栏下 100px
        assert_eq!(
            fit_into(rect(300, 482, 1000, 650), WORK, MINIMUM, DEFAULT),
            Some(rect(300, 382, 1000, 650))
        );
    }

    #[test]
    fn window_past_the_left_and_top_edges_moves_in() {
        assert_eq!(
            fit_into(rect(-200, -40, 1000, 650), WORK, MINIMUM, DEFAULT),
            Some(rect(0, 0, 1000, 650))
        );
    }

    #[test]
    fn window_larger_than_the_work_area_shrinks_to_it() {
        // 在更大的显示器上保存了尺寸，之后换成小屏
        assert_eq!(
            fit_into(rect(50, 50, 2400, 1400), WORK, MINIMUM, DEFAULT),
            Some(rect(0, 0, 1920, 1032))
        );
    }

    #[test]
    fn secondary_monitor_with_negative_coordinates() {
        // 主屏左侧的副屏，任务栏在左边 60px
        let work = rect(-1860, 0, 1860, 1080);
        assert_eq!(
            fit_into(rect(-1950, 200, 1000, 650), work, MINIMUM, DEFAULT),
            Some(rect(-1860, 200, 1000, 650))
        );
        assert_eq!(
            fit_into(rect(-1500, 200, 1000, 650), work, MINIMUM, DEFAULT),
            None
        );
    }

    #[test]
    fn only_the_corrupted_dimension_is_reset() {
        assert_eq!(
            fit_into(rect(100, 100, 1400, 30), WORK, MINIMUM, DEFAULT),
            Some(rect(100, 100, 1400, 650))
        );
        assert_eq!(
            fit_into(rect(100, 100, 30, 800), WORK, MINIMUM, DEFAULT),
            Some(rect(100, 100, 1100, 800))
        );
    }

    #[test]
    fn recovered_window_near_screen_edge_moves_back_into_work_area() {
        assert_eq!(
            fit_into(rect(1700, 950, 180, 30), WORK, MINIMUM, DEFAULT),
            Some(rect(820, 382, 1100, 650))
        );
    }

    #[test]
    fn logical_bounds_account_for_dpi_and_window_frame() {
        let frame = PhysicalSize::new(16, 39);
        let minimum = outer_size(LogicalSize::new(1000.0, 640.0), 1.5, frame);
        let default = outer_size(LogicalSize::new(1100.0, 650.0), 1.5, frame);
        assert_eq!(minimum, PhysicalSize::new(1516, 999));
        assert_eq!(default, PhysicalSize::new(1666, 1014));
        assert_eq!(
            fit_into(rect(100, 100, 180, 30), WORK, minimum, default),
            Some(rect(100, 18, 1666, 1014))
        );
        assert_eq!(
            fit_into(rect(100, 18, 1666, 1014), WORK, minimum, default),
            None
        );
    }

    #[test]
    fn work_area_smaller_than_minimum_is_stable_after_recovery() {
        let work = rect(0, 0, 800, 500);
        let recovered = fit_into(rect(100, 100, 180, 30), work, MINIMUM, DEFAULT).unwrap();
        assert_eq!(recovered, work);
        assert_eq!(fit_into(recovered, work, MINIMUM, DEFAULT), None);
    }

    #[test]
    fn unavailable_work_area_does_not_collapse_the_window() {
        for work in [rect(0, 0, 0, 1032), rect(0, 0, 1920, 0)] {
            assert_eq!(
                fit_into(rect(100, 100, 1100, 650), work, MINIMUM, DEFAULT),
                None
            );
        }
    }
}

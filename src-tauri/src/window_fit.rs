//! 启动时把主窗口收回显示器工作区内。
//!
//! window-state 插件恢复位置时只检查窗口是否和某个显示器的整块区域有交集，
//! 不看任务栏 / Dock 占掉的部分。窗口底部若曾被拖到任务栏下面，每次启动都会
//! 恢复到同一位置，底部按钮（例如更新弹窗的按钮）一直被挡住。

use tauri::{PhysicalPosition, PhysicalSize, Runtime, WebviewWindow};

/// 物理像素下的矩形。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct Rect {
    x: i32,
    y: i32,
    width: u32,
    height: u32,
}

/// 把 `window` 收进 `work`：比工作区大就缩小，再把位置夹到工作区内。
/// 已经完整位于工作区内时返回 `None`。
fn fit_into(window: Rect, work: Rect) -> Option<Rect> {
    let width = window.width.min(work.width);
    let height = window.height.min(work.height);
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

/// 在主窗口首次显示前调用（window-state 插件已在窗口创建时恢复了位置）。
pub fn fit_to_work_area<R: Runtime>(window: &WebviewWindow<R>) {
    // 最大化 / 全屏窗口由系统负责放进工作区
    if window.is_maximized().unwrap_or(false) || window.is_fullscreen().unwrap_or(false) {
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

    let Some(fitted) = fit_into(current, work) else {
        return;
    };
    log::info!("主窗口超出显示器工作区，已收回: {current:?} -> {fitted:?} (工作区 {work:?})");
    if (fitted.width, fitted.height) != (current.width, current.height) {
        // set_size 设置的是内容区尺寸，按外框缩小的量扣掉
        let _ = window.set_size(PhysicalSize::new(
            inner.width.saturating_sub(current.width - fitted.width),
            inner.height.saturating_sub(current.height - fitted.height),
        ));
    }
    let _ = window.set_position(PhysicalPosition::new(fitted.x, fitted.y));
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

    #[test]
    fn window_inside_the_work_area_is_left_alone() {
        assert_eq!(fit_into(rect(100, 100, 1000, 650), WORK), None);
        assert_eq!(fit_into(rect(0, 0, 1920, 1032), WORK), None);
    }

    #[test]
    fn window_under_the_taskbar_moves_up() {
        // #9206：窗口底部压在任务栏下 100px
        assert_eq!(
            fit_into(rect(300, 482, 1000, 650), WORK),
            Some(rect(300, 382, 1000, 650))
        );
    }

    #[test]
    fn window_past_the_left_and_top_edges_moves_in() {
        assert_eq!(
            fit_into(rect(-200, -40, 1000, 650), WORK),
            Some(rect(0, 0, 1000, 650))
        );
    }

    #[test]
    fn window_larger_than_the_work_area_shrinks_to_it() {
        // 在更大的显示器上保存了尺寸，之后换成小屏
        assert_eq!(
            fit_into(rect(50, 50, 2400, 1400), WORK),
            Some(rect(0, 0, 1920, 1032))
        );
    }

    #[test]
    fn secondary_monitor_with_negative_coordinates() {
        // 主屏左侧的副屏，任务栏在左边 60px
        let work = rect(-1860, 0, 1860, 1080);
        assert_eq!(
            fit_into(rect(-1950, 200, 1000, 650), work),
            Some(rect(-1860, 200, 1000, 650))
        );
        assert_eq!(fit_into(rect(-1500, 200, 1000, 650), work), None);
    }
}

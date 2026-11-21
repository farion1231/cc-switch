//! 恢复主窗口的可用尺寸，并收回显示器工作区内。
//!
//! window-state 插件恢复位置时只检查窗口是否和某个显示器的整块区域有交集，
//! 不看任务栏 / Dock 占掉的部分。窗口底部若曾被拖到任务栏下面，每次启动都会
//! 恢复到同一位置，底部按钮（例如更新弹窗的按钮）一直被挡住。
//! 插件也会原样恢复异常小的非零尺寸（#9496），不能只依赖系统的最小尺寸限制。

use tauri::utils::config::WindowConfig;
use tauri::{LogicalSize, Manager, PhysicalPosition, PhysicalSize, Runtime, WebviewWindow};

/// 物理像素下的矩形。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct Rect {
    x: i32,
    y: i32,
    width: u32,
    height: u32,
}

/// Wayland 等环境可读取尺寸，但窗口位置由合成器管理。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct WindowGeometry {
    position: Option<PhysicalPosition<i32>>,
    size: PhysicalSize<u32>,
}

/// 将默认内容区尺寸转换为物理外框尺寸。
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

/// 判定下限不加外框，恢复目标仍按默认内容区尺寸加外框。
fn size_bounds(
    config: &WindowConfig,
    scale: f64,
    frame: PhysicalSize<u32>,
) -> (PhysicalSize<u32>, PhysicalSize<u32>) {
    // Windows 无边框窗口的系统最小尺寸约束整窗，阴影 inset 不能再加到阈值上。
    // macOS / Linux 的最小内容区外还可能有边框，用这个保守下限也不会误判。
    let minimum = LogicalSize::new(
        config.min_width.unwrap_or(0.0),
        config.min_height.unwrap_or(0.0),
    )
    .to_physical(scale);
    let default = outer_size(LogicalSize::new(config.width, config.height), scale, frame);
    (minimum, default)
}

/// 过小的维度恢复默认值，过大的缩到工作区，再把位置夹到工作区内。
/// 正常尺寸保持不变；小屏幕以工作区为限，避免每次显示都重复修正。
fn fit_into(
    window: WindowGeometry,
    work: Rect,
    minimum: PhysicalSize<u32>,
    default: PhysicalSize<u32>,
) -> Option<WindowGeometry> {
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
    let width = fit_dimension(window.size.width, minimum.width, default.width, work.width);
    let height = fit_dimension(
        window.size.height,
        minimum.height,
        default.height,
        work.height,
    );
    let max_x = work.x.saturating_add((work.width - width) as i32);
    let max_y = work.y.saturating_add((work.height - height) as i32);
    let fitted = WindowGeometry {
        position: window.position.map(|position| {
            PhysicalPosition::new(
                position.x.clamp(work.x, max_x),
                position.y.clamp(work.y, max_y),
            )
        }),
        size: PhysicalSize::new(width, height),
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
    let (Ok(size), Ok(inner)) = (window.outer_size(), window.inner_size()) else {
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
    let current = WindowGeometry {
        // 位置不可用只跳过位置修正，仍恢复异常尺寸。
        position: window.outer_position().ok(),
        size,
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
    let (minimum, default) = size_bounds(config, scale, frame);
    let Some(fitted) = fit_into(current, work, minimum, default) else {
        return;
    };
    if fitted.size != current.size {
        // set_size 接收内容区尺寸；同时支持放大和缩小，避免无符号减法下溢。
        if let Err(error) = window.set_size(PhysicalSize::new(
            fitted.size.width.saturating_sub(frame.width).max(1),
            fitted.size.height.saturating_sub(frame.height).max(1),
        )) {
            log::warn!("修正主窗口尺寸失败: {error}");
            return;
        }
    }
    if let Some(position) = fitted.position {
        if let Err(error) = window.set_position(position) {
            log::warn!("修正主窗口位置失败: {error}");
            return;
        }
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

    const fn geometry(x: i32, y: i32, width: u32, height: u32) -> WindowGeometry {
        WindowGeometry {
            position: Some(PhysicalPosition::new(x, y)),
            size: PhysicalSize::new(width, height),
        }
    }

    // 1920×1080 显示器，底部 48px 任务栏
    const WORK: Rect = rect(0, 0, 1920, 1032);
    const MINIMUM: PhysicalSize<u32> = PhysicalSize::new(1000, 640);
    const DEFAULT: PhysicalSize<u32> = PhysicalSize::new(1100, 650);

    fn windows_size_bounds(
        scale: f64,
        frame: PhysicalSize<u32>,
    ) -> (PhysicalSize<u32>, PhysicalSize<u32>) {
        let config: serde_json::Value =
            serde_json::from_str(include_str!("../tauri.windows.conf.json")).unwrap();
        let window = serde_json::from_value(config["app"]["windows"][0].clone()).unwrap();
        size_bounds(&window, scale, frame)
    }

    #[test]
    fn system_minimum_window_with_shadow_insets_is_left_alone() {
        let work = rect(0, 0, 3840, 2160);
        for (scale, frame_width, frame_height) in
            [(1.0, 16, 9), (1.25, 20, 11), (1.5, 24, 14), (2.0, 32, 18)]
        {
            let frame = PhysicalSize::new(frame_width, frame_height);
            let (minimum, default) = windows_size_bounds(scale, frame);
            // Windows 的无边框最小整窗不包含 WM_NCCALCSIZE 扣掉的阴影 inset。
            let system_minimum = LogicalSize::new(1000.0, 640.0).to_physical::<u32>(scale);
            for size in [
                system_minimum,
                PhysicalSize::new(system_minimum.width, default.height),
                PhysicalSize::new(default.width, system_minimum.height),
                PhysicalSize::new(
                    system_minimum.width + frame.width,
                    system_minimum.height + frame.height,
                ),
            ] {
                for position in [Some(PhysicalPosition::new(100, 100)), None] {
                    assert_eq!(
                        fit_into(WindowGeometry { position, size }, work, minimum, default),
                        None,
                        "normal minimum window changed at scale {scale}, size {size:?}",
                    );
                }
            }
        }
    }

    #[test]
    fn undersized_dimensions_with_shadow_insets_still_recover() {
        let (minimum, default) = windows_size_bounds(1.25, PhysicalSize::new(20, 11));
        assert_eq!(default, PhysicalSize::new(1395, 824));
        for (width, height, expected_width, expected_height) in [
            (180, 30, 1395, 824),
            (0, 0, 1395, 824),
            (1249, 900, 1395, 900),
            (1500, 799, 1500, 824),
        ] {
            for position in [Some(PhysicalPosition::new(100, 100)), None] {
                assert_eq!(
                    fit_into(
                        WindowGeometry {
                            position,
                            size: PhysicalSize::new(width, height),
                        },
                        WORK,
                        minimum,
                        default,
                    ),
                    Some(WindowGeometry {
                        position,
                        size: PhysicalSize::new(expected_width, expected_height),
                    }),
                );
            }
        }
    }

    #[test]
    fn corrupted_saved_strip_recovers_to_default_size() {
        // #9496: a nonzero strip inside the work area must not survive restoration.
        for (width, height) in [(180, 30), (0, 0), (1, 1)] {
            assert_eq!(
                fit_into(geometry(100, 100, width, height), WORK, MINIMUM, DEFAULT),
                Some(geometry(100, 100, 1100, 650))
            );
        }
    }

    #[test]
    fn unavailable_position_does_not_block_size_recovery() {
        for (width, height, expected_width, expected_height) in [
            (180, 30, 1100, 650),
            (0, 0, 1100, 650),
            (1400, 30, 1400, 650),
            (30, 800, 1100, 800),
            (2400, 1400, 1920, 1032),
        ] {
            let current = WindowGeometry {
                position: None,
                size: PhysicalSize::new(width, height),
            };
            assert_eq!(
                fit_into(current, WORK, MINIMUM, DEFAULT),
                Some(WindowGeometry {
                    position: None,
                    size: PhysicalSize::new(expected_width, expected_height),
                }),
            );
        }
    }

    #[test]
    fn normal_size_without_position_is_left_alone() {
        let current = WindowGeometry {
            position: None,
            size: PhysicalSize::new(1400, 800),
        };
        assert_eq!(fit_into(current, WORK, MINIMUM, DEFAULT), None);
    }

    #[test]
    fn small_work_area_without_position_is_stable_after_recovery() {
        let work = rect(-800, 100, 800, 500);
        let current = WindowGeometry {
            position: None,
            size: PhysicalSize::new(180, 30),
        };
        let recovered = fit_into(current, work, MINIMUM, DEFAULT).unwrap();
        assert_eq!(
            recovered,
            WindowGeometry {
                position: None,
                size: PhysicalSize::new(800, 500),
            },
        );
        assert_eq!(fit_into(recovered, work, MINIMUM, DEFAULT), None);
    }

    #[test]
    fn unavailable_work_area_without_position_does_not_collapse_the_window() {
        let current = WindowGeometry {
            position: None,
            size: PhysicalSize::new(180, 30),
        };
        for work in [rect(0, 0, 0, 1032), rect(0, 0, 1920, 0)] {
            assert_eq!(fit_into(current, work, MINIMUM, DEFAULT), None);
        }
    }

    #[test]
    fn window_inside_the_work_area_is_left_alone() {
        assert_eq!(
            fit_into(geometry(100, 100, 1000, 650), WORK, MINIMUM, DEFAULT),
            None
        );
        assert_eq!(
            fit_into(geometry(0, 0, 1920, 1032), WORK, MINIMUM, DEFAULT),
            None
        );
    }

    #[test]
    fn window_under_the_taskbar_moves_up() {
        // #9206：窗口底部压在任务栏下 100px
        assert_eq!(
            fit_into(geometry(300, 482, 1000, 650), WORK, MINIMUM, DEFAULT),
            Some(geometry(300, 382, 1000, 650))
        );
    }

    #[test]
    fn window_past_the_left_and_top_edges_moves_in() {
        assert_eq!(
            fit_into(geometry(-200, -40, 1000, 650), WORK, MINIMUM, DEFAULT),
            Some(geometry(0, 0, 1000, 650))
        );
    }

    #[test]
    fn window_larger_than_the_work_area_shrinks_to_it() {
        // 在更大的显示器上保存了尺寸，之后换成小屏
        assert_eq!(
            fit_into(geometry(50, 50, 2400, 1400), WORK, MINIMUM, DEFAULT),
            Some(geometry(0, 0, 1920, 1032))
        );
    }

    #[test]
    fn secondary_monitor_with_negative_coordinates() {
        // 主屏左侧的副屏，任务栏在左边 60px
        let work = rect(-1860, 0, 1860, 1080);
        assert_eq!(
            fit_into(geometry(-1950, 200, 1000, 650), work, MINIMUM, DEFAULT),
            Some(geometry(-1860, 200, 1000, 650))
        );
        assert_eq!(
            fit_into(geometry(-1500, 200, 1000, 650), work, MINIMUM, DEFAULT),
            None
        );
    }

    #[test]
    fn only_the_corrupted_dimension_is_reset() {
        assert_eq!(
            fit_into(geometry(100, 100, 1400, 30), WORK, MINIMUM, DEFAULT),
            Some(geometry(100, 100, 1400, 650))
        );
        assert_eq!(
            fit_into(geometry(100, 100, 30, 800), WORK, MINIMUM, DEFAULT),
            Some(geometry(100, 100, 1100, 800))
        );
    }

    #[test]
    fn recovered_window_near_screen_edge_moves_back_into_work_area() {
        assert_eq!(
            fit_into(geometry(1700, 950, 180, 30), WORK, MINIMUM, DEFAULT),
            Some(geometry(820, 382, 1100, 650))
        );
    }

    #[test]
    fn logical_bounds_account_for_dpi_and_window_frame() {
        let frame = PhysicalSize::new(16, 39);
        let (minimum, default) = windows_size_bounds(1.5, frame);
        assert_eq!(minimum, PhysicalSize::new(1500, 960));
        assert_eq!(default, PhysicalSize::new(1666, 1014));
        assert_eq!(
            fit_into(geometry(100, 100, 180, 30), WORK, minimum, default),
            Some(geometry(100, 18, 1666, 1014))
        );
        assert_eq!(
            fit_into(geometry(100, 18, 1666, 1014), WORK, minimum, default),
            None
        );
    }

    #[test]
    fn work_area_smaller_than_minimum_is_stable_after_recovery() {
        let work = rect(0, 0, 800, 500);
        let recovered = fit_into(geometry(100, 100, 180, 30), work, MINIMUM, DEFAULT).unwrap();
        assert_eq!(recovered, geometry(0, 0, 800, 500));
        assert_eq!(fit_into(recovered, work, MINIMUM, DEFAULT), None);
    }

    #[test]
    fn unavailable_work_area_does_not_collapse_the_window() {
        for work in [rect(0, 0, 0, 1032), rect(0, 0, 1920, 0)] {
            assert_eq!(
                fit_into(geometry(100, 100, 1100, 650), work, MINIMUM, DEFAULT),
                None
            );
        }
    }
}

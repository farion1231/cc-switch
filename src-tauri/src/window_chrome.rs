//! 窗口标题栏按钮布局。
//!
//! Linux 桌面允许把关闭 / 最小化 / 最大化放到标题栏左侧或右侧，并指定
//! 顺序（GNOME 的 button-layout、KDE 的 ButtonsOnLeft / ButtonsOnRight 最终
//! 都写进 GTK 的 `gtk-decoration-layout`）。Tao 0.35 的 Wayland CSD 头把
//! 布局硬编码成 `menu:minimize,maximize,close`，应用内自绘按钮也固定在最右
//! 端，两者都不跟随系统设置。
//!
//! 这里读取 GTK 的真实设置，同时供原生标题栏修补和前端自绘按钮使用。

#![cfg_attr(not(target_os = "linux"), allow(dead_code))]

/// 前端可渲染的三个窗口按钮。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WindowControl {
    Minimize,
    Maximize,
    Close,
}

impl WindowControl {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Minimize => "minimize",
            Self::Maximize => "maximize",
            Self::Close => "close",
        }
    }

    fn from_name(name: &str) -> Option<Self> {
        match name.trim() {
            "minimize" => Some(Self::Minimize),
            "maximize" => Some(Self::Maximize),
            "close" => Some(Self::Close),
            _ => None,
        }
    }
}

/// 一个窗口按钮在三种交互状态下的 PNG（data URL）与尺寸。
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct WindowControlImages {
    pub normal: String,
    pub hover: String,
    pub pressed: String,
    pub width: u32,
    pub height: u32,
}

/// 系统主题渲染出的窗口按钮图片。缺失的档由前端回退到内置图标。
#[derive(Debug, Clone, Default, PartialEq, Eq, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct WindowControlImagesMap {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub close: Option<WindowControlImages>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub minimize: Option<WindowControlImages>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub maximize: Option<WindowControlImages>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub restore: Option<WindowControlImages>,
}

/// 前端渲染窗口按钮所需的布局。
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct WindowControlsLayout {
    /// `"left"` 或 `"right"`。
    pub side: &'static str,
    /// 从左到右的按钮顺序，只包含 minimize / maximize / close。
    pub order: Vec<&'static str>,
    /// 系统主题按钮图片；取不到时前端用内置图标。
    #[serde(skip_serializing_if = "Option::is_none")]
    pub images: Option<WindowControlImagesMap>,
}

impl WindowControlsLayout {
    pub fn default_right() -> Self {
        Self {
            side: "right",
            order: vec!["minimize", "maximize", "close"],
            images: None,
        }
    }

    /// 交给 GTK HeaderBar 的布局字符串（保留 menu / icon 等占位）。
    pub fn gtk_layout(raw: &str) -> String {
        let raw = raw.trim();
        if raw.is_empty() {
            "menu:minimize,maximize,close".to_string()
        } else {
            raw.to_string()
        }
    }
}

/// 读取当前系统的窗口按钮布局。
///
/// 必须在 GTK 主线程调用；不依赖 GTK 的平台直接返回 Windows 式右侧布局。
#[cfg(target_os = "linux")]
pub fn current_layout(use_gtk_style: bool) -> WindowControlsLayout {
    use gtk::prelude::GtkSettingsExt;

    let raw = gtk::Settings::default()
        .and_then(|settings| settings.gtk_decoration_layout())
        .map(|layout| layout.to_string())
        .unwrap_or_default();
    let mut layout = parse_gtk_decoration_layout(&raw);
    if use_gtk_style {
        layout.images = render_native_button_images();
    }
    layout
}

#[cfg(not(target_os = "linux"))]
pub fn current_layout(_use_gtk_style: bool) -> WindowControlsLayout {
    WindowControlsLayout::default_right()
}

/// 原始 GTK 布局字符串，用于修补原生标题栏。
#[cfg(target_os = "linux")]
pub fn current_gtk_layout() -> String {
    use gtk::prelude::GtkSettingsExt;

    let raw = gtk::Settings::default()
        .and_then(|settings| settings.gtk_decoration_layout())
        .map(|layout| layout.to_string())
        .unwrap_or_default();
    WindowControlsLayout::gtk_layout(&raw)
}

/// 解析 `left:right` 形式的 `gtk-decoration-layout`。
///
/// GTK 惯例：没有冒号时整串都属于右侧；两侧都有按钮时选按钮更多的一侧，
/// 数量相同则优先有 close 的一侧。某一侧缺按钮时用另一侧补齐，保证自绘
/// 标题栏始终有完整的最小化 / 最大化 / 关闭。
pub(crate) fn parse_gtk_decoration_layout(raw: &str) -> WindowControlsLayout {
    let raw = raw.trim();
    if raw.is_empty() {
        return WindowControlsLayout::default_right();
    }

    let (left_raw, right_raw) = match raw.split_once(':') {
        Some((left, right)) => (left, right),
        None => ("", raw),
    };
    let left = parse_side(left_raw);
    let right = parse_side(right_raw);

    if left.is_empty() && right.is_empty() {
        return WindowControlsLayout::default_right();
    }

    let left_has_close = left.contains(&WindowControl::Close);
    let use_left = left.len() > right.len() || (left.len() == right.len() && left_has_close);

    let mut primary = if use_left { left } else { right };
    // 自绘按钮必须始终提供三个操作；系统只声明了一部分时按默认顺序补齐。
    for control in [
        WindowControl::Minimize,
        WindowControl::Maximize,
        WindowControl::Close,
    ] {
        if !primary.contains(&control) {
            primary.push(control);
        }
    }

    WindowControlsLayout {
        side: if use_left { "left" } else { "right" },
        order: primary.into_iter().map(WindowControl::as_str).collect(),
        images: None,
    }
}

/// 用当前 GTK 主题离屏渲染标题栏按钮，导出三种交互状态的 PNG。
///
/// 这是“跟随系统按钮样式”的核心：不猜主题格式，也不解析 KWin 插件；
/// 直接让 GTK 在离屏 HeaderBar 里按当前主题的 CSS 画一遍按钮。KDE 的
/// macos27-csd、GNOME/Adwaita、Breeze-GTK 等只要改了 GTK 按钮样式，
/// 这里拿到的就是用户实际看到的颜色、圆角和悬停符号。
#[cfg(target_os = "linux")]
fn render_native_button_images() -> Option<WindowControlImagesMap> {
    let normal = render_button_set(false);
    let restore = render_button_set(true).and_then(|mut images| images.maximize.take());
    let mut images = normal?;
    images.restore = restore;
    let count = [
        images.close.is_some(),
        images.minimize.is_some(),
        images.maximize.is_some(),
        images.restore.is_some(),
    ]
    .into_iter()
    .filter(|present| *present)
    .count();
    log::info!("Linux: 已导出 {count} 个系统窗口按钮样式");
    Some(images)
}

#[cfg(target_os = "linux")]
fn render_button_set(maximized: bool) -> Option<WindowControlImagesMap> {
    use gtk::prelude::*;

    let window = gtk::OffscreenWindow::new();
    window.set_default_size(400, 100);
    {
        let context = window.style_context();
        context.add_class("background");
        context.add_class("csd");
        if maximized {
            context.add_class("maximized");
        }
    }

    let header = gtk::HeaderBar::new();
    header.set_show_close_button(false);
    header.set_title(Some("CC Switch"));
    {
        let context = header.style_context();
        context.add_class("titlebar");
        context.add_class("header-bar");
    }
    window.set_titlebar(Some(&header));

    let specs: &[(&str, &str)] = if maximized {
        &[("maximize", "window-restore-symbolic")]
    } else {
        &[
            ("close", "window-close-symbolic"),
            ("minimize", "window-minimize-symbolic"),
            ("maximize", "window-maximize-symbolic"),
        ]
    };

    let mut buttons = Vec::with_capacity(specs.len());
    for (kind, icon_name) in specs {
        let button = gtk::Button::new();
        {
            let context = button.style_context();
            context.add_class("titlebutton");
            context.add_class(kind);
        }
        button.set_relief(gtk::ReliefStyle::None);
        let image = gtk::Image::from_icon_name(Some(icon_name), gtk::IconSize::Button);
        button.set_image(Some(&image));
        header.pack_start(&button);
        buttons.push((*kind, button));
    }

    window.show_all();
    flush_gtk_events();

    header.size_allocate(&gtk::Allocation::new(0, 0, 400, 48));
    flush_gtk_events();

    // HeaderBar 给自定义按钮只分 16px 宽，会把 22px 的圆点裁掉。这里按
    // 主题画出来的实际按钮宽度留出余量，再单独分配；渲染后不再跑主循环，
    // 避免 HeaderBar 把分配改回去。
    let mut x = 0;
    let mut sizes = Vec::with_capacity(buttons.len());
    for (_, button) in &buttons {
        let (minimum, natural) = button.preferred_size();
        let width = (natural.width.max(minimum.width) + 8).clamp(24, 34);
        let height = (natural.height.max(minimum.height) + 4).clamp(24, 34);
        button.size_allocate(&gtk::Allocation::new(x, 0, width, height));
        sizes.push((width as u32, height as u32));
        x += width;
    }

    let mut images = WindowControlImagesMap::default();
    for (index, (kind, button)) in buttons.iter().enumerate() {
        let (width, height) = sizes[index];
        // 主题没给这个按钮画样式时会得到全透明图：只剩透明点击区，不如回退到内置图标。
        let Some(images_for_kind) = render_button_states(button, width, height) else {
            log::info!("Linux: 系统主题未提供 {kind} 按钮样式，使用内置图标");
            continue;
        };
        match *kind {
            "close" => images.close = Some(images_for_kind),
            "minimize" => images.minimize = Some(images_for_kind),
            "maximize" => images.maximize = Some(images_for_kind),
            _ => {}
        }
    }

    let any = images.close.is_some() || images.minimize.is_some() || images.maximize.is_some();
    // GTK 持有顶层窗口的引用；仅释放 Rust 句柄不会销毁离屏窗口。
    window.close();
    any.then_some(images)
}

#[cfg(target_os = "linux")]
fn render_button_states(
    button: &gtk::Button,
    width: u32,
    height: u32,
) -> Option<WindowControlImages> {
    let normal = render_button_png(button, gtk::StateFlags::empty(), width, height)?;
    let hover = render_button_png(button, gtk::StateFlags::PRELIGHT, width, height)?;
    let pressed = render_button_png(
        button,
        gtk::StateFlags::PRELIGHT | gtk::StateFlags::ACTIVE,
        width,
        height,
    )?;
    Some(WindowControlImages {
        normal,
        hover,
        pressed,
        width,
        height,
    })
}

/// 渲染单个按钮状态为 PNG data URL；画面完全透明时返回 None。
#[cfg(target_os = "linux")]
fn render_button_png(
    button: &gtk::Button,
    flags: gtk::StateFlags,
    width: u32,
    height: u32,
) -> Option<String> {
    use base64::Engine as _;
    use gtk::prelude::*;

    button.unset_state_flags(
        gtk::StateFlags::PRELIGHT | gtk::StateFlags::ACTIVE | gtk::StateFlags::BACKDROP,
    );
    if !flags.is_empty() {
        button.set_state_flags(flags, false);
    }
    button.queue_draw();

    let mut surface =
        gtk::cairo::ImageSurface::create(gtk::cairo::Format::ARgb32, width as i32, height as i32)
            .ok()?;
    {
        let context = gtk::cairo::Context::new(&surface).ok()?;
        // Widget::draw 用的是控件自身坐标，从 (0,0) 开始画；这里不能再按
        // button.allocation().x() 平移，否则第二个、第三个按钮会被推出画布。
        button.draw(&context);
    }
    surface.flush();

    let visible = {
        let data = surface.data().ok()?;
        has_visible_pixels(&data)
    };
    if !visible {
        return None;
    }

    let mut png = Vec::new();
    surface.write_to_png(&mut png).ok()?;
    Some(format!(
        "data:image/png;base64,{}",
        base64::engine::general_purpose::STANDARD.encode(png)
    ))
}

/// ARGB32 像素缓冲里是否存在非透明像素。
#[cfg(target_os = "linux")]
fn has_visible_pixels(argb32: &[u8]) -> bool {
    argb32
        .chunks_exact(4)
        .any(|pixel| u32::from_ne_bytes([pixel[0], pixel[1], pixel[2], pixel[3]]) >> 24 != 0)
}

/// 处理离屏控件 show / allocate 需要的主循环事件。限制轮数，避免主题里
/// 持续动画时把命令卡住。
#[cfg(target_os = "linux")]
fn flush_gtk_events() {
    for _ in 0..100 {
        if !gtk::events_pending() {
            break;
        }
        gtk::main_iteration_do(false);
    }
}

fn parse_side(raw: &str) -> Vec<WindowControl> {
    let mut result = Vec::new();
    for name in raw.split(',') {
        let Some(control) = WindowControl::from_name(name) else {
            continue;
        };
        if !result.contains(&control) {
            result.push(control);
        }
    }
    result
}

#[cfg(test)]
mod tests {
    use super::*;

    #[cfg(target_os = "linux")]
    #[test]
    fn transparent_pixels_fall_back_but_visible_black_does_not() {
        assert!(!has_visible_pixels(&[]));
        assert!(!has_visible_pixels(&0x00ffffff_u32.to_ne_bytes()));
        assert!(has_visible_pixels(&0x01000000_u32.to_ne_bytes()));
        assert!(has_visible_pixels(&0xff000000_u32.to_ne_bytes()));
    }

    #[test]
    fn gtk_style_preference_survives_settings_roundtrip() {
        let settings: crate::settings::AppSettings =
            serde_json::from_value(serde_json::json!({"useAppWindowControls": true})).unwrap();
        assert!(settings.use_gtk_window_controls);
        let settings: crate::settings::AppSettings = serde_json::from_value(
            serde_json::json!({"useAppWindowControls": true, "useGtkWindowControls": false}),
        )
        .unwrap();
        assert!(settings.use_app_window_controls);
        assert!(!settings.use_gtk_window_controls);
        let saved = serde_json::to_value(settings).unwrap();
        assert_eq!(saved["useGtkWindowControls"], false);
    }

    #[test]
    fn kde_left_layout_is_followed() {
        let layout = parse_gtk_decoration_layout("close,minimize,maximize:icon");
        assert_eq!(layout.side, "left");
        assert_eq!(layout.order, vec!["close", "minimize", "maximize"]);
    }

    #[test]
    fn gnome_right_layout_is_followed() {
        let layout = parse_gtk_decoration_layout("appmenu:minimize,maximize,close");
        assert_eq!(layout.side, "right");
        assert_eq!(layout.order, vec!["minimize", "maximize", "close"]);
    }

    #[test]
    fn missing_colon_means_right() {
        let layout = parse_gtk_decoration_layout("minimize,maximize,close");
        assert_eq!(layout.side, "right");
        assert_eq!(layout.order, vec!["minimize", "maximize", "close"]);
    }

    #[test]
    fn split_layout_keeps_all_three_buttons() {
        let layout = parse_gtk_decoration_layout("close:minimize,maximize");
        assert_eq!(layout.side, "right");
        assert_eq!(layout.order, vec!["minimize", "maximize", "close"]);
    }

    #[test]
    fn empty_layout_falls_back_to_right() {
        let layout = parse_gtk_decoration_layout("");
        assert_eq!(layout, WindowControlsLayout::default_right());
    }

    #[test]
    fn menu_and_icon_placeholders_are_ignored() {
        let layout = parse_gtk_decoration_layout("icon,menu:close");
        assert_eq!(layout.side, "right");
        assert_eq!(layout.order, vec!["close", "minimize", "maximize"]);
    }
}

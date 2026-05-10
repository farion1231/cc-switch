#[cfg(target_os = "macos")]
use crate::app_config::AppType;
#[cfg(target_os = "macos")]
use crate::error::AppError;
#[cfg(target_os = "macos")]
use crate::provider::Provider;
#[cfg(target_os = "macos")]
use crate::settings::VisibleApps;
#[cfg(target_os = "macos")]
use crate::store::AppState;
#[cfg(target_os = "macos")]
use tauri::image::Image;
#[cfg(target_os = "macos")]
use tauri::Manager;

#[cfg(target_os = "macos")]
const DEFAULT_ICON_BYTES: &[u8] = include_bytes!("../icons/tray/macos/statusbar_template_3x.png");

#[cfg(target_os = "macos")]
const CANVAS_HEIGHT: u32 = 54;
#[cfg(target_os = "macos")]
const SLOT_WIDTH: u32 = 52;
#[cfg(target_os = "macos")]
const SIDE_PADDING: u32 = 4;
#[cfg(target_os = "macos")]
const PROVIDER_SIZE: u32 = 42;
#[cfg(target_os = "macos")]
const BADGE_SIZE: u32 = 22;
#[cfg(target_os = "macos")]
const BADGE_ICON_SIZE: u32 = 16;
#[cfg(target_os = "macos")]
const PROVIDER_OFFSET_X: i32 = 1;
#[cfg(target_os = "macos")]
const PROVIDER_OFFSET_Y: i32 = 4;
#[cfg(target_os = "macos")]
const BADGE_OFFSET_X: i32 = 29;
#[cfg(target_os = "macos")]
const BADGE_OFFSET_Y: i32 = 29;

#[cfg(target_os = "macos")]
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct TrayIconSlot {
    pub app_type: AppType,
    pub provider_id: String,
    pub provider_name: String,
    pub provider_icon: Option<String>,
    pub provider_icon_color: Option<String>,
}

#[cfg(target_os = "macos")]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct Color {
    r: u8,
    g: u8,
    b: u8,
    a: u8,
}

#[cfg(target_os = "macos")]
impl Color {
    const WHITE: Self = Self {
        r: 255,
        g: 255,
        b: 255,
        a: 255,
    };
    const DARK: Self = Self {
        r: 31,
        g: 41,
        b: 55,
        a: 255,
    };
    const BADGE_FILL: Self = Self {
        r: 255,
        g: 255,
        b: 255,
        a: 248,
    };
    const BADGE_RING: Self = Self {
        r: 17,
        g: 24,
        b: 39,
        a: 36,
    };
    const PROVIDER_PLATE_FILL: Self = Self {
        r: 255,
        g: 255,
        b: 255,
        a: 238,
    };
    const PROVIDER_PLATE_RING: Self = Self {
        r: 17,
        g: 24,
        b: 39,
        a: 28,
    };
}

#[cfg(target_os = "macos")]
struct Canvas {
    width: u32,
    height: u32,
    rgba: Vec<u8>,
}

#[cfg(target_os = "macos")]
impl Canvas {
    fn new(width: u32, height: u32) -> Self {
        Self {
            width,
            height,
            rgba: vec![0; (width * height * 4) as usize],
        }
    }

    fn set_pixel(&mut self, x: i32, y: i32, color: Color) {
        if x < 0 || y < 0 || x >= self.width as i32 || y >= self.height as i32 || color.a == 0 {
            return;
        }

        let idx = ((y as u32 * self.width + x as u32) * 4) as usize;
        let src_a = color.a as f32 / 255.0;
        let dst_a = self.rgba[idx + 3] as f32 / 255.0;
        let out_a = src_a + dst_a * (1.0 - src_a);
        if out_a <= f32::EPSILON {
            return;
        }

        let blend = |src: u8, dst: u8| {
            ((src as f32 * src_a + dst as f32 * dst_a * (1.0 - src_a)) / out_a).round() as u8
        };
        self.rgba[idx] = blend(color.r, self.rgba[idx]);
        self.rgba[idx + 1] = blend(color.g, self.rgba[idx + 1]);
        self.rgba[idx + 2] = blend(color.b, self.rgba[idx + 2]);
        self.rgba[idx + 3] = (out_a * 255.0).round() as u8;
    }

    fn fill_rect(&mut self, x: i32, y: i32, w: u32, h: u32, color: Color) {
        for py in y..y + h as i32 {
            for px in x..x + w as i32 {
                self.set_pixel(px, py, color);
            }
        }
    }

    fn fill_circle(&mut self, cx: i32, cy: i32, radius: i32, color: Color) {
        let r2 = radius * radius;
        for y in cy - radius..=cy + radius {
            for x in cx - radius..=cx + radius {
                let dx = x - cx;
                let dy = y - cy;
                if dx * dx + dy * dy <= r2 {
                    self.set_pixel(x, y, color);
                }
            }
        }
    }

    fn fill_rounded_rect(&mut self, x: i32, y: i32, w: u32, h: u32, radius: i32, color: Color) {
        let right = x + w as i32 - 1;
        let bottom = y + h as i32 - 1;
        self.fill_rect(
            x + radius,
            y,
            w.saturating_sub((radius * 2) as u32),
            h,
            color,
        );
        self.fill_rect(
            x,
            y + radius,
            w,
            h.saturating_sub((radius * 2) as u32),
            color,
        );
        self.fill_circle(x + radius, y + radius, radius, color);
        self.fill_circle(right - radius, y + radius, radius, color);
        self.fill_circle(x + radius, bottom - radius, radius, color);
        self.fill_circle(right - radius, bottom - radius, radius, color);
    }
}

#[cfg(target_os = "macos")]
pub fn default_macos_tray_icon() -> Option<Image<'static>> {
    match Image::from_bytes(DEFAULT_ICON_BYTES) {
        Ok(icon) => Some(icon),
        Err(err) => {
            log::warn!("Failed to load macOS tray icon: {err}");
            None
        }
    }
}

#[cfg(not(target_os = "macos"))]
pub fn refresh_tray_icon(_app: &tauri::AppHandle) {}

#[cfg(target_os = "macos")]
pub fn refresh_tray_icon(app: &tauri::AppHandle) {
    if let Err(err) = refresh_tray_icon_inner(app) {
        log::warn!("[TrayIcon] 刷新动态图标失败: {err}");
        restore_default_tray_icon(app);
    }
}

#[cfg(target_os = "macos")]
fn refresh_tray_icon_inner(app: &tauri::AppHandle) -> Result<(), AppError> {
    let settings = crate::settings::get_settings();
    if !settings.dynamic_tray_icon_enabled {
        restore_default_tray_icon(app);
        return Ok(());
    }

    let Some(state) = app.try_state::<AppState>() else {
        return Ok(());
    };
    let visible_apps = settings.visible_apps.unwrap_or_default();
    let slots = build_slots(state.inner(), &visible_apps)?;
    if slots.is_empty() {
        restore_default_tray_icon(app);
        return Ok(());
    }

    let image = render_slots(&slots);
    if let Some(tray) = app.tray_by_id(crate::tray::TRAY_ID) {
        tray.set_icon(Some(image))
            .map_err(|e| AppError::Message(format!("设置动态图标失败: {e}")))?;
        tray.set_icon_as_template(false)
            .map_err(|e| AppError::Message(format!("设置动态图标模板状态失败: {e}")))?;
    }
    Ok(())
}

#[cfg(target_os = "macos")]
fn restore_default_tray_icon(app: &tauri::AppHandle) {
    let Some(icon) = default_macos_tray_icon() else {
        return;
    };
    if let Some(tray) = app.tray_by_id(crate::tray::TRAY_ID) {
        if let Err(err) = tray.set_icon(Some(icon)) {
            log::warn!("[TrayIcon] 恢复默认托盘图标失败: {err}");
            return;
        }
        if let Err(err) = tray.set_icon_as_template(true) {
            log::warn!("[TrayIcon] 恢复默认托盘图标模板状态失败: {err}");
        }
    }
}

#[cfg(target_os = "macos")]
fn build_slots(
    state: &AppState,
    visible_apps: &VisibleApps,
) -> Result<Vec<TrayIconSlot>, AppError> {
    let mut slots = Vec::new();
    for app_type in visible_app_types(visible_apps) {
        let current_id = match crate::settings::get_effective_current_provider(&state.db, &app_type)
        {
            Ok(Some(id)) => id,
            Ok(None) => continue,
            Err(err) => {
                log::debug!(
                    "[TrayIcon] 读取 {} 当前供应商失败: {err}",
                    app_type.as_str()
                );
                continue;
            }
        };

        let provider = match state.db.get_provider_by_id(&current_id, app_type.as_str()) {
            Ok(Some(provider)) => provider,
            Ok(None) => continue,
            Err(err) => {
                log::debug!(
                    "[TrayIcon] 读取 {} 供应商 {current_id} 失败: {err}",
                    app_type.as_str()
                );
                continue;
            }
        };

        slots.push(slot_from_provider(app_type, &current_id, &provider));
    }
    Ok(slots)
}

#[cfg(target_os = "macos")]
fn visible_app_types(visible_apps: &VisibleApps) -> Vec<AppType> {
    AppType::all()
        .filter(|app_type| visible_apps.is_visible(app_type))
        .collect()
}

#[cfg(target_os = "macos")]
fn slot_from_provider(app_type: AppType, provider_id: &str, provider: &Provider) -> TrayIconSlot {
    TrayIconSlot {
        app_type,
        provider_id: provider_id.to_string(),
        provider_name: provider.name.clone(),
        provider_icon: provider.icon.clone(),
        provider_icon_color: provider.icon_color.clone(),
    }
}

#[cfg(target_os = "macos")]
fn render_slots(slots: &[TrayIconSlot]) -> Image<'static> {
    let width = SIDE_PADDING * 2 + SLOT_WIDTH * slots.len() as u32;
    let mut canvas = Canvas::new(width.max(CANVAS_HEIGHT), CANVAS_HEIGHT);

    for (index, slot) in slots.iter().enumerate() {
        draw_slot(&mut canvas, SIDE_PADDING + SLOT_WIDTH * index as u32, slot);
    }

    Image::new_owned(canvas.rgba, canvas.width, canvas.height)
}

#[cfg(target_os = "macos")]
fn draw_slot(canvas: &mut Canvas, x: u32, slot: &TrayIconSlot) {
    let provider_x = x as i32 + PROVIDER_OFFSET_X;
    let provider_y = PROVIDER_OFFSET_Y;
    if let Some(icon) = provider_icon_image(slot) {
        if needs_contrast_plate(&icon) {
            draw_provider_icon_plate(canvas, provider_x, provider_y);
        }
        draw_image_fit(
            canvas,
            &icon,
            provider_x,
            provider_y,
            PROVIDER_SIZE,
            PROVIDER_SIZE,
        );
    } else {
        draw_provider_fallback(canvas, slot, provider_x, provider_y);
    }

    draw_agent_badge(canvas, x, slot.app_type.clone());
}

#[cfg(target_os = "macos")]
fn draw_provider_icon_plate(canvas: &mut Canvas, x: i32, y: i32) {
    canvas.fill_rounded_rect(
        x - 1,
        y - 1,
        PROVIDER_SIZE + 2,
        PROVIDER_SIZE + 2,
        10,
        Color::PROVIDER_PLATE_RING,
    );
    canvas.fill_rounded_rect(
        x,
        y,
        PROVIDER_SIZE,
        PROVIDER_SIZE,
        9,
        Color::PROVIDER_PLATE_FILL,
    );
}

#[cfg(target_os = "macos")]
fn draw_provider_fallback(canvas: &mut Canvas, slot: &TrayIconSlot, x: i32, y: i32) {
    let provider_color = provider_color(slot);
    canvas.fill_rounded_rect(x, y, PROVIDER_SIZE, PROVIDER_SIZE, 9, provider_color);
    draw_centered_text(
        canvas,
        &initials(&slot.provider_name),
        x,
        y + 12,
        PROVIDER_SIZE,
        2,
        readable_text_color(provider_color),
    );
}

#[cfg(target_os = "macos")]
fn draw_agent_badge(canvas: &mut Canvas, x: u32, app_type: AppType) {
    let badge_x = x as i32 + BADGE_OFFSET_X;
    let badge_y = BADGE_OFFSET_Y;
    let center_x = badge_x + BADGE_SIZE as i32 / 2;
    let center_y = badge_y + BADGE_SIZE as i32 / 2;
    let radius = BADGE_SIZE as i32 / 2;

    canvas.fill_circle(center_x, center_y, radius, Color::BADGE_RING);
    canvas.fill_circle(center_x, center_y, radius - 1, Color::BADGE_FILL);

    if let Some(icon) = icon_image(agent_icon_name(app_type)) {
        let icon_x = badge_x + ((BADGE_SIZE - BADGE_ICON_SIZE) / 2) as i32;
        let icon_y = badge_y + ((BADGE_SIZE - BADGE_ICON_SIZE) / 2) as i32;
        draw_image_fit(
            canvas,
            &icon,
            icon_x,
            icon_y,
            BADGE_ICON_SIZE,
            BADGE_ICON_SIZE,
        );
    }
}

#[cfg(target_os = "macos")]
fn draw_image_fit(canvas: &mut Canvas, image: &Image<'_>, x: i32, y: i32, max_w: u32, max_h: u32) {
    let Some(bounds) = image_content_bounds(image) else {
        return;
    };

    let src_w = bounds.width();
    let src_h = bounds.height();
    let scale = (max_w as f32 / src_w as f32).min(max_h as f32 / src_h as f32);
    let dest_w = (src_w as f32 * scale).round().clamp(1.0, max_w as f32) as u32;
    let dest_h = (src_h as f32 * scale).round().clamp(1.0, max_h as f32) as u32;
    let dest_x = x + ((max_w - dest_w) / 2) as i32;
    let dest_y = y + ((max_h - dest_h) / 2) as i32;

    let rgba = image.rgba();
    for dy in 0..dest_h {
        for dx in 0..dest_w {
            let sx = bounds.left
                + ((dx as f32 + 0.5) * src_w as f32 / dest_w as f32)
                    .floor()
                    .min((src_w - 1) as f32) as u32;
            let sy = bounds.top
                + ((dy as f32 + 0.5) * src_h as f32 / dest_h as f32)
                    .floor()
                    .min((src_h - 1) as f32) as u32;
            let idx = ((sy * image.width() + sx) * 4) as usize;
            canvas.set_pixel(
                dest_x + dx as i32,
                dest_y + dy as i32,
                Color {
                    r: rgba[idx],
                    g: rgba[idx + 1],
                    b: rgba[idx + 2],
                    a: rgba[idx + 3],
                },
            );
        }
    }
}

#[cfg(target_os = "macos")]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct ImageBounds {
    left: u32,
    top: u32,
    right: u32,
    bottom: u32,
}

#[cfg(target_os = "macos")]
impl ImageBounds {
    fn width(self) -> u32 {
        self.right - self.left + 1
    }

    fn height(self) -> u32 {
        self.bottom - self.top + 1
    }
}

#[cfg(target_os = "macos")]
fn image_content_bounds(image: &Image<'_>) -> Option<ImageBounds> {
    let mut bounds = ImageBounds {
        left: image.width(),
        top: image.height(),
        right: 0,
        bottom: 0,
    };

    for y in 0..image.height() {
        for x in 0..image.width() {
            let idx = ((y * image.width() + x) * 4 + 3) as usize;
            if image.rgba()[idx] > 8 {
                bounds.left = bounds.left.min(x);
                bounds.top = bounds.top.min(y);
                bounds.right = bounds.right.max(x);
                bounds.bottom = bounds.bottom.max(y);
            }
        }
    }

    if bounds.left > bounds.right || bounds.top > bounds.bottom {
        None
    } else {
        Some(bounds)
    }
}

#[cfg(target_os = "macos")]
fn needs_contrast_plate(image: &Image<'_>) -> bool {
    let Some(bounds) = image_content_bounds(image) else {
        return false;
    };
    let rgba = image.rgba();
    let mut alpha_pixels = 0u32;
    let mut weighted_luminance = 0.0f32;
    let mut alpha_weight = 0.0f32;

    for y in bounds.top..=bounds.bottom {
        for x in bounds.left..=bounds.right {
            let idx = ((y * image.width() + x) * 4) as usize;
            let alpha = rgba[idx + 3];
            if alpha <= 8 {
                continue;
            }
            alpha_pixels += 1;
            let weight = alpha as f32 / 255.0;
            alpha_weight += weight;
            weighted_luminance += (0.299 * rgba[idx] as f32
                + 0.587 * rgba[idx + 1] as f32
                + 0.114 * rgba[idx + 2] as f32)
                * weight;
        }
    }

    if alpha_weight <= f32::EPSILON {
        return false;
    }

    let coverage = alpha_pixels as f32 / (bounds.width() * bounds.height()) as f32;
    let luminance = weighted_luminance / alpha_weight;
    coverage < 0.72 && luminance < 80.0
}

#[cfg(target_os = "macos")]
fn provider_color(slot: &TrayIconSlot) -> Color {
    if let Some(color) = slot
        .provider_icon_color
        .as_deref()
        .and_then(parse_hex_color)
    {
        return color;
    }

    if let Some((_, color)) = slot
        .provider_icon
        .as_deref()
        .and_then(provider_icon_mapping)
    {
        return color;
    }

    color_from_text(&slot.provider_name)
}

#[cfg(target_os = "macos")]
fn provider_icon_name(slot: &TrayIconSlot) -> Option<&'static str> {
    if let Some(icon_name) = slot
        .provider_icon
        .as_deref()
        .and_then(crate::tray_icon_assets::icon_asset_name)
    {
        return Some(icon_name);
    }

    infer_icon_name_from_text(&slot.provider_name)
}

#[cfg(target_os = "macos")]
fn infer_icon_name_from_text(text: &str) -> Option<&'static str> {
    let compact: String = text
        .chars()
        .filter(|c| c.is_ascii_alphanumeric())
        .map(|c| c.to_ascii_lowercase())
        .collect();
    if let Some(icon_name) = crate::tray_icon_assets::icon_asset_name(&compact) {
        return Some(icon_name);
    }

    text.split(|c: char| !c.is_ascii_alphanumeric())
        .filter(|part| !part.is_empty())
        .find_map(crate::tray_icon_assets::icon_asset_name)
}

#[cfg(target_os = "macos")]
fn provider_icon_image(slot: &TrayIconSlot) -> Option<Image<'static>> {
    provider_icon_name(slot).and_then(icon_image)
}

#[cfg(target_os = "macos")]
fn icon_image(name: &str) -> Option<Image<'static>> {
    let bytes = crate::tray_icon_assets::icon_png_bytes(name)?;
    match Image::from_bytes(bytes) {
        Ok(image) => Some(image.to_owned()),
        Err(err) => {
            log::debug!("[TrayIcon] 解析图标 {name} 失败: {err}");
            None
        }
    }
}

#[cfg(target_os = "macos")]
fn agent_icon_name(app_type: AppType) -> &'static str {
    match app_type {
        AppType::Claude | AppType::ClaudeDesktop => "claude",
        AppType::Codex => "openai",
        AppType::Gemini => "gemini",
        AppType::OpenCode => "opencode",
        AppType::OpenClaw => "openclaw",
        AppType::Hermes => "hermes",
    }
}

#[cfg(target_os = "macos")]
fn provider_icon_mapping(icon: &str) -> Option<(&'static str, Color)> {
    let icon = icon.trim().to_lowercase();
    match icon.as_str() {
        "openai" | "chatgpt" => Some(("AI", rgb(16, 163, 127))),
        "anthropic" | "claude" => Some(("CL", rgb(212, 145, 93))),
        "deepseek" => Some(("DS", rgb(30, 136, 229))),
        "gemini" | "google" => Some(("G", rgb(66, 133, 244))),
        "qwen" | "bailian" | "alibaba" => Some(("QW", rgb(255, 106, 0))),
        "githubcopilot" | "copilot" | "github" => Some(("GH", rgb(36, 41, 47))),
        "kimi" => Some(("KI", rgb(99, 102, 241))),
        "zhipu" | "glm" | "chatglm" => Some(("GL", rgb(15, 98, 254))),
        "doubao" | "bytedance" => Some(("DB", rgb(20, 20, 20))),
        "minimax" => Some(("MM", rgb(255, 107, 107))),
        "moonshot" => Some(("MS", rgb(99, 102, 241))),
        "mistral" => Some(("MI", rgb(255, 112, 0))),
        "openrouter" => Some(("OR", rgb(25, 25, 25))),
        "siliconflow" => Some(("SF", rgb(36, 122, 255))),
        "xai" | "grok" => Some(("X", rgb(18, 18, 18))),
        _ => None,
    }
}

#[cfg(target_os = "macos")]
fn draw_centered_text(
    canvas: &mut Canvas,
    text: &str,
    x: i32,
    y: i32,
    width: u32,
    scale: i32,
    color: Color,
) {
    let normalized: String = text
        .chars()
        .filter(|c| c.is_ascii_alphanumeric())
        .take(2)
        .map(|c| c.to_ascii_uppercase())
        .collect();
    if normalized.is_empty() {
        return;
    }

    let char_count = normalized.chars().count() as i32;
    let text_width = (char_count * 5 + (char_count - 1)) * scale;
    let start_x = x + ((width as i32 - text_width) / 2).max(0);
    let mut cursor_x = start_x;
    for ch in normalized.chars() {
        draw_char(canvas, ch, cursor_x, y, scale, color);
        cursor_x += 6 * scale;
    }
}

#[cfg(target_os = "macos")]
fn draw_char(canvas: &mut Canvas, ch: char, x: i32, y: i32, scale: i32, color: Color) {
    let Some(rows) = glyph(ch) else {
        return;
    };
    for (row_idx, row) in rows.iter().enumerate() {
        for col in 0..5 {
            if row & (1 << (4 - col)) != 0 {
                canvas.fill_rect(
                    x + col * scale,
                    y + row_idx as i32 * scale,
                    scale as u32,
                    scale as u32,
                    color,
                );
            }
        }
    }
}

#[cfg(target_os = "macos")]
fn glyph(ch: char) -> Option<[u8; 7]> {
    Some(match ch {
        '0' => [
            0b01110, 0b10001, 0b10011, 0b10101, 0b11001, 0b10001, 0b01110,
        ],
        '1' => [
            0b00100, 0b01100, 0b00100, 0b00100, 0b00100, 0b00100, 0b01110,
        ],
        '2' => [
            0b01110, 0b10001, 0b00001, 0b00010, 0b00100, 0b01000, 0b11111,
        ],
        '3' => [
            0b11110, 0b00001, 0b00001, 0b01110, 0b00001, 0b00001, 0b11110,
        ],
        '4' => [
            0b00010, 0b00110, 0b01010, 0b10010, 0b11111, 0b00010, 0b00010,
        ],
        '5' => [
            0b11111, 0b10000, 0b10000, 0b11110, 0b00001, 0b00001, 0b11110,
        ],
        '6' => [
            0b01110, 0b10000, 0b10000, 0b11110, 0b10001, 0b10001, 0b01110,
        ],
        '7' => [
            0b11111, 0b00001, 0b00010, 0b00100, 0b01000, 0b01000, 0b01000,
        ],
        '8' => [
            0b01110, 0b10001, 0b10001, 0b01110, 0b10001, 0b10001, 0b01110,
        ],
        '9' => [
            0b01110, 0b10001, 0b10001, 0b01111, 0b00001, 0b00001, 0b01110,
        ],
        'A' => [
            0b01110, 0b10001, 0b10001, 0b11111, 0b10001, 0b10001, 0b10001,
        ],
        'B' => [
            0b11110, 0b10001, 0b10001, 0b11110, 0b10001, 0b10001, 0b11110,
        ],
        'C' => [
            0b01111, 0b10000, 0b10000, 0b10000, 0b10000, 0b10000, 0b01111,
        ],
        'D' => [
            0b11110, 0b10001, 0b10001, 0b10001, 0b10001, 0b10001, 0b11110,
        ],
        'E' => [
            0b11111, 0b10000, 0b10000, 0b11110, 0b10000, 0b10000, 0b11111,
        ],
        'F' => [
            0b11111, 0b10000, 0b10000, 0b11110, 0b10000, 0b10000, 0b10000,
        ],
        'G' => [
            0b01110, 0b10001, 0b10000, 0b10111, 0b10001, 0b10001, 0b01111,
        ],
        'H' => [
            0b10001, 0b10001, 0b10001, 0b11111, 0b10001, 0b10001, 0b10001,
        ],
        'I' => [
            0b01110, 0b00100, 0b00100, 0b00100, 0b00100, 0b00100, 0b01110,
        ],
        'J' => [
            0b00111, 0b00010, 0b00010, 0b00010, 0b00010, 0b10010, 0b01100,
        ],
        'K' => [
            0b10001, 0b10010, 0b10100, 0b11000, 0b10100, 0b10010, 0b10001,
        ],
        'L' => [
            0b10000, 0b10000, 0b10000, 0b10000, 0b10000, 0b10000, 0b11111,
        ],
        'M' => [
            0b10001, 0b11011, 0b10101, 0b10101, 0b10001, 0b10001, 0b10001,
        ],
        'N' => [
            0b10001, 0b11001, 0b10101, 0b10011, 0b10001, 0b10001, 0b10001,
        ],
        'O' => [
            0b01110, 0b10001, 0b10001, 0b10001, 0b10001, 0b10001, 0b01110,
        ],
        'P' => [
            0b11110, 0b10001, 0b10001, 0b11110, 0b10000, 0b10000, 0b10000,
        ],
        'Q' => [
            0b01110, 0b10001, 0b10001, 0b10001, 0b10101, 0b10010, 0b01101,
        ],
        'R' => [
            0b11110, 0b10001, 0b10001, 0b11110, 0b10100, 0b10010, 0b10001,
        ],
        'S' => [
            0b01111, 0b10000, 0b10000, 0b01110, 0b00001, 0b00001, 0b11110,
        ],
        'T' => [
            0b11111, 0b00100, 0b00100, 0b00100, 0b00100, 0b00100, 0b00100,
        ],
        'U' => [
            0b10001, 0b10001, 0b10001, 0b10001, 0b10001, 0b10001, 0b01110,
        ],
        'V' => [
            0b10001, 0b10001, 0b10001, 0b10001, 0b10001, 0b01010, 0b00100,
        ],
        'W' => [
            0b10001, 0b10001, 0b10001, 0b10101, 0b10101, 0b10101, 0b01010,
        ],
        'X' => [
            0b10001, 0b10001, 0b01010, 0b00100, 0b01010, 0b10001, 0b10001,
        ],
        'Y' => [
            0b10001, 0b10001, 0b01010, 0b00100, 0b00100, 0b00100, 0b00100,
        ],
        'Z' => [
            0b11111, 0b00001, 0b00010, 0b00100, 0b01000, 0b10000, 0b11111,
        ],
        _ => return None,
    })
}

#[cfg(target_os = "macos")]
fn initials(name: &str) -> String {
    let words: Vec<String> = name
        .split(|c: char| !c.is_ascii_alphanumeric())
        .filter(|s| !s.is_empty())
        .map(|s| s.to_ascii_uppercase())
        .collect();

    let mut result = String::new();
    if words.len() >= 2 {
        result.push(words[0].chars().next().unwrap_or('P'));
        result.push(words[1].chars().next().unwrap_or('P'));
    } else if let Some(word) = words.first() {
        result.extend(word.chars().take(2));
    }

    if result.is_empty() {
        "P".to_string()
    } else {
        result
    }
}

#[cfg(target_os = "macos")]
fn parse_hex_color(input: &str) -> Option<Color> {
    let value = input.trim().trim_start_matches('#');
    if value.len() != 6 {
        return None;
    }
    let r = u8::from_str_radix(&value[0..2], 16).ok()?;
    let g = u8::from_str_radix(&value[2..4], 16).ok()?;
    let b = u8::from_str_radix(&value[4..6], 16).ok()?;
    Some(rgb(r, g, b))
}

#[cfg(target_os = "macos")]
const fn rgb(r: u8, g: u8, b: u8) -> Color {
    Color { r, g, b, a: 255 }
}

#[cfg(target_os = "macos")]
fn readable_text_color(color: Color) -> Color {
    let luminance = 0.299 * color.r as f32 + 0.587 * color.g as f32 + 0.114 * color.b as f32;
    if luminance > 168.0 {
        Color::DARK
    } else {
        Color::WHITE
    }
}

#[cfg(target_os = "macos")]
fn color_from_text(text: &str) -> Color {
    let palette = [
        rgb(37, 99, 235),
        rgb(5, 150, 105),
        rgb(217, 119, 6),
        rgb(220, 38, 38),
        rgb(124, 58, 237),
        rgb(8, 145, 178),
        rgb(79, 70, 229),
    ];
    let hash = text.bytes().fold(0usize, |acc, b| {
        acc.wrapping_mul(31).wrapping_add(b as usize)
    });
    palette[hash % palette.len()]
}

#[cfg(test)]
mod tests {
    use super::*;

    #[cfg(target_os = "macos")]
    #[test]
    fn initials_prefers_word_starts() {
        assert_eq!(initials("Deep Seek"), "DS");
        assert_eq!(initials("CustomRouter"), "CU");
        assert_eq!(initials("  "), "P");
    }

    #[cfg(target_os = "macos")]
    #[test]
    fn provider_icon_name_uses_known_asset_key() {
        let slot = TrayIconSlot {
            app_type: AppType::Codex,
            provider_id: "p1".to_string(),
            provider_name: "Custom Provider".to_string(),
            provider_icon: Some("deepseek".to_string()),
            provider_icon_color: None,
        };
        assert_eq!(provider_icon_name(&slot), Some("deepseek"));
    }

    #[cfg(target_os = "macos")]
    #[test]
    fn provider_icon_name_aliases_provider_keys() {
        let slot = TrayIconSlot {
            app_type: AppType::Claude,
            provider_id: "p1".to_string(),
            provider_name: "Moonshot".to_string(),
            provider_icon: Some("moonshot".to_string()),
            provider_icon_color: None,
        };
        assert_eq!(provider_icon_name(&slot), Some("kimi"));
    }

    #[cfg(target_os = "macos")]
    #[test]
    fn provider_icon_name_infers_from_provider_name_when_icon_is_missing() {
        let slot = TrayIconSlot {
            app_type: AppType::OpenCode,
            provider_id: "p1".to_string(),
            provider_name: "Kimi For Coding".to_string(),
            provider_icon: None,
            provider_icon_color: None,
        };
        assert_eq!(provider_icon_name(&slot), Some("kimi"));
    }

    #[cfg(target_os = "macos")]
    #[test]
    fn agent_icon_name_uses_builtin_icons() {
        assert_eq!(agent_icon_name(AppType::Claude), "claude");
        assert_eq!(agent_icon_name(AppType::ClaudeDesktop), "claude");
        assert_eq!(agent_icon_name(AppType::Codex), "openai");
        assert_eq!(agent_icon_name(AppType::Gemini), "gemini");
        assert_eq!(agent_icon_name(AppType::OpenCode), "opencode");
        assert_eq!(agent_icon_name(AppType::OpenClaw), "openclaw");
        assert_eq!(agent_icon_name(AppType::Hermes), "hermes");
    }

    #[cfg(target_os = "macos")]
    #[test]
    fn contrast_plate_is_only_needed_for_dark_transparent_icons() {
        assert!(needs_contrast_plate(&diagonal_test_icon(rgb(17, 24, 39))));
        assert!(!needs_contrast_plate(&diagonal_test_icon(Color::WHITE)));
        assert!(!needs_contrast_plate(&solid_test_icon(rgb(17, 24, 39))));
    }

    #[cfg(target_os = "macos")]
    #[test]
    fn parse_hex_color_accepts_hash_prefix() {
        assert_eq!(parse_hex_color("#0EA5E9"), Some(rgb(14, 165, 233)));
        assert_eq!(parse_hex_color("bad"), None);
    }

    #[cfg(target_os = "macos")]
    #[test]
    fn render_width_scales_with_slot_count() {
        let slots = vec![
            TrayIconSlot {
                app_type: AppType::Claude,
                provider_id: "a".to_string(),
                provider_name: "Anthropic".to_string(),
                provider_icon: Some("claude".to_string()),
                provider_icon_color: None,
            },
            TrayIconSlot {
                app_type: AppType::Codex,
                provider_id: "b".to_string(),
                provider_name: "DeepSeek".to_string(),
                provider_icon: None,
                provider_icon_color: Some("#1E88E5".to_string()),
            },
        ];
        let image = render_slots(&slots);
        assert_eq!(image.height(), CANVAS_HEIGHT);
        assert_eq!(image.width(), SIDE_PADDING * 2 + SLOT_WIDTH * 2);
    }

    #[cfg(target_os = "macos")]
    #[test]
    fn visible_app_types_follow_visible_apps() {
        let visible = VisibleApps {
            claude: true,
            claude_desktop: false,
            codex: true,
            gemini: false,
            opencode: false,
            openclaw: true,
            hermes: false,
        };

        assert_eq!(
            visible_app_types(&visible),
            vec![AppType::Claude, AppType::Codex, AppType::OpenClaw]
        );
    }

    #[cfg(target_os = "macos")]
    fn diagonal_test_icon(color: Color) -> Image<'static> {
        let mut rgba = vec![0; 16 * 16 * 4];
        for offset in 4..12 {
            set_test_pixel(&mut rgba, offset, offset, color);
            set_test_pixel(&mut rgba, 15 - offset, offset, color);
        }
        Image::new_owned(rgba, 16, 16)
    }

    #[cfg(target_os = "macos")]
    fn solid_test_icon(color: Color) -> Image<'static> {
        let mut rgba = vec![0; 16 * 16 * 4];
        for y in 0..16 {
            for x in 0..16 {
                set_test_pixel(&mut rgba, x, y, color);
            }
        }
        Image::new_owned(rgba, 16, 16)
    }

    #[cfg(target_os = "macos")]
    fn set_test_pixel(rgba: &mut [u8], x: usize, y: usize, color: Color) {
        let idx = (y * 16 + x) * 4;
        rgba[idx] = color.r;
        rgba[idx + 1] = color.g;
        rgba[idx + 2] = color.b;
        rgba[idx + 3] = color.a;
    }
}

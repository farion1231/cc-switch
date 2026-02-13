use ratatui::style::Color;

pub const PRIMARY: Color = Color::Cyan;
pub const SUCCESS: Color = Color::Green;
pub const WARNING: Color = Color::Yellow;
pub const ERROR: Color = Color::Red;
pub const MUTED: Color = Color::DarkGray;

pub const ICON_ACTIVE: &str = "●";
pub const ICON_INACTIVE: &str = "○";
pub const ICON_SELECTED: &str = "▶";
pub const LIST_HIGHLIGHT_SYMBOL: &str = "▶ ";
pub const ICON_SUCCESS: &str = "✓";
pub const ICON_ERROR: &str = "✗";
pub const ICON_WARNING: &str = "⚠";
pub const ICON_INFO: &str = "ℹ";
pub const ICON_LOCK: &str = "🔒";
pub const ICON_SPINNER: &str = "⏳";

pub const SPINNER_FRAMES: [&str; 10] = ["⠋", "⠙", "⠹", "⠸", "⠼", "⠴", "⠦", "⠧", "⠇", "⠏"];

use crate::settings::get_settings;
use tauri::AppHandle;
use tauri_plugin_notification::NotificationExt;

use super::types::ClaudeNotifyEventType;

fn notification_copy(event_type: &ClaudeNotifyEventType) -> (&'static str, &'static str) {
    let lang = get_settings().language.unwrap_or_else(|| "zh".to_string());

    match (lang.as_str(), event_type) {
        ("en", ClaudeNotifyEventType::PermissionPrompt) => (
            "Claude Code needs your confirmation",
            "There is a permission request waiting in the terminal.",
        ),
        ("en", ClaudeNotifyEventType::IdlePrompt) => (
            "Claude Code is waiting for you",
            "The current round is paused. Return to the terminal.",
        ),
        ("en", ClaudeNotifyEventType::Stop) => (
            "Claude Code finished the current round",
            "Return to the terminal to review the result.",
        ),
        ("ja", ClaudeNotifyEventType::PermissionPrompt) => (
            "Claude Code が確認を待っています",
            "ターミナルで権限確認が必要です。",
        ),
        ("ja", ClaudeNotifyEventType::IdlePrompt) => (
            "Claude Code があなたを待っています",
            "現在のラウンドは一時停止中です。ターミナルに戻ってください。",
        ),
        ("ja", ClaudeNotifyEventType::Stop) => (
            "Claude Code が現在のラウンドを終了しました",
            "結果を確認するためターミナルに戻ってください。",
        ),
        (_, ClaudeNotifyEventType::PermissionPrompt) => {
            ("Claude Code 需要你的确认", "终端中有权限请求，请返回查看。")
        }
        (_, ClaudeNotifyEventType::IdlePrompt) => {
            ("Claude Code 正在等待你", "当前轮次已暂停，请返回终端查看。")
        }
        (_, ClaudeNotifyEventType::Stop) => ("Claude Code 已结束当前轮次", "请返回终端查看结果。"),
    }
}

pub fn show_toast(app: &AppHandle, event_type: &ClaudeNotifyEventType) -> Result<(), String> {
    let (title, body) = notification_copy(event_type);
    app.notification()
        .builder()
        .title(title)
        .body(body)
        .show()
        .map_err(|e| format!("显示 Claude 通知失败: {e}"))
}

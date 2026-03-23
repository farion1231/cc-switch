#![allow(non_snake_case)]

use serde::Serialize;
use serde_json::{Map, Value};
use std::fs;
use std::path::{Path, PathBuf};

use crate::config::{atomic_write, get_app_config_dir, get_claude_settings_path, write_text_file};
use crate::error::AppError;

const HOOKS_MARKER: &str = "cc-switch-claude-notify";
const NOTIFY_ENDPOINT_PATH: &str = "/hooks/claude-notify";
const HOOK_SCRIPT_FILE: &str = "cc-switch-claude-notify-hook.ps1";

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ClaudeNotifyStatus {
    pub port: Option<u16>,
    pub listening: bool,
    pub hooks_applied: bool,
}

async fn current_runtime_status(
    state: &tauri::State<'_, crate::store::AppState>,
) -> Result<ClaudeNotifyStatus, String> {
    let hooks_applied = is_claude_notify_hooks_applied().map_err(|e| e.to_string())?;
    let settings = crate::settings::get_settings();
    let runtime = state.claude_notify_service.get_status().await;

    Ok(ClaudeNotifyStatus {
        port: runtime.port.or(settings.claude_notify_port),
        listening: runtime.listening,
        hooks_applied,
    })
}


fn notification_runtime_toggle_changed(
    existing: &crate::settings::AppSettings,
    merged: &crate::settings::AppSettings,
) -> bool {
    existing.enable_claude_background_notifications != merged.enable_claude_background_notifications
}

pub async fn sync_claude_notify_runtime_if_needed(
    app: tauri::AppHandle,
    state: &tauri::State<'_, crate::store::AppState>,
    existing: &crate::settings::AppSettings,
    merged: &crate::settings::AppSettings,
) -> Result<(), String> {
    if !notification_runtime_toggle_changed(existing, merged) {
        return Ok(());
    }

    state.claude_notify_service.set_app_handle(app).await;
    state.claude_notify_service.sync_with_settings().await?;
    Ok(())
}

fn settings_path() -> PathBuf {
    get_claude_settings_path()
}

fn read_settings_json() -> Result<Value, AppError> {
    let path = settings_path();
    if !path.exists() {
        return Ok(serde_json::json!({}));
    }
    let content = fs::read_to_string(&path).map_err(|e| AppError::io(&path, e))?;
    serde_json::from_str::<Value>(&content).map_err(|e| AppError::json(&path, e))
}

fn write_settings_json(root: &Value) -> Result<(), AppError> {
    let path = settings_path();
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).map_err(|e| AppError::io(parent, e))?;
    }
    let json =
        serde_json::to_string_pretty(root).map_err(|e| AppError::JsonSerialize { source: e })?;
    atomic_write(&path, format!("{json}\n").as_bytes())
}

fn hook_script_path() -> PathBuf {
    get_app_config_dir()
        .join("claude-notify")
        .join(HOOK_SCRIPT_FILE)
}

fn ensure_hook_script() -> Result<PathBuf, AppError> {
    let path = hook_script_path();
    let script = format!(
        r#"param(
    [Parameter(Position = 0)]
    [string]$Mode,
    [Parameter(Position = 1)]
    [int]$Port
)

$raw = [Console]::In.ReadToEnd()
if ([string]::IsNullOrWhiteSpace($raw)) {{
    exit 0
}}

try {{
    $json = $raw | ConvertFrom-Json
}} catch {{
    exit 0
}}

$eventType = $null
$notificationType = $null

switch ($Mode) {{
    'notification' {{
        if ($json.notification_type -eq 'permission_prompt') {{
            $eventType = 'permission_prompt'
            $notificationType = $json.notification_type
        }} elseif ($json.notification_type -eq 'idle_prompt') {{
            $eventType = 'idle_prompt'
            $notificationType = $json.notification_type
        }} else {{
            exit 0
        }}
    }}
    'stop' {{
        $eventType = 'stop'
    }}
    default {{
        exit 0
    }}
}}

if (-not $json.session_id) {{
    exit 0
}}

$body = @{{
    sourceApp = 'claude-code'
    eventType = $eventType
    sessionId = $json.session_id
    cwd = $json.cwd
    timestamp = [DateTimeOffset]::UtcNow.ToUnixTimeMilliseconds()
}}

if ($notificationType) {{
    $body.notificationType = $notificationType
}}

try {{
    Invoke-RestMethod -Uri ('http://127.0.0.1:{{0}}{path}' -f $Port) -Method Post -ContentType 'application/json' -Body ($body | ConvertTo-Json -Compress) | Out-Null
}} catch {{
}}

exit 0
"#,
        path = NOTIFY_ENDPOINT_PATH,
    );
    write_text_file(&path, &script)?;
    Ok(path)
}

fn build_hook_command(script_path: &Path, mode: &str, port: u16) -> String {
    format!(
        "powershell.exe -NoProfile -ExecutionPolicy Bypass -File \"{}\" {} {} # {}",
        script_path.display(),
        mode,
        port,
        HOOKS_MARKER
    )
}

fn build_hooks_block(port: u16, script_path: &Path) -> Value {
    serde_json::json!({
        "Notification": [
            {
                "matcher": "permission_prompt|idle_prompt",
                "hooks": [
                    {
                        "type": "command",
                        "command": build_hook_command(script_path, "notification", port)
                    }
                ]
            }
        ],
        "Stop": [
            {
                "hooks": [
                    {
                        "type": "command",
                        "command": build_hook_command(script_path, "stop", port)
                    }
                ]
            }
        ]
    })
}

fn has_marker(value: &Value) -> bool {
    match value {
        Value::String(s) => s.contains(HOOKS_MARKER),
        Value::Array(arr) => arr.iter().any(has_marker),
        Value::Object(map) => map.values().any(has_marker),
        _ => false,
    }
}

fn merge_hook_entries(existing: Option<&Value>, incoming: &Value) -> Value {
    let mut merged = Vec::new();

    if let Some(Value::Array(items)) = existing {
        for item in items {
            if !has_marker(item) {
                merged.push(item.clone());
            }
        }
    }

    if let Value::Array(items) = incoming {
        merged.extend(items.iter().cloned());
    }

    Value::Array(merged)
}

fn clear_managed_entries(existing: Option<&Value>) -> Option<Value> {
    match existing {
        Some(Value::Array(items)) => {
            let kept: Vec<Value> = items
                .iter()
                .filter(|item| !has_marker(item))
                .cloned()
                .collect();
            if kept.is_empty() {
                None
            } else {
                Some(Value::Array(kept))
            }
        }
        Some(other) if !has_marker(other) => Some(other.clone()),
        _ => None,
    }
}

pub fn apply_claude_notify_hooks(port: u16) -> Result<bool, AppError> {
    let mut root = read_settings_json()?;
    let obj = root
        .as_object_mut()
        .ok_or_else(|| AppError::Config("Claude settings.json 根必须是对象".into()))?;

    let script_path = ensure_hook_script()?;
    let incoming_hooks = build_hooks_block(port, &script_path);
    let hooks_value = obj
        .entry("hooks")
        .or_insert_with(|| Value::Object(Map::new()));
    let hooks_obj = hooks_value
        .as_object_mut()
        .ok_or_else(|| AppError::Config("settings.hooks 必须是对象".into()))?;

    if let Value::Object(incoming_map) = incoming_hooks {
        for (event_name, incoming_entries) in incoming_map {
            let merged = merge_hook_entries(hooks_obj.get(&event_name), &incoming_entries);
            hooks_obj.insert(event_name, merged);
        }
    }

    write_settings_json(&root)?;
    Ok(true)
}

pub fn clear_claude_notify_hooks() -> Result<bool, AppError> {
    let mut root = read_settings_json()?;
    let obj = root
        .as_object_mut()
        .ok_or_else(|| AppError::Config("Claude settings.json 根必须是对象".into()))?;

    let Some(hooks_value) = obj.get_mut("hooks") else {
        return Ok(false);
    };
    let hooks_obj = hooks_value
        .as_object_mut()
        .ok_or_else(|| AppError::Config("settings.hooks 必须是对象".into()))?;

    for key in ["Notification", "Stop"] {
        match clear_managed_entries(hooks_obj.get(key)) {
            Some(cleaned) => {
                hooks_obj.insert(key.to_string(), cleaned);
            }
            None => {
                hooks_obj.remove(key);
            }
        }
    }

    if hooks_obj.is_empty() {
        obj.remove("hooks");
    }

    write_settings_json(&root)?;
    Ok(true)
}

pub fn is_claude_notify_hooks_applied() -> Result<bool, AppError> {
    let root = read_settings_json()?;
    let Some(hooks) = root.get("hooks") else {
        return Ok(false);
    };
    Ok(has_marker(hooks))
}

#[tauri::command]
pub async fn apply_claude_notify_hook_config(
    app: tauri::AppHandle,
    port: u16,
    state: tauri::State<'_, crate::store::AppState>,
) -> Result<bool, String> {
    let result = apply_claude_notify_hooks(port).map_err(|e| e.to_string())?;
    state.claude_notify_service.set_app_handle(app).await;
    state
        .claude_notify_service
        .sync_with_settings()
        .await
        .map_err(|e| e.to_string())?;
    Ok(result)
}

#[tauri::command]
pub async fn clear_claude_notify_hook_config(
    state: tauri::State<'_, crate::store::AppState>,
) -> Result<bool, String> {
    let result = clear_claude_notify_hooks().map_err(|e| e.to_string())?;
    state.claude_notify_service.stop().await?;
    Ok(result)
}

#[tauri::command]
pub async fn get_claude_notify_status(
    state: tauri::State<'_, crate::store::AppState>,
) -> Result<ClaudeNotifyStatus, String> {
    current_runtime_status(&state).await
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::{get_app_config_dir, get_claude_settings_path, get_home_dir};
    use crate::settings::{update_settings, AppSettings};
    use serde_json::json;
    use std::sync::{Mutex, OnceLock};

    fn ensure_test_home() -> &'static Path {
        static HOME: OnceLock<PathBuf> = OnceLock::new();
        HOME.get_or_init(|| {
            let base = std::env::temp_dir().join("cc-switch-claude-notify-test-home");
            if base.exists() {
                let _ = std::fs::remove_dir_all(&base);
            }
            std::fs::create_dir_all(&base).expect("create test home");
            std::env::set_var("CC_SWITCH_TEST_HOME", &base);
            std::env::set_var("HOME", &base);
            #[cfg(windows)]
            std::env::set_var("USERPROFILE", &base);
            base
        })
        .as_path()
    }

    fn reset_test_fs() {
        let home = ensure_test_home();
        for sub in [".claude", ".cc-switch"] {
            let path = home.join(sub);
            if path.exists() {
                let _ = std::fs::remove_dir_all(&path);
            }
        }
        let _ = update_settings(AppSettings::default());
    }

    fn test_mutex() -> &'static Mutex<()> {
        static MUTEX: OnceLock<Mutex<()>> = OnceLock::new();
        MUTEX.get_or_init(|| Mutex::new(()))
    }

    #[test]
    fn apply_hook_config_preserves_existing_non_managed_hooks() {
        let _guard = test_mutex().lock().expect("acquire test mutex");
        reset_test_fs();
        let home = ensure_test_home();
        assert_eq!(get_home_dir(), home);
        let path = get_claude_settings_path();
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent).expect("create claude dir");
        }
        std::fs::write(
            &path,
            serde_json::to_string_pretty(&json!({
                "hooks": {
                    "Notification": [
                        {
                            "matcher": "auth_success",
                            "hooks": [
                                {"type": "command", "command": "echo external"}
                            ]
                        }
                    ]
                }
            }))
            .expect("serialize settings"),
        )
        .expect("write settings");

        apply_claude_notify_hooks(43123).expect("apply hooks");

        let content = std::fs::read_to_string(&path).expect("read settings");
        assert!(content.contains("auth_success"));
        assert!(content.contains(HOOKS_MARKER));
        assert!(content.contains("permission_prompt|idle_prompt"));
        assert!(content.contains("43123"));
        assert!(home.join(".claude").exists());
    }

    #[test]
    fn clear_hook_config_only_removes_managed_entries() {
        let _guard = test_mutex().lock().expect("acquire test mutex");
        reset_test_fs();
        ensure_test_home();
        apply_claude_notify_hooks(43123).expect("apply hooks");
        clear_claude_notify_hooks().expect("clear hooks");

        let path = get_claude_settings_path();
        let content = std::fs::read_to_string(&path).expect("read settings");
        assert!(!content.contains(HOOKS_MARKER));
        assert!(!content.contains("permission_prompt|idle_prompt"));
    }

    #[test]
    fn apply_hook_config_writes_managed_script_file() {
        let _guard = test_mutex().lock().expect("acquire test mutex");
        reset_test_fs();

        apply_claude_notify_hooks(43123).expect("apply hooks");

        let script_path = get_app_config_dir()
            .join("claude-notify")
            .join(HOOK_SCRIPT_FILE);
        assert!(script_path.exists());
        let script = std::fs::read_to_string(&script_path).expect("read hook script");
        assert!(script.contains(NOTIFY_ENDPOINT_PATH));
        assert!(script.contains("ConvertFrom-Json"));
    }
}

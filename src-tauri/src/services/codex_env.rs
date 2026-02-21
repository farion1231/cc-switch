use crate::error::AppError;
use crate::provider::Provider;
use serde_json::Value;
use std::collections::{HashMap, HashSet};
use std::fs;
use std::path::PathBuf;

/// Sync Codex Azure auth.json into shell/OS environment and clean up when leaving Azure.
///
/// - prev_auth/config: previous live Codex auth/config (if available) before this switch.
/// - Applies current auth when the target provider is Azure.
/// - Removes previously applied vars when switching away from Azure.
pub fn sync_codex_shell_env(
    provider: &Provider,
    prev_auth: Option<Value>,
    prev_config: Option<String>,
) -> Result<(), AppError> {
    let prev_is_azure = is_azure_config(prev_config.as_deref());
    let prev_keys = extract_keys(&prev_auth);

    let new_is_azure = is_azure_provider(provider);
    if !prev_keys.is_empty() && (!new_is_azure || prev_is_azure) {
        // Remove if we are leaving Azure or refreshing Azure (avoid stale values)
        remove_env_vars(&prev_keys)?;
    }

    if new_is_azure {
        let new_vars = extract_auth_vars(provider)?;
        if !new_vars.is_empty() {
            apply_env_vars(&new_vars)?;
        }
    }

    Ok(())
}

fn extract_keys(auth: &Option<Value>) -> HashSet<String> {
    auth.as_ref()
        .and_then(|v| v.as_object())
        .map(|obj| obj.keys().cloned().collect())
        .unwrap_or_default()
}

fn extract_auth_vars(provider: &Provider) -> Result<HashMap<String, String>, AppError> {
    let auth = provider
        .settings_config
        .get("auth")
        .and_then(|v| v.as_object())
        .ok_or_else(|| AppError::Message("Codex auth.json 必须是对象".to_string()))?;

    let mut vars = HashMap::new();
    for (key, value) in auth {
        match value {
            Value::String(s) => {
                vars.insert(key.clone(), s.clone());
            }
            Value::Number(n) => {
                vars.insert(key.clone(), n.to_string());
            }
            Value::Bool(b) => {
                vars.insert(key.clone(), b.to_string());
            }
            _ => {
                // ignore null/arrays/objects
            }
        }
    }

    Ok(vars)
}

fn is_azure_config(config_text: Option<&str>) -> bool {
    if let Some(cfg) = config_text {
        if let Ok(toml_val) = cfg.parse::<toml::Value>() {
            if toml_val
                .get("model_provider")
                .and_then(|v| v.as_str())
                .map(|s| s.eq_ignore_ascii_case("azure"))
                .unwrap_or(false)
            {
                return true;
            }

            if toml_val
                .get("model_providers")
                .and_then(|v| v.as_table())
                .map(|t| t.contains_key("azure"))
                .unwrap_or(false)
            {
                return true;
            }
        }

        if cfg.contains("openai.azure.com") {
            return true;
        }
    }
    false
}

fn is_azure_provider(provider: &Provider) -> bool {
    if let Some(cfg) = provider
        .settings_config
        .get("config")
        .and_then(|v| v.as_str())
    {
        if is_azure_config(Some(cfg)) {
            return true;
        }
    }

    let id_has_azure = provider.id.to_lowercase().contains("azure");
    let name_has_azure = provider.name.to_lowercase().contains("azure");
    id_has_azure || name_has_azure
}

fn apply_env_vars(vars: &HashMap<String, String>) -> Result<(), AppError> {
    for (k, v) in vars {
        std::env::set_var(k, v);
    }

    #[cfg(target_os = "windows")]
    {
        apply_env_vars_windows(vars)?;
    }

    #[cfg(not(target_os = "windows"))]
    {
        apply_env_vars_unix(vars)?;
    }

    Ok(())
}

fn remove_env_vars(var_names: &HashSet<String>) -> Result<(), AppError> {
    if var_names.is_empty() {
        return Ok(());
    }

    for key in var_names {
        std::env::remove_var(key);
    }

    #[cfg(target_os = "windows")]
    {
        remove_env_vars_windows(var_names)?;
    }

    #[cfg(not(target_os = "windows"))]
    {
        remove_env_vars_unix(var_names)?;
    }

    Ok(())
}

#[cfg(target_os = "windows")]
fn apply_env_vars_windows(vars: &HashMap<String, String>) -> Result<(), AppError> {
    use winreg::enums::*;
    use winreg::RegKey;

    let hkcu = RegKey::predef(HKEY_CURRENT_USER);
    let (env_key, _) = hkcu
        .create_subkey_with_flags("Environment", KEY_WRITE)
        .map_err(|e| AppError::Message(format!("打开注册表失败: {e}")))?;

    for (key, value) in vars {
        env_key
            .set_value(key, value)
            .map_err(|e| AppError::Message(format!("写入环境变量 {key} 失败: {e}")))?;
    }

    Ok(())
}

#[cfg(target_os = "windows")]
fn remove_env_vars_windows(var_names: &HashSet<String>) -> Result<(), AppError> {
    use winreg::enums::*;
    use winreg::RegKey;

    let hkcu = RegKey::predef(HKEY_CURRENT_USER);
    let env_key = hkcu
        .open_subkey_with_flags("Environment", KEY_ALL_ACCESS)
        .map_err(|e| AppError::Message(format!("打开注册表失败: {e}")))?;

    for key in var_names {
        if let Err(e) = env_key.delete_value(key) {
            if e.raw_os_error().unwrap_or_default() != 2 {
                return Err(AppError::Message(format!("删除环境变量 {key} 失败: {e}")));
            }
        }
    }

    Ok(())
}

#[cfg(not(target_os = "windows"))]
#[derive(Clone, Copy)]
enum ShellKind {
    ShLike,
    Fish,
}

#[cfg(not(target_os = "windows"))]
fn candidate_shell_files() -> Vec<(ShellKind, PathBuf)> {
    let home = dirs::home_dir().unwrap_or_else(|| PathBuf::from("/tmp"));
    vec![
        (ShellKind::ShLike, home.join(".zshrc")),
        (ShellKind::ShLike, home.join(".zprofile")),
        (ShellKind::ShLike, home.join(".bashrc")),
        (ShellKind::ShLike, home.join(".bash_profile")),
        (ShellKind::ShLike, home.join(".profile")),
        (ShellKind::Fish, home.join(".config/fish/config.fish")),
    ]
}

#[cfg(not(target_os = "windows"))]
fn apply_env_vars_unix(vars: &HashMap<String, String>) -> Result<(), AppError> {
    // Prefer current shell rc; fall back to first existing or first candidate
    let (preferred_kind, preferred_path) = detect_shell_config();
    let mut targets = candidate_shell_files();
    targets.retain(|(_, p)| p != &preferred_path);
    targets.insert(0, (preferred_kind, preferred_path));

    for (kind, path) in targets {
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent).map_err(|e| AppError::io(parent, e))?;
        }

        let existing = fs::read_to_string(&path).unwrap_or_default();
        let keys: HashSet<String> = vars.keys().cloned().collect();
        let mut lines: Vec<String> = existing
            .lines()
            .filter(|line| !line_sets_target(line, &keys, kind))
            .map(|s| s.to_string())
            .collect();

        if !lines.is_empty() && !lines.last().unwrap().is_empty() {
            lines.push(String::new());
        }

        lines.extend(render_exports(kind, vars));
        fs::write(&path, lines.join("\n")).map_err(|e| AppError::io(&path, e))?;
        break; // write only once
    }

    Ok(())
}

#[cfg(not(target_os = "windows"))]
fn remove_env_vars_unix(var_names: &HashSet<String>) -> Result<(), AppError> {
    let targets = candidate_shell_files();
    for (kind, path) in targets {
        if !path.exists() {
            continue;
        }
        let content = fs::read_to_string(&path).map_err(|e| AppError::io(&path, e))?;
        let filtered: Vec<String> = content
            .lines()
            .filter(|line| !line_sets_target(line, var_names, kind))
            .map(|s| s.to_string())
            .collect();
        fs::write(&path, filtered.join("\n")).map_err(|e| AppError::io(&path, e))?;
    }
    Ok(())
}

#[cfg(not(target_os = "windows"))]
fn detect_shell_config() -> (ShellKind, PathBuf) {
    let home = dirs::home_dir().unwrap_or_else(|| PathBuf::from("/tmp"));
    let shell = std::env::var("SHELL").unwrap_or_default();
    if shell.contains("fish") {
        (ShellKind::Fish, home.join(".config/fish/config.fish"))
    } else if shell.contains("zsh") {
        (ShellKind::ShLike, home.join(".zshrc"))
    } else if shell.contains("bash") {
        (ShellKind::ShLike, home.join(".bashrc"))
    } else {
        (ShellKind::ShLike, home.join(".profile"))
    }
}

#[cfg(not(target_os = "windows"))]
fn line_sets_target(line: &str, targets: &HashSet<String>, shell_kind: ShellKind) -> bool {
    let trimmed = line.trim();
    if trimmed.is_empty() || trimmed.starts_with('#') {
        return false;
    }

    match shell_kind {
        ShellKind::Fish => {
            let parts: Vec<&str> = trimmed.split_whitespace().collect();
            if parts.first() == Some(&"set") {
                let name_idx = if parts
                    .get(1)
                    .map(|p| *p == "-x" || *p == "-Ux")
                    .unwrap_or(false)
                {
                    2
                } else {
                    1
                };
                if let Some(var_name) = parts.get(name_idx) {
                    return targets.contains(*var_name);
                }
            }
            false
        }
        ShellKind::ShLike => {
            let export_line = trimmed.strip_prefix("export ").unwrap_or(trimmed);
            if let Some(eq_pos) = export_line.find('=') {
                let var_name = export_line[..eq_pos].trim();
                targets.contains(var_name)
            } else {
                false
            }
        }
    }
}

#[cfg(not(target_os = "windows"))]
fn render_exports(shell_kind: ShellKind, vars: &HashMap<String, String>) -> Vec<String> {
    let mut rendered = Vec::new();
    for (key, value) in vars {
        let sanitized = value.replace('\\', "\\\\").replace('"', "\\\"");
        match shell_kind {
            ShellKind::Fish => rendered.push(format!(r#"set -x {key} "{sanitized}""#)),
            ShellKind::ShLike => rendered.push(format!(r#"export {key}="{sanitized}""#)),
        }
    }
    rendered
}

use crate::bridges::support::map_core_err;
use crate::config::ConfigStatus;
use crate::error::AppError;

fn normalize_onboarding_file() -> Result<(), AppError> {
    let path = crate::config::get_claude_mcp_path();
    let Ok(content) = std::fs::read_to_string(&path) else {
        return Ok(());
    };
    let trimmed = content.trim_end_matches('\n');
    if trimmed.len() == content.len() {
        return Ok(());
    }
    std::fs::write(&path, trimmed).map_err(|source| AppError::io(&path, source))
}

pub fn legacy_get_status() -> Result<ConfigStatus, AppError> {
    crate::claude_plugin::claude_config_status()
        .map(|(exists, path)| ConfigStatus {
            exists,
            path: path.to_string_lossy().to_string(),
        })
        .map_err(|e| AppError::Message(e.to_string()))
}

pub fn get_status() -> Result<ConfigStatus, AppError> {
    cc_switch_core::ClaudePluginService::get_status()
        .map(|status| ConfigStatus {
            exists: status.exists,
            path: status.path,
        })
        .map_err(map_core_err)
}

pub fn legacy_read_config() -> Result<Option<String>, AppError> {
    crate::claude_plugin::read_claude_config().map_err(|e| AppError::Message(e.to_string()))
}

pub fn read_config() -> Result<Option<String>, AppError> {
    cc_switch_core::ClaudePluginService::read_config().map_err(map_core_err)
}

pub fn legacy_apply_config(official: bool) -> Result<bool, AppError> {
    if official {
        crate::claude_plugin::clear_claude_config().map_err(|e| AppError::Message(e.to_string()))
    } else {
        crate::claude_plugin::write_claude_config().map_err(|e| AppError::Message(e.to_string()))
    }
}

pub fn apply_config(official: bool) -> Result<bool, AppError> {
    cc_switch_core::ClaudePluginService::apply_config(official).map_err(map_core_err)
}

pub fn legacy_is_applied() -> Result<bool, AppError> {
    crate::claude_plugin::is_claude_config_applied().map_err(|e| AppError::Message(e.to_string()))
}

pub fn is_applied() -> Result<bool, AppError> {
    cc_switch_core::ClaudePluginService::is_applied().map_err(map_core_err)
}

pub fn legacy_apply_onboarding_skip() -> Result<bool, AppError> {
    crate::claude_mcp::set_has_completed_onboarding().map_err(|e| AppError::Message(e.to_string()))
}

pub fn apply_onboarding_skip() -> Result<bool, AppError> {
    let changed =
        cc_switch_core::ClaudePluginService::apply_onboarding_skip().map_err(map_core_err)?;
    normalize_onboarding_file()?;
    Ok(changed)
}

pub fn legacy_clear_onboarding_skip() -> Result<bool, AppError> {
    crate::claude_mcp::clear_has_completed_onboarding()
        .map_err(|e| AppError::Message(e.to_string()))
}

pub fn clear_onboarding_skip() -> Result<bool, AppError> {
    let changed =
        cc_switch_core::ClaudePluginService::clear_onboarding_skip().map_err(map_core_err)?;
    normalize_onboarding_file()?;
    Ok(changed)
}

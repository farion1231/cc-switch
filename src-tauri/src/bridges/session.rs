use crate::error::AppError;
use crate::session_manager;

use super::support::convert;

fn map_core_err(err: cc_switch_core::AppError) -> String {
    err.to_string()
}

pub fn legacy_list_sessions() -> Vec<session_manager::SessionMeta> {
    session_manager::scan_sessions()
}

pub fn list_sessions() -> Result<Vec<session_manager::SessionMeta>, String> {
    let sessions = cc_switch_core::SessionService::list_sessions();
    convert(sessions).map_err(|err: AppError| err.to_string())
}

pub fn legacy_get_session_messages(
    provider_id: &str,
    source_path: &str,
) -> Result<Vec<session_manager::SessionMessage>, String> {
    session_manager::load_messages(provider_id, source_path).map_err(|err| err.to_string())
}

pub fn get_session_messages(
    provider_id: &str,
    source_path: &str,
) -> Result<Vec<session_manager::SessionMessage>, String> {
    let messages = cc_switch_core::SessionService::get_session_messages(provider_id, source_path)
        .map_err(map_core_err)?;
    convert(messages).map_err(|err: AppError| err.to_string())
}

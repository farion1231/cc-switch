use crate::bridges::support::{convert, fresh_core_state, map_core_err};
use crate::error::AppError;

pub fn legacy_read_standard_local_file() -> Result<crate::services::omo::OmoLocalFileData, AppError>
{
    legacy_read_local_file(&crate::services::omo::STANDARD)
}

pub fn legacy_read_local_file(
    variant: &crate::services::omo::OmoVariant,
) -> Result<crate::services::omo::OmoLocalFileData, AppError> {
    crate::services::OmoService::read_local_file(variant)
}

pub fn read_standard_local_file() -> Result<crate::services::omo::OmoLocalFileData, AppError> {
    read_local_file(&cc_switch_core::STANDARD)
}

pub fn read_local_file(
    variant: &cc_switch_core::OmoVariant,
) -> Result<crate::services::omo::OmoLocalFileData, AppError> {
    let data = cc_switch_core::OmoService::read_local_file(variant).map_err(map_core_err)?;
    convert(data)
}

pub fn legacy_get_standard_provider_id(
    state: &crate::store::AppState,
) -> Result<Option<String>, AppError> {
    legacy_get_current_provider_id(state, "omo")
}

pub fn legacy_get_current_provider_id(
    state: &crate::store::AppState,
    category: &str,
) -> Result<Option<String>, AppError> {
    Ok(state
        .db
        .get_current_omo_provider("opencode", category)?
        .map(|provider| provider.id))
}

pub fn get_current_provider_id(
    variant: &cc_switch_core::OmoVariant,
) -> Result<Option<String>, AppError> {
    let state = fresh_core_state()?;
    cc_switch_core::OmoService::get_current_provider_id(&state, variant).map_err(map_core_err)
}

pub fn get_standard_provider_id() -> Result<Option<String>, AppError> {
    get_current_provider_id(&cc_switch_core::STANDARD)
}

pub fn legacy_disable_standard_current(state: &crate::store::AppState) -> Result<(), AppError> {
    legacy_disable_current(state, &crate::services::omo::STANDARD)
}

pub fn legacy_disable_current(
    state: &crate::store::AppState,
    variant: &crate::services::omo::OmoVariant,
) -> Result<(), AppError> {
    let providers = state.db.get_all_providers("opencode")?;
    for (id, provider) in &providers {
        if provider.category.as_deref() == Some(variant.category) {
            state
                .db
                .clear_omo_provider_current("opencode", id, variant.category)?;
        }
    }
    crate::services::OmoService::delete_config_file(variant)
}

pub fn disable_current(variant: &cc_switch_core::OmoVariant) -> Result<(), AppError> {
    let state = fresh_core_state()?;
    cc_switch_core::OmoService::disable_current(&state, variant).map_err(map_core_err)
}

pub fn disable_standard_current() -> Result<(), AppError> {
    disable_current(&cc_switch_core::STANDARD)
}

pub fn standard_config_exists() -> bool {
    cc_switch_core::opencode_config::get_opencode_dir()
        .join("oh-my-opencode.jsonc")
        .exists()
}

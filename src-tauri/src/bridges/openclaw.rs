use crate::bridges::support::{convert, fresh_core_state, map_core_err};
use crate::error::AppError;
use std::collections::HashMap;

pub fn legacy_get_env_as_core(
) -> Result<cc_switch_core::openclaw_config::OpenClawEnvConfig, AppError> {
    let env = crate::openclaw_config::get_env_config()?;
    convert(env)
}

pub fn legacy_set_env_from_core(
    env: cc_switch_core::openclaw_config::OpenClawEnvConfig,
) -> Result<(), AppError> {
    let env = convert(env)?;
    crate::openclaw_config::set_env_config(&env)
}

pub fn import_openclaw_providers_from_live() -> Result<usize, AppError> {
    let state = fresh_core_state()?;
    cc_switch_core::ProviderService::import_openclaw_providers_from_live(&state)
        .map_err(map_core_err)
}

pub fn get_openclaw_live_provider_ids() -> Result<Vec<String>, AppError> {
    cc_switch_core::openclaw_config::get_providers()
        .map(|providers| providers.keys().cloned().collect())
        .map_err(map_core_err)
}

pub fn get_default_model() -> Result<Option<crate::openclaw_config::OpenClawDefaultModel>, AppError>
{
    let model = cc_switch_core::openclaw_config::get_default_model().map_err(map_core_err)?;
    convert(model)
}

pub fn set_default_model(
    model: crate::openclaw_config::OpenClawDefaultModel,
) -> Result<(), AppError> {
    let model = convert(model)?;
    cc_switch_core::openclaw_config::set_default_model(&model).map_err(map_core_err)
}

pub fn get_model_catalog(
) -> Result<Option<HashMap<String, crate::openclaw_config::OpenClawModelCatalogEntry>>, AppError> {
    let catalog = cc_switch_core::openclaw_config::get_model_catalog().map_err(map_core_err)?;
    convert(catalog)
}

pub fn set_model_catalog(
    catalog: HashMap<String, crate::openclaw_config::OpenClawModelCatalogEntry>,
) -> Result<(), AppError> {
    let catalog = convert(catalog)?;
    cc_switch_core::openclaw_config::set_model_catalog(&catalog).map_err(map_core_err)
}

pub fn get_agents_defaults(
) -> Result<Option<crate::openclaw_config::OpenClawAgentsDefaults>, AppError> {
    let defaults = cc_switch_core::openclaw_config::get_agents_defaults().map_err(map_core_err)?;
    convert(defaults)
}

pub fn set_agents_defaults(
    defaults: crate::openclaw_config::OpenClawAgentsDefaults,
) -> Result<(), AppError> {
    let defaults = convert(defaults)?;
    cc_switch_core::openclaw_config::set_agents_defaults(&defaults).map_err(map_core_err)
}

pub fn get_env() -> Result<crate::openclaw_config::OpenClawEnvConfig, AppError> {
    let env = cc_switch_core::openclaw_config::get_env_config().map_err(map_core_err)?;
    convert(env)
}

pub fn get_env_as_core() -> Result<cc_switch_core::openclaw_config::OpenClawEnvConfig, AppError> {
    cc_switch_core::openclaw_config::get_env_config().map_err(map_core_err)
}

pub fn set_env_from_core(
    env: cc_switch_core::openclaw_config::OpenClawEnvConfig,
) -> Result<(), AppError> {
    cc_switch_core::openclaw_config::set_env_config(&env).map_err(map_core_err)
}

pub fn set_env(env: crate::openclaw_config::OpenClawEnvConfig) -> Result<(), AppError> {
    let env = convert(env)?;
    cc_switch_core::openclaw_config::set_env_config(&env).map_err(map_core_err)
}

pub fn get_tools() -> Result<crate::openclaw_config::OpenClawToolsConfig, AppError> {
    let tools = cc_switch_core::openclaw_config::get_tools_config().map_err(map_core_err)?;
    convert(tools)
}

pub fn set_tools(tools: crate::openclaw_config::OpenClawToolsConfig) -> Result<(), AppError> {
    let tools = convert(tools)?;
    cc_switch_core::openclaw_config::set_tools_config(&tools).map_err(map_core_err)
}

pub fn read_config_file() -> Result<String, AppError> {
    let path = cc_switch_core::openclaw_config::get_openclaw_config_path();
    std::fs::read_to_string(&path).map_err(|source| AppError::io(&path, source))
}

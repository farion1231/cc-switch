use crate::app_config::AppType;
use crate::error::{format_skill_error, AppError};
use crate::services::provider_security::{
    apply_selected_credentials, get_security_status, CredentialSource, MutationOutcome,
    ProviderMutationRequest, ProviderSecurityStatus, RecoveryMode, RecoveryResult,
};
use crate::services::ProviderService;
use crate::store::AppState;
use std::collections::BTreeSet;
use std::str::FromStr;
use tauri::State;

fn command_error(err: AppError) -> String {
    let message = err.to_string();
    for code in [
        "provider_revision_conflict",
        "provider_credentials_missing",
        "configuration_inconsistent",
        "live_projection_failed",
    ] {
        if message.contains(code) {
            return format_skill_error(code, &[], None);
        }
    }
    format_skill_error("provider_security_error", &[], None)
}

#[tauri::command]
pub fn get_provider_security_status(
    state: State<'_, AppState>,
    app: String,
    id: String,
) -> Result<ProviderSecurityStatus, String> {
    let app_type = AppType::from_str(&app).map_err(command_error)?;
    get_security_status(state.db.as_ref(), app_type, &id).map_err(command_error)
}

#[tauri::command]
pub async fn import_live_provider_credentials(
    state: State<'_, AppState>,
    app: String,
    id: String,
    #[allow(non_snake_case)] expectedRevision: i64,
    fields: Vec<String>,
) -> Result<MutationOutcome, String> {
    let app_type = AppType::from_str(&app).map_err(command_error)?;
    if matches!(app_type, AppType::ClaudeDesktop) {
        return Err(format_skill_error(
            "provider_credentials_missing",
            &[],
            None,
        ));
    }
    let confirmed_credential_fields: BTreeSet<String> = fields.into_iter().collect();
    let mut provider = state
        .db
        .get_provider_by_id(&id, app_type.as_str())
        .map_err(command_error)?
        .ok_or_else(|| format_skill_error("provider_credentials_missing", &[], None))?;
    let live_settings =
        ProviderService::read_live_settings(app_type.clone()).map_err(command_error)?;
    apply_selected_credentials(
        &mut provider,
        &live_settings,
        &app_type,
        &confirmed_credential_fields,
    )
    .map_err(command_error)?;

    state
        .provider_mutation_coordinator
        .mutate(ProviderMutationRequest {
            app_type,
            provider,
            expected_revision: expectedRevision,
            source: CredentialSource::ExplicitLiveImport,
            confirmed_credential_fields,
            skip_live_projection: false,
        })
        .await
        .map_err(command_error)
}

#[tauri::command]
pub async fn recover_app_configuration(
    state: State<'_, AppState>,
    app: String,
    mode: RecoveryMode,
) -> Result<RecoveryResult, String> {
    let app_type = AppType::from_str(&app).map_err(command_error)?;
    state
        .provider_mutation_coordinator
        .recover(app_type, mode)
        .await
        .map_err(command_error)
}

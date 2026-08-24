use serde::Serialize;
use serde_json::{json, Value};

use crate::app_config::AppType;
use crate::codex_config::{CodexCatalogToolProfile, CodexPaths, CodexTarget};
use crate::database::Database;
use crate::error::AppError;
use crate::provider::{CodexDesktopMode, Provider};

const GATEWAY_TOKEN_SETTING_KEY: &str = "codex_desktop_gateway_token";
const GATEWAY_PREFIX: &str = "/codex-desktop/v1";

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CodexDesktopStatus {
    pub configured: bool,
    pub directory_conflict: bool,
    pub cli_config_dir: String,
    pub desktop_config_dir: String,
    pub mode: Option<CodexDesktopMode>,
    pub expected_base_url: Option<String>,
    pub actual_base_url: Option<String>,
    pub proxy_running: bool,
    pub gateway_token_configured: bool,
}

pub fn get_or_create_gateway_token(db: &Database) -> Result<String, AppError> {
    if let Some(token) = db.get_setting(GATEWAY_TOKEN_SETTING_KEY)? {
        let token = token.trim();
        if !token.is_empty() {
            return Ok(token.to_string());
        }
    }

    let token = format!("ccs-{}", uuid::Uuid::new_v4().simple());
    db.set_setting(GATEWAY_TOKEN_SETTING_KEY, &token)?;
    Ok(token)
}

pub fn provider_mode(provider: &Provider) -> CodexDesktopMode {
    provider
        .meta
        .as_ref()
        .and_then(|meta| meta.codex_desktop_mode.clone())
        .unwrap_or_else(|| {
            if is_compatible_direct_provider(provider) {
                CodexDesktopMode::Direct
            } else {
                CodexDesktopMode::Proxy
            }
        })
}

pub fn is_compatible_direct_provider(provider: &Provider) -> bool {
    if provider.uses_managed_account_auth()
        || provider
            .meta
            .as_ref()
            .and_then(|meta| meta.is_full_url)
            .unwrap_or(false)
    {
        return false;
    }
    if provider.category.as_deref() == Some("official") {
        return true;
    }
    matches!(
        crate::proxy::providers::resolve_codex_catalog_tool_profile(provider),
        CodexCatalogToolProfile::NativeResponses
    )
}

pub fn validate_provider(provider: &Provider) -> Result<(), AppError> {
    if matches!(provider_mode(provider), CodexDesktopMode::Direct)
        && !is_compatible_direct_provider(provider)
    {
        return Err(AppError::localized(
            "codex_desktop.direct_unsupported",
            "该供应商需要协议转换或动态认证，请改用本地网关模式。",
            "This provider requires protocol conversion or dynamic authentication. Use local gateway mode.",
        ));
    }
    Ok(())
}

pub fn proxy_gateway_base_url_from_db(db: &Database) -> Result<String, AppError> {
    let config = futures::executor::block_on(db.get_proxy_config())?;
    if config.listen_port == 0 {
        return Err(AppError::Config(
            "Codex Desktop gateway requires a concrete listen port".to_string(),
        ));
    }
    let host = match config.listen_address.as_str() {
        "0.0.0.0" => "127.0.0.1".to_string(),
        "::" => "[::1]".to_string(),
        value if value.contains(':') && !value.starts_with('[') => format!("[{value}]"),
        value => value.to_string(),
    };
    Ok(format!(
        "http://{host}:{}{GATEWAY_PREFIX}",
        config.listen_port
    ))
}

pub fn apply_provider(db: &Database, provider: &Provider) -> Result<(), AppError> {
    crate::codex_config::ensure_codex_target_isolated(CodexTarget::Desktop)?;
    validate_provider(provider)?;

    let settings = provider.settings_config.as_object().ok_or_else(|| {
        AppError::Config("Codex Desktop provider configuration must be an object".to_string())
    })?;
    let auth = settings.get("auth").cloned().unwrap_or_else(|| json!({}));
    let config_text = settings.get("config").and_then(Value::as_str).unwrap_or("");

    match provider_mode(provider) {
        CodexDesktopMode::Direct => {
            let profile = crate::proxy::providers::resolve_codex_catalog_tool_profile(provider);
            crate::codex_config::write_codex_provider_live_with_catalog_for(
                CodexTarget::Desktop,
                &provider.settings_config,
                provider.category.as_deref(),
                &auth,
                Some(config_text),
                profile,
            )?;
            if provider
                .meta
                .as_ref()
                .and_then(|meta| meta.managed_account_id_for("codex_oauth"))
                .is_some()
            {
                crate::codex_config::record_codex_managed_oauth_live_auth_for(
                    CodexTarget::Desktop,
                    &auth,
                )?;
            }
        }
        CodexDesktopMode::Proxy => {
            let base_url = proxy_gateway_base_url_from_db(db)?;
            let token = get_or_create_gateway_token(db)?;
            let routed = crate::codex_config::apply_codex_desktop_gateway_route(
                config_text,
                &base_url,
                &token,
            )?;
            let routed = crate::codex_config::prepare_codex_config_text_with_model_catalog_for(
                CodexTarget::Desktop,
                &provider.settings_config,
                &routed,
                CodexCatalogToolProfile::ProxyChat,
            )?;
            crate::codex_config::write_codex_live_atomic_for(
                CodexTarget::Desktop,
                &json!({ "OPENAI_API_KEY": token }),
                Some(&routed),
            )?;
            crate::config::delete_file(
                &CodexPaths::for_target(CodexTarget::Desktop).managed_oauth_marker,
            )?;
        }
    }
    Ok(())
}

pub fn get_status(db: &Database, proxy_running: bool) -> Result<CodexDesktopStatus, AppError> {
    let cli_paths = CodexPaths::for_target(CodexTarget::Cli);
    let desktop_paths = CodexPaths::for_target(CodexTarget::Desktop);
    let directory_conflict = crate::codex_config::codex_config_dirs_conflict();
    let config_text = std::fs::read_to_string(&desktop_paths.config).unwrap_or_default();
    let configured = desktop_paths.auth.exists() || desktop_paths.config.exists();
    let actual_base_url = crate::codex_config::extract_codex_base_url(&config_text);
    let current = crate::settings::get_effective_current_provider(db, &AppType::CodexDesktop)?
        .and_then(|id| {
            db.get_provider_by_id(&id, AppType::CodexDesktop.as_str())
                .ok()
                .flatten()
        });
    let mode = current.as_ref().map(provider_mode);
    let expected_base_url = match mode {
        Some(CodexDesktopMode::Proxy) => proxy_gateway_base_url_from_db(db).ok(),
        Some(CodexDesktopMode::Direct) => current
            .as_ref()
            .and_then(|provider| {
                provider
                    .settings_config
                    .get("config")
                    .and_then(Value::as_str)
            })
            .and_then(crate::codex_config::extract_codex_base_url),
        None => None,
    };
    let gateway_token_configured = db
        .get_setting(GATEWAY_TOKEN_SETTING_KEY)?
        .is_some_and(|token| !token.trim().is_empty());

    Ok(CodexDesktopStatus {
        configured,
        directory_conflict,
        cli_config_dir: cli_paths.root.to_string_lossy().to_string(),
        desktop_config_dir: desktop_paths.root.to_string_lossy().to_string(),
        mode,
        expected_base_url,
        actual_base_url,
        proxy_running,
        gateway_token_configured,
    })
}

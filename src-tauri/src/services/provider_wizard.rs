//! Safe provider onboarding primitives.
//!
//! This module only probes and reports capabilities. It never persists a
//! provider or writes an IDE configuration file.

use crate::proxy::http_client;
use crate::services::model_fetch::build_models_url_candidates;
use crate::{app_config::AppType, codex_config, config};
use crate::{provider::Provider, provider::ProviderMeta, store::AppState};
use once_cell::sync::Lazy;
use reqwest::header::{HeaderMap, HeaderName, HeaderValue, AUTHORIZATION};
use reqwest::StatusCode;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::time::Duration;
use tokio::sync::Mutex;
use url::Url;

const PROBE_TIMEOUT: Duration = Duration::from_secs(15);
const MAX_MODELS_BODY_BYTES: usize = 2 * 1024 * 1024;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum UpstreamProtocol {
    AnthropicMessages,
    OpenAiChat,
    OpenAiResponses,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AuthMode {
    Bearer,
    XApiKey,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum UrlMode {
    Base,
    FullEndpoint,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ProbeConfidence {
    High,
    Medium,
    Low,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ProviderProbeInput {
    pub base_url: String,
    pub api_key: String,
    pub models_url: Option<String>,
    pub model: Option<String>,
    pub allow_inference_probe: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DetectedModel {
    pub id: String,
    pub owned_by: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ProtocolCapability {
    pub protocol: UpstreamProtocol,
    pub endpoint: String,
    pub auth_mode: AuthMode,
    pub supported: bool,
    pub confidence: ProbeConfidence,
    pub evidence: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ProviderProbeResult {
    pub normalized_base_url: String,
    pub url_mode: UrlMode,
    pub models: Vec<DetectedModel>,
    pub capabilities: Vec<ProtocolCapability>,
    pub recommended_model: Option<String>,
    pub warnings: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ProviderInstallSelection {
    pub name: String,
    pub base_url: String,
    pub api_key: String,
    pub model: String,
    pub claude_protocol: Option<UpstreamProtocol>,
    pub codex_protocol: Option<UpstreamProtocol>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AppInstallPreview {
    pub app: AppType,
    pub provider_id: String,
    pub protocol: UpstreamProtocol,
    pub mode: String,
    pub model: String,
    pub files_to_change: Vec<String>,
    pub restart_required: bool,
    pub redacted_config: Value,
    pub warnings: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ProviderInstallPreview {
    pub provider_id: String,
    pub normalized_base_url: String,
    pub url_mode: UrlMode,
    pub claude: Option<AppInstallPreview>,
    pub codex: Option<AppInstallPreview>,
    pub proxy_will_start: bool,
    pub warnings: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ApplyProviderInstallResult {
    pub applied_apps: Vec<AppType>,
    pub rolled_back: bool,
    pub rollback_errors: Vec<String>,
    pub restart_required_apps: Vec<AppType>,
}

static INSTALL_LOCK: Lazy<Mutex<()>> = Lazy::new(|| Mutex::new(()));

#[derive(Debug, Clone, Copy)]
struct EndpointCandidate {
    protocol: UpstreamProtocol,
    path: &'static str,
    preferred_auth: AuthMode,
}

const ENDPOINTS: [EndpointCandidate; 3] = [
    EndpointCandidate {
        protocol: UpstreamProtocol::AnthropicMessages,
        path: "/v1/messages",
        preferred_auth: AuthMode::XApiKey,
    },
    EndpointCandidate {
        protocol: UpstreamProtocol::OpenAiResponses,
        path: "/v1/responses",
        preferred_auth: AuthMode::Bearer,
    },
    EndpointCandidate {
        protocol: UpstreamProtocol::OpenAiChat,
        path: "/v1/chat/completions",
        preferred_auth: AuthMode::Bearer,
    },
];

pub async fn probe_provider_capabilities(
    input: ProviderProbeInput,
) -> Result<ProviderProbeResult, String> {
    if input.api_key.trim().is_empty() {
        return Err("API key is required".to_string());
    }

    let (normalized_base_url, url_mode, detected_protocol) = normalize_base_url(&input.base_url)?;
    let models = fetch_models_for_probe(
        &normalized_base_url,
        url_mode,
        input.models_url.as_deref(),
        &input.api_key,
    )
    .await?;

    let selected_model = input
        .model
        .filter(|model| !model.trim().is_empty())
        .or_else(|| models.first().map(|model| model.id.clone()));

    let mut warnings = Vec::new();
    if selected_model.is_none() {
        warnings.push("No model was discovered; enter a model manually.".to_string());
    }

    if !input.allow_inference_probe {
        warnings.push(
            "Inference probing was not authorized; protocol capabilities were not tested."
                .to_string(),
        );
        return Ok(ProviderProbeResult {
            normalized_base_url,
            url_mode,
            models,
            capabilities: detected_protocol
                .map(|candidate| ProtocolCapability {
                    protocol: candidate.protocol,
                    endpoint: candidate.path.to_string(),
                    auth_mode: candidate.preferred_auth,
                    supported: false,
                    confidence: ProbeConfidence::Low,
                    evidence: vec!["Endpoint inferred from the supplied URL.".to_string()],
                })
                .into_iter()
                .collect(),
            recommended_model: selected_model,
            warnings,
        });
    }

    let Some(model) = selected_model.clone() else {
        return Err("A model is required before inference probing".to_string());
    };

    let client = http_client::get();
    let mut capabilities = Vec::new();
    for candidate in ENDPOINTS {
        if url_mode == UrlMode::FullEndpoint
            && detected_protocol.is_some_and(|detected| detected.protocol != candidate.protocol)
        {
            continue;
        }

        let endpoint = endpoint_url(&normalized_base_url, url_mode, candidate.path)?;
        let capability =
            probe_endpoint(&client, &endpoint, candidate, &input.api_key, &model).await;
        capabilities.push(capability);
    }

    if capabilities.iter().all(|capability| !capability.supported) {
        warnings.push("No protocol probe succeeded; review URL, auth mode, and model.".to_string());
    }

    Ok(ProviderProbeResult {
        normalized_base_url,
        url_mode,
        models,
        capabilities,
        recommended_model: Some(model),
        warnings,
    })
}

pub fn preview_provider_install(
    selection: ProviderInstallSelection,
) -> Result<ProviderInstallPreview, String> {
    let name = selection.name.trim();
    if name.is_empty() {
        return Err("Provider name is required".to_string());
    }
    if selection.api_key.trim().is_empty() {
        return Err("API key is required".to_string());
    }
    let model = selection.model.trim();
    if model.is_empty() {
        return Err("Model is required".to_string());
    }

    let (normalized_base_url, url_mode, _) = normalize_base_url(&selection.base_url)?;
    let provider_id = wizard_provider_id(name, &normalized_base_url);
    let mut warnings = Vec::new();
    let claude = selection
        .claude_protocol
        .map(|protocol| build_claude_preview(&selection, &provider_id, protocol, model));
    let codex = selection
        .codex_protocol
        .map(|protocol| build_codex_preview(&selection, &provider_id, protocol, model));

    if claude.is_none() && codex.is_none() {
        warnings.push("Select at least one application to configure.".to_string());
    }
    let proxy_will_start = claude
        .as_ref()
        .is_some_and(|preview| preview.mode == "proxy")
        || codex
            .as_ref()
            .is_some_and(|preview| preview.mode == "proxy");

    Ok(ProviderInstallPreview {
        provider_id,
        normalized_base_url,
        url_mode,
        claude,
        codex,
        proxy_will_start,
        warnings,
    })
}

pub async fn apply_provider_install(
    state: &AppState,
    selection: ProviderInstallSelection,
) -> Result<ApplyProviderInstallResult, String> {
    let _lock = INSTALL_LOCK.lock().await;
    let preview = preview_provider_install(selection.clone())?;
    let mut snapshots = Vec::new();
    let mut applied_apps = Vec::new();
    let mut restart_required_apps = Vec::new();

    for (app, protocol) in [
        (AppType::Claude, selection.claude_protocol),
        (AppType::Codex, selection.codex_protocol),
    ] {
        let Some(protocol) = protocol else {
            continue;
        };
        let provider =
            build_provider_for_app(&selection, &preview.provider_id, app.clone(), protocol)?;
        let previous_provider = state
            .db
            .get_provider_by_id(&provider.id, app.as_str())
            .map_err(|error| error.to_string())?;
        let previous_current = crate::settings::get_current_provider(&app);
        let previous_takeover = state
            .db
            .get_proxy_config_for_app(app.as_str())
            .await
            .map_err(|error| error.to_string())?
            .enabled;
        snapshots.push(InstallSnapshot {
            app: app.clone(),
            provider_id: provider.id.clone(),
            previous_provider,
            previous_current,
            previous_takeover,
        });

        let app_proxy = match app {
            AppType::Claude => preview
                .claude
                .as_ref()
                .is_some_and(|item| item.mode == "proxy"),
            AppType::Codex => preview
                .codex
                .as_ref()
                .is_some_and(|item| item.mode == "proxy"),
            _ => false,
        };
        if let Err(error) = apply_one_app(state, app.clone(), provider, app_proxy).await {
            let rollback_errors = rollback_install(state, &snapshots).await;
            return Err(format_apply_error(error, rollback_errors));
        }
        applied_apps.push(app.clone());
        if app == AppType::Codex || app_proxy {
            restart_required_apps.push(app);
        }
    }

    Ok(ApplyProviderInstallResult {
        applied_apps,
        rolled_back: false,
        rollback_errors: Vec::new(),
        restart_required_apps,
    })
}

#[derive(Debug)]
struct InstallSnapshot {
    app: AppType,
    provider_id: String,
    previous_provider: Option<Provider>,
    previous_current: Option<String>,
    previous_takeover: bool,
}

async fn apply_one_app(
    state: &AppState,
    app: AppType,
    provider: Provider,
    proxy_will_start: bool,
) -> Result<(), String> {
    crate::services::provider::ProviderService::add(state, app.clone(), provider.clone(), true)
        .map_err(|error| error.to_string())?;

    if proxy_will_start && app.supports_local_proxy() {
        state
            .proxy_service
            .set_takeover_for_app(app.as_str(), true)
            .await?;
    }

    crate::services::provider::ProviderService::switch(state, app, &provider.id)
        .map(|_| ())
        .map_err(|error| error.to_string())
}

async fn rollback_install(state: &AppState, snapshots: &[InstallSnapshot]) -> Vec<String> {
    let mut errors = Vec::new();
    for snapshot in snapshots.iter().rev() {
        if let Err(error) = rollback_one_app(state, snapshot).await {
            errors.push(format!("{}: {error}", snapshot.app.as_str()));
        }
    }
    errors
}

async fn rollback_one_app(state: &AppState, snapshot: &InstallSnapshot) -> Result<(), String> {
    if snapshot.previous_takeover {
        state
            .proxy_service
            .set_takeover_for_app(snapshot.app.as_str(), true)
            .await?;
    } else if snapshot.app.supports_local_proxy() {
        state
            .proxy_service
            .set_takeover_for_app(snapshot.app.as_str(), false)
            .await?;
    }

    if let Some(previous_provider) = &snapshot.previous_provider {
        state
            .db
            .save_provider(snapshot.app.as_str(), previous_provider)
            .map_err(|error| error.to_string())?;
        if let Some(previous_current) = &snapshot.previous_current {
            crate::settings::set_current_provider(&snapshot.app, Some(previous_current))
                .map_err(|error| error.to_string())?;
            state
                .db
                .set_current_provider(snapshot.app.as_str(), previous_current)
                .map_err(|error| error.to_string())?;
            crate::services::provider::ProviderService::switch(
                state,
                snapshot.app.clone(),
                previous_current,
            )
            .map_err(|error| error.to_string())?;
        }
    } else {
        state
            .db
            .delete_provider(snapshot.app.as_str(), &snapshot.provider_id)
            .map_err(|error| error.to_string())?;
        crate::settings::set_current_provider(&snapshot.app, None)
            .map_err(|error| error.to_string())?;
    }
    Ok(())
}

fn format_apply_error(error: String, rollback_errors: Vec<String>) -> String {
    if rollback_errors.is_empty() {
        format!("Provider setup failed and was rolled back: {error}")
    } else {
        format!(
            "Provider setup failed: {error}; rollback also failed: {}",
            rollback_errors.join("; ")
        )
    }
}

fn build_provider_for_app(
    selection: &ProviderInstallSelection,
    provider_id: &str,
    app: AppType,
    protocol: UpstreamProtocol,
) -> Result<Provider, String> {
    let (base_url, url_mode, _) = normalize_base_url(&selection.base_url)?;
    let model = selection.model.trim();
    let mut provider = match app {
        AppType::Claude => {
            let auth_field = if protocol == UpstreamProtocol::AnthropicMessages {
                "ANTHROPIC_API_KEY"
            } else {
                "ANTHROPIC_AUTH_TOKEN"
            };
            let settings = serde_json::json!({
                "env": {
                    "ANTHROPIC_BASE_URL": base_url,
                    auth_field: selection.api_key.clone(),
                    "ANTHROPIC_MODEL": model,
                }
            });
            Provider::with_id(
                provider_id.to_string(),
                selection.name.clone(),
                settings,
                None,
            )
        }
        AppType::Codex => {
            let config_text = format!(
                "model_provider = \"{provider_id}\"\nmodel = \"{model}\"\n\n[model_providers.{provider_id}]\nname = \"{}\"\nbase_url = \"{base_url}\"\nwire_api = \"responses\"\nrequires_openai_auth = true\n",
                selection.name.trim()
            );
            let settings = serde_json::json!({
                "auth": {"OPENAI_API_KEY": selection.api_key.clone()},
                "config": config_text,
            });
            Provider::with_id(
                provider_id.to_string(),
                selection.name.clone(),
                settings,
                None,
            )
        }
        _ => return Err(format!("Unsupported wizard app: {}", app.as_str())),
    };

    let mut meta = ProviderMeta {
        api_format: Some(protocol_string(protocol).to_string()),
        is_full_url: (url_mode == UrlMode::FullEndpoint).then_some(true),
        ..ProviderMeta::default()
    };
    if app == AppType::Claude {
        meta.api_key_field = Some(if protocol == UpstreamProtocol::AnthropicMessages {
            "ANTHROPIC_API_KEY".to_string()
        } else {
            "ANTHROPIC_AUTH_TOKEN".to_string()
        });
    } else if protocol == UpstreamProtocol::AnthropicMessages {
        meta.api_key_field = Some("ANTHROPIC_API_KEY".to_string());
        meta.impersonate_claude_code = Some(false);
    }
    provider.meta = Some(meta);
    Ok(provider)
}

fn protocol_string(protocol: UpstreamProtocol) -> &'static str {
    match protocol {
        UpstreamProtocol::AnthropicMessages => "anthropic",
        UpstreamProtocol::OpenAiChat => "openai_chat",
        UpstreamProtocol::OpenAiResponses => "openai_responses",
    }
}

fn build_claude_preview(
    selection: &ProviderInstallSelection,
    provider_id: &str,
    protocol: UpstreamProtocol,
    model: &str,
) -> AppInstallPreview {
    let proxy = protocol != UpstreamProtocol::AnthropicMessages;
    let auth_field = if protocol == UpstreamProtocol::AnthropicMessages {
        "ANTHROPIC_API_KEY"
    } else {
        "ANTHROPIC_AUTH_TOKEN"
    };
    let redacted_config = serde_json::json!({
        "env": {
            "ANTHROPIC_BASE_URL": selection.base_url.trim().trim_end_matches('/'),
            auth_field: redact_secret(&selection.api_key),
            "ANTHROPIC_MODEL": model,
        },
        "meta": {
            "apiFormat": protocol,
            "apiKeyField": auth_field,
        }
    });
    let mut warnings = Vec::new();
    if proxy {
        warnings
            .push("Claude Code must use CC Switch local routing for this protocol.".to_string());
    }
    AppInstallPreview {
        app: AppType::Claude,
        provider_id: provider_id.to_string(),
        protocol,
        mode: if proxy { "proxy" } else { "direct" }.to_string(),
        model: model.to_string(),
        files_to_change: vec![config::get_claude_settings_path().display().to_string()],
        restart_required: proxy,
        redacted_config,
        warnings,
    }
}

fn build_codex_preview(
    selection: &ProviderInstallSelection,
    provider_id: &str,
    protocol: UpstreamProtocol,
    model: &str,
) -> AppInstallPreview {
    let proxy = protocol != UpstreamProtocol::OpenAiResponses;
    let base_url = selection.base_url.trim().trim_end_matches('/');
    let config_text = format!(
        "model_provider = \"{provider_id}\"\nmodel = \"{model}\"\n\n[model_providers.{provider_id}]\nname = \"{}\"\nbase_url = \"{base_url}\"\nwire_api = \"responses\"\nrequires_openai_auth = true\n",
        selection.name.trim()
    );
    let redacted_config = serde_json::json!({
        "auth": {"OPENAI_API_KEY": redact_secret(&selection.api_key)},
        "config": config_text,
        "meta": {"apiFormat": protocol}
    });
    let mut warnings = Vec::new();
    if proxy {
        warnings.push("Codex must use CC Switch local routing for this protocol.".to_string());
    }
    AppInstallPreview {
        app: AppType::Codex,
        provider_id: provider_id.to_string(),
        protocol,
        mode: if proxy { "proxy" } else { "direct" }.to_string(),
        model: model.to_string(),
        files_to_change: vec![
            codex_config::get_codex_auth_path().display().to_string(),
            codex_config::get_codex_config_path().display().to_string(),
        ],
        restart_required: true,
        redacted_config,
        warnings,
    }
}

fn wizard_provider_id(name: &str, base_url: &str) -> String {
    use sha2::{Digest, Sha256};

    let slug = name
        .chars()
        .filter(|character| character.is_ascii_alphanumeric() || *character == '-')
        .collect::<String>()
        .to_ascii_lowercase();
    let mut hasher = Sha256::new();
    hasher.update(name.as_bytes());
    hasher.update([0]);
    hasher.update(base_url.as_bytes());
    let digest = hasher.finalize();
    let suffix = digest[..4]
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect::<String>();
    format!(
        "wizard-{}-{}",
        if slug.is_empty() { "provider" } else { &slug },
        suffix
    )
}

fn redact_secret(secret: &str) -> String {
    let secret = secret.trim();
    if secret.len() <= 8 {
        return "********".to_string();
    }
    format!("{}...{}", &secret[..3], &secret[secret.len() - 4..])
}

fn normalize_base_url(raw: &str) -> Result<(String, UrlMode, Option<EndpointCandidate>), String> {
    let trimmed = raw.trim().trim_end_matches('/');
    if trimmed.is_empty() {
        return Err("Base URL is required".to_string());
    }

    let parsed = Url::parse(trimmed).map_err(|error| format!("Invalid base URL: {error}"))?;
    if !matches!(parsed.scheme(), "https" | "http") {
        return Err("Base URL must use http or https".to_string());
    }
    if parsed.username() != "" || parsed.password().is_some() {
        return Err("Base URL must not contain credentials".to_string());
    }
    if parsed.fragment().is_some() {
        return Err("Base URL must not contain a fragment".to_string());
    }
    if parsed.scheme() == "http"
        && !matches!(parsed.host_str(), Some("localhost" | "127.0.0.1" | "::1"))
    {
        return Err("HTTP is only allowed for localhost during provider setup".to_string());
    }

    let detected = ENDPOINTS.iter().copied().find(|candidate| {
        parsed
            .path()
            .trim_end_matches('/')
            .ends_with(candidate.path)
    });
    let mode = if detected.is_some() {
        UrlMode::FullEndpoint
    } else {
        UrlMode::Base
    };
    Ok((trimmed.to_string(), mode, detected))
}

fn endpoint_url(base_url: &str, mode: UrlMode, path: &str) -> Result<String, String> {
    if mode == UrlMode::FullEndpoint {
        return Ok(base_url.to_string());
    }

    let mut base = Url::parse(base_url).map_err(|error| format!("Invalid base URL: {error}"))?;
    let current_path = base.path().trim_end_matches('/');
    let joined_path = if current_path.ends_with("/v1") {
        format!("{current_path}{}", path.strip_prefix("/v1").unwrap_or(path))
    } else {
        format!("{current_path}/v1{path}")
    };
    base.set_path(&joined_path);
    Ok(base.to_string().trim_end_matches('/').to_string())
}

async fn fetch_models_for_probe(
    base_url: &str,
    url_mode: UrlMode,
    models_url_override: Option<&str>,
    api_key: &str,
) -> Result<Vec<DetectedModel>, String> {
    let candidates = build_models_url_candidates(
        base_url,
        url_mode == UrlMode::FullEndpoint,
        models_url_override,
    )?;
    let client = http_client::get();
    let mut last_status = None;

    for url in candidates {
        for auth_mode in [AuthMode::Bearer, AuthMode::XApiKey] {
            let response = client
                .get(&url)
                .headers(auth_headers(api_key, auth_mode)?)
                .timeout(PROBE_TIMEOUT)
                .send()
                .await
                .map_err(|error| format!("Model discovery request failed: {error}"))?;
            last_status = Some(response.status());
            if !response.status().is_success() {
                continue;
            }
            let body = response
                .bytes()
                .await
                .map_err(|error| format!("Model discovery response failed: {error}"))?;
            if body.len() > MAX_MODELS_BODY_BYTES {
                return Err("Model discovery response is too large".to_string());
            }
            let value: Value = serde_json::from_slice(&body)
                .map_err(|error| format!("Model discovery response is invalid JSON: {error}"))?;
            return Ok(value
                .get("data")
                .and_then(Value::as_array)
                .into_iter()
                .flatten()
                .filter_map(|model| {
                    Some(DetectedModel {
                        id: model.get("id")?.as_str()?.to_string(),
                        owned_by: model
                            .get("owned_by")
                            .and_then(Value::as_str)
                            .map(str::to_string),
                    })
                })
                .collect());
        }
    }

    Err(format!(
        "Model discovery failed{}",
        last_status
            .map(|status| format!(" with HTTP {status}"))
            .unwrap_or_default()
    ))
}

async fn probe_endpoint(
    client: &reqwest::Client,
    endpoint: &str,
    candidate: EndpointCandidate,
    api_key: &str,
    model: &str,
) -> ProtocolCapability {
    let auth_modes = [
        candidate.preferred_auth,
        alternate_auth(candidate.preferred_auth),
    ];
    let mut evidence = Vec::new();

    for auth_mode in auth_modes {
        let body = match candidate.protocol {
            UpstreamProtocol::AnthropicMessages | UpstreamProtocol::OpenAiChat => {
                serde_json::json!({
                    "model": model,
                    "max_tokens": 1,
                    "stream": false,
                    "messages": [{"role": "user", "content": "ping"}]
                })
            }
            UpstreamProtocol::OpenAiResponses => serde_json::json!({
                "model": model,
                "max_output_tokens": 1,
                "stream": false,
                "input": "ping"
            }),
        };
        let request = client
            .post(endpoint)
            .headers(match auth_headers(api_key, auth_mode) {
                Ok(headers) => headers,
                Err(error) => {
                    evidence.push(error);
                    continue;
                }
            })
            .header("content-type", "application/json")
            .json(&body)
            .timeout(PROBE_TIMEOUT);

        let response = match request.send().await {
            Ok(response) => response,
            Err(error) => {
                evidence.push(format!("{auth_mode:?}: network error: {error}"));
                continue;
            }
        };
        let status = response.status();
        if status.is_success() {
            evidence.push(format!("{auth_mode:?}: HTTP {status}"));
            return ProtocolCapability {
                protocol: candidate.protocol,
                endpoint: endpoint.to_string(),
                auth_mode,
                supported: true,
                confidence: ProbeConfidence::High,
                evidence,
            };
        }
        if matches!(
            status,
            StatusCode::BAD_REQUEST | StatusCode::UNPROCESSABLE_ENTITY
        ) || status == StatusCode::TOO_MANY_REQUESTS
        {
            evidence.push(format!(
                "{auth_mode:?}: HTTP {status}, endpoint accepted request"
            ));
            return ProtocolCapability {
                protocol: candidate.protocol,
                endpoint: endpoint.to_string(),
                auth_mode,
                supported: true,
                confidence: ProbeConfidence::Medium,
                evidence,
            };
        }
        evidence.push(format!("{auth_mode:?}: HTTP {status}"));
    }

    ProtocolCapability {
        protocol: candidate.protocol,
        endpoint: endpoint.to_string(),
        auth_mode: candidate.preferred_auth,
        supported: false,
        confidence: ProbeConfidence::Low,
        evidence,
    }
}

fn auth_headers(api_key: &str, auth_mode: AuthMode) -> Result<HeaderMap, String> {
    let mut headers = HeaderMap::new();
    let value = HeaderValue::from_str(api_key)
        .map_err(|error| format!("Invalid API key header value: {error}"))?;
    match auth_mode {
        AuthMode::Bearer => {
            let bearer = HeaderValue::from_str(&format!("Bearer {api_key}"))
                .map_err(|error| format!("Invalid bearer header value: {error}"))?;
            headers.insert(AUTHORIZATION, bearer);
        }
        AuthMode::XApiKey => {
            headers.insert(HeaderName::from_static("x-api-key"), value);
            headers.insert(
                HeaderName::from_static("anthropic-version"),
                HeaderValue::from_static("2023-06-01"),
            );
        }
    }
    Ok(headers)
}

fn alternate_auth(auth_mode: AuthMode) -> AuthMode {
    match auth_mode {
        AuthMode::Bearer => AuthMode::XApiKey,
        AuthMode::XApiKey => AuthMode::Bearer,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::{
        routing::{get, post},
        Json, Router,
    };
    use serde_json::json;
    use serial_test::serial;
    use std::env;
    use tempfile::TempDir;
    use tokio::sync::oneshot;

    struct TempHome {
        _dir: TempDir,
        home: Option<String>,
        userprofile: Option<String>,
        test_home: Option<String>,
    }

    impl TempHome {
        fn new() -> Self {
            let dir = TempDir::new().expect("create temp home");
            let home = env::var("HOME").ok();
            let userprofile = env::var("USERPROFILE").ok();
            let test_home = env::var("CC_SWITCH_TEST_HOME").ok();
            env::set_var("HOME", dir.path());
            env::set_var("USERPROFILE", dir.path());
            env::set_var("CC_SWITCH_TEST_HOME", dir.path());
            crate::settings::reload_settings().expect("reload settings");
            Self {
                _dir: dir,
                home,
                userprofile,
                test_home,
            }
        }
    }

    impl Drop for TempHome {
        fn drop(&mut self) {
            match &self.home {
                Some(value) => env::set_var("HOME", value),
                None => env::remove_var("HOME"),
            }
            match &self.userprofile {
                Some(value) => env::set_var("USERPROFILE", value),
                None => env::remove_var("USERPROFILE"),
            }
            match &self.test_home {
                Some(value) => env::set_var("CC_SWITCH_TEST_HOME", value),
                None => env::remove_var("CC_SWITCH_TEST_HOME"),
            }
        }
    }

    #[test]
    fn normalizes_supported_full_endpoints() {
        let (_, mode, protocol) = normalize_base_url("https://api.example/v1/responses/").unwrap();
        assert_eq!(mode, UrlMode::FullEndpoint);
        assert_eq!(
            protocol.unwrap().protocol,
            UpstreamProtocol::OpenAiResponses
        );
    }

    #[test]
    fn rejects_remote_http_and_embedded_credentials() {
        assert!(normalize_base_url("http://api.example/v1").is_err());
        assert!(normalize_base_url("https://user:pass@api.example/v1").is_err());
        assert!(normalize_base_url("https://api.example/v1#fragment").is_err());
    }

    #[test]
    fn appends_v1_without_duplicating_it() {
        assert_eq!(
            endpoint_url("https://api.example", UrlMode::Base, "/responses").unwrap(),
            "https://api.example/v1/responses"
        );
        assert_eq!(
            endpoint_url("https://api.example/v1", UrlMode::Base, "/responses").unwrap(),
            "https://api.example/v1/responses"
        );
    }

    #[tokio::test]
    async fn probes_models_and_all_supported_protocols_without_exposing_key() {
        async fn models() -> Json<Value> {
            Json(json!({
                "data": [{"id": "test-model", "owned_by": "test"}]
            }))
        }

        async fn inference() -> Json<Value> {
            Json(json!({"id": "probe-response"}))
        }

        let app = Router::new()
            .route("/v1/models", get(models))
            .route("/v1/messages", post(inference))
            .route("/v1/chat/completions", post(inference))
            .route("/v1/responses", post(inference));
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
            .await
            .expect("bind probe server");
        let address = listener.local_addr().expect("probe server address");
        let (shutdown_tx, shutdown_rx) = oneshot::channel();
        let server = tokio::spawn(async move {
            axum::serve(listener, app)
                .with_graceful_shutdown(async {
                    let _ = shutdown_rx.await;
                })
                .await
                .expect("serve probe server");
        });

        let result = probe_provider_capabilities(ProviderProbeInput {
            base_url: format!("http://{address}/v1"),
            api_key: "secret-probe-key".to_string(),
            models_url: None,
            model: None,
            allow_inference_probe: true,
        })
        .await
        .expect("probe provider");

        assert_eq!(result.recommended_model.as_deref(), Some("test-model"));
        assert_eq!(result.capabilities.len(), 3);
        assert!(result
            .capabilities
            .iter()
            .all(|capability| capability.supported));
        assert_eq!(
            result
                .capabilities
                .iter()
                .find(|capability| capability.protocol == UpstreamProtocol::AnthropicMessages)
                .expect("anthropic capability")
                .auth_mode,
            AuthMode::XApiKey
        );
        assert!(result
            .capabilities
            .iter()
            .filter(|capability| capability.protocol != UpstreamProtocol::AnthropicMessages)
            .all(|capability| capability.auth_mode == AuthMode::Bearer));
        assert!(!serde_json::to_string(&result)
            .expect("serialize result")
            .contains("secret-probe-key"));

        let _ = shutdown_tx.send(());
        server.await.expect("join probe server");
    }

    #[test]
    fn preview_generates_redacted_configs_and_routing_requirements() {
        let preview = preview_provider_install(ProviderInstallSelection {
            name: "Example Gateway".to_string(),
            base_url: "https://gateway.example/v1".to_string(),
            api_key: "sk-secret-value-1234".to_string(),
            model: "provider-model".to_string(),
            claude_protocol: Some(UpstreamProtocol::OpenAiChat),
            codex_protocol: Some(UpstreamProtocol::OpenAiResponses),
        })
        .expect("build preview");

        let serialized = serde_json::to_string(&preview).expect("serialize preview");
        assert!(!serialized.contains("sk-secret-value-1234"));
        assert!(preview.proxy_will_start);
        assert_eq!(preview.claude.as_ref().unwrap().mode, "proxy");
        assert_eq!(preview.codex.as_ref().unwrap().mode, "direct");
        assert!(preview
            .claude
            .as_ref()
            .unwrap()
            .redacted_config
            .to_string()
            .contains("sk-...1234"));
    }

    #[tokio::test]
    #[serial]
    async fn apply_installs_claude_and_codex_for_native_protocols() {
        let _home = TempHome::new();
        let db = std::sync::Arc::new(crate::database::Database::memory().expect("create db"));
        let state = crate::store::AppState::new(db.clone());
        let selection = ProviderInstallSelection {
            name: "Example Gateway".to_string(),
            base_url: "https://gateway.example/v1".to_string(),
            api_key: "sk-secret-value-1234".to_string(),
            model: "provider-model".to_string(),
            claude_protocol: Some(UpstreamProtocol::AnthropicMessages),
            codex_protocol: Some(UpstreamProtocol::OpenAiResponses),
        };

        let result = apply_provider_install(&state, selection)
            .await
            .expect("apply setup");
        assert_eq!(result.applied_apps, vec![AppType::Claude, AppType::Codex]);
        assert!(!result.rolled_back);

        let provider_id = wizard_provider_id("Example Gateway", "https://gateway.example/v1");
        assert!(db
            .get_provider_by_id(&provider_id, "claude")
            .expect("read Claude provider")
            .is_some());
        assert!(db
            .get_provider_by_id(&provider_id, "codex")
            .expect("read Codex provider")
            .is_some());
        assert_eq!(
            crate::settings::get_effective_current_provider(&db, &AppType::Claude)
                .expect("Claude current"),
            Some(provider_id.clone())
        );
        assert_eq!(
            crate::settings::get_effective_current_provider(&db, &AppType::Codex)
                .expect("Codex current"),
            Some(provider_id)
        );
    }
}

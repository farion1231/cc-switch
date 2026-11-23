//! Portable CC Switch profiles. This protocol intentionally does not transfer
//! live client files, remote-account sessions or database backups.

use std::collections::{BTreeMap, BTreeSet};
use std::io::{self, Read, Write};

use serde::{de::DeserializeOwned, Deserialize, Serialize};
use serde_json::{json, Map, Value};

use crate::app_config::AppType;
use crate::database::Database;
use crate::error::AppError;
use crate::live::{engine::DeviceStore, project::claude::ClaudeProjection};
use crate::mode::state::{AppLiveState, LiveState};
use crate::provider::{ClaudeDesktopMode, Provider, ProviderMeta};
use crate::proxy::model_mapper::strip_one_m_suffix_for_upstream;
use crate::services::ProviderService;

const VERSION: u32 = 1;
const SOURCE: &str = "cc-switch";
const MAX_BYTES: usize = 1024 * 1024;
const MAX_PROFILES: usize = 50;
const SECRET_FIELDS: &[&str] = &[
    "ANTHROPIC_AUTH_TOKEN",
    "ANTHROPIC_API_KEY",
    "OPENROUTER_API_KEY",
    "OPENAI_API_KEY",
    "GEMINI_API_KEY",
    "apiKey",
    "api_key",
];
const URL_FIELDS: &[&str] = &[
    "ANTHROPIC_BASE_URL",
    "apiBaseUrl",
    "base_url",
    "baseURL",
    "apiEndpoint",
];
const DESKTOP_FIELDS: &[&str] = &[
    "base_url",
    "baseURL",
    "apiEndpoint",
    "apiKey",
    "api_key",
    "model",
];

#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Deserialize, Serialize)]
#[serde(rename_all = "kebab-case")]
enum PortableApp {
    Claude,
    ClaudeDesktop,
}

impl PortableApp {
    fn app(self) -> AppType {
        match self {
            Self::Claude => AppType::Claude,
            Self::ClaudeDesktop => AppType::ClaudeDesktop,
        }
    }
}

#[derive(Clone, Copy, PartialEq, Eq, Deserialize, Serialize)]
#[serde(rename_all = "lowercase")]
enum Kind {
    Direct,
    Routing,
    Aggregation,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct ExportRequest {
    profiles: Vec<Selection>,
    #[serde(default)]
    include_credentials: bool,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Selection {
    app: PortableApp,
    id: String,
}

#[derive(Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct Bundle {
    version: u32,
    source: String,
    profiles: Vec<Profile>,
}

#[derive(Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct Profile {
    id: String,
    name: String,
    kind: Kind,
    config: ProfileConfig,
}

#[derive(Deserialize, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct ProfileConfig {
    app: PortableApp,
    provider: Provider,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    stack_key: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    selected_model: Option<SelectedModel>,
}

#[derive(Deserialize, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct SelectedModel {
    upstream: String,
    one_m: bool,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct Summary<'a> {
    app: PortableApp,
    id: &'a str,
    name: &'a str,
    kind: Kind,
    has_credential: bool,
    requires_account: bool,
    importable: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    reason: Option<&'static str>,
}

fn invalid(message: &str) -> AppError {
    AppError::InvalidInput(message.into())
}

fn read_json<T: DeserializeOwned>(mut input: impl Read) -> Result<T, AppError> {
    let mut bytes = Vec::new();
    input
        .by_ref()
        .take(MAX_BYTES as u64 + 1)
        .read_to_end(&mut bytes)
        .map_err(|_| invalid("Cannot read the portable provider input"))?;
    if bytes.len() > MAX_BYTES {
        return Err(invalid("Portable provider input exceeds 1 MiB"));
    }
    serde_json::from_slice(&bytes).map_err(|_| invalid("Invalid portable provider JSON"))
}

fn canonical_endpoint(raw: &str) -> Option<String> {
    let url = url::Url::parse(raw).ok()?;
    let public_host = match url.host()? {
        url::Host::Domain(name) => {
            let name = name.trim_end_matches('.');
            name.contains('.') && !name.ends_with(".localhost") && !name.ends_with(".local")
        }
        url::Host::Ipv4(ip) => {
            !ip.is_loopback() && !ip.is_private() && !ip.is_unspecified() && !ip.is_link_local()
        }
        url::Host::Ipv6(ip) => {
            ip.to_ipv4_mapped().is_none()
                && !ip.is_loopback()
                && !ip.is_unspecified()
                && !ip.is_unique_local()
                && !ip.is_unicast_link_local()
        }
    };
    let safe = url.scheme() == "https"
        && public_host
        && url.username().is_empty()
        && url.password().is_none()
        && url.query().is_none()
        && url.fragment().is_none();
    safe.then(|| url.to_string())
}

fn live_state() -> Result<LiveState, AppError> {
    let store = DeviceStore::for_device();
    let path = store.state_path();
    match std::fs::File::open(&path) {
        Ok(file) => read_json(file),
        Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(LiveState::default()),
        Err(error) => Err(AppError::io(&path, error)),
    }
}

fn selected_claude_model() -> Result<Option<String>, AppError> {
    let path = crate::config::get_claude_settings_path();
    match std::fs::File::open(&path) {
        Ok(file) => {
            let settings: Value = read_json(file)?;
            Ok(settings
                .get("model")
                .and_then(Value::as_str)
                .map(str::to_owned))
        }
        Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(None),
        Err(error) => Err(AppError::io(&path, error)),
    }
}

fn requires_routing(provider: &Provider) -> bool {
    provider.uses_managed_account_auth()
        || provider.meta.as_ref().is_some_and(|meta| {
            meta.claude_desktop_mode == Some(ClaudeDesktopMode::Proxy)
                || meta.is_full_url == Some(true)
                || meta
                    .api_format
                    .as_deref()
                    .is_some_and(|format| !matches!(format, "" | "anthropic"))
        })
}

fn portable_kind(provider: &Provider, state: &AppLiveState) -> Kind {
    if state.stack.enabled && state.mode_state().is_proxy() && state.stack.is_member(&provider.id) {
        Kind::Aggregation
    } else if requires_routing(provider) || state.mode_state().routes_to(&provider.id) {
        Kind::Routing
    } else {
        Kind::Direct
    }
}

fn portable_env(key: &str) -> bool {
    URL_FIELDS.contains(&key)
        || SECRET_FIELDS.contains(&key)
        || matches!(
            key,
            "ANTHROPIC_MODEL" | "ANTHROPIC_SMALL_FAST_MODEL" | "CLAUDE_CODE_SUBAGENT_MODEL"
        )
        || ["HAIKU", "SONNET", "OPUS", "FABLE"].iter().any(|role| {
            matches!(
                key.strip_prefix(&format!("ANTHROPIC_DEFAULT_{role}_MODEL")),
                Some("" | "_NAME" | "_DESCRIPTION")
            )
        })
        || crate::live::floor::claude_exclusive_env(key) && key != "CLAUDE_CODE_EXTRA_BODY"
}

fn model_id(text: &str) -> bool {
    !text.is_empty()
        && text.len() <= 512
        && text
            .chars()
            .all(|ch| ch.is_alphanumeric() || "/-_.:@[]".contains(ch))
}

fn label(text: &str) -> bool {
    text.len() <= 1000 && !text.chars().any(char::is_control)
}

fn portable_model_field(key: &str, value: Value) -> Option<Value> {
    match key {
        "modelOverrides" => {
            let overrides: Map<String, Value> = value
                .as_object()?
                .iter()
                .filter(|(model, mapped)| model_id(model) && mapped.as_str().is_some_and(model_id))
                .map(|(model, mapped)| (model.clone(), mapped.clone()))
                .collect();
            (!overrides.is_empty()).then_some(Value::Object(overrides))
        }
        "fallbackModel" => {
            let models: Vec<_> = value
                .as_array()?
                .iter()
                .filter(|model| model.as_str().is_some_and(model_id))
                .take(3)
                .cloned()
                .collect();
            (!models.is_empty()).then_some(Value::Array(models))
        }
        key if key.ends_with("_NAME") || key.ends_with("_DESCRIPTION") => {
            value.as_str().is_some_and(label).then_some(value)
        }
        key if crate::live::floor::claude_exclusive_env(key) => {
            (value.is_number() || value.is_boolean() || value.as_str().is_some_and(label))
                .then_some(value)
        }
        _ => value.as_str().is_some_and(model_id).then_some(value),
    }
}

fn portable_fields(
    fields: impl IntoIterator<Item = (String, Value)>,
    include_credentials: bool,
) -> Result<Map<String, Value>, AppError> {
    let mut result = Map::new();
    for (key, value) in fields {
        let projected = if URL_FIELDS.contains(&key.as_str()) {
            let endpoint = value.as_str().and_then(canonical_endpoint).ok_or_else(|| {
                invalid("Portable upstream URLs must be HTTPS and contain no credentials, query or fragment")
            })?;
            Some(Value::String(endpoint))
        } else if SECRET_FIELDS.contains(&key.as_str()) {
            let token = value.as_str().unwrap_or_default().trim();
            (include_credentials
                && !token.is_empty()
                && !token.starts_with("PROXY_MANAGED")
                && !token.chars().any(char::is_control))
            .then_some(Value::String(token.into()))
        } else {
            portable_model_field(&key, value)
        };
        if let Some(value) = projected {
            result.insert(key, value);
        }
    }
    Ok(result)
}

fn portable_provider(
    app: PortableApp,
    provider: &Provider,
    include_credentials: bool,
) -> Result<Provider, AppError> {
    if provider.has_official_category() || provider.settings_config.as_object().is_none() {
        return Err(invalid(
            "Only third-party Claude providers with a settings object are portable",
        ));
    }
    let include_credentials = include_credentials && !provider.uses_managed_account_auth();
    let source = &provider.settings_config;
    let (top, env) = match app {
        PortableApp::Claude => {
            let projection = ClaudeProjection::of(source);
            (
                projection
                    .top
                    .into_iter()
                    .filter(|(key, _)| {
                        matches!(
                            key.as_str(),
                            "model"
                                | "fallbackModel"
                                | "modelOverrides"
                                | "primaryModel"
                                | "smallFastModel"
                                | "advisorModel"
                                | "apiBaseUrl"
                        )
                    })
                    .collect::<Vec<_>>(),
                projection
                    .env
                    .into_iter()
                    .chain(projection.exclusive)
                    .filter(|(key, _)| portable_env(key))
                    .collect::<Vec<_>>(),
            )
        }
        PortableApp::ClaudeDesktop => (
            source
                .as_object()
                .unwrap()
                .iter()
                .filter(|(key, _)| DESKTOP_FIELDS.contains(&key.as_str()))
                .map(|(key, value)| (key.clone(), value.clone()))
                .collect(),
            source
                .get("env")
                .and_then(Value::as_object)
                .into_iter()
                .flatten()
                .filter(|(key, _)| portable_env(key))
                .map(|(key, value)| (key.clone(), value.clone()))
                .collect(),
        ),
    };
    let mut settings = portable_fields(top, include_credentials)?;
    settings.insert(
        "env".into(),
        Value::Object(portable_fields(env, include_credentials)?),
    );
    let mut portable = Provider::with_id(
        provider.id.clone(),
        provider.name.clone(),
        Value::Object(settings),
        None,
    );
    portable.category = Some("custom".into());
    if let Some(meta) = &provider.meta {
        let custom_endpoints: std::collections::HashMap<_, _> = meta
            .custom_endpoints
            .iter()
            .filter_map(|(key, endpoint)| {
                let url = canonical_endpoint(key)?;
                if canonical_endpoint(&endpoint.url).as_deref() != Some(url.as_str()) {
                    return None;
                }
                let mut endpoint = endpoint.clone();
                endpoint.url = url.clone();
                endpoint.last_used = None;
                Some((url, endpoint))
            })
            .collect();
        let stack_models = meta.stack_models.as_ref().map(|models| {
            models
                .iter()
                .filter(|model| model_id(&model.model))
                .map(|model| {
                    let mut model = model.clone();
                    model.display_name = model.display_name.filter(|value| label(value));
                    model
                })
                .collect()
        });
        let desktop_routes = meta
            .claude_desktop_model_routes
            .iter()
            .filter(|(key, route)| {
                model_id(key) && (route.model.is_empty() || model_id(&route.model))
            })
            .map(|(key, route)| {
                let mut route = route.clone();
                route.label_override = route.label_override.filter(|value| label(value));
                (key.clone(), route)
            })
            .collect();
        portable.meta = Some(ProviderMeta {
            api_format: meta.api_format.clone(),
            api_key_field: meta.api_key_field.clone(),
            is_full_url: meta.is_full_url,
            provider_type: meta.provider_type.clone(),
            claude_reasoning_effort: meta.claude_reasoning_effort.clone(),
            stack_models,
            claude_desktop_mode: meta.claude_desktop_mode.clone(),
            claude_desktop_model_routes: desktop_routes,
            endpoint_auto_select: if custom_endpoints.is_empty() {
                None
            } else {
                meta.endpoint_auto_select
            },
            custom_endpoints,
            ..ProviderMeta::default()
        });
    }
    if provider.uses_managed_account_auth() && !portable.uses_managed_account_auth() {
        return Err(invalid(
            "This managed account cannot be resolved on the destination device",
        ));
    }
    Ok(portable)
}

fn profile_id(app: PortableApp, id: &str) -> String {
    format!("{}--{id}", app.app().as_str())
}

fn has_credential(profile: &Profile) -> bool {
    let provider = &profile.config.provider;
    let settings = &provider.settings_config;
    let keys: &[&str] = match (profile.config.app, profile.kind) {
        (PortableApp::ClaudeDesktop, Kind::Direct) => &["ANTHROPIC_AUTH_TOKEN"],
        (PortableApp::Claude, Kind::Direct) => &["ANTHROPIC_AUTH_TOKEN", "ANTHROPIC_API_KEY"],
        _ => SECRET_FIELDS,
    };
    keys.iter().any(|key| {
        let env = settings.get("env").and_then(|env| env.get(*key));
        let value = if profile.kind == Kind::Direct {
            env
        } else {
            env.or_else(|| settings.get(*key))
        };
        value
            .and_then(Value::as_str)
            .is_some_and(|value| !value.is_empty())
    })
}

fn prepare_import(profile: &Profile) -> Result<(AppType, Provider), &'static str> {
    let mut provider = profile.config.provider.clone();
    if provider.uses_managed_account_auth() {
        return Err("account-authorization-required");
    }
    if !has_credential(profile) {
        return Err("credential-required");
    }
    let app = profile.config.app.app();
    ProviderService::prepare_new_provider(&app, &mut provider)
        .map_err(|_| "invalid-provider-settings")?;
    Ok((app, provider))
}

fn summary(profile: &Profile) -> Summary<'_> {
    let provider = &profile.config.provider;
    let reason = prepare_import(profile).err();
    Summary {
        app: profile.config.app,
        id: &provider.id,
        name: &provider.name,
        kind: profile.kind,
        has_credential: has_credential(profile),
        requires_account: provider.uses_managed_account_auth(),
        importable: reason.is_none(),
        reason,
    }
}

fn make_profile(
    pair: &Selection,
    provider: &Provider,
    state: &LiveState,
    include_credentials: bool,
    client_model: Option<&str>,
) -> Result<Profile, AppError> {
    let empty = AppLiveState::default();
    let current = state.apps.get(pair.app.app().as_str()).unwrap_or(&empty);
    let kind = portable_kind(provider, current);
    let stack_key = current.stack.key_of(&provider.id);
    let selected_model = match (pair.app, kind, stack_key, client_model) {
        (PortableApp::Claude, Kind::Aggregation, Some(key), Some(model)) => {
            crate::mode::stack::claude_models(key, provider)
                .into_iter()
                .find(|entry| {
                    entry.id == model && model_id(strip_one_m_suffix_for_upstream(&entry.upstream))
                })
                .map(|entry| SelectedModel {
                    upstream: strip_one_m_suffix_for_upstream(&entry.upstream).to_owned(),
                    one_m: entry.one_m,
                })
        }
        _ => None,
    };
    Ok(Profile {
        id: profile_id(pair.app, &provider.id),
        name: provider.name.clone(),
        kind,
        config: ProfileConfig {
            app: pair.app,
            provider: portable_provider(pair.app, provider, include_credentials)?,
            stack_key: stack_key.map(str::to_owned),
            selected_model,
        },
    })
}

fn export_selected(
    db: &Database,
    state: &LiveState,
    request: ExportRequest,
    client_model: Option<&str>,
) -> Result<Bundle, AppError> {
    if request.profiles.is_empty() || request.profiles.len() > MAX_PROFILES {
        return Err(invalid("Select between 1 and 50 Claude providers"));
    }
    let apps: BTreeSet<_> = request.profiles.iter().map(|pair| pair.app).collect();
    // The native rows reader hydrates provider_endpoints as well as settings.
    let providers: BTreeMap<_, _> = apps
        .into_iter()
        .map(|app| db.get_provider_rows(&app.app()).map(|rows| (app, rows)))
        .collect::<Result<_, _>>()?;
    let mut seen = BTreeSet::new();
    let mut profiles = Vec::new();
    for pair in request.profiles {
        if !seen.insert((pair.app, pair.id.clone())) {
            return Err(invalid("Duplicate provider in the selection"));
        }
        let provider = providers[&pair.app]
            .get(&pair.id)?
            .ok_or_else(|| invalid("A selected provider does not exist"))?;
        profiles.push(make_profile(
            &pair,
            provider,
            state,
            request.include_credentials,
            client_model,
        )?);
    }
    Ok(Bundle {
        version: VERSION,
        source: SOURCE.into(),
        profiles,
    })
}

fn validated_bundle(mut bundle: Bundle) -> Result<Bundle, AppError> {
    if bundle.version != VERSION
        || bundle.source != SOURCE
        || bundle.profiles.is_empty()
        || bundle.profiles.len() > MAX_PROFILES
    {
        return Err(invalid("Unsupported or empty CC Switch provider bundle"));
    }
    let mut seen = BTreeSet::new();
    for profile in &mut bundle.profiles {
        let config = &mut profile.config;
        let original = &config.provider;
        if original.id.trim().is_empty()
            || original.id.len() > 150
            || original.name.trim().is_empty()
            || original.name.len() > 160
            || original.id.chars().any(char::is_control)
            || original.name.chars().any(char::is_control)
            || profile.id != profile_id(config.app, &original.id)
            || profile.name != original.name
            || !seen.insert((config.app, original.id.clone()))
            || matches!(profile.kind, Kind::Direct) && requires_routing(original)
        {
            return Err(invalid(
                "Invalid or duplicate provider in the portable bundle",
            ));
        }
        if config.stack_key.as_ref().is_some_and(|key| {
            key.is_empty()
                || key.len() > 160
                || !key
                    .bytes()
                    .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_'))
        }) {
            return Err(invalid("Invalid aggregation provider key"));
        }
        if let Some(model) = &config.selected_model {
            let valid = model_id(&model.upstream)
                && config.app == PortableApp::Claude
                && profile.kind == Kind::Aggregation
                && config.stack_key.as_ref().is_some_and(|key| {
                    crate::mode::stack::claude_models(key, original)
                        .into_iter()
                        .any(|entry| {
                            strip_one_m_suffix_for_upstream(&entry.upstream) == model.upstream
                                && entry.one_m == model.one_m
                        })
                });
            if !valid {
                return Err(invalid(
                    "Selected model is not published by this aggregation provider",
                ));
            }
        }
        config.provider = portable_provider(config.app, original, true)?;
    }
    Ok(bundle)
}

fn run(command: &str, input: impl Read) -> Result<Value, AppError> {
    match command {
        "list" => {
            let db = Database::open_for_provider_bundle(false)?;
            let state = live_state()?;
            let mut profiles = Vec::new();
            for app in [PortableApp::Claude, PortableApp::ClaudeDesktop] {
                for (_, provider) in db
                    .get_provider_rows(&app.app())?
                    .into_readable("portable provider list")
                {
                    let pair = Selection {
                        app,
                        id: provider.id.clone(),
                    };
                    if let Ok(profile) = make_profile(&pair, &provider, &state, true, None) {
                        profiles.push(profile);
                    }
                }
            }
            Ok(
                json!({"version": VERSION, "source": SOURCE, "profiles": profiles.iter().map(summary).collect::<Vec<_>>()}),
            )
        }
        "export" => {
            let db = Database::open_for_provider_bundle(false)?;
            let request: ExportRequest = read_json(input)?;
            let client_model = if request
                .profiles
                .iter()
                .any(|item| item.app == PortableApp::Claude)
            {
                selected_claude_model().ok().flatten()
            } else {
                None
            };
            let bundle = validated_bundle(export_selected(
                &db,
                &live_state()?,
                request,
                client_model.as_deref(),
            )?)?;
            serde_json::to_value(bundle)
                .map_err(|_| invalid("Cannot serialize the portable bundle"))
        }
        "preview" => {
            let bundle = validated_bundle(read_json(input)?)?;
            Ok(
                json!({"version": VERSION, "source": SOURCE, "profiles": bundle.profiles.iter().map(summary).collect::<Vec<_>>()}),
            )
        }
        "import" => {
            let bundle = validated_bundle(read_json(input)?)?;
            let db = Database::open_for_provider_bundle(true)?;
            import_bundle(&db, &bundle)
        }
        _ => Err(invalid(
            "Usage: cc-switch provider-bundle list|export|preview|import",
        )),
    }
}

fn import_bundle(db: &Database, bundle: &Bundle) -> Result<Value, AppError> {
    let mut entries = Vec::new();
    let mut skipped_profiles = Vec::new();
    for profile in &bundle.profiles {
        match prepare_import(profile) {
            Ok(entry) => entries.push(entry),
            Err(reason) => skipped_profiles.push(json!({
                "app": profile.config.app,
                "id": profile.config.provider.id,
                "reason": reason,
            })),
        }
    }
    let (imported, conflicts) = db.import_provider_bundle(&entries)?;
    Ok(json!({
        "imported": imported,
        "skipped": conflicts + skipped_profiles.len(),
        "conflicts": conflicts,
        "skippedProfiles": skipped_profiles,
        "activated": false,
    }))
}

pub fn run_cli(args: &[String]) -> std::process::ExitCode {
    let command = args.first().map(String::as_str).unwrap_or("");
    let result = if args.len() == 1 {
        run(command, io::stdin().lock())
    } else {
        Err(invalid(
            "Usage: cc-switch provider-bundle list|export|preview|import",
        ))
    };
    match result {
        Ok(value) => {
            let mut output = io::stdout().lock();
            match serde_json::to_vec(&value) {
                Ok(bytes)
                    if bytes.len() <= MAX_BYTES
                        && output.write_all(&bytes).is_ok()
                        && output.write_all(b"\n").is_ok() =>
                {
                    std::process::ExitCode::SUCCESS
                }
                _ => {
                    eprintln!("Cannot write the portable provider output");
                    std::process::ExitCode::FAILURE
                }
            }
        }
        Err(error) => {
            eprintln!("{error}");
            std::process::ExitCode::FAILURE
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::mode::state::Mode;
    use crate::provider::ClaudeStackModel;

    fn provider(id: &str, name: &str) -> Provider {
        let mut provider = Provider::with_id(
            id.into(),
            name.into(),
            json!({
                "model": "claude-opus-5-5",
                "apiKeyHelper": "should-never-execute",
                "hooks": {"post": "should-never-execute"},
                "env": {
                    "ANTHROPIC_BASE_URL": "https://gateway.example/api",
                    "ANTHROPIC_AUTH_TOKEN": "synthetic-provider-key",
                    "ANTHROPIC_DEFAULT_OPUS_MODEL": "claude-opus-5-5",
                    "CLAUDE_CODE_AUTO_MODE_SERVER": "0",
                    "CLAUDE_CODE_OAUTH_TOKEN": "synthetic-account-session",
                    "SSH_AUTH_SOCK": "/private/connector.sock"
                }
            }),
            None,
        );
        provider.meta = Some(ProviderMeta {
            stack_models: Some(vec![ClaudeStackModel {
                model: "claude-opus-5-5".into(),
                one_m: true,
                ..ClaudeStackModel::default()
            }]),
            ..ProviderMeta::default()
        });
        provider
    }

    #[test]
    fn export_projects_only_provider_settings_and_resolves_the_selected_stack_model() {
        let db = Database::memory().unwrap();
        let provider = provider("vendor", "Vendor");
        db.save_provider(&AppType::Claude, &provider).unwrap();
        db.save_provider(&AppType::Claude, &self::provider("unselected", "Other"))
            .unwrap();
        let mut state = LiveState::default();
        let app = state.apps.entry("claude".into()).or_default();
        app.mode = Some(Mode::Proxy);
        app.stack.enabled = true;
        app.stack.members.push("vendor".into());
        app.stack.keys.insert("vendor".into(), "vendor".into());
        let selected =
            crate::mode::stack::encode(&AppType::Claude, "vendor", "claude-opus-5-5", true);
        let request = ExportRequest {
            profiles: vec![Selection {
                app: PortableApp::Claude,
                id: "vendor".into(),
            }],
            include_credentials: false,
        };
        let bundle =
            validated_bundle(export_selected(&db, &state, request, Some(&selected)).unwrap())
                .unwrap();
        assert_eq!(bundle.profiles.len(), 1);
        let profile = &bundle.profiles[0];
        assert!(profile.kind == Kind::Aggregation);
        assert_eq!(
            profile.config.selected_model.as_ref().unwrap().upstream,
            "claude-opus-5-5"
        );
        assert!(profile.config.selected_model.as_ref().unwrap().one_m);
        assert_eq!(
            profile.config.provider.settings_config["env"]["CLAUDE_CODE_AUTO_MODE_SERVER"],
            "0"
        );
        let wire = serde_json::to_string(&bundle).unwrap();
        for private in [
            "synthetic-provider-key",
            "synthetic-account-session",
            "should-never-execute",
            "SSH_AUTH_SOCK",
        ] {
            assert!(!wire.contains(private));
        }
        let explicit = portable_provider(PortableApp::Claude, &provider, true).unwrap();
        assert_eq!(
            explicit.settings_config["env"]["ANTHROPIC_AUTH_TOKEN"],
            "synthetic-provider-key"
        );
        assert!(explicit.settings_config["env"]
            .get("CLAUDE_CODE_OAUTH_TOKEN")
            .is_none());
    }

    #[test]
    fn rejects_host_placeholders_on_both_export_and_preview() {
        for address in [
            "http://127.0.0.1:15721",
            "https://localhost.",
            "https://[::ffff:127.0.0.1]",
            "https://gateway.example/?token=synthetic",
            "https://user:synthetic@gateway.example",
        ] {
            let mut provider = provider("fixture", "Fixture");
            provider.settings_config["env"]["ANTHROPIC_BASE_URL"] = json!(address);
            assert!(portable_provider(PortableApp::Claude, &provider, false).is_err());
            let profile = Profile {
                id: "claude--fixture".into(),
                name: "Fixture".into(),
                kind: Kind::Direct,
                config: ProfileConfig {
                    app: PortableApp::Claude,
                    provider,
                    stack_key: None,
                    selected_model: None,
                },
            };
            assert!(validated_bundle(Bundle {
                version: VERSION,
                source: SOURCE.into(),
                profiles: vec![profile],
            })
            .is_err());
        }
    }

    #[test]
    fn import_keeps_existing_and_selected_provider_when_repeated() {
        let db = Database::memory().unwrap();
        db.save_provider(&AppType::Claude, &provider("existing", "Already here"))
            .unwrap();
        db.set_current_provider(&AppType::Claude, "existing")
            .unwrap();
        let duplicate = provider("another-id", "already here");
        let new = provider("new", "New provider");
        let entries = [(AppType::Claude, duplicate), (AppType::Claude, new)];
        assert_eq!(db.import_provider_bundle(&entries).unwrap(), (1, 1));
        assert_eq!(db.import_provider_bundle(&entries).unwrap(), (0, 2));
        assert_eq!(
            db.get_current_provider(&AppType::Claude)
                .unwrap()
                .as_deref(),
            Some("existing")
        );
        assert!(db
            .get_provider_by_id("another-id", &AppType::Claude)
            .unwrap()
            .is_none());
    }

    #[test]
    fn redacted_desktop_preview_does_not_block_a_valid_import() {
        let db = Database::memory().unwrap();
        let source = provider("desktop-fixture", "Desktop fixture");
        let desktop = make_profile(
            &Selection {
                app: PortableApp::ClaudeDesktop,
                id: source.id.clone(),
            },
            &source,
            &LiveState::default(),
            false,
            None,
        )
        .unwrap();
        assert!(!summary(&desktop).importable);
        assert_eq!(summary(&desktop).reason, Some("credential-required"));

        let source = provider("code-fixture", "Code fixture");
        let code = make_profile(
            &Selection {
                app: PortableApp::Claude,
                id: source.id.clone(),
            },
            &source,
            &LiveState::default(),
            true,
            None,
        )
        .unwrap();
        assert!(summary(&code).importable);
        let bundle = validated_bundle(Bundle {
            version: VERSION,
            source: SOURCE.into(),
            profiles: vec![desktop, code],
        })
        .unwrap();
        let result = import_bundle(&db, &bundle).unwrap();
        assert_eq!(result["imported"], 1);
        assert_eq!(result["skipped"], 1);
        assert_eq!(
            result["skippedProfiles"][0]["reason"],
            "credential-required"
        );
        assert!(db
            .get_provider_by_id("code-fixture", &AppType::Claude)
            .unwrap()
            .is_some());
        assert!(db
            .get_provider_by_id("desktop-fixture", &AppType::ClaudeDesktop)
            .unwrap()
            .is_none());
    }

    #[test]
    fn export_hydrates_native_endpoint_rows_and_discards_nested_secrets() {
        let db = Database::memory().unwrap();
        let mut source = provider("fixture", "Fixture");
        source.settings_config["modelOverrides"] = json!({
            "claude-sonnet": "anthropic/claude-sonnet",
            "private": {"key": "synthetic-nested-secret"}
        });
        source.settings_config["fallbackModel"] =
            json!(["claude-sonnet", {"token":"synthetic-nested-secret"}]);
        source.settings_config["env"]["ANTHROPIC_DEFAULT_OPUS_MODEL_NAME"] =
            json!({"token":"synthetic-nested-secret"});
        let meta = source.meta.as_mut().unwrap();
        meta.endpoint_auto_select = Some(true);
        meta.custom_endpoints.insert(
            "https://public.example/api".into(),
            crate::settings::CustomEndpoint {
                url: "https://public.example/api?key=synthetic-nested-secret".into(),
                added_at: 1,
                last_used: Some(2),
            },
        );
        let sanitized = portable_provider(PortableApp::Claude, &source, false).unwrap();
        assert!(sanitized.meta.unwrap().custom_endpoints.is_empty());
        db.save_provider(&AppType::Claude, &source).unwrap();
        db.add_custom_endpoint(&AppType::Claude, "fixture", "https://ALT.example:443/api")
            .unwrap();
        let bundle = export_selected(
            &db,
            &LiveState::default(),
            ExportRequest {
                profiles: vec![Selection {
                    app: PortableApp::Claude,
                    id: "fixture".into(),
                }],
                include_credentials: false,
            },
            None,
        )
        .unwrap();
        let exported = &bundle.profiles[0].config.provider;
        assert_eq!(
            exported.settings_config["modelOverrides"],
            json!({"claude-sonnet": "anthropic/claude-sonnet"})
        );
        assert_eq!(
            exported.settings_config["fallbackModel"],
            json!(["claude-sonnet"])
        );
        let meta = exported.meta.as_ref().unwrap();
        assert_eq!(meta.endpoint_auto_select, Some(true));
        assert_eq!(
            meta.custom_endpoints["https://alt.example/api"].url,
            "https://alt.example/api"
        );
        assert!(!serde_json::to_string(&bundle)
            .unwrap()
            .contains("synthetic-nested-secret"));
    }
}

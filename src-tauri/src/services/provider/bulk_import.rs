use super::ProviderService;
use crate::{
    app_config::AppType, deeplink::DeepLinkImportRequest, error::AppError, provider::Provider,
    store::AppState,
};
use serde::Serialize;
use serde_json::Value;
mod formats;

const MAX_BYTES: usize = 1024 * 1024;
const MAX_ROWS: usize = 100;
const FIELDS: [&str; 6] = [
    "name",
    "base_url",
    "api_key",
    "model",
    "api_format",
    "context_window",
];

#[derive(Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct BulkProviderIssue {
    pub row: Option<usize>,
    pub field: Option<&'static str>,
    pub code: &'static str,
    pub limit: Option<usize>,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct BulkProviderPreviewRow {
    pub row: usize,
    pub name: String,
    pub base_url: String,
    pub model: Option<String>,
    pub context_window: Option<u64>,
    pub api_format: String,
    pub key_hint: String,
    pub key_may_resolve_from_environment: bool,
    pub requires_routing: bool,
    pub duplicate: bool,
    pub same_name: bool,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct BulkProviderPreview {
    pub app: String,
    pub rows: Vec<BulkProviderPreviewRow>,
    pub errors: Vec<BulkProviderIssue>,
    pub token: Option<String>,
    pub new_count: usize,
    pub duplicate_count: usize,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct BulkProviderImportResult {
    pub added_ids: Vec<String>,
    pub skipped: usize,
}

struct Prepared {
    providers: Vec<Provider>,
    rows: Vec<BulkProviderPreviewRow>,
    errors: Vec<BulkProviderIssue>,
}

fn issue(
    row: Option<usize>,
    field: Option<&'static str>,
    code: &'static str,
    limit: Option<usize>,
) -> BulkProviderIssue {
    BulkProviderIssue {
        row,
        field,
        code,
        limit,
    }
}

fn text_field(
    value: &Value,
    field: &'static str,
    row: usize,
    required: bool,
    max: usize,
    errors: &mut Vec<BulkProviderIssue>,
) -> Option<String> {
    match value.get(field) {
        None | Some(Value::Null) if !required => None,
        Some(Value::String(raw)) => {
            let text = raw.trim();
            if text.is_empty() {
                if required {
                    errors.push(issue(Some(row), Some(field), "required", None));
                }
                None
            } else if text.len() > max || text.chars().any(char::is_control) {
                errors.push(issue(Some(row), Some(field), "invalidValue", Some(max)));
                None
            } else {
                Some(text.to_string())
            }
        }
        _ => {
            errors.push(issue(Some(row), Some(field), "required", None));
            None
        }
    }
}

fn prepare(app: &AppType, source: &str) -> Result<Prepared, AppError> {
    let spec = formats::format(app);
    let mut result = Prepared {
        providers: Vec::new(),
        rows: Vec::new(),
        errors: Vec::new(),
    };
    if source.len() > MAX_BYTES {
        result
            .errors
            .push(issue(None, None, "tooLarge", Some(MAX_BYTES)));
        return Ok(result);
    }
    let data: Value = match serde_json::from_str(source.trim_start_matches('\u{feff}')) {
        Ok(value) => value,
        Err(_) => {
            result.errors.push(issue(None, None, "invalidJson", None));
            return Ok(result);
        }
    };
    let Some(rows) = data.as_array() else {
        result.errors.push(issue(None, None, "notArray", None));
        return Ok(result);
    };
    if rows.is_empty() {
        result.errors.push(issue(None, None, "empty", None));
        return Ok(result);
    }
    if rows.len() > MAX_ROWS {
        result
            .errors
            .push(issue(None, None, "tooManyRows", Some(MAX_ROWS)));
        return Ok(result);
    }
    for (index, value) in rows.iter().enumerate() {
        let row = index + 1;
        let before = result.errors.len();
        let Some(object) = value.as_object() else {
            result
                .errors
                .push(issue(Some(row), None, "invalidRow", None));
            continue;
        };
        if object.keys().any(|key| !FIELDS.contains(&key.as_str())) {
            result
                .errors
                .push(issue(Some(row), None, "unknownFields", None));
        }
        let name = text_field(value, "name", row, true, 256, &mut result.errors);
        let endpoint = text_field(value, "base_url", row, true, 4096, &mut result.errors);
        let api_key = text_field(value, "api_key", row, true, 8192, &mut result.errors);
        let model = text_field(
            value,
            "model",
            row,
            spec.model_required,
            256,
            &mut result.errors,
        );
        let context_window = value.get("context_window").and_then(Value::as_u64);
        if *app == AppType::Kimi {
            if context_window.is_none_or(|size| size == 0 || size > 9007199254740991) {
                result.errors.push(issue(
                    Some(row),
                    Some("context_window"),
                    "invalidContext",
                    None,
                ));
            }
        } else if value.get("context_window").is_some() {
            result.errors.push(issue(
                Some(row),
                Some("context_window"),
                "unsupportedContext",
                None,
            ));
        }
        let format = text_field(value, "api_format", row, false, 32, &mut result.errors)
            .map(|v| v.to_ascii_lowercase());
        let endpoint = endpoint.and_then(|text| match url::Url::parse(&text) {
            Ok(url)
                if matches!(url.scheme(), "http" | "https")
                    && url.host_str().is_some()
                    && url.username().is_empty()
                    && url.password().is_none()
                    && url.query().is_none()
                    && url.fragment().is_none()
                    && !text.contains(',') =>
            {
                Some(url.to_string().trim_end_matches('/').to_string())
            }
            _ => {
                result
                    .errors
                    .push(issue(Some(row), Some("base_url"), "invalidUrl", None));
                None
            }
        });
        if api_key
            .as_ref()
            .is_some_and(|key| key.chars().all(|c| c == '*' || c == '•'))
        {
            result
                .errors
                .push(issue(Some(row), Some("api_key"), "maskedKey", None));
        }
        if let Some(format) = &format {
            let supported = spec.allowed.contains(&format.as_str());
            if !supported {
                result
                    .errors
                    .push(issue(Some(row), Some("api_format"), "invalidFormat", None));
            }
        }
        if *app == AppType::ClaudeDesktop
            && format.as_deref().is_some_and(|f| f != "anthropic")
            && model.is_none()
        {
            result
                .errors
                .push(issue(Some(row), Some("model"), "required", None));
        }
        for (field, text) in [
            ("name", &name),
            ("base_url", &endpoint),
            ("api_key", &api_key),
            ("model", &model),
        ] {
            if text
                .as_deref()
                .is_some_and(|text| formats::dynamic_value(app, text, field == "api_key"))
            {
                result
                    .errors
                    .push(issue(Some(row), Some(field), "dynamicValue", None));
            }
        }
        if result.errors.len() != before {
            continue;
        }
        let (Some(name), Some(endpoint), Some(api_key)) = (name, endpoint, api_key) else {
            continue;
        };
        let default_format = spec.default;
        let api_format = format.unwrap_or_else(|| default_format.into());
        let request = DeepLinkImportRequest {
            version: "v1".into(),
            resource: "provider".into(),
            app: Some(app.as_str().into()),
            name: Some(name.clone()),
            endpoint: Some(endpoint.clone()),
            api_key: Some(api_key.clone()),
            model,
            api_format: (api_format != default_format).then(|| api_format.clone()),
            ..Default::default()
        };
        let provider = formats::build(app, &request, &api_format, context_window);
        let Ok(provider) = provider else {
            result
                .errors
                .push(issue(Some(row), None, "invalidConfig", None));
            continue;
        };
        let display_model = if *app == AppType::Codex {
            provider
                .settings_config
                .get("config")
                .and_then(Value::as_str)
                .and_then(|text| toml::from_str::<toml::Value>(text).ok())
                .and_then(|config| {
                    config
                        .get("model")
                        .and_then(toml::Value::as_str)
                        .map(String::from)
                })
        } else if *app == AppType::GrokBuild {
            provider
                .settings_config
                .get("config")
                .and_then(Value::as_str)
                .and_then(|text| toml::from_str::<toml::Value>(text).ok())
                .and_then(|config| {
                    config
                        .get("models")
                        .and_then(|models| models.get("default"))
                        .and_then(toml::Value::as_str)
                        .map(String::from)
                })
        } else {
            request.model.clone()
        };
        let key_hint = if api_key.chars().count() <= 8 {
            "••••••••".into()
        } else {
            format!(
                "••••{}",
                api_key
                    .chars()
                    .rev()
                    .take(4)
                    .collect::<Vec<_>>()
                    .into_iter()
                    .rev()
                    .collect::<String>()
            )
        };
        result.rows.push(BulkProviderPreviewRow {
            row,
            name,
            base_url: endpoint,
            model: display_model,
            context_window,
            key_hint,
            key_may_resolve_from_environment: *app == AppType::Omp,
            requires_routing: if *app == AppType::ClaudeDesktop {
                crate::claude_desktop_config::needs_routing(&provider)
            } else {
                app.has_route_mode() && api_format != default_format
            },
            api_format,
            duplicate: false,
            same_name: false,
        });
        result.providers.push(provider);
    }
    Ok(result)
}

fn storage_error(error: AppError) -> AppError {
    if matches!(&error,AppError::Message(key) if key=="provider.bulkImport.errors.previewChanged") {
        error
    } else {
        AppError::Message("provider.bulkImport.errors.storageFailed".into())
    }
}

impl ProviderService {
    pub fn preview_bulk_import(
        state: &AppState,
        app: AppType,
        source: &str,
    ) -> Result<BulkProviderPreview, AppError> {
        let prepared = prepare(&app, source)?;
        let mut preview = BulkProviderPreview {
            app: app.as_str().into(),
            rows: prepared.rows,
            errors: prepared.errors,
            token: None,
            new_count: 0,
            duplicate_count: 0,
        };
        if !preview.errors.is_empty() {
            return Ok(preview);
        }
        let plan = state
            .db
            .preview_provider_batch(app.as_str(), &prepared.providers)
            .map_err(storage_error)?;
        for ((row, duplicate), same_name) in preview
            .rows
            .iter_mut()
            .zip(plan.duplicates)
            .zip(plan.same_names)
        {
            row.duplicate = duplicate;
            row.same_name = same_name;
            if duplicate {
                preview.duplicate_count += 1;
            } else {
                preview.new_count += 1;
            }
        }
        preview.token = Some(plan.token);
        Ok(preview)
    }

    pub fn import_bulk(
        state: &AppState,
        app: AppType,
        source: &str,
        preview_token: &str,
    ) -> Result<BulkProviderImportResult, AppError> {
        let prepared = prepare(&app, source)?;
        if !prepared.errors.is_empty() {
            return Err(AppError::Message(
                "provider.bulkImport.errors.validationFailed".into(),
            ));
        }
        // Serialize with switching, but do not settle pending live-file operations:
        // this path only appends rows and must never project or recover native files.
        let _switch_guard =
            futures::executor::block_on(state.proxy_service.lock_switch_for_app(app.as_str()));
        let (added_ids, skipped) = state
            .db
            .append_provider_batch(app.as_str(), &prepared.providers, preview_token)
            .map_err(storage_error)?;
        Ok(BulkProviderImportResult { added_ids, skipped })
    }
}

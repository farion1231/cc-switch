use crossterm::event::{KeyCode, KeyEvent};
use serde_json::{json, Value};
use tui_textarea::TextArea;

use crate::app_config::AppType;
use crate::provider::Provider;

use super::input::TextInput;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FormMode {
    Add,
    Edit,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ProviderTemplate {
    OpenRouter,
    Official,
    Custom,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FormField {
    Template,
    Name,
    BaseUrl,
    Model,
    ApiKey,
    CodexConfig,
    Notes,
    Save,
}

pub struct ProviderForm {
    pub(super) mode: FormMode,
    pub(super) tool: AppType,

    pub(super) template: ProviderTemplate,
    pub(super) template_open: bool,
    pub(super) template_index: usize,

    pub(super) focus_index: usize,

    pub(super) name: TextInput,
    pub(super) base_url: TextInput,
    pub(super) model: TextInput,
    pub(super) api_key: TextInput,
    pub(super) notes: TextArea<'static>,

    pub(super) codex_config: String,

    pub(super) editing_id: Option<String>,
    pub(super) original_provider: Option<Provider>,

    pub(super) error: Option<String>,
}

impl ProviderForm {
    pub fn new_add(tool: AppType) -> Self {
        let template = default_template(&tool);
        let (base_url, model) = template_defaults(&tool, template);

        let mut notes = TextArea::default();
        notes.set_placeholder_text("Notes (optional)");

        Self {
            mode: FormMode::Add,
            tool,
            template,
            template_open: false,
            template_index: 0,
            focus_index: 0,
            name: TextInput::new(""),
            base_url: TextInput::new(base_url),
            model: TextInput::new(model.unwrap_or("")),
            api_key: TextInput::masked(""),
            notes,
            codex_config: String::new(),
            editing_id: None,
            original_provider: None,
            error: None,
        }
    }

    pub fn new_edit(tool: AppType, id: String, provider: Provider) -> Self {
        let base_url = extract_base_url(&tool, &provider.settings_config).unwrap_or_default();
        let model = extract_model(&tool, &provider.settings_config).unwrap_or_default();
        let api_key = extract_api_key(&tool, &provider.settings_config).unwrap_or_default();
        let codex_config = provider
            .settings_config
            .get("config")
            .and_then(|v| v.as_str())
            .unwrap_or("")
            .to_string();

        let mut notes = TextArea::from(provider.notes.clone().unwrap_or_default().lines());
        notes.set_placeholder_text("Notes (optional)");

        Self {
            mode: FormMode::Edit,
            tool,
            template: ProviderTemplate::Custom,
            template_open: false,
            template_index: 0,
            focus_index: 0,
            name: TextInput::new(provider.name.clone()),
            base_url: TextInput::new(base_url),
            model: TextInput::new(model),
            api_key: TextInput::masked(api_key),
            notes,
            codex_config,
            editing_id: Some(id),
            original_provider: Some(provider),
            error: None,
        }
    }

    pub fn fields(&self) -> Vec<FormField> {
        let include_codex_config = matches!(self.tool, AppType::Codex);
        match self.mode {
            FormMode::Add => vec![
                FormField::Template,
                FormField::Name,
                FormField::BaseUrl,
                FormField::Model,
                FormField::ApiKey,
                FormField::CodexConfig,
                FormField::Notes,
                FormField::Save,
            ],
            FormMode::Edit => vec![
                FormField::Name,
                FormField::BaseUrl,
                FormField::Model,
                FormField::ApiKey,
                FormField::CodexConfig,
                FormField::Notes,
                FormField::Save,
            ],
        }
        .into_iter()
        .filter(|f| include_codex_config || *f != FormField::CodexConfig)
        .collect()
    }

    pub fn focus_field(&self) -> FormField {
        let fields = self.fields();
        let idx = self.focus_index.min(fields.len().saturating_sub(1));
        fields[idx]
    }

    pub fn focus_next(&mut self) {
        let len = self.fields().len();
        if len == 0 {
            self.focus_index = 0;
            return;
        }
        self.focus_index = (self.focus_index + 1) % len;
    }

    pub fn focus_prev(&mut self) {
        let len = self.fields().len();
        if len == 0 {
            self.focus_index = 0;
            return;
        }
        self.focus_index = if self.focus_index == 0 {
            len - 1
        } else {
            self.focus_index - 1
        };
    }

    pub fn template_options(&self) -> &'static [ProviderTemplate] {
        template_options(&self.tool)
    }

    pub fn template_label(&self, template: ProviderTemplate) -> &'static str {
        template_label(&self.tool, template)
    }

    pub fn open_template_picker(&mut self) {
        if self.mode != FormMode::Add {
            return;
        }
        self.template_open = true;
        self.template_index = self
            .template_options()
            .iter()
            .position(|t| *t == self.template)
            .unwrap_or(0);
    }

    pub fn handle_template_key(&mut self, key: KeyEvent) -> bool {
        if !self.template_open {
            return false;
        }

        match key.code {
            KeyCode::Esc => {
                self.template_open = false;
                true
            }
            KeyCode::Up | KeyCode::Char('k') => {
                if self.template_index > 0 {
                    self.template_index -= 1;
                }
                true
            }
            KeyCode::Down | KeyCode::Char('j') => {
                let max = self.template_options().len().saturating_sub(1);
                self.template_index = (self.template_index + 1).min(max);
                true
            }
            KeyCode::Enter => {
                let Some(new_template) = self.template_options().get(self.template_index).copied()
                else {
                    self.template_open = false;
                    return true;
                };

                self.template = new_template;
                self.template_open = false;

                let (base_url, model) = template_defaults(&self.tool, self.template);
                self.base_url.set_value(base_url);
                self.model.set_value(model.unwrap_or(""));
                true
            }
            _ => false,
        }
    }

    pub fn validate(&self) -> Result<(), String> {
        let name = self.name.value().trim();
        if name.is_empty() {
            return Err("Provider name cannot be empty".to_string());
        }

        let base_url = self.base_url.value().trim();
        if base_url.is_empty() {
            return Err("Base URL cannot be empty".to_string());
        }
        if let Err(err) = url::Url::parse(base_url) {
            return Err(format!("Invalid Base URL: {err}"));
        }

        let api_key = self.api_key.value().trim();
        if api_key.is_empty() {
            return Err("API key cannot be empty".to_string());
        }

        Ok(())
    }

    pub fn build_provider(&mut self) -> Result<(String, Provider), String> {
        self.validate()?;

        let name = self.name.value().trim().to_string();
        let base_url = self.base_url.value().trim().to_string();
        let model = self.model.value().trim().to_string();
        let api_key = self.api_key.value().trim().to_string();

        let notes_text = self.notes.lines().join("\n");
        let notes = if notes_text.trim().is_empty() {
            None
        } else {
            Some(notes_text)
        };

        let (id, mut provider) = match self.mode {
            FormMode::Add => {
                let id = crate::cli::crud::generate_provider_id(&name);
                let settings_config = build_settings(self, &name, &api_key, &base_url, &model)?;

                (
                    id.clone(),
                    Provider {
                        id,
                        name,
                        settings_config,
                        website_url: None,
                        category: Some("custom".to_string()),
                        created_at: Some(crate::cli::crud::now_millis()),
                        sort_index: None,
                        notes,
                        meta: None,
                        icon: None,
                        icon_color: None,
                        in_failover_queue: false,
                    },
                )
            }
            FormMode::Edit => {
                let id = self
                    .editing_id
                    .clone()
                    .ok_or_else(|| "Missing provider id".to_string())?;
                let mut provider = self
                    .original_provider
                    .clone()
                    .ok_or_else(|| "Missing original provider".to_string())?;

                provider.name = name;
                provider.notes = notes;

                apply_settings(
                    &self.tool,
                    &provider.name,
                    &api_key,
                    &base_url,
                    &model,
                    &mut self.codex_config,
                    &mut provider.settings_config,
                )?;

                (id.clone(), provider)
            }
        };

        if provider.id.trim().is_empty() {
            provider.id = id.clone();
        }

        Ok((id, provider))
    }

    pub fn ensure_codex_config_seeded(&mut self) {
        if !matches!(self.tool, AppType::Codex) {
            return;
        }
        if !self.codex_config.trim().is_empty() {
            return;
        }

        let name = self.name.value().trim();
        let base_url = self.base_url.value().trim();
        let model = self.model.value().trim();
        let model_opt = (!model.is_empty()).then_some(model);

        self.codex_config = crate::cli::crud::build_codex_config_toml(name, base_url, model_opt);
    }

    pub fn apply_codex_config_from_editor(&mut self, text: String) -> Result<(), String> {
        crate::codex_config::validate_config_toml(&text).map_err(|e| e.to_string())?;

        self.codex_config = text;
        if let Some(base_url) = extract_codex_base_url(&self.codex_config) {
            self.base_url.set_value(base_url);
        }
        if let Some(model) = extract_codex_model(&self.codex_config) {
            self.model.set_value(model);
        }
        Ok(())
    }
}

fn default_template(app_type: &AppType) -> ProviderTemplate {
    match app_type {
        AppType::Claude => ProviderTemplate::OpenRouter,
        AppType::Codex | AppType::Gemini => ProviderTemplate::Official,
    }
}

fn template_options(app_type: &AppType) -> &'static [ProviderTemplate] {
    match app_type {
        AppType::Claude => &[
            ProviderTemplate::OpenRouter,
            ProviderTemplate::Official,
            ProviderTemplate::Custom,
        ],
        AppType::Codex | AppType::Gemini => &[ProviderTemplate::Official, ProviderTemplate::Custom],
    }
}

fn template_label(app_type: &AppType, template: ProviderTemplate) -> &'static str {
    match (app_type, template) {
        (AppType::Claude, ProviderTemplate::OpenRouter) => "OpenRouter (recommended)",
        (AppType::Claude, ProviderTemplate::Official) => "Anthropic Direct",
        (AppType::Claude, ProviderTemplate::Custom) => "Custom API",
        (AppType::Codex, ProviderTemplate::Official) => "OpenAI Direct (recommended)",
        (AppType::Codex, ProviderTemplate::Custom) => "Custom OpenAI-compatible",
        (AppType::Gemini, ProviderTemplate::Official) => "Google AI (recommended)",
        (AppType::Gemini, ProviderTemplate::Custom) => "Custom API",
        _ => "Custom",
    }
}

fn template_defaults(
    app_type: &AppType,
    template: ProviderTemplate,
) -> (&'static str, Option<&'static str>) {
    match (app_type, template) {
        (AppType::Claude, ProviderTemplate::OpenRouter) => ("https://openrouter.ai/api", None),
        (AppType::Claude, ProviderTemplate::Official) => ("https://api.anthropic.com", None),
        (AppType::Claude, ProviderTemplate::Custom) => ("https://api.anthropic.com", None),
        (AppType::Codex, ProviderTemplate::Official) => {
            ("https://api.openai.com/v1", Some("gpt-5-codex"))
        }
        (AppType::Codex, ProviderTemplate::Custom) => {
            ("https://api.openai.com/v1", Some("gpt-5-codex"))
        }
        (AppType::Gemini, ProviderTemplate::Official) => (
            "https://generativelanguage.googleapis.com",
            Some("gemini-1.5-pro"),
        ),
        (AppType::Gemini, ProviderTemplate::Custom) => (
            "https://generativelanguage.googleapis.com",
            Some("gemini-1.5-pro"),
        ),
        _ => ("", None),
    }
}

fn build_settings(
    form: &mut ProviderForm,
    name: &str,
    api_key: &str,
    base_url: &str,
    model: &str,
) -> Result<Value, String> {
    let model = (!model.trim().is_empty()).then_some(model.trim());
    match form.tool {
        AppType::Claude => Ok(crate::cli::crud::build_claude_settings(
            api_key, base_url, model,
        )),
        AppType::Codex => {
            let mut settings =
                crate::cli::crud::build_codex_settings(name, api_key, base_url, model);
            if !form.codex_config.trim().is_empty() {
                let updated = crate::cli::crud::update_codex_config_toml(
                    form.codex_config.as_str(),
                    base_url,
                    model,
                )?;
                form.codex_config = updated.clone();
                if let Some(obj) = settings.as_object_mut() {
                    obj.insert("config".to_string(), Value::String(updated));
                }
            }
            Ok(settings)
        }
        AppType::Gemini => Ok(crate::cli::crud::build_gemini_settings(
            api_key, base_url, model,
        )),
    }
}

fn apply_settings(
    app_type: &AppType,
    name: &str,
    api_key: &str,
    base_url: &str,
    model: &str,
    codex_config: &mut String,
    settings: &mut Value,
) -> Result<(), String> {
    match app_type {
        AppType::Claude => {
            let env = ensure_env(settings)?;
            env.insert(
                "ANTHROPIC_AUTH_TOKEN".to_string(),
                Value::String(api_key.trim().to_string()),
            );
            env.remove("ANTHROPIC_API_KEY");
            env.insert(
                "ANTHROPIC_BASE_URL".to_string(),
                Value::String(base_url.trim().trim_end_matches('/').to_string()),
            );

            if model.trim().is_empty() {
                env.remove("ANTHROPIC_MODEL");
                env.remove("ANTHROPIC_DEFAULT_HAIKU_MODEL");
                env.remove("ANTHROPIC_DEFAULT_SONNET_MODEL");
                env.remove("ANTHROPIC_DEFAULT_OPUS_MODEL");
            } else {
                let m = model.trim().to_string();
                env.insert("ANTHROPIC_MODEL".to_string(), Value::String(m.clone()));
                env.insert(
                    "ANTHROPIC_DEFAULT_HAIKU_MODEL".to_string(),
                    Value::String(m.clone()),
                );
                env.insert(
                    "ANTHROPIC_DEFAULT_SONNET_MODEL".to_string(),
                    Value::String(m.clone()),
                );
                env.insert("ANTHROPIC_DEFAULT_OPUS_MODEL".to_string(), Value::String(m));
            }
            Ok(())
        }
        AppType::Gemini => {
            let env = ensure_env(settings)?;
            env.insert(
                "GEMINI_API_KEY".to_string(),
                Value::String(api_key.trim().to_string()),
            );
            env.insert(
                "GOOGLE_GEMINI_BASE_URL".to_string(),
                Value::String(base_url.trim().trim_end_matches('/').to_string()),
            );
            if model.trim().is_empty() {
                env.remove("GEMINI_MODEL");
            } else {
                env.insert(
                    "GEMINI_MODEL".to_string(),
                    Value::String(model.trim().to_string()),
                );
            }
            Ok(())
        }
        AppType::Codex => {
            let settings_obj = settings
                .as_object_mut()
                .ok_or_else(|| "settingsConfig must be a JSON object".to_string())?;

            let auth_obj = settings_obj.entry("auth").or_insert_with(|| json!({}));
            let auth_map = auth_obj
                .as_object_mut()
                .ok_or_else(|| "Codex auth must be a JSON object".to_string())?;
            auth_map.insert(
                "OPENAI_API_KEY".to_string(),
                Value::String(api_key.trim().to_string()),
            );

            let base_url = base_url.trim().trim_end_matches('/').to_string();
            let model_opt = (!model.trim().is_empty()).then_some(model.trim());

            if codex_config.trim().is_empty() {
                *codex_config =
                    crate::cli::crud::build_codex_config_toml(name, &base_url, model_opt);
            } else {
                *codex_config =
                    crate::cli::crud::update_codex_config_toml(codex_config, &base_url, model_opt)?;
            }

            settings_obj.insert("config".to_string(), Value::String(codex_config.clone()));
            Ok(())
        }
    }
}

fn ensure_env(settings: &mut Value) -> Result<&mut serde_json::Map<String, Value>, String> {
    if !settings.is_object() {
        *settings = json!({});
    }
    let obj = settings
        .as_object_mut()
        .ok_or_else(|| "settingsConfig must be a JSON object".to_string())?;

    let env = obj.entry("env").or_insert_with(|| json!({}));
    env.as_object_mut()
        .ok_or_else(|| "env must be a JSON object".to_string())
}

fn extract_api_key(app_type: &AppType, settings: &Value) -> Option<String> {
    match app_type {
        AppType::Claude => settings
            .pointer("/env/ANTHROPIC_AUTH_TOKEN")
            .or_else(|| settings.pointer("/env/ANTHROPIC_API_KEY"))
            .and_then(|v| v.as_str())
            .map(|s| s.to_string()),
        AppType::Gemini => settings
            .pointer("/env/GEMINI_API_KEY")
            .and_then(|v| v.as_str())
            .map(|s| s.to_string()),
        AppType::Codex => settings
            .pointer("/auth/OPENAI_API_KEY")
            .and_then(|v| v.as_str())
            .map(|s| s.to_string()),
    }
}

fn extract_base_url(app_type: &AppType, settings: &Value) -> Option<String> {
    match app_type {
        AppType::Claude => settings
            .pointer("/env/ANTHROPIC_BASE_URL")
            .and_then(|v| v.as_str())
            .map(|s| s.trim_end_matches('/').to_string()),
        AppType::Gemini => settings
            .pointer("/env/GOOGLE_GEMINI_BASE_URL")
            .and_then(|v| v.as_str())
            .map(|s| s.trim_end_matches('/').to_string()),
        AppType::Codex => {
            let cfg = settings.get("config").and_then(|v| v.as_str())?;
            extract_codex_base_url(cfg)
        }
    }
}

fn extract_model(app_type: &AppType, settings: &Value) -> Option<String> {
    match app_type {
        AppType::Claude => settings
            .pointer("/env/ANTHROPIC_MODEL")
            .and_then(|v| v.as_str())
            .map(|s| s.to_string()),
        AppType::Gemini => settings
            .pointer("/env/GEMINI_MODEL")
            .and_then(|v| v.as_str())
            .map(|s| s.to_string()),
        AppType::Codex => {
            let cfg = settings.get("config").and_then(|v| v.as_str())?;
            extract_codex_model(cfg)
        }
    }
}

fn extract_codex_model(config_toml: &str) -> Option<String> {
    let Ok(value) = toml::from_str::<toml::Value>(config_toml) else {
        return None;
    };
    value
        .get("model")
        .and_then(|v| v.as_str())
        .map(|s| s.to_string())
}

fn extract_codex_base_url(config_toml: &str) -> Option<String> {
    let Ok(value) = toml::from_str::<toml::Value>(config_toml) else {
        return None;
    };

    if let Some(url) = value.get("base_url").and_then(|v| v.as_str()) {
        return Some(url.trim_end_matches('/').to_string());
    }

    let provider_key = value.get("model_provider").and_then(|v| v.as_str());
    if let (Some(key), Some(model_providers)) = (provider_key, value.get("model_providers")) {
        if let Some(url) = model_providers
            .get(key)
            .and_then(|t| t.get("base_url"))
            .and_then(|v| v.as_str())
        {
            return Some(url.trim_end_matches('/').to_string());
        }
    }

    if let Some(table) = value.get("model_providers").and_then(|v| v.as_table()) {
        if let Some((_k, provider)) = table.iter().next() {
            if let Some(url) = provider.get("base_url").and_then(|v| v.as_str()) {
                return Some(url.trim_end_matches('/').to_string());
            }
        }
    }

    None
}

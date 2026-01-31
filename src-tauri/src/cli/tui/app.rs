use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use std::time::{Duration, Instant};

use crate::app_config::AppType;
use crate::provider::Provider;
use crate::services::ProviderService;
use crate::store::AppState;

use super::form::{FormField, FormMode, ProviderForm};
use super::input::TextInput;
use tui_textarea::TextArea;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Screen {
    Main,
    Form,
    ConfirmDelete,
    CodexTomlEditor,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StatusKind {
    Info,
    Success,
    Error,
}

#[derive(Debug, Clone)]
pub struct StatusMessage {
    pub kind: StatusKind,
    pub text: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ConfirmChoice {
    Yes,
    No,
}

#[derive(Debug, Clone)]
pub struct ConfirmDelete {
    pub provider_id: String,
    pub provider_name: String,
    pub choice: ConfirmChoice,
    pub error: Option<String>,
}

#[derive(Debug, Clone)]
pub struct CodexTomlEditor {
    pub textarea: TextArea<'static>,
    pub error: Option<String>,
}

#[derive(Clone)]
pub struct ProviderEntry {
    pub id: String,
    pub provider: Provider,
}

pub struct App {
    current_tool: AppType,
    screen: Screen,
    providers: Vec<ProviderEntry>,
    selected: usize,
    active_id: Option<String>,
    status: Option<StatusMessage>,
    status_set_at: Option<Instant>,
    details_scroll: u16,
    form: Option<ProviderForm>,
    confirm_delete: Option<ConfirmDelete>,
    codex_editor: Option<CodexTomlEditor>,
    tick: u64,
    help_open: bool,
    search_open: bool,
    search_query: TextInput,
    quit: bool,
}

impl App {
    pub fn new(state: &AppState) -> Result<Self, String> {
        let mut app = Self {
            current_tool: AppType::Claude,
            screen: Screen::Main,
            providers: Vec::new(),
            selected: 0,
            active_id: None,
            status: None,
            status_set_at: None,
            details_scroll: 0,
            form: None,
            confirm_delete: None,
            codex_editor: None,
            tick: 0,
            help_open: false,
            search_open: false,
            search_query: TextInput::new(""),
            quit: false,
        };
        app.reload(state)?;
        Ok(app)
    }

    pub fn should_quit(&self) -> bool {
        self.quit
    }

    pub fn current_tool(&self) -> &AppType {
        &self.current_tool
    }

    pub fn screen(&self) -> Screen {
        self.screen
    }

    pub fn form(&self) -> Option<&ProviderForm> {
        self.form.as_ref()
    }

    pub fn confirm_delete(&self) -> Option<&ConfirmDelete> {
        self.confirm_delete.as_ref()
    }

    pub fn codex_editor(&self) -> Option<&CodexTomlEditor> {
        self.codex_editor.as_ref()
    }

    pub fn help_open(&self) -> bool {
        self.help_open
    }

    pub fn search_open(&self) -> bool {
        self.search_open
    }

    pub fn search_query(&self) -> &str {
        self.search_query.value()
    }

    pub fn visible_providers(&self) -> Vec<&ProviderEntry> {
        self.visible_indices()
            .into_iter()
            .filter_map(|idx| self.providers.get(idx))
            .collect()
    }

    pub fn selected_visible_index(&self) -> Option<usize> {
        let visible = self.visible_indices();
        if visible.is_empty() {
            return None;
        }
        visible
            .iter()
            .position(|idx| *idx == self.selected)
            .or(Some(0))
    }

    pub fn providers(&self) -> &[ProviderEntry] {
        &self.providers
    }

    pub fn selected_index(&self) -> Option<usize> {
        if self.providers.is_empty() {
            None
        } else {
            Some(self.selected.min(self.providers.len().saturating_sub(1)))
        }
    }

    pub fn active_id(&self) -> Option<&str> {
        self.active_id.as_deref()
    }

    pub fn status(&self) -> Option<&StatusMessage> {
        self.status.as_ref()
    }

    pub fn spinner_frame(&self) -> &'static str {
        let idx = (self.tick as usize) % super::theme::SPINNER_FRAMES.len();
        super::theme::SPINNER_FRAMES[idx]
    }

    pub fn on_tick(&mut self) {
        self.tick = self.tick.wrapping_add(1);

        if let Some(set_at) = self.status_set_at {
            if set_at.elapsed() >= Duration::from_secs(3) {
                self.clear_status();
            }
        }
    }

    pub fn details_scroll(&self) -> u16 {
        self.details_scroll
    }

    pub fn selected_provider(&self) -> Option<&ProviderEntry> {
        let idx = self.selected_index()?;
        self.providers.get(idx)
    }

    pub fn handle_key(&mut self, state: &AppState, key: KeyEvent) -> Result<(), String> {
        if key.modifiers.contains(KeyModifiers::CONTROL) && key.code == KeyCode::Char('c') {
            self.quit = true;
            return Ok(());
        }

        if self.help_open {
            self.help_open = false;
            return Ok(());
        }
        if key.code == KeyCode::Char('?') {
            self.help_open = true;
            return Ok(());
        }

        match self.screen {
            Screen::Main => self.handle_key_main(state, key),
            Screen::Form => self.handle_key_form(state, key),
            Screen::ConfirmDelete => self.handle_key_confirm_delete(state, key),
            Screen::CodexTomlEditor => self.handle_key_codex_editor(key),
        }
    }

    fn handle_key_main(&mut self, state: &AppState, key: KeyEvent) -> Result<(), String> {
        if self.search_open && self.handle_search_key(state, key)? {
            return Ok(());
        }

        match key.code {
            KeyCode::Char('q') => {
                self.quit = true;
            }
            KeyCode::Up | KeyCode::Char('k') => self.select_prev(),
            KeyCode::Down | KeyCode::Char('j') => self.select_next(),
            KeyCode::Char('g') => self.select_first(),
            KeyCode::Char('G') => self.select_last(),
            KeyCode::Tab => self.next_tool(state)?,
            KeyCode::BackTab => self.prev_tool(state)?,
            KeyCode::Enter => self.switch_selected(state)?,
            KeyCode::PageUp => self.scroll_details_up(),
            KeyCode::PageDown => self.scroll_details_down(),
            KeyCode::Char('a') => self.open_add_form(),
            KeyCode::Char('e') => self.open_edit_form(),
            KeyCode::Char('d') => self.open_delete_confirm(),
            KeyCode::Char('/') => self.open_search(),
            _ => {}
        }
        Ok(())
    }

    fn handle_search_key(&mut self, _state: &AppState, key: KeyEvent) -> Result<bool, String> {
        match key.code {
            KeyCode::Esc => {
                self.search_open = false;
                self.search_query.set_value("");
                self.ensure_selected_visible();
                Ok(true)
            }
            KeyCode::Enter => {
                self.search_open = false;
                Ok(true)
            }
            KeyCode::Backspace
            | KeyCode::Delete
            | KeyCode::Left
            | KeyCode::Right
            | KeyCode::Home
            | KeyCode::End
            | KeyCode::Char(_) => {
                self.search_query.handle_key(key);
                self.ensure_selected_visible();
                Ok(true)
            }
            _ => Ok(false),
        }
    }

    fn handle_key_form(&mut self, state: &AppState, key: KeyEvent) -> Result<(), String> {
        let Some(form) = self.form.as_mut() else {
            self.screen = Screen::Main;
            return Ok(());
        };

        if form.template_open {
            form.handle_template_key(key);
            return Ok(());
        }

        match key.code {
            KeyCode::Esc => {
                self.form = None;
                self.screen = Screen::Main;
            }
            KeyCode::Tab => form.focus_next(),
            KeyCode::BackTab => form.focus_prev(),
            KeyCode::Enter => match form.focus_field() {
                FormField::Template => form.open_template_picker(),
                FormField::CodexConfig => self.open_codex_editor(),
                FormField::Save => self.save_form(state)?,
                FormField::Notes => {
                    form.notes.input(key);
                }
                _ => {}
            },
            _ => match form.focus_field() {
                FormField::Template => {}
                FormField::Name => {
                    form.name.handle_key(key);
                }
                FormField::BaseUrl => {
                    form.base_url.handle_key(key);
                }
                FormField::Model => {
                    form.model.handle_key(key);
                }
                FormField::ApiKey => {
                    form.api_key.handle_key(key);
                }
                FormField::CodexConfig => {}
                FormField::Notes => {
                    if key.code == KeyCode::Tab || key.code == KeyCode::BackTab {
                        // Handled above
                    } else {
                        form.notes.input(key);
                    }
                }
                FormField::Save => {}
            },
        }

        Ok(())
    }

    fn handle_key_codex_editor(&mut self, key: KeyEvent) -> Result<(), String> {
        let Some(editor) = self.codex_editor.as_mut() else {
            self.screen = Screen::Form;
            return Ok(());
        };

        if key.modifiers.contains(KeyModifiers::CONTROL) && key.code == KeyCode::Char('s') {
            let text = editor.textarea.lines().join("\n");
            if let Some(form) = self.form.as_mut() {
                match form.apply_codex_config_from_editor(text) {
                    Ok(_) => {
                        self.codex_editor = None;
                        self.screen = Screen::Form;
                    }
                    Err(err) => {
                        editor.error = Some(err);
                    }
                }
            } else {
                self.codex_editor = None;
                self.screen = Screen::Main;
            }
            return Ok(());
        }

        match key.code {
            KeyCode::Esc => {
                self.codex_editor = None;
                self.screen = Screen::Form;
            }
            _ => {
                editor.textarea.input(key);
            }
        }

        Ok(())
    }

    fn handle_key_confirm_delete(&mut self, state: &AppState, key: KeyEvent) -> Result<(), String> {
        let Some(dialog) = self.confirm_delete.as_mut() else {
            self.screen = Screen::Main;
            return Ok(());
        };

        match key.code {
            KeyCode::Esc => {
                self.confirm_delete = None;
                self.screen = Screen::Main;
            }
            KeyCode::Left
            | KeyCode::Right
            | KeyCode::Up
            | KeyCode::Down
            | KeyCode::Tab
            | KeyCode::BackTab => {
                dialog.choice = match dialog.choice {
                    ConfirmChoice::Yes => ConfirmChoice::No,
                    ConfirmChoice::No => ConfirmChoice::Yes,
                };
            }
            KeyCode::Char('y') | KeyCode::Char('Y') => {
                dialog.choice = ConfirmChoice::Yes;
                self.confirm_delete_apply(state)?;
            }
            KeyCode::Char('n') | KeyCode::Char('N') => {
                self.confirm_delete = None;
                self.screen = Screen::Main;
            }
            KeyCode::Enter => match dialog.choice {
                ConfirmChoice::Yes => self.confirm_delete_apply(state)?,
                ConfirmChoice::No => {
                    self.confirm_delete = None;
                    self.screen = Screen::Main;
                }
            },
            _ => {}
        }

        Ok(())
    }

    fn confirm_delete_apply(&mut self, state: &AppState) -> Result<(), String> {
        let Some(dialog) = self.confirm_delete.as_ref() else {
            return Ok(());
        };
        let provider_id = dialog.provider_id.clone();
        let provider_name = dialog.provider_name.clone();

        match ProviderService::delete(state, self.current_tool.clone(), &provider_id) {
            Ok(_) => {
                self.set_status(
                    StatusKind::Success,
                    format!("Deleted \"{}\"", provider_name),
                );
                self.confirm_delete = None;
                self.screen = Screen::Main;
                self.reload(state)?;
            }
            Err(err) => {
                if let Some(dialog) = self.confirm_delete.as_mut() {
                    dialog.error = Some(err.to_string());
                }
            }
        }

        Ok(())
    }

    fn open_add_form(&mut self) {
        self.form = Some(ProviderForm::new_add(self.current_tool.clone()));
        self.screen = Screen::Form;
        self.clear_status();
    }

    fn open_codex_editor(&mut self) {
        let Some(form) = self.form.as_mut() else {
            return;
        };
        if !matches!(form.tool, AppType::Codex) {
            return;
        }

        form.ensure_codex_config_seeded();

        let textarea = TextArea::from(form.codex_config.lines());
        self.codex_editor = Some(CodexTomlEditor {
            textarea,
            error: None,
        });
        self.screen = Screen::CodexTomlEditor;
    }

    fn open_edit_form(&mut self) {
        let Some(entry) = self.selected_provider() else {
            self.set_status(StatusKind::Info, "No provider selected.");
            return;
        };

        self.form = Some(ProviderForm::new_edit(
            self.current_tool.clone(),
            entry.id.clone(),
            entry.provider.clone(),
        ));
        self.screen = Screen::Form;
        self.clear_status();
        self.details_scroll = 0;
    }

    fn open_delete_confirm(&mut self) {
        let Some(entry) = self.selected_provider() else {
            self.set_status(StatusKind::Info, "No provider selected.");
            return;
        };

        if self.active_id.as_deref() == Some(entry.id.as_str()) {
            self.set_status(StatusKind::Error, "Cannot delete the active provider.");
            return;
        }

        self.confirm_delete = Some(ConfirmDelete {
            provider_id: entry.id.clone(),
            provider_name: entry.provider.name.clone(),
            choice: ConfirmChoice::No,
            error: None,
        });
        self.screen = Screen::ConfirmDelete;
        self.clear_status();
    }

    fn save_form(&mut self, state: &AppState) -> Result<(), String> {
        let Some(form) = self.form.as_mut() else {
            return Ok(());
        };

        let mode = form.mode;
        let tool = form.tool.clone();
        let (id, provider) = match form.build_provider() {
            Ok(v) => v,
            Err(err) => {
                form.error = Some(err);
                return Ok(());
            }
        };

        let result = match mode {
            FormMode::Add => ProviderService::add(state, tool.clone(), provider),
            FormMode::Edit => ProviderService::update(state, tool.clone(), provider),
        };

        match result {
            Ok(_) => {
                self.set_status(
                    StatusKind::Success,
                    match mode {
                        FormMode::Add => "Provider created",
                        FormMode::Edit => "Provider updated",
                    },
                );
                self.form = None;
                self.screen = Screen::Main;
                self.reload(state)?;
                if let Some(idx) = self.providers.iter().position(|p| p.id == id) {
                    self.selected = idx;
                    self.details_scroll = 0;
                }
            }
            Err(err) => {
                if let Some(form) = self.form.as_mut() {
                    form.error = Some(err.to_string());
                }
            }
        }

        Ok(())
    }

    fn set_status(&mut self, kind: StatusKind, text: impl Into<String>) {
        self.status = Some(StatusMessage {
            kind,
            text: text.into(),
        });
        self.status_set_at = Some(Instant::now());
    }

    fn clear_status(&mut self) {
        self.status = None;
        self.status_set_at = None;
    }

    fn select_prev(&mut self) {
        self.details_scroll = 0;
        let visible = self.visible_indices();
        if visible.is_empty() {
            self.selected = 0;
            return;
        }
        let cur = visible
            .iter()
            .position(|idx| *idx == self.selected)
            .unwrap_or(0);
        let next = cur.saturating_sub(1);
        self.selected = visible[next];
    }

    fn select_next(&mut self) {
        self.details_scroll = 0;
        let visible = self.visible_indices();
        if visible.is_empty() {
            self.selected = 0;
            return;
        }
        let cur = visible
            .iter()
            .position(|idx| *idx == self.selected)
            .unwrap_or(0);
        let next = (cur + 1).min(visible.len().saturating_sub(1));
        self.selected = visible[next];
    }

    fn select_first(&mut self) {
        self.details_scroll = 0;
        let visible = self.visible_indices();
        if let Some(first) = visible.first().copied() {
            self.selected = first;
        } else {
            self.selected = 0;
        }
    }

    fn select_last(&mut self) {
        self.details_scroll = 0;
        let visible = self.visible_indices();
        if visible.is_empty() {
            self.selected = 0;
        } else {
            self.selected = *visible.last().unwrap_or(&0);
        }
    }

    fn next_tool(&mut self, state: &AppState) -> Result<(), String> {
        self.current_tool = match self.current_tool {
            AppType::Claude => AppType::Codex,
            AppType::Codex => AppType::Gemini,
            AppType::Gemini => AppType::Claude,
        };
        self.selected = 0;
        self.details_scroll = 0;
        self.reload(state)?;
        Ok(())
    }

    fn prev_tool(&mut self, state: &AppState) -> Result<(), String> {
        self.current_tool = match self.current_tool {
            AppType::Claude => AppType::Gemini,
            AppType::Codex => AppType::Claude,
            AppType::Gemini => AppType::Codex,
        };
        self.selected = 0;
        self.details_scroll = 0;
        self.reload(state)?;
        Ok(())
    }

    fn switch_selected(&mut self, state: &AppState) -> Result<(), String> {
        let Some(entry) = self.selected_provider() else {
            self.set_status(
                StatusKind::Info,
                "No providers configured. Use the GUI to add providers.",
            );
            return Ok(());
        };

        if self.active_id.as_deref() == Some(entry.id.as_str()) {
            self.set_status(
                StatusKind::Info,
                format!("\"{}\" is already active.", entry.provider.name),
            );
            return Ok(());
        }

        if let Err(err) = ProviderService::switch(state, self.current_tool.clone(), &entry.id) {
            self.set_status(
                StatusKind::Error,
                format!("Failed to switch provider: {err}"),
            );
            return Ok(());
        }

        self.set_status(
            StatusKind::Success,
            format!(
                "Switched {} to \"{}\"",
                self.current_tool.as_str(),
                entry.provider.name
            ),
        );

        if let Err(err) = self.reload(state) {
            self.set_status(
                StatusKind::Error,
                format!("Switched, but failed to refresh UI: {err}"),
            );
        }
        Ok(())
    }

    fn scroll_details_up(&mut self) {
        self.details_scroll = self.details_scroll.saturating_sub(1);
    }

    fn scroll_details_down(&mut self) {
        self.details_scroll = self.details_scroll.saturating_add(1);
    }

    fn reload(&mut self, state: &AppState) -> Result<(), String> {
        let providers =
            ProviderService::list(state, self.current_tool.clone()).map_err(|e| e.to_string())?;
        self.providers = providers
            .into_iter()
            .map(|(id, provider)| ProviderEntry { id, provider })
            .collect();

        let current = ProviderService::current(state, self.current_tool.clone()).ok();
        self.active_id = current
            .as_deref()
            .filter(|s| !s.trim().is_empty())
            .map(|s| s.to_string());

        self.selected = self.selected.min(self.providers.len().saturating_sub(1));
        self.ensure_selected_visible();

        Ok(())
    }

    fn open_search(&mut self) {
        self.search_open = true;
    }

    fn visible_indices(&self) -> Vec<usize> {
        if self.providers.is_empty() {
            return Vec::new();
        }

        let query = self.search_query.value().trim().to_ascii_lowercase();
        if query.is_empty() {
            return (0..self.providers.len()).collect();
        }

        self.providers
            .iter()
            .enumerate()
            .filter(|(_, p)| {
                p.provider.name.to_ascii_lowercase().contains(&query)
                    || p.id.to_ascii_lowercase().contains(&query)
            })
            .map(|(idx, _)| idx)
            .collect()
    }

    fn ensure_selected_visible(&mut self) {
        let visible = self.visible_indices();
        if visible.is_empty() {
            self.selected = 0;
            return;
        }
        if !visible.iter().any(|idx| *idx == self.selected) {
            self.selected = visible[0];
            self.details_scroll = 0;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn test_app(names: &[&str]) -> App {
        let providers = names
            .iter()
            .enumerate()
            .map(|(idx, name)| {
                let id = format!("id-{idx}");
                ProviderEntry {
                    id: id.clone(),
                    provider: Provider::with_id(id, (*name).to_string(), json!({}), None),
                }
            })
            .collect::<Vec<_>>();

        App {
            current_tool: AppType::Claude,
            screen: Screen::Main,
            providers,
            selected: 0,
            active_id: None,
            status: None,
            status_set_at: None,
            details_scroll: 0,
            form: None,
            confirm_delete: None,
            codex_editor: None,
            tick: 0,
            help_open: false,
            search_open: false,
            search_query: TextInput::new(""),
            quit: false,
        }
    }

    #[test]
    fn search_filter_keeps_selection_visible() {
        let mut app = test_app(&["Alpha", "Beta", "Gamma"]);
        app.selected = 0;

        app.search_query.set_value("ga");
        app.ensure_selected_visible();

        assert_eq!(
            app.selected_provider().unwrap().provider.name,
            "Gamma",
            "selection should move to first visible match"
        );
    }

    #[test]
    fn status_auto_fades_after_timeout() {
        let mut app = test_app(&["Alpha"]);
        app.set_status(StatusKind::Info, "hello");
        app.status_set_at = Some(Instant::now() - Duration::from_secs(4));
        app.on_tick();
        assert!(app.status.is_none(), "status should fade out");
    }

    #[test]
    fn spinner_advances_with_ticks() {
        let mut app = test_app(&["Alpha"]);
        let first = app.spinner_frame();
        app.on_tick();
        let second = app.spinner_frame();
        assert_ne!(first, second);
    }
}

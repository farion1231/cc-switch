use ratatui::layout::{Constraint, Direction, Layout, Rect};
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span, Text};
use ratatui::widgets::{
    Block, BorderType, Borders, Clear, List, ListItem, ListState, Paragraph, Tabs, Wrap,
};
use ratatui::Frame;

use crate::app_config::AppType;

use super::app::{App, CodexTomlEditor, ConfirmChoice, ConfirmDelete, Screen, StatusKind};
use super::form::{FormField, FormMode, ProviderForm};
use super::theme;

pub fn render(frame: &mut Frame, app: &App) {
    let root = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(3),
            Constraint::Min(0),
            Constraint::Length(3),
        ])
        .split(frame.area());

    render_header(frame, app, root[0]);
    render_body(frame, app, root[1]);
    render_footer(frame, app, root[2]);

    if let Some(form) = app.form() {
        render_form_overlay(frame, app, form);
    }
    if let Some(dialog) = app.confirm_delete() {
        render_confirm_delete_overlay(frame, dialog);
    }
    if let Some(editor) = app.codex_editor() {
        render_codex_editor_overlay(frame, editor);
    }
    if app.help_open() {
        render_help_overlay(frame, app);
    }
}

fn render_header(frame: &mut Frame, app: &App, area: ratatui::layout::Rect) {
    let titles = ["Claude", "Codex", "Gemini"]
        .iter()
        .map(|t| Line::from(Span::styled(*t, Style::default().fg(theme::MUTED))))
        .collect::<Vec<_>>();

    let selected = match app.current_tool() {
        AppType::Claude => 0,
        AppType::Codex => 1,
        AppType::Gemini => 2,
    };

    let tabs = Tabs::new(titles)
        .block(
            Block::default()
                .borders(Borders::ALL)
                .border_type(BorderType::Rounded)
                .title(format!("CC-Switch TUI {}", app.spinner_frame())),
        )
        .select(selected)
        .highlight_style(
            Style::default()
                .fg(theme::PRIMARY)
                .add_modifier(Modifier::BOLD),
        );

    frame.render_widget(tabs, area);
}

fn render_body(frame: &mut Frame, app: &App, area: ratatui::layout::Rect) {
    let cols = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([Constraint::Percentage(40), Constraint::Percentage(60)])
        .split(area);

    render_provider_list(frame, app, cols[0]);
    render_provider_details(frame, app, cols[1]);
}

fn render_provider_list(frame: &mut Frame, app: &App, area: ratatui::layout::Rect) {
    let active_id = app.active_id();
    let query = app.search_query().trim();
    let query_lower = query.to_ascii_lowercase();

    let visible = app.visible_providers();
    let items: Vec<ListItem> = if visible.is_empty() {
        let msg = if app.providers().is_empty() {
            "No providers configured. Press 'a' to add."
        } else {
            "No matches."
        };
        vec![ListItem::new(Line::from(Span::styled(
            msg,
            Style::default().fg(Color::DarkGray),
        )))]
    } else {
        visible
            .iter()
            .map(|entry| {
                let is_active = active_id == Some(entry.id.as_str());
                let marker = if is_active {
                    theme::ICON_ACTIVE
                } else {
                    theme::ICON_INACTIVE
                };
                let marker_style = if is_active {
                    Style::default().fg(theme::SUCCESS)
                } else {
                    Style::default().fg(theme::MUTED)
                };
                let base_style = if is_active {
                    Style::default().fg(theme::SUCCESS)
                } else {
                    Style::default()
                };

                let mut spans: Vec<Span> = vec![Span::styled(marker, marker_style), Span::raw(" ")];

                let name = entry.provider.name.as_str();
                if !query_lower.is_empty() {
                    let haystack = name.to_ascii_lowercase();
                    if let Some(pos) = haystack.find(&query_lower) {
                        let (pre, rest) = name.split_at(pos);
                        let (matched, suf) = rest.split_at(query_lower.len());
                        spans.push(Span::styled(pre.to_string(), base_style));
                        spans.push(Span::styled(
                            matched.to_string(),
                            Style::default()
                                .fg(theme::WARNING)
                                .add_modifier(Modifier::BOLD),
                        ));
                        spans.push(Span::styled(suf.to_string(), base_style));
                    } else {
                        spans.push(Span::styled(name.to_string(), base_style));
                    }
                } else {
                    spans.push(Span::styled(name.to_string(), base_style));
                }

                if is_active {
                    spans.push(Span::raw(" "));
                    spans.push(Span::styled(
                        "[Active]",
                        Style::default()
                            .fg(theme::SUCCESS)
                            .add_modifier(Modifier::BOLD),
                    ));
                }

                ListItem::new(Line::from(spans))
            })
            .collect()
    };

    let list = List::new(items)
        .block(
            Block::default()
                .borders(Borders::ALL)
                .border_type(BorderType::Rounded)
                .title("Providers"),
        )
        .highlight_style(Style::default().add_modifier(Modifier::REVERSED))
        .highlight_symbol(theme::LIST_HIGHLIGHT_SYMBOL);

    let mut state = ListState::default();
    state.select(if visible.is_empty() {
        None
    } else {
        app.selected_visible_index()
    });

    frame.render_stateful_widget(list, area, &mut state);
}

fn render_provider_details(frame: &mut Frame, app: &App, area: ratatui::layout::Rect) {
    let block = Block::default()
        .borders(Borders::ALL)
        .border_type(BorderType::Rounded)
        .title("Provider Details");

    let Some(entry) = app.selected_provider() else {
        let p = Paragraph::new(Text::from(
            "No providers configured. Use the GUI to add providers.",
        ))
        .block(block)
        .wrap(Wrap { trim: false });
        frame.render_widget(p, area);
        return;
    };

    let provider = &entry.provider;
    let is_active = app.active_id() == Some(entry.id.as_str());

    let base_url = extract_base_url(app.current_tool(), &provider.settings_config);
    let model = extract_model(app.current_tool(), &provider.settings_config);
    let api_key =
        extract_api_key(app.current_tool(), &provider.settings_config).map(|s| mask_secret(&s));

    let mut lines = Vec::new();
    lines.push(kv("Name", provider.name.as_str(), true));
    lines.push(kv("ID", entry.id.as_str(), false));
    lines.push(kv(
        "Status",
        if is_active { "Active" } else { "Inactive" },
        false,
    ));
    if let Some(url) = base_url.as_deref() {
        lines.push(kv("Base URL", url, false));
    }
    if let Some(model) = model.as_deref() {
        lines.push(kv("Model", model, false));
    }
    if let Some(key) = api_key.as_deref() {
        lines.push(kv("API Key", key, false));
    }
    if let Some(site) = provider
        .website_url
        .as_deref()
        .filter(|s| !s.trim().is_empty())
    {
        lines.push(kv("Website", site, false));
    }
    if let Some(notes) = provider.notes.as_deref().filter(|s| !s.trim().is_empty()) {
        lines.push(kv("Notes", notes, false));
    }

    let text = Text::from(lines);
    let p = Paragraph::new(text)
        .block(block)
        .wrap(Wrap { trim: false })
        .scroll((app.details_scroll(), 0));

    frame.render_widget(p, area);
}

fn render_footer(frame: &mut Frame, app: &App, area: ratatui::layout::Rect) {
    let hints = match app.screen() {
        Screen::Main => {
            "↑/↓/j/k Move  Tab Switch Tool  Enter Switch  a Add  e Edit  d Delete  / Search  ? Help  q Quit"
        }
        Screen::Form => "Tab Next  S-Tab Prev  Enter Select/Save  Esc Cancel",
        Screen::ConfirmDelete => "←/→ Select  Enter Confirm  Esc Cancel",
        Screen::CodexTomlEditor => "Ctrl+S Save  Esc Cancel",
    };

    let mut spans = vec![Span::styled(hints, Style::default().fg(theme::MUTED))];

    if app.screen() == Screen::Main && (app.search_open() || !app.search_query().trim().is_empty())
    {
        spans.push(Span::raw("  "));
        let prefix = if app.search_open() { "/" } else { "Filter:" };
        let query = app.search_query();
        let text = if query.trim().is_empty() {
            format!("{prefix} ")
        } else if app.search_open() {
            format!("{prefix}{query}")
        } else {
            format!("{prefix} {query}")
        };
        spans.push(Span::styled(
            text,
            Style::default()
                .fg(theme::PRIMARY)
                .add_modifier(if app.search_open() {
                    Modifier::BOLD
                } else {
                    Modifier::empty()
                }),
        ));
    }

    if let Some(status) = app.status() {
        let style = match status.kind {
            StatusKind::Info => Style::default().fg(theme::PRIMARY),
            StatusKind::Success => Style::default().fg(theme::SUCCESS),
            StatusKind::Error => Style::default().fg(theme::ERROR),
        };
        let icon = match status.kind {
            StatusKind::Info => theme::ICON_INFO,
            StatusKind::Success => theme::ICON_SUCCESS,
            StatusKind::Error => theme::ICON_ERROR,
        };
        spans.push(Span::raw("  "));
        spans.push(Span::styled(format!("{icon} "), style));
        spans.push(Span::styled(status.text.as_str(), style));
    }

    let p = Paragraph::new(Line::from(spans))
        .block(
            Block::default()
                .borders(Borders::ALL)
                .border_type(BorderType::Rounded),
        )
        .wrap(Wrap { trim: true });
    frame.render_widget(p, area);
}

fn render_help_overlay(frame: &mut Frame, app: &App) {
    let area = centered_rect(80, 80, frame.area());
    frame.render_widget(Clear, area);

    let block = Block::default()
        .borders(Borders::ALL)
        .border_type(BorderType::Double)
        .title("Help");
    frame.render_widget(block.clone(), area);

    let inner = block.inner(area);

    let lines: Vec<Line> = match app.screen() {
        Screen::Main => vec![
            Line::from("Navigation"),
            Line::from("  ↑/↓ or j/k   Move selection"),
            Line::from("  g / G        First / last provider"),
            Line::from("  PgUp/PgDn    Scroll details"),
            Line::from(""),
            Line::from("Actions"),
            Line::from("  Enter        Switch provider"),
            Line::from("  a            Add provider"),
            Line::from("  e            Edit provider"),
            Line::from("  d            Delete provider"),
            Line::from("  /            Search/filter"),
            Line::from("  Tab/S-Tab    Switch tool tab"),
            Line::from("  q            Quit"),
            Line::from(""),
            Line::from("Press any key to close"),
        ],
        Screen::Form => vec![
            Line::from("Form Mode"),
            Line::from("  Tab/S-Tab    Next/previous field"),
            Line::from("  Enter        Open/select / Save on button"),
            Line::from("  Esc          Cancel"),
            Line::from(""),
            Line::from("Notes"),
            Line::from("  Enter        New line"),
            Line::from(""),
            Line::from("Codex"),
            Line::from("  On config field: Enter to edit TOML"),
            Line::from("  In editor: Ctrl+S save, Esc cancel"),
            Line::from(""),
            Line::from("Press any key to close"),
        ],
        Screen::ConfirmDelete => vec![
            Line::from("Delete Confirmation"),
            Line::from("  ←/→          Select Yes/No"),
            Line::from("  Enter        Confirm"),
            Line::from("  Esc          Cancel"),
            Line::from(""),
            Line::from("Press any key to close"),
        ],
        Screen::CodexTomlEditor => vec![
            Line::from("Codex TOML Editor"),
            Line::from("  Ctrl+S       Save (validates TOML)"),
            Line::from("  Esc          Cancel"),
            Line::from(""),
            Line::from("Press any key to close"),
        ],
    };

    let p = Paragraph::new(Text::from(lines)).wrap(Wrap { trim: false });
    frame.render_widget(p, inner);
}

fn render_codex_editor_overlay(frame: &mut Frame, editor: &CodexTomlEditor) {
    let area = centered_rect(90, 90, frame.area());
    frame.render_widget(Clear, area);

    let block = Block::default()
        .borders(Borders::ALL)
        .border_type(BorderType::Double)
        .title("Codex config.toml");
    frame.render_widget(block.clone(), area);

    let inner = block.inner(area);
    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(3),
            Constraint::Min(0),
            Constraint::Length(2),
        ])
        .split(inner);

    let hints =
        Paragraph::new("TOML hints: model_provider, model, base_url, [model_providers.<key>]")
            .style(Style::default().fg(Color::DarkGray))
            .wrap(Wrap { trim: true });
    frame.render_widget(hints, chunks[0]);

    let text_block = Block::default()
        .borders(Borders::ALL)
        .border_type(BorderType::Rounded)
        .title("config.toml");
    let text_inner = text_block.inner(chunks[1]);
    frame.render_widget(text_block, chunks[1]);
    frame.render_widget(&editor.textarea, text_inner);

    let err = Paragraph::new(editor.error.as_deref().unwrap_or(""))
        .style(Style::default().fg(Color::Red))
        .wrap(Wrap { trim: true });
    frame.render_widget(err, chunks[2]);
}

fn render_confirm_delete_overlay(frame: &mut Frame, dialog: &ConfirmDelete) {
    let area = centered_rect(60, 30, frame.area());
    frame.render_widget(Clear, area);

    let block = Block::default()
        .borders(Borders::ALL)
        .border_type(BorderType::Double)
        .title("Delete Provider");
    frame.render_widget(block.clone(), area);

    let inner = block.inner(area);
    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(2),
            Constraint::Length(2),
            Constraint::Length(3),
        ])
        .split(inner);

    let question = Paragraph::new(format!(
        "Delete \"{}\"? This cannot be undone.",
        dialog.provider_name
    ))
    .wrap(Wrap { trim: true });
    frame.render_widget(question, chunks[0]);

    let err = Paragraph::new(dialog.error.as_deref().unwrap_or(""))
        .style(Style::default().fg(Color::Red))
        .wrap(Wrap { trim: true });
    frame.render_widget(err, chunks[1]);

    let (yes_style, no_style) = match dialog.choice {
        ConfirmChoice::Yes => (
            Style::default()
                .fg(Color::Black)
                .bg(Color::Red)
                .add_modifier(Modifier::BOLD),
            Style::default().fg(Color::Gray),
        ),
        ConfirmChoice::No => (
            Style::default().fg(Color::Gray),
            Style::default()
                .fg(Color::Black)
                .bg(Color::Yellow)
                .add_modifier(Modifier::BOLD),
        ),
    };

    let buttons = Line::from(vec![
        Span::styled("[ Yes ]", yes_style),
        Span::raw("   "),
        Span::styled("[ No ]", no_style),
    ]);
    frame.render_widget(
        Paragraph::new(buttons)
            .block(Block::default().borders(Borders::NONE))
            .wrap(Wrap { trim: true }),
        chunks[2],
    );
}

fn render_form_overlay(frame: &mut Frame, app: &App, form: &ProviderForm) {
    let area = centered_rect(80, 80, frame.area());
    frame.render_widget(Clear, area);

    let title = match form.mode {
        FormMode::Add => format!("Add Provider - {}", form.tool.as_str()),
        FormMode::Edit => format!("Edit Provider - {}", form.tool.as_str()),
    };

    let block = Block::default()
        .borders(Borders::ALL)
        .border_type(BorderType::Double)
        .title(title);
    frame.render_widget(block.clone(), area);

    let inner = block.inner(area);

    let fields = form.fields();
    let mut constraints = Vec::with_capacity(fields.len() + 2);
    for f in &fields {
        constraints.push(match f {
            FormField::Notes => Constraint::Min(7),
            _ => Constraint::Length(3),
        });
    }
    constraints.push(Constraint::Length(2));

    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints(constraints)
        .split(inner);

    for (idx, field) in fields.iter().enumerate() {
        render_form_field(frame, form, *field, chunks[idx]);
    }

    render_form_error(frame, form, chunks[fields.len()]);

    if form.template_open {
        render_template_picker(frame, app, form, area);
    }
}

fn render_form_error(frame: &mut Frame, form: &ProviderForm, area: Rect) {
    let text = form.error.as_deref().unwrap_or("");
    let p = Paragraph::new(text)
        .style(Style::default().fg(Color::Red))
        .wrap(Wrap { trim: true });
    frame.render_widget(p, area);
}

fn render_form_field(frame: &mut Frame, form: &ProviderForm, field: FormField, area: Rect) {
    let focused = form.focus_field() == field;
    let border_style = if focused {
        Style::default()
            .fg(Color::Yellow)
            .add_modifier(Modifier::BOLD)
    } else {
        Style::default()
    };

    match field {
        FormField::Template => {
            let text = format!("{}  ▼", form.template_label(form.template));
            let p = Paragraph::new(text).block(
                Block::default()
                    .borders(Borders::ALL)
                    .border_style(border_style)
                    .title("Template"),
            );
            frame.render_widget(p, area);
        }
        FormField::Name => {
            let p = Paragraph::new(form.name.display_value()).block(
                Block::default()
                    .borders(Borders::ALL)
                    .border_style(border_style)
                    .title("Name"),
            );
            frame.render_widget(p, area);
        }
        FormField::BaseUrl => {
            let p = Paragraph::new(form.base_url.display_value()).block(
                Block::default()
                    .borders(Borders::ALL)
                    .border_style(border_style)
                    .title("Base URL"),
            );
            frame.render_widget(p, area);
        }
        FormField::Model => {
            let p = Paragraph::new(form.model.display_value()).block(
                Block::default()
                    .borders(Borders::ALL)
                    .border_style(border_style)
                    .title("Model (optional)"),
            );
            frame.render_widget(p, area);
        }
        FormField::ApiKey => {
            let p = Paragraph::new(form.api_key.display_value()).block(
                Block::default()
                    .borders(Borders::ALL)
                    .border_style(border_style)
                    .title("API Key"),
            );
            frame.render_widget(p, area);
        }
        FormField::CodexConfig => {
            let preview = form
                .codex_config
                .lines()
                .next()
                .filter(|s| !s.trim().is_empty())
                .unwrap_or("(press Enter to edit)");
            let p = Paragraph::new(preview).block(
                Block::default()
                    .borders(Borders::ALL)
                    .border_style(border_style)
                    .title("Codex config.toml"),
            );
            frame.render_widget(p, area);
        }
        FormField::Notes => {
            let block = Block::default()
                .borders(Borders::ALL)
                .border_style(border_style)
                .title("Notes (optional)");
            let inner = block.inner(area);
            frame.render_widget(block, area);
            frame.render_widget(&form.notes, inner);
        }
        FormField::Save => {
            let label = match form.mode {
                FormMode::Add => "[ Save ]",
                FormMode::Edit => "[ Update ]",
            };
            let style = if focused {
                Style::default()
                    .fg(Color::Black)
                    .bg(Color::Yellow)
                    .add_modifier(Modifier::BOLD)
            } else {
                Style::default()
                    .fg(Color::Yellow)
                    .add_modifier(Modifier::BOLD)
            };
            let p = Paragraph::new(label)
                .style(style)
                .block(
                    Block::default()
                        .borders(Borders::ALL)
                        .border_style(border_style),
                )
                .wrap(Wrap { trim: true });
            frame.render_widget(p, area);
        }
    }
}

fn render_template_picker(frame: &mut Frame, _app: &App, form: &ProviderForm, modal_area: Rect) {
    let width = modal_area.width.saturating_sub(10).min(60);
    let height = 7.min(modal_area.height.saturating_sub(6));
    let x = modal_area.x + (modal_area.width.saturating_sub(width)) / 2;
    let y = modal_area.y + 4;
    let area = Rect {
        x,
        y,
        width,
        height,
    };

    frame.render_widget(Clear, area);

    let opts = form
        .template_options()
        .iter()
        .map(|t| ListItem::new(form.template_label(*t)))
        .collect::<Vec<_>>();
    let list = List::new(opts)
        .block(
            Block::default()
                .borders(Borders::ALL)
                .border_type(BorderType::Double)
                .title("Select Template"),
        )
        .highlight_style(Style::default().add_modifier(Modifier::REVERSED))
        .highlight_symbol("▶ ");

    let mut state = ListState::default();
    state.select(Some(form.template_index));

    frame.render_stateful_widget(list, area, &mut state);
}

fn centered_rect(percent_x: u16, percent_y: u16, r: Rect) -> Rect {
    let popup_layout = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Percentage((100 - percent_y) / 2),
            Constraint::Percentage(percent_y),
            Constraint::Percentage((100 - percent_y) / 2),
        ])
        .split(r);

    Layout::default()
        .direction(Direction::Horizontal)
        .constraints([
            Constraint::Percentage((100 - percent_x) / 2),
            Constraint::Percentage(percent_x),
            Constraint::Percentage((100 - percent_x) / 2),
        ])
        .split(popup_layout[1])[1]
}

fn kv<'a>(key: &'a str, value: &'a str, emphasize: bool) -> Line<'a> {
    let key_style = Style::default()
        .fg(theme::WARNING)
        .add_modifier(Modifier::BOLD);
    let mut value_style = Style::default();
    if emphasize {
        value_style = value_style.add_modifier(Modifier::BOLD);
    }
    Line::from(vec![
        Span::styled(format!("{key}: "), key_style),
        Span::styled(value.to_string(), value_style),
    ])
}

fn mask_secret(secret: &str) -> String {
    let s = secret.trim();
    if s.len() <= 8 {
        return "****".to_string();
    }
    let prefix = &s[..4];
    let suffix = &s[s.len() - 4..];
    format!("{prefix}****...{suffix}")
}

fn extract_api_key(app_type: &AppType, settings: &serde_json::Value) -> Option<String> {
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

fn extract_base_url(app_type: &AppType, settings: &serde_json::Value) -> Option<String> {
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

fn extract_model(app_type: &AppType, settings: &serde_json::Value) -> Option<String> {
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

    // Fallback: first model_providers.*.base_url
    if let Some(table) = value.get("model_providers").and_then(|v| v.as_table()) {
        if let Some((_k, provider)) = table.iter().next() {
            if let Some(url) = provider.get("base_url").and_then(|v| v.as_str()) {
                return Some(url.trim_end_matches('/').to_string());
            }
        }
    }

    None
}

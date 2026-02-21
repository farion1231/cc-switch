use crate::tui::app::{App, Focus};
use ratatui::prelude::*;
use ratatui::widgets::{Block, Borders, List, ListItem, ListState};

pub fn render(f: &mut Frame, area: Rect, app: &App) {
    let focused = app.focus == Focus::AppTabs;
    let border_style = if focused {
        Style::default().fg(Color::Cyan)
    } else {
        Style::default().fg(Color::DarkGray)
    };

    let items: Vec<ListItem> = app
        .apps
        .iter()
        .enumerate()
        .map(|(i, a)| {
            let marker = if i == app.selected_app { "▸ " } else { "  " };
            let style = if i == app.selected_app {
                Style::default().fg(Color::Yellow).bold()
            } else {
                Style::default()
            };
            ListItem::new(format!("{marker}{}", capitalize(a.as_str()))).style(style)
        })
        .collect();

    let list = List::new(items).block(
        Block::default()
            .borders(Borders::ALL)
            .border_style(border_style)
            .title(" Apps "),
    );

    let mut state = ListState::default().with_selected(Some(app.selected_app));
    f.render_stateful_widget(list, area, &mut state);
}

fn capitalize(s: &str) -> String {
    let mut c = s.chars();
    match c.next() {
        None => String::new(),
        Some(f) => f.to_uppercase().to_string() + c.as_str(),
    }
}

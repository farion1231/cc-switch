use crate::tui::app::{App, Focus};
use ratatui::prelude::*;
use ratatui::widgets::{Block, Borders, Row, Table, TableState};

pub fn render(f: &mut Frame, area: Rect, app: &App) {
    let focused = app.focus == Focus::MainPanel;
    let border_style = if focused {
        Style::default().fg(Color::Cyan)
    } else {
        Style::default().fg(Color::DarkGray)
    };

    let rows: Vec<Row> = app
        .providers
        .iter()
        .enumerate()
        .map(|(i, (id, p))| {
            let marker = if id == &app.current_id { "→" } else { "" };
            let sel = if i == app.selected_provider && focused {
                Style::default().bg(Color::DarkGray)
            } else {
                Style::default()
            };
            Row::new(vec![
                marker.to_string(),
                (i + 1).to_string(),
                p.name.clone(),
            ])
            .style(sel)
        })
        .collect();

    let widths = [
        Constraint::Length(1),
        Constraint::Length(3),
        Constraint::Fill(1),
    ];

    let table = Table::new(rows, widths)
        .header(Row::new(vec!["", "#", "Name"]).style(Style::default().bold()))
        .block(
            Block::default()
                .borders(Borders::ALL)
                .border_style(border_style)
                .title(" Providers "),
        );

    let mut state = TableState::default().with_selected(Some(app.selected_provider));
    f.render_stateful_widget(table, area, &mut state);
}

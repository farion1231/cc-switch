use crate::tui::app::App;
use crate::tui::components;
use ratatui::prelude::*;
use ratatui::widgets::Paragraph;

pub fn render(f: &mut Frame, app: &App) {
    let chunks = Layout::vertical([
        Constraint::Length(1),
        Constraint::Fill(1),
        Constraint::Length(1),
    ])
    .split(f.area());

    render_title_bar(f, chunks[0], app);

    let main_cols = Layout::horizontal([
        Constraint::Length(12),
        Constraint::Fill(3),
        Constraint::Fill(2),
    ])
    .split(chunks[1]);

    components::app_tabs::render(f, main_cols[0], app);
    components::provider_list::render(f, main_cols[1], app);
    components::detail::render(f, main_cols[2], app);

    components::status_bar::render(f, chunks[2]);
    components::modal::render(f, app);
}

fn render_title_bar(f: &mut Frame, area: Rect, app: &App) {
    let title = format!(" CC Switch v{}", app.version);
    let pad = area.width.saturating_sub(title.len() as u16);
    let line = Line::from(vec![
        Span::styled(title, Style::default().bold()),
        Span::raw(" ".repeat(pad as usize)),
    ]);
    f.render_widget(
        Paragraph::new(line).style(Style::default().bg(Color::DarkGray)),
        area,
    );
}
